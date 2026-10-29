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

/// Survey-request storage. Not cleared with the leave: a rescan belongs to the radio, not the link.
pub static RESCAN: Latch<()> = Latch::new();

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SetupAsk {
    /// The strip's left cell: close the mode and go back to the network this device keeps.
    ///
    /// The other direction of [`LEAVE`], and read by the other loop: a leave is acted on by the
    /// link that is up, a cancel by the access point that is.
    Cancel { from: Front },
    /// The strip's right cell: offer the access point instead of the list, or the other way round.
    Front { from: Front },
    /// The box's other answer: put the access point up open (`true`) or back under a passphrase
    /// (`false`). The target travels so a re-read cannot reverse a request already answered.
    Pass { from: Front, open: bool },
}

impl SetupAsk {
    /// Whether the front now holding the mode is the one this request was made on. A `Signal`
    /// keeps an unread request for the next reader, and the reader after a switch is the other
    /// front; without this the box would open and close at once.
    pub fn applies_to(self, front: Front) -> bool {
        let from = match self {
            Self::Cancel { from } | Self::Front { from } | Self::Pass { from, .. } => from,
        };
        from == front
    }
}

/// Setup-request storage; the render loop publishes into it, the network task takes.
///
/// Cleared when the mode is entered, so a request that outlived the setup it was about cannot take
/// down the next one.
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

/// The page the panel is on, for the network task, which cannot reach device state. A value and a
/// wake rather than a message: the reader asks whether the list is up. The render loop writes, the
/// network task reads; the same field says how the splash ended.
pub struct PageWatch {
    page: AtomicU8,
    /// `0` while the splash is up, `1` once it ended without a request, `2` with one.
    splash: AtomicU8,
    changed: Signal<CriticalSectionRawMutex, ()>,
}

