use alloc::boxed::Box;
use alloc::vec::Vec;
use core::future::Future;
use core::sync::atomic::{AtomicU8, Ordering};

use crate::SharedDemand;
use crate::frame::{FrameClock, Wait};
use crate::input::{CaptureWindow, IntentBus};
use crate::playback::{PlaybackBus, PlaybackCommand};
use embassy_futures::select::select3;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, TrySendError};
use embassy_sync::signal::Signal;
use embassy_time::{Instant, Timer};
use iot_core::demand::ClockGovernor;
use iot_core::demand::ClockSink;
use iot_core::demand::{DemandKey, EffectiveDemand, Mode};
use iot_core::diagnostics::DiagnosticsSink;
use iot_core::drivers::camera::CameraTarget;
use iot_core::drivers::light::{
    Fill, Rgb, RgbLight, backlight_level, group_hue, hsv_to_rgb, should_repaint, smooth_brightness,
};
use iot_core::drivers::network::{ApList, Front, JoinRequest, Ssid};
use iot_core::intent::Intent;
use iot_core::render::{
    Activity, LightAppearance, RenderController, Renderer, Slot, SlotAppearance,
};
use iot_core::state::{
    Breath, DeviceManager, DeviceState, DisplayPage, LampLine, NetworkCommand, PlaybackPhase,
    PlaybackState,
};

pub use crate::frame::STEP_MS;

pub struct Latch<T> {
    value: Signal<CriticalSectionRawMutex, T>,
}

impl<T: Send> Latch<T> {
    const fn new() -> Self {
        Self {
            value: Signal::new(),
        }
    }

    pub fn set(&self, value: T) {
        self.value.signal(value);
    }

    pub async fn ready(&self) -> T {
        self.value.wait().await
    }

    pub fn clear(&self) {
        self.value.reset();
    }
}

/// Leave-request storage, cleared before an association so a leave cannot outlive its link.
pub static LEAVE: Latch<()> = Latch::new();

/// Survey-request storage.
pub static RESCAN: Latch<()> = Latch::new();

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SetupAsk {
    Cancel { from: Front },
    Front { from: Front },
    Pass { from: Front, open: bool },
}

impl SetupAsk {
    /// Whether the front now holding the mode is the one this request was made on.
    pub fn applies_to(self, front: Front) -> bool {
        let from = match self {
            Self::Cancel { from } | Self::Front { from } | Self::Pass { from, .. } => from,
        };
        from == front
    }
}

/// Setup-request storage; the render loop publishes into it, the network task takes.
pub static SETUP_ASK: Latch<SetupAsk> = Latch::new();

pub static FORGET: Latch<Ssid> = Latch::new();

pub struct Handoff<T> {
    slot: Channel<CriticalSectionRawMutex, T, 1>,
}

impl<T: Send> Handoff<T> {
    const fn new() -> Self {
        Self {
            slot: Channel::new(),
        }
    }

    pub fn take(&self) -> Option<T> {
        self.slot.try_receive().ok()
    }

    pub fn ready(&self) -> impl Future<Output = ()> {
        self.slot.ready_to_receive()
    }

    pub fn publish(&self, asked: T) {
        let _ = self.slot.try_send(asked);
    }
}

pub static JOIN: Handoff<JoinRequest> = Handoff::new();

pub struct Latest<T> {
    value: Signal<CriticalSectionRawMutex, T>,
    arrived: Signal<CriticalSectionRawMutex, ()>,
}

impl<T: Copy + Send> Latest<T> {
    const fn new() -> Self {
        Self {
            value: Signal::new(),
            arrived: Signal::new(),
        }
    }

    pub fn take(&self) -> Option<T> {
        self.value.try_take()
    }

    pub fn ready(&self) -> impl Future<Output = ()> {
        self.arrived.wait()
    }

    /// By reference, because the 344-byte list would otherwise be copied into the survey
    /// future's frame, and static memory is the scarce resource here.
    pub fn publish(&self, value: &T) {
        self.value.signal(*value);
        self.arrived.signal(());
    }
}

pub static SCAN: Latest<ApList> = Latest::new();

/// The page the panel is on, for the network task, which cannot reach device state.
pub struct PageWatch {
    page: AtomicU8,
    splash: AtomicU8,
    changed: Signal<CriticalSectionRawMutex, ()>,
}