impl PageWatch {
    const fn new() -> Self {
        Self {
            // The page the state itself boots on. The render loop publishes before any reader
            // could have a reason to ask, and a *down* page is the safe way for this to be wrong.
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
    ///
    /// A page that has not changed is not news, so the wake is not signalled: the render loop
    /// publishes on every pass, and a wake per pass would be a task woken twenty times a second to
    /// find nothing to do.
    pub fn publish(&self, page: DisplayPage) {
        if self.page.swap(page as u8, Ordering::Relaxed) != page as u8 {
            self.changed.signal(());
        }
    }

    /// Say the splash is over and whether the button asked for setup mode while it was up.
    ///
    /// One value rather than a flag for each half, because the landing and the request are one
    /// decision: the network task starts setup mode instead of joining anything on the strength of
    /// this, and a reader that saw the landing before its request would join a network the person
    /// had just asked to configure.
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
    ///
    /// The answer is what starts the survey's idle clock over: arriving at the page is an act, and
    /// the list a user is shown on arrival is the one that has to be fresh.
    pub async fn wait_for_list(&self) -> bool {
        let mut waited = false;
        while !self.shows_list() {
            waited = true;
            self.changed.wait().await;
        }
        waited
    }

    /// Resolve when the panel comes *back* to the list's page. [`wait_for_list`](Self::wait_for_list)
    /// answers even when already true, which as an interrupt makes the rest no rest at all; this
    /// waits for the page to change.
    pub async fn wait_for_return(&self) {
        let mut seen = self.page.load(Ordering::Relaxed);
        loop {
            self.changed.wait().await;
            let page = self.page.load(Ordering::Relaxed);
            if page == seen {
                // A signal that was already pending when this was called, or a change that landed
                // back where it started: neither is a return.
                continue;
            }
            seen = page;
            if page == DisplayPage::Network as u8 {
                return;
            }
        }
    }

    /// Resolve when the panel leaves the list's page.
    ///
    /// The mirror of [`wait_for_return`](Self::wait_for_return), and for the caller that is
    /// browsing rather than resting: a page nobody is on is not one to scan for, so the survey ends
    /// with it and starts again when it comes back.
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
        // Catch a fresh renderer up with the surface before the next diff:
        // every slot it subscribes to is synced in one shot.
        let slots: Vec<Slot> = entry.renderer.slots().to_vec();
        for slot in slots {
            if let Some(appearance) = self.controller.current(slot) {
                entry.activity = entry.renderer.on_appearance(appearance, now_ms);
            }
        }
        self.renderers.push(entry);
    }

    /// One frame: reconcile `state` against the controller, fan changes out to
    /// matching renderers, then step time-driven animations. Returns `true`
    /// while any renderer still animates.
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

/// Renderer for one physical light surface: the WS2812 strip on the DevKitC-1 or the
/// ST7789 panel (with its LEDC backlight) on the S3 board. Breathing is time-driven:
/// the render layer steps it every tick to produce frames from the schedule. A surface
/// subscribes to its own light slot plus the device-level diagnostics, forwarding the
/// snapshot to the light bus it sinks.
///
/// The panel also implements [`CameraTarget`], and on the Camera page the camera draws
/// instead of the light — see [`Self::step`].
pub struct LightRenderer<R: RgbLight + DiagnosticsSink> {
    instance: u8,
    light: R,
    slots: [Slot; 2],
    last: Option<Rgb>,
    breath: Option<Breath>,
    /// The appearance the light declared, kept so a page change can put it back: the page is
    /// what decides whether the light is drawn at all (see [`Self::present`]), and a page that
    /// stopped showing it is not a light that moved.
    appearance: Option<LightAppearance>,
    /// The page the surface was last told about. `step` gets no appearance, so it reads the
    /// camera's turn off this rather than being handed the snapshot a second time.
    page: Option<DisplayPage>,
    /// What the build and the page ask of the panel itself, as opposed to of what the
    /// page draws. Kept here because `step` gets no appearance, and a parked loop is
    /// exactly the case that needs the answer.
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
    ///
    /// Taken by value and erased to [`CameraTarget`], so the renderer names a shape and not a
    /// sensor — the same reason the light arrives as [`RgbLight`].
    pub fn with_camera(mut self, camera: Box<dyn iot_core::drivers::camera::FrameSource>) -> Self {
        self.light.attach_camera(camera);
        self.page = None;
        self
    }

    fn drive(&mut self, fill: Fill, color: Rgb) {
        // One place every frame leaves through, so one place an unlit panel stops them.
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

    /// Paint one breathing frame and the matching backlight level. `drive` only
    /// rewrites panel RAM when the color moved at least `repaint_step`, but the
    /// backlight tracks the envelope on every tick so the PWM ramps smoothly.
    fn draw(&mut self, now_ms: u32, breath: Breath) {
        let color = breath_frame(now_ms, breath);
        self.drive(Fill::Uniform, color);
        self.light.set_backlight(backlight_for(now_ms, breath));
    }

    /// The ground a page draws for itself, on the surfaces that draw pages: the Network page is
    /// a setup screen — a list, a keyboard, a status line — and the fill behind it is that
    /// screen's black, at full backlight and holding still. The Camera page's ground is that
    /// same black, so its starting readout has a paper before the first frame; once a frame
    /// arrives the camera fills the whole panel and the readout is stamped over it. The light
    /// surface sharing the panel is that fill, so on these pages it is neither coloured nor
    /// breathing.
    ///
    /// Asked of the page rather than derived in the state, because the light is not what
    /// changed: it is the same light it was, and it comes back exactly as it was when the page
    /// does — see [`Self::present`] and the diagnostics arm of [`Renderer::on_appearance`].
    fn page_ground(&self) -> Option<Rgb> {
        self.page.and_then(|page| self.light.page_ground(page))
    }

    /// Shows the appearance the light declared — or, on a page that draws its own ground, the
    /// page's instead. The one place an appearance becomes fills and backlight levels, so the
    /// page rule has one place to live.
    fn present(&mut self, now_ms: u32) {
        let Some(declared) = self.appearance else {
            // No appearance has arrived yet: the surface keeps what bring-up gave it rather
            // than being driven to a state nothing declared.
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
            // A diagnostics bump must not disturb the animation cadence: keep the
            // breathing activity, and let panel surfaces repaint the digits.
            SlotAppearance::Diagnostics(diagnostics) => {
                let page_changed = self.page != Some(diagnostics.page);
                self.light.consume(&diagnostics);
                self.page = Some(diagnostics.page);
                let was_collecting = self.display.is_collecting();
                self.display = diagnostics.demand.claim(DemandKey::DISPLAY).mode;
                if was_collecting != self.display.is_collecting() {
                    // The lit/dark crossing cannot wait for a colour to move: the mode
                    // changes the backlight, and nothing else would ask for that frame.
                    self.last = None;
                }
                match self.display {
                    Mode::Off => {
                        // Nothing on screen is wanted, so the loop parks: the ceiling below
                        // darkens the panel, the park stops spending on frames nobody sees.
                        self.light.set_backlight_ceiling(0);
                        self.breath = None;
                        return Activity::Idle;
                    }
                    Mode::OnChange => {
                        // Lit but at the floor, and still on the animation clock — dimming
                        // the panel is not a reason to stop breathing.
                        self.light
                            .set_backlight_ceiling(DeviceManager::BACKLIGHT_FLOOR_PCT);
                    }
                    Mode::Continuous => self.light.set_backlight_ceiling(100),
                }
                // A new page starts from a buffer neither the camera nor the previous fill
                // owns, so the repaint gate below has nothing to compare against and would
                // skip the one frame that replaces it. Without this, leaving the camera page
                // is only repainted when the breathing colour happens to have moved far
                // enough by the next tick.
                if page_changed {
                    self.last = None;
                    // And the page is what decides whether the light is drawn at all, so the
                    // appearance is shown again: leaving a page that drew its own ground has to
                    // put the light back, and nothing else would ask for it — the light did
                    // not move, the page did.
                    self.present(now_ms);
                }
                if self.on_camera_page() {
                    // The camera page is time-driven by the frame arriving, which this diff
                    // cannot see — the panel is always breathing from boot, so the render loop
                    // would otherwise park and the chain would never be polled again.
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
        // The camera page draws the sensor's picture and the light draws nothing, but the
        // poll inside this call is what repairs the chain — so it has to run on every tick
        // rather than when a frame is due. It stays time-driven either way: a page whose
        // render is an arriving frame has to be stepped to see the frame arrive.
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
    // The front end setup mode is offered on, as the last drain left it. Logged because it is what
    // the strip and the box are drawn from: a publish that moved it would be a control changing
    // under the finger, which is invisible in any other line.
    let mut last_front = manager.state().link.front;
    // The lamp's last frame, for the same reason as the page and the front: a light-only board's
    // whole language is in these transitions, and a WS2812 cannot be read back.
    let mut last_lamp = manager.state().status_light();
    let mut frame = FrameClock::new(Instant::now());
    // This loop's, not a static: what the chip was last asked for belongs to the one
    // loop that asks it, so a second `run` in one process does not answer for the first.
    let mut governor = ClockGovernor::new();
    let mut playback = PlaybackDispatch::new(manager.playback_state());
    // Boxed, though it is one value: the bench's own state is a field of this loop's future, and
    // the loop's future is the largest thing on the one stack the product has to fit the radio,
    // the panel and the audio into. Behind a pointer the future pays four bytes for a hand it
    // never uses, and the bench's own bytes live on the heap it already allocates from.
    let mut bench = Box::new(crate::panel::PanelBench::new());
    loop {
        while let Ok(intent) = intent_bus.try_receive() {
            match intent {
                Intent::Operation(op) => {
                    manager.apply_operation(op);
                    let business = iot_core::intent::translate(&op, &manager.state());
                    manager.apply_business(business);
                }
                // A producer that already speaks business (e.g. a network
                // drive) goes straight to the state.
                Intent::Business(business) => manager.apply_business(business),
            }
            // A page change, one line, whoever caused it: the cost report names a page once per
            // window, which cannot show whether a gesture advanced one page or three. Outside
            // the match because the splash's end arrives as a business intent, and a check that
            // lived in the operation arm reported that transition against the *next* tap
            // instead — a page named twenty seconds after it had already been drawn.
            if manager.state().page != last_page {
                log::info!("[INPUT] page is now {:?}", manager.state().page);
                last_page = manager.state().page;
            }
            // What travels to the driver is the diff against what the driver
            // has been told, and not the intent: the state layer is what decides
            // a tap was heard, so a command built from the intent alone would
            // replay a sound it just dropped.
            playback.dispatch(playback_bus, manager.playback_state());
        }
        if manager.state().link.front != last_front {
            last_front = manager.state().link.front;
            log::info!(
                "[WIFI] setup mode is offered on the {:?} front",
                manager.state().link.front
            );
        }
        // The lamp is the only surface a board without a screen has, and nothing on the bench can
        // read a WS2812 back: log the frames it is handed, transitions only, so a flash is a
        // countable pair of lines rather than a claim about what the eye should have seen.
        let lamp = manager.state().status_light();
        if lamp != last_lamp {
            log::info!("{}", LampLine(lamp));
            last_lamp = lamp;
        }
        // The page, for the one reader that cannot see the state: the network task asks whether
        // the list it would survey is on the panel at all. Published after the intents, so a page
        // this pass changed is the one the survey hears about.
        inbound.page.publish(manager.state().page);
        // And once, the one thing the network task cannot read for itself: whether the page that
        // just ended was a request for setup mode. After the intents, so a hold and the clock's
        // ending that landed in the same pass are one answer rather than two.
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
        // A bench build's own hand on the glass, which is nothing at all in any other build: the
        // taps it publishes are read by the next pass, exactly as a tap from the touch driver is.
        bench.step(manager, intent_bus);
        if let Some(sample) = capture.take() {
            manager.apply_capture(sample);
        }
        // The link's list the same way, and after the intents for the same reason: a list that
        // landed in this tick is drawn by this pass rather than the next.
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
        // The page is the product's declaration of what it wants, so the demand follows it
        // here rather than each consumer deriving its own. The polling loop reads
        // whatever is current on its next tick. The configurator is not called here — it
        // needs the board's capabilities and answers against the part that refused, so
        // it sits beside the parts.
        demand.lock(|current| {
            let next = manager.demand();
            if &*current.borrow() != next {
                *current.borrow_mut() = next.clone();
            }
        });
        // The governor returns what it did; the wording and the channel are this layer's.
        // The clock itself is reported by the part that moves it, in MHz rather than preset:
        // `Speed` is this layer's vocabulary, not a frequency anyone can check.
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
                // A wake is readiness, not a receive: nothing pops, so the drain at the top of
                // the next pass still sees the message.
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

/// What changed between two playback states, as the commands that would carry
/// that change to the driver.
///
/// A diff rather than a translation because the state layer owns both rules: it
/// knows a tap over a sounding one was dropped, and a command built from the
/// intent would not. The finish needs no command — it is the driver reporting
/// back, not the state asking — so it is not here.
struct PlaybackDispatch {
    phase: PlaybackPhase,
    muted: bool,
}

impl PlaybackDispatch {
    /// Starts from what the state already says, which on a board that has just
    /// come up is a speaker nobody has asked to play anything on.
    fn new(state: PlaybackState) -> Self {
        Self {
            phase: state.phase,
            muted: state.muted,
        }
    }

    /// Carries the difference to the driver, and reports nothing.
    ///
    /// A diff is taken against **what the driver has been told**, not against the
    /// previous intent's state, because a full bus is the one case where those come
    /// apart. A play dropped instead of queued would leave the page in `Playing`
    /// with nothing that can ever finish it, so a change that does not fit stays
    /// pending and is asked for again on the next pass.
    fn dispatch(&mut self, bus: &PlaybackBus, after: PlaybackState) {
        if after.muted != self.muted && queue(bus, PlaybackCommand::SetMuted(after.muted)) {
            self.muted = after.muted;
        }
        match after.phase {
            // The finish is the driver's word and the driver is the only thing
            // that has it, so this record only ever follows the state out of
            // `Playing`; starting a sound is this side's to ask for.
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

/// Queues a command, reporting whether it went in. The bus is never waited on:
/// the render loop is the reader of the intent bus, and a `Play` that blocked
/// here while the playback task was blocked reporting a finish into a full
/// intent bus would be a cycle with no way out of it.
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

    /// Counts what reached the panel rather than what was asked of it.
    struct FakePanel {
        fills: u32,
        color: Rgb,
        backlight: u8,
        ceiling: u8,
        /// Whether this surface draws the device's pages, as the real panel does and an
        /// addressable LED does not.
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

        /// The panel: the surface the product's pages are drawn on.
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

    /// The diagnostics for a page a surface is being told about.
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

    /// The Ambient page breathes forever, so it proves the gate is on the panel rather
    /// than on the diagnostics slot.
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

        // The gate clears `last`, so relighting cannot inherit an undrawn colour and skip
        // the frame that shows the panel is back.
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

    /// The Network page's own ground: this surface is the fill behind the setup screen, so the
    /// light is not what it shows.
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

        // The page is what is on the panel: a light switched off does not take the screen with
        // it, or the page could not be read to switch it back on.
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

        // The light never moved; the page did. Nothing in the appearance diff would ask for the
        // colour back, so the page change is what has to.
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
        // The page rule is asked of the surface: an addressable LED has no screen to put a
        // setup list on, so a page it cannot draw must not darken the one output it has.
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

    /// The one-row list a round publishes, marked or not as the store would have it.
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
        // The loop parks on the wake and takes the list at the top of the pass after it, so a wake
        // that took the list would leave that pass nothing to draw — and nothing would publish it a
        // second time.
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

        // Away alone is not a return either — the loop has to run on the panel being on the list.
        watch.publish(DisplayPage::Ambient);
        assert!(back.as_mut().poll(&mut cx()).is_pending());
        watch.publish(DisplayPage::Network);
        assert!(
            back.as_mut().poll(&mut cx()).is_ready(),
            "away and back is the return the pause waits on"
        );
    }
}