impl PageWatch {
    const fn new() -> Self {
        Self {
            page: AtomicU8::new(DisplayPage::Ambient as u8),
            splash: AtomicU8::new(0),
            changed: Signal::new(),
        }
    }

    /// Whether the panel is on the page that shows the scan list.
    pub fn shows_list(&self) -> bool {
        self.page.load(Ordering::Relaxed) == DisplayPage::Network as u8
    }

    /// Hand over the page the loop is drawing.
    pub fn publish(&self, page: DisplayPage) {
        if self.page.swap(page as u8, Ordering::Relaxed) != page as u8 {
            self.changed.signal(());
        }
    }

    /// Say the splash is over and whether the button asked for setup mode while it was up.
    pub fn end_splash(&self, setup: bool) {
        self.splash
            .store(if setup { 2 } else { 1 }, Ordering::Relaxed);
        self.changed.signal(());
    }

    /// Resolve once the splash is over, and say whether it was a request for setup mode.
    pub async fn wait_for_splash_end(&self) -> bool {
        loop {
            match self.splash.load(Ordering::Relaxed) {
                0 => self.changed.wait().await,
                ended => return ended == 2,
            }
        }
    }

    /// Resolve when the panel is on the list's page, and say whether it had to wait.
    pub async fn wait_for_list(&self) -> bool {
        let mut waited = false;
        while !self.shows_list() {
            waited = true;
            self.changed.wait().await;
        }
        waited
    }

    /// Resolve when the panel comes *back* to the list's page.
    pub async fn wait_for_return(&self) {
        let mut seen = self.page.load(Ordering::Relaxed);
        loop {
            self.changed.wait().await;
            let page = self.page.load(Ordering::Relaxed);
            if page == seen {
                continue;
            }
            seen = page;
            if page == DisplayPage::Network as u8 {
                return;
            }
        }
    }

    /// Resolve when the panel leaves the list's page.
    pub async fn wait_for_leave(&self) {
        let mut seen = self.page.load(Ordering::Relaxed);
        loop {
            self.changed.wait().await;
            let page = self.page.load(Ordering::Relaxed);
            if page == seen {
                continue;
            }
            seen = page;
            if page != DisplayPage::Network as u8 {
                return;
            }
        }
    }
}

/// Page storage; the render loop publishes into it, the network task reads it.
pub static PAGE: PageWatch = PageWatch::new();

pub struct Outbound {
    pub joins: &'static Handoff<JoinRequest>,
    pub leave: &'static Latch<()>,
    pub rescan: &'static Latch<()>,
    pub setup_ask: &'static Latch<SetupAsk>,
    pub forget: &'static Latch<Ssid>,
}

pub static OUTBOUND: Outbound = Outbound {
    joins: &JOIN,
    leave: &LEAVE,
    rescan: &RESCAN,
    setup_ask: &SETUP_ASK,
    forget: &FORGET,
};

pub struct Inbound {
    pub scan: &'static Latest<ApList>,
    pub page: &'static PageWatch,
}

pub static INBOUND: Inbound = Inbound {
    scan: &SCAN,
    page: &PAGE,
};

struct Entry {
    activity: Activity,
    renderer: Box<dyn Renderer>,
}

/// Render-layer host: central controller and renderer registry.
pub struct Render {
    controller: RenderController,
    renderers: Vec<Entry>,
}

impl Render {
    pub const fn new() -> Self {
        Self {
            controller: RenderController::new(),
            renderers: Vec::new(),
        }
    }

    pub fn register(&mut self, renderer: Box<dyn Renderer>, now_ms: u32) {
        let mut entry = Entry {
            activity: Activity::Idle,
            renderer,
        };
        let slots: Vec<Slot> = entry.renderer.slots().to_vec();
        for slot in slots {
            if let Some(appearance) = self.controller.current(slot) {
                entry.activity = entry.renderer.on_appearance(appearance, now_ms);
            }
        }
        self.renderers.push(entry);
    }

    /// One frame: reconcile `state` against the controller, fan changes out to matching
    /// renderers, then step time-driven animations.
    pub fn tick(&mut self, state: &DeviceState, demand: &EffectiveDemand, now_ms: u32) -> bool {
        let controller = &mut self.controller;
        let entries = &mut self.renderers;
        controller.reconcile(state, demand, |appearance| {
            let slot = appearance.slot();
            for entry in entries.iter_mut() {
                if entry.renderer.slots().contains(&slot) {
                    entry.activity = entry.renderer.on_appearance(appearance.clone(), now_ms);
                }
            }
        });
        let mut any = false;
        for entry in self.renderers.iter_mut() {
            if entry.activity == Activity::TimeDriven {
                entry.activity = entry.renderer.step(now_ms);
            }
            if entry.activity == Activity::TimeDriven {
                any = true;
            }
        }
        any
    }
}

impl Default for Render {
    fn default() -> Self {
        Self::new()
    }
}

/// Renderer for one physical light surface: the WS2812 strip on the DevKitC-1 or the ST7789
/// panel (with its LEDC backlight) on the S3 board.
pub struct LightRenderer<R: RgbLight + DiagnosticsSink> {
    instance: u8,
    light: R,
    slots: [Slot; 2],
    last: Option<Rgb>,
    breath: Option<Breath>,
    appearance: Option<LightAppearance>,
    page: Option<DisplayPage>,
    display: Mode,
}

impl<R: RgbLight + DiagnosticsSink + CameraTarget> LightRenderer<R> {
    pub fn new(instance: u8, light: R) -> Self {
        Self {
            instance,
            light,
            slots: [Slot::Light(instance), Slot::Diagnostics],
            last: None,
            breath: None,
            appearance: None,
            page: None,
            display: Mode::Continuous,
        }
    }

    /// Binds the camera that fills this surface's frame buffer.
    pub fn with_camera(mut self, camera: Box<dyn iot_core::drivers::camera::FrameSource>) -> Self {
        self.light.attach_camera(camera);
        self.page = None;
        self
    }

    fn drive(&mut self, fill: Fill, color: Rgb) {
        if !self.display.is_collecting() {
            self.last = None;
            return;
        }
        let step = self.light.repaint_step();
        let repaint = match self.last {
            None => true,
            Some(last) => should_repaint(last, color, step),
        };
        if repaint {
            self.last = Some(color);
            self.light.set_fill(fill, color);
        }
    }

    fn draw(&mut self, now_ms: u32, breath: Breath) {
        let color = breath_frame(now_ms, breath);
        self.drive(Fill::Uniform, color);
        self.light.set_backlight(backlight_for(now_ms, breath));
    }

    fn page_ground(&self) -> Option<Rgb> {
        self.page.and_then(|page| self.light.page_ground(page))
    }

    fn present(&mut self, now_ms: u32) {
        let Some(declared) = self.appearance else {
            return;
        };
        match self.page_ground() {
            Some(ground) => {
                self.breath = None;
                self.drive(Fill::Uniform, ground);
                self.light.set_backlight(100);
            }
            None => match declared {
                LightAppearance::Off => {
                    self.breath = None;
                    self.drive(Fill::Uniform, Rgb(0, 0, 0));
                    self.light.set_backlight(0);
                }
                LightAppearance::Color(color) => {
                    self.breath = None;
                    self.drive(Fill::Uniform, color);
                    self.light.set_backlight(100);
                }
                LightAppearance::Breathing(breath) => {
                    self.breath = Some(breath);
                    self.draw(now_ms, breath);
                }
            },
        }
    }
}

impl<R: RgbLight + DiagnosticsSink + CameraTarget> Renderer for LightRenderer<R> {
    fn slots(&self) -> &[Slot] {
        &self.slots
    }

    fn on_appearance(&mut self, appearance: SlotAppearance, now_ms: u32) -> Activity {
        match appearance {
            SlotAppearance::Diagnostics(diagnostics) => {
                let page_changed = self.page != Some(diagnostics.page);
                self.light.consume(&diagnostics);
                self.page = Some(diagnostics.page);
                let was_collecting = self.display.is_collecting();
                self.display = diagnostics.demand.claim(DemandKey::DISPLAY).mode;
                if was_collecting != self.display.is_collecting() {
                    self.last = None;
                }
                match self.display {
                    Mode::Off => {
                        self.light.set_backlight_ceiling(0);
                        self.breath = None;
                        return Activity::Idle;
                    }
                    Mode::OnChange => {
                        self.light
                            .set_backlight_ceiling(DeviceManager::BACKLIGHT_FLOOR_PCT);
                    }
                    Mode::Continuous => self.light.set_backlight_ceiling(100),
                }
                if page_changed {
                    self.last = None;
                    self.present(now_ms);
                }
                if self.on_camera_page() {
                    return Activity::TimeDriven;
                }
                if self.breath.is_some() {
                    Activity::TimeDriven
                } else {
                    Activity::Idle
                }
            }
            SlotAppearance::Light {
                instance,
                appearance,
            } => {
                debug_assert_eq!(instance, self.instance);
                self.appearance = Some(appearance);
                self.present(now_ms);
                if self.breath.is_some() {
                    Activity::TimeDriven
                } else {
                    Activity::Idle
                }
            }
        }
    }

    fn step(&mut self, now_ms: u32) -> Activity {
        if self.on_camera_page() {
            self.light.paint_camera(u64::from(now_ms));
            return Activity::TimeDriven;
        }
        match self.breath {
            Some(breath) => {
                self.draw(now_ms, breath);
                Activity::TimeDriven
            }
            None => Activity::Idle,
        }
    }
}

impl<R: RgbLight + DiagnosticsSink + CameraTarget> LightRenderer<R> {
    fn on_camera_page(&self) -> bool {
        self.page
            .is_some_and(|page| self.light.page_draws_frames(page))
    }
}

fn breath_frame(now_ms: u32, breath: Breath) -> Rgb {
    let hue = group_hue(
        now_ms,
        breath.hue_period_ms,
        breath.hue_span,
        breath.hue,
        breath.group,
        breath.group_len,
    );
    let value = smooth_brightness(
        now_ms,
        breath.period_ms,
        breath.min_brightness,
        breath.max_brightness,
    );
    hsv_to_rgb(hue, breath.saturation, value)
}

fn backlight_for(now_ms: u32, breath: Breath) -> u8 {
    backlight_level(
        now_ms,
        breath.period_ms,
        breath.min_brightness,
        breath.max_brightness,
        DeviceManager::BACKLIGHT_FLOOR_PCT,
    )
}

#[allow(clippy::too_many_arguments)]
pub async fn render_loop<C: ClockSink>(
    intent_bus: &'static IntentBus,
    capture: &'static CaptureWindow,
    outbound: &'static Outbound,
    inbound: &'static Inbound,
    playback_bus: &'static PlaybackBus,
    mut render: Render,
    manager: &mut DeviceManager,
    clock: &mut C,
    demand: &SharedDemand,
) -> ! {
    let mut last_page = manager.state().page;
    let mut last_front = manager.state().link.front;
    let mut last_lamp = manager.state().status_light();
    let mut frame = FrameClock::new(Instant::now());
    // This loop's, not a static: what the chip was last asked for belongs to the one
    // loop that asks it, so a second `run` in one process does not answer for the first.
    let mut governor = ClockGovernor::new();
    let mut playback = PlaybackDispatch::new(manager.playback_state());
    let mut bench = Box::new(crate::panel::PanelBench::new());
    loop {
        while let Ok(intent) = intent_bus.try_receive() {
            match intent {
                Intent::Operation(op) => {
                    manager.apply_operation(op);
                    let business = iot_core::intent::translate(&op, &manager.state());
                    manager.apply_business(business);
                }
                Intent::Business(business) => manager.apply_business(business),
            }
            if manager.state().page != last_page {
                log::info!("[INPUT] page is now {:?}", manager.state().page);
                last_page = manager.state().page;
            }
            playback.dispatch(playback_bus, manager.playback_state());
        }
        if manager.state().link.front != last_front {
            last_front = manager.state().link.front;
            log::info!(
                "[WIFI] setup mode is offered on the {:?} front",
                manager.state().link.front
            );
        }
        let lamp = manager.state().status_light();
        if lamp != last_lamp {
            log::info!("{}", LampLine(lamp));
            last_lamp = lamp;
        }
        inbound.page.publish(manager.state().page);
        if let Some(setup) = manager.take_splash_end() {
            log::info!(
                "[INPUT] the splash is over; setup mode {}",
                if setup {
                    "was asked for"
                } else {
                    "was not asked for"
                }
            );
            inbound.page.end_splash(setup);
        }
        bench.step(manager, intent_bus);
        if let Some(sample) = capture.take() {
            manager.apply_capture(sample);
        }
        if let Some(list) = inbound.scan.take() {
            manager.apply_scan(list);
        }
        let asked_on = manager.state().link.front;
        while let Some(command) = manager.take_command() {
            match command {
                NetworkCommand::Join(asked) => {
                    match asked {
                        JoinRequest::Typed(credentials) => log::info!(
                            "[WIFI] the panel named a network: {:?}",
                            credentials
                                .ssid
                                .as_display_text()
                                .unwrap_or("<unprintable>"),
                        ),
                        JoinRequest::Held(ssid) => log::info!(
                            "[WIFI] the panel asked for the copy this device keeps of {:?}",
                            ssid.as_display_text().unwrap_or("<unprintable>"),
                        ),
                    }
                    outbound.joins.publish(asked);
                }
                NetworkCommand::Leave => {
                    log::info!("[WIFI] leaving the link to offer the setup access point");
                    outbound.leave.set(());
                }
                NetworkCommand::LeaveSetup => {
                    log::info!("[WIFI] the panel asked to close setup mode");
                    outbound.setup_ask.set(SetupAsk::Cancel { from: asked_on });
                }
                NetworkCommand::SwitchFront => {
                    log::info!("[WIFI] the panel asked for setup mode's other front end");
                    outbound.setup_ask.set(SetupAsk::Front { from: asked_on });
                }
                NetworkCommand::SetupOpen(open) => {
                    log::info!(
                        "[WIFI] the panel asked for the setup access point to be {}",
                        if open { "open" } else { "under a passphrase" },
                    );
                    outbound.setup_ask.set(SetupAsk::Pass {
                        from: asked_on,
                        open,
                    });
                }
                NetworkCommand::ScanNow => {
                    log::info!("[WIFI] the panel asked to look around now");
                    outbound.rescan.set(());
                }
                NetworkCommand::Forget(ssid) => {
                    log::info!(
                        "[WIFI] the panel asked to forget {:?}",
                        ssid.as_display_text().unwrap_or("<unprintable>"),
                    );
                    outbound.forget.set(ssid);
                }
            }
        }
        // The lamp's news is stepped on the wall clock, not the frame counter. The counter is a
        // logical animation clock: after another future holds the executor (the radio's
        // association does, for a few hundred milliseconds), the deadline below is in the past and
        // the loop replays the skipped steps at full speed to catch up. An announcement measured
        // in frames would then be over before it was seen — measured on the C6, three 100 ms
        // flashes collapsed into 19 ms right after an association. The renderers keep the counter;
        // a flash is a duration the user's eye measures.
        manager.tick_notice(Instant::now().as_millis() as u32);
        let state = manager.state();
        demand.lock(|current| {
            let next = manager.demand();
            if &*current.borrow() != next {
                *current.borrow_mut() = next.clone();
            }
        });
        if let Some(applied) = governor.apply(manager.demand(), clock)
            && let Some(short) = applied.undercuts
        {
            log::warn!(
                "[CLOCK] a page asked for {:?}, but {} is running and needs {:?} \
                 — applying the page's choice",
                applied.asked,
                short.key,
                short.needed,
            );
        }
        let active =
            render.tick(&state, manager.demand(), frame.elapsed_ms()) || manager.notice_active();
        match frame.tick(active) {
            Wait::At(deadline) => Timer::at(deadline).await,
            Wait::Park => {
                select3(
                    intent_bus.ready_to_receive(),
                    capture.ready(),
                    inbound.scan.ready(),
                )
                .await;
                frame.reanchor(Instant::now());
            }
        }
    }
}

struct PlaybackDispatch {
    phase: PlaybackPhase,
    muted: bool,
}

impl PlaybackDispatch {
    fn new(state: PlaybackState) -> Self {
        Self {
            phase: state.phase,
            muted: state.muted,
        }
    }

    fn dispatch(&mut self, bus: &PlaybackBus, after: PlaybackState) {
        if after.muted != self.muted && queue(bus, PlaybackCommand::SetMuted(after.muted)) {
            self.muted = after.muted;
        }
        match after.phase {
            PlaybackPhase::Idle => self.phase = PlaybackPhase::Idle,
            PlaybackPhase::Playing if self.phase != PlaybackPhase::Playing => {
                if queue(bus, PlaybackCommand::Play(after.sound)) {
                    self.phase = PlaybackPhase::Playing;
                }
            }
            PlaybackPhase::Playing => {}
        }
    }
}

fn queue(bus: &PlaybackBus, command: PlaybackCommand) -> bool {
    match bus.try_send(command) {
        Ok(()) => true,
        Err(TrySendError::Full(command)) => {
            log::error!("[PLAY] {command:?} had no room, asking again");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::future::Future;
    use core::task::{Context, Waker};
    use iot_core::diagnostics::Diagnostics;
    use iot_core::drivers::camera::FrameSource;
    use iot_core::drivers::network::AccessPoint;
    use iot_core::render::{SlotAppearance, light_appearance};
    use iot_core::state::LightState;

    struct FakePanel {
        fills: u32,
        color: Rgb,
        backlight: u8,
        ceiling: u8,
        pages: bool,
    }

    impl FakePanel {
        fn new() -> Self {
            Self {
                fills: 0,
                color: Rgb(0, 0, 0),
                backlight: 100,
                ceiling: 100,
                pages: false,
            }
        }

        fn panel() -> Self {
            Self {
                pages: true,
                ..Self::new()
            }
        }
    }

    impl RgbLight for FakePanel {
        fn set_fill(&mut self, _fill: Fill, color: Rgb) {
            self.fills += 1;
            self.color = color;
        }

        fn set_backlight(&mut self, level_pct: u8) {
            self.backlight = level_pct.min(self.ceiling);
        }

        fn set_backlight_ceiling(&mut self, level_pct: u8) {
            self.ceiling = level_pct;
            self.backlight = self.backlight.min(level_pct);
        }

        fn page_ground(&self, page: DisplayPage) -> Option<Rgb> {
            (self.pages && matches!(page, DisplayPage::Network | DisplayPage::Camera))
                .then_some(Rgb(0, 0, 0))
        }

        fn page_draws_frames(&self, page: DisplayPage) -> bool {
            self.pages && page == DisplayPage::Camera
        }
    }

    impl DiagnosticsSink for FakePanel {
        fn consume(&mut self, _diagnostics: &Diagnostics) {}

        fn draws_pages(&self) -> bool {
            self.pages
        }
    }

    impl CameraTarget for FakePanel {
        fn paint_camera(&mut self, _now_ms: u64) -> bool {
            false
        }

        fn attach_camera(&mut self, _camera: Box<dyn FrameSource>) {}
    }

    fn panel_asked(display: Mode) -> SlotAppearance {
        panel_on(DisplayPage::Ambient, display)
    }

    fn panel_on(page: DisplayPage, display: Mode) -> SlotAppearance {
        let mut demand = EffectiveDemand::default();
        demand.set(
            DemandKey::DISPLAY,
            iot_core::demand::Claim::new(display, 0, iot_core::demand::Sensitivity::Primary),
        );
        SlotAppearance::Diagnostics(Diagnostics {
            demand,
            page,
            ..Diagnostics::default()
        })
    }

    fn ambient_breath(instance: u8) -> SlotAppearance {
        SlotAppearance::Light {
            instance,
            appearance: light_appearance(LightState::boot()),
        }
    }

    fn renderer(panel: FakePanel) -> LightRenderer<FakePanel> {
        LightRenderer::new(0, panel)
    }

    #[test]
    fn a_lit_panel_keeps_breathing_on_a_page_that_never_settles() {
        let mut r = renderer(FakePanel::new());
        assert_eq!(
            r.on_appearance(panel_asked(Mode::Continuous), 0),
            Activity::Idle,
            "the first appearance only tells the panel what it is for"
        );
        assert_eq!(r.on_appearance(ambient_breath(0), 0), Activity::TimeDriven);
        r.step(1_000);
        r.step(1_100);
        assert!(r.light.fills > 0, "a lit panel keeps painting");
    }

    #[test]
    fn an_unlit_panel_ships_nothing_from_any_slot() {
        let mut r = renderer(FakePanel::new());
        r.on_appearance(panel_asked(Mode::Off), 0);
        assert_eq!(r.light.ceiling, 0, "and its backlight is off");
        let fills_before = r.light.fills;

        r.on_appearance(ambient_breath(0), 0);
        r.step(1_000);
        r.step(1_100);
        r.on_appearance(
            SlotAppearance::Light {
                instance: 0,
                appearance: LightAppearance::Color(Rgb(255, 0, 0)),
            },
            1_200,
        );

        assert_eq!(
            r.light.fills, fills_before,
            "the breathing loop and the colour change both leave through the same gate"
        );
        assert_eq!(r.light.backlight, 0);
    }

    #[test]
    fn a_panel_relit_after_being_dark_paints_the_frame_the_darkness_swallowed() {
        let mut r = renderer(FakePanel::new());
        r.on_appearance(panel_asked(Mode::Off), 0);
        r.on_appearance(ambient_breath(0), 0);
        r.step(1_000);
        assert_eq!(r.light.fills, 0);

        r.on_appearance(panel_asked(Mode::Continuous), 2_000);
        r.step(2_000);
        assert_eq!(
            r.light.fills, 1,
            "the first frame after relighting is the one that matters"
        );
        assert_eq!(r.light.ceiling, 100);
    }

    #[test]
    fn a_dimmed_panel_is_brightness_only_and_keeps_its_cadence() {
        let mut r = renderer(FakePanel::new());
        r.on_appearance(panel_asked(Mode::OnChange), 0);
        assert_eq!(r.light.ceiling, DeviceManager::BACKLIGHT_FLOOR_PCT);

        r.on_appearance(ambient_breath(0), 0);
        assert_eq!(
            r.on_appearance(ambient_breath(0), 0),
            Activity::TimeDriven,
            "dimming is a brightness, not a rate"
        );
        r.step(1_000);
        assert!(r.light.fills > 0);
    }

    #[test]
    fn a_setup_request_only_answers_the_front_end_it_was_made_on() {
        for ask in [
            SetupAsk::Front { from: Front::Panel },
            SetupAsk::Cancel { from: Front::Panel },
            SetupAsk::Pass {
                from: Front::Panel,
                open: true,
            },
        ] {
            assert!(
                ask.applies_to(Front::Panel),
                "{ask:?} was asked on the panel"
            );
            assert!(
                !ask.applies_to(Front::AccessPoint),
                "{ask:?} is not an answer the access point's front end asked for"
            );
        }
        for ask in [
            SetupAsk::Front {
                from: Front::AccessPoint,
            },
            SetupAsk::Cancel {
                from: Front::AccessPoint,
            },
            SetupAsk::Pass {
                from: Front::AccessPoint,
                open: false,
            },
        ] {
            assert!(
                ask.applies_to(Front::AccessPoint),
                "{ask:?} was asked on the phone"
            );
            assert!(
                !ask.applies_to(Front::Panel),
                "{ask:?} is not an answer the panel's front end asked for"
            );
        }
    }

    #[test]
    fn the_network_page_grounds_the_panel_black_and_still() {
        let mut r = renderer(FakePanel::panel());
        r.on_appearance(panel_on(DisplayPage::Network, Mode::Continuous), 0);
        assert_eq!(
            r.on_appearance(ambient_breath(0), 0),
            Activity::Idle,
            "a breathing light does not breathe through the page's ground"
        );
        assert_eq!(r.light.color, Rgb(0, 0, 0), "the ground is black");
        assert_eq!(
            r.light.backlight, 100,
            "and the setup screen is lit to read"
        );
        let fills = r.light.fills;
        assert_eq!(r.step(1_000), Activity::Idle);
        r.step(1_100);
        assert_eq!(
            r.light.fills, fills,
            "and the loop parks instead of repainting"
        );
        assert_eq!(
            r.light.backlight, 100,
            "the backlight holds instead of ramping"
        );

        r.on_appearance(
            SlotAppearance::Light {
                instance: 0,
                appearance: LightAppearance::Off,
            },
            1_200,
        );
        assert_eq!(r.light.color, Rgb(0, 0, 0));
        assert_eq!(r.light.backlight, 100);
    }

    #[test]
    fn the_camera_page_grounds_the_panel_black() {
        let mut r = renderer(FakePanel::panel());
        r.on_appearance(panel_on(DisplayPage::Ambient, Mode::Continuous), 0);
        r.on_appearance(
            SlotAppearance::Light {
                instance: 0,
                appearance: LightAppearance::Color(Rgb(10, 20, 30)),
            },
            0,
        );
        assert_eq!(r.light.color, Rgb(10, 20, 30));

        assert_eq!(
            r.on_appearance(panel_on(DisplayPage::Camera, Mode::Continuous), 1),
            Activity::TimeDriven,
            "a camera page is driven by arriving frames, not by the light"
        );
        assert_eq!(
            r.light.color,
            Rgb(0, 0, 0),
            "the camera page's ground is the page's black, not the light's colour"
        );
        assert_eq!(r.light.backlight, 100);
    }

    #[test]
    fn leaving_the_page_that_grounds_the_panel_puts_the_light_back() {
        let mut r = renderer(FakePanel::panel());
        r.on_appearance(panel_on(DisplayPage::Network, Mode::Continuous), 0);
        r.on_appearance(ambient_breath(0), 0);
        assert_eq!(r.light.color, Rgb(0, 0, 0));

        assert_eq!(
            r.on_appearance(panel_on(DisplayPage::Ambient, Mode::Continuous), 100),
            Activity::TimeDriven,
            "the breathing the page was hiding resumes"
        );
        let LightAppearance::Breathing(breath) = light_appearance(LightState::boot()) else {
            panic!("the boot light breathes");
        };
        assert_eq!(r.light.color, breath_frame(100, breath));
    }

    #[test]
    fn a_surface_that_draws_no_page_keeps_showing_the_light() {
        let mut r = renderer(FakePanel::new());
        r.on_appearance(panel_on(DisplayPage::Network, Mode::Continuous), 0);
        assert_eq!(r.on_appearance(ambient_breath(0), 0), Activity::TimeDriven);
        assert!(
            r.light.fills > 0,
            "the light is still what this surface shows"
        );
    }

    fn cx() -> Context<'static> {
        Context::from_waker(Waker::noop())
    }

    fn round_list(saved: bool) -> ApList {
        let mut list = ApList::from_ranked(&[AccessPoint {
            ssid: Ssid::from_octets(b"target"),
            bssid: [7; 6],
            channel: 11,
            rssi: -50,
            secured: true,
        }]);
        list.mark_saved(u8::from(saved));
        list
    }

    #[test]
    fn a_list_published_before_the_last_one_is_taken_replaces_it_rather_than_being_refused() {
        let latest = Latest::new();
        latest.publish(&round_list(true));
        latest.publish(&round_list(false));

        let list = latest.take().expect("a list is waiting");
        assert!(
            !list.is_saved(0),
            "the newest list is the one the page is handed, and the mark is off it"
        );
        assert!(latest.take().is_none(), "and it is handed over once");
    }

    #[test]
    fn the_wake_a_park_waits_on_does_not_take_the_list_it_woke_for() {
        let latest = Latest::new();
        latest.publish(&round_list(true));
        let mut wake = core::pin::pin!(latest.ready());
        assert!(
            wake.as_mut().poll(&mut cx()).is_ready(),
            "the wake is up as soon as a list is waiting"
        );
        assert!(
            latest.take().is_some(),
            "the take at the top of the pass is the one that takes the list"
        );
    }

    #[test]
    fn a_rest_on_the_list_is_not_cut_short_by_the_list_being_up() {
        // The board read this as a survey every 790 ms against a 5,000 ms pause: the pause was
        // waiting on `wait_for_list`, which is true the moment it is asked on the page the pause is
        // spent on. The two futures are asked side by side here so the difference is the test.
        let watch = PageWatch::new();
        watch.publish(DisplayPage::Network);
        let mut level = core::pin::pin!(watch.wait_for_list());
        let mut back = core::pin::pin!(watch.wait_for_return());
        assert!(
            level.as_mut().poll(&mut cx()).is_ready(),
            "the level check answers \"is the list up?\", and it is"
        );
        assert!(
            back.as_mut().poll(&mut cx()).is_pending(),
            "a return cannot be satisfied by never having left"
        );

        watch.publish(DisplayPage::Ambient);
        assert!(back.as_mut().poll(&mut cx()).is_pending());
        watch.publish(DisplayPage::Network);
        assert!(
            back.as_mut().poll(&mut cx()).is_ready(),
            "away and back is the return the pause waits on"
        );
    }
}
