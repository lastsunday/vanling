use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::SharedDemand;
use crate::input::IntentBus;
use crate::playback::{PlaybackBus, PlaybackCommand};
use embassy_futures::select::select;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, TrySendError};
use embassy_time::{Duration, Instant, Timer};
use iot_core::demand::ClockGovernor;
use iot_core::demand::ClockSink;
use iot_core::demand::{EffectiveDemand, Mode, PlaneKey};
use iot_core::diagnostics::DiagnosticsSink;
use iot_core::drivers::camera::CameraTarget;
use iot_core::drivers::light::{
    Fill, Rgb, RgbLight, backlight_level, group_hue, hsv_to_rgb, should_repaint, smooth_brightness,
};
use iot_core::intent::Intent;
use iot_core::render::{
    Activity, LightAppearance, RenderController, Renderer, Slot, SlotAppearance,
};
use iot_core::state::{
    Breath, DeviceManager, DeviceState, DisplayPage, PlaybackPhase, PlaybackState,
};

/// Frame cadence of the render loop.
pub const STEP_MS: u32 = 20;

/// Capacity of the cross-task render bus.
pub const RENDER_BUS_CAPACITY: usize = 8;

/// Cross-task render messages: renderers register when their resource exists and
/// unregister when it disappears. The render task is the single owner draining
/// this bus.
pub enum RenderMsg {
    /// Cross-task renderers must be `Send`; in-task boot renderers (the light)
    /// register directly with [`Render::register`].
    Register(Box<dyn Renderer + Send>),
    Unregister(RenderToken),
}

/// Opaque handle to a registered renderer, assigned by the render layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderToken(pub u64);

/// Cross-task render channel type. The render task consumes it via an explicit
/// argument (never a hidden global); producers on other tasks send to
/// [`RENDER_BUS`].
pub type RenderBus = Channel<CriticalSectionRawMutex, RenderMsg, RENDER_BUS_CAPACITY>;

/// Single shared render bus: storage for the cross-task renderer registry and
/// the producers' global entry point.
pub static RENDER_BUS: RenderBus = RenderBus::new();

struct Entry {
    token: RenderToken,
    activity: Activity,
    renderer: Box<dyn Renderer>,
}

/// Render-layer host: central controller, renderer registry, cross-task bus.
pub struct Render {
    controller: RenderController,
    renderers: Vec<Entry>,
    next_token: u64,
}

impl Render {
    pub const fn new() -> Self {
        Self {
            controller: RenderController::new(),
            renderers: Vec::new(),
            next_token: 0,
        }
    }

    pub fn register(&mut self, renderer: Box<dyn Renderer>, now_ms: u32) -> RenderToken {
        let token = RenderToken(self.next_token);
        self.next_token += 1;
        let mut entry = Entry {
            token,
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
        token
    }

    pub fn unregister(&mut self, token: RenderToken) {
        if let Some(index) = self.renderers.iter().position(|e| e.token == token) {
            self.renderers.remove(index);
        }
    }

    /// One frame: reconcile `state` against the controller, fan changes out to
    /// matching renderers, then step time-driven animations. Returns `true`
    /// while any renderer still animates.
    pub fn tick(
        &mut self,
        bus: &RenderBus,
        state: &DeviceState,
        demand: &EffectiveDemand,
        now_ms: u32,
    ) -> bool {
        self.drain_bus(bus, now_ms);
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

    fn drain_bus(&mut self, bus: &RenderBus, now_ms: u32) {
        while let Ok(msg) = bus.try_receive() {
            match msg {
                RenderMsg::Register(renderer) => {
                    self.register(renderer, now_ms);
                }
                RenderMsg::Unregister(token) => self.unregister(token),
            }
        }
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
                self.display = diagnostics.demand.plane(PlaneKey::DISPLAY).mode;
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
                match appearance {
                    LightAppearance::Off => {
                        self.breath = None;
                        self.drive(Fill::Uniform, Rgb(0, 0, 0));
                        self.light.set_backlight(0);
                        Activity::Idle
                    }
                    LightAppearance::Color(color) => {
                        self.breath = None;
                        self.drive(Fill::Uniform, color);
                        self.light.set_backlight(100);
                        Activity::Idle
                    }
                    LightAppearance::Breathing(breath) => {
                        self.breath = Some(breath);
                        self.draw(now_ms, breath);
                        Activity::TimeDriven
                    }
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
        self.page == Some(DisplayPage::Camera)
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

/// Render loop: drains the management pipe, interpreting operation intents against the
/// manager (the pipeline context) and applying business intents wholesale, and parks on
/// the intent/render buses once the surface settles instead of busy-stepping.
pub async fn render_loop<C: ClockSink>(
    intent_bus: &'static IntentBus,
    render_bus: &'static RenderBus,
    playback_bus: &'static PlaybackBus,
    mut render: Render,
    manager: &mut DeviceManager,
    clock: &mut C,
    demand: &SharedDemand,
) -> ! {
    let mut elapsed_ms: u32 = 0;
    let mut last_page = manager.state().page;
    let mut next_frame: Instant = Instant::now();
    // This loop's, not a static: what the chip was last asked for belongs to the one
    // loop that asks it, so a second `run` in one process does not answer for the first.
    let mut governor = ClockGovernor::new();
    let mut playback = PlaybackDispatch::new(manager.playback_state());
    loop {
        while let Ok(intent) = intent_bus.try_receive() {
            match intent {
                // An operation is interpreted against the freshest owner of
                // the device state: record its diagnostics, translate the
                // business meaning, and apply the target — the one place the
                // two planes meet.
                Intent::Operation(op) => {
                    manager.apply_operation(op);
                    let business = iot_core::intent::translate(&op, &manager.state());
                    manager.apply_business(business);
                    // A page change, one line: the cost report names a page once per window,
                    // which cannot show whether a gesture advanced one page or three.
                    if manager.state().page != last_page {
                        log::info!("[INPUT] page is now {:?}", manager.state().page);
                        last_page = manager.state().page;
                    }
                }
                // A producer that already speaks business (e.g. a network
                // drive) goes straight to the state.
                Intent::Business(business) => manager.apply_business(business),
            }
            // What travels to the driver is the diff against what the driver
            // has been told, and not the intent: the state layer is what decides
            // a tap was heard, so a command built from the intent alone would
            // replay a sound it just dropped.
            playback.dispatch(playback_bus, manager.playback_state());
        }
        let state = manager.state();
        // The page is the product's declaration of what it wants, so the demand follows it
        // here rather than each consumer deriving its own. The polling loop reads
        // whatever is current on its next tick. The configurator is not called here — it
        // needs the board's capabilities and answers against the part that refused, so
        // it sits beside the parts.
        demand.lock(|current| *current.borrow_mut() = manager.demand().clone());
        // The governor returns what it did; the wording and the channel are this layer's.
        if let Some(applied) = governor.apply(manager.demand(), clock) {
            if let Some(short) = applied.undercuts {
                log::warn!(
                    "[CLOCK] a page asked for {:?}, but {:?} is running and needs {:?} \
                     — applying the page's choice",
                    applied.asked,
                    short.plane,
                    short.needed,
                );
            }
            log::info!("[CLOCK] running at {:?}", applied.running);
        }
        let active = render.tick(render_bus, &state, manager.demand(), elapsed_ms);
        elapsed_ms = elapsed_ms.wrapping_add(STEP_MS);
        if active {
            // Absolute deadline: work between frames never piles drift onto
            // the animation clock.
            next_frame += Duration::from_millis(STEP_MS.into());
            Timer::at(next_frame).await;
        } else {
            // The wake is a readiness signal, not a receive: it never pops, so
            // the drain at the top of the next pass sees the message.
            select(intent_bus.ready_to_receive(), render_bus.ready_to_receive()).await;
            // Re-anchor so missed frames while parked do not replay as a burst
            // when the light resumes animating.
            next_frame = Instant::now();
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
    use iot_core::diagnostics::Diagnostics;
    use iot_core::drivers::camera::FrameSource;
    use iot_core::render::{SlotAppearance, light_appearance};
    use iot_core::state::LightState;

    /// Counts what reached the panel rather than what was asked of it.
    struct FakePanel {
        fills: u32,
        backlight: u8,
        ceiling: u8,
    }

    impl FakePanel {
        fn new() -> Self {
            Self {
                fills: 0,
                backlight: 100,
                ceiling: 100,
            }
        }
    }

    impl RgbLight for FakePanel {
        fn set_fill(&mut self, _fill: Fill, _color: Rgb) {
            self.fills += 1;
        }

        fn set_backlight(&mut self, level_pct: u8) {
            self.backlight = level_pct.min(self.ceiling);
        }

        fn set_backlight_ceiling(&mut self, level_pct: u8) {
            self.ceiling = level_pct;
            self.backlight = self.backlight.min(level_pct);
        }
    }

    impl DiagnosticsSink for FakePanel {
        fn consume(&mut self, _diagnostics: &Diagnostics) {}
    }

    impl CameraTarget for FakePanel {
        fn paint_camera(&mut self, _now_ms: u64) -> bool {
            false
        }

        fn attach_camera(&mut self, _camera: Box<dyn FrameSource>) {}
    }

    fn panel_asked(display: Mode) -> SlotAppearance {
        let mut demand = EffectiveDemand::default();
        demand.set(
            PlaneKey::DISPLAY,
            iot_core::demand::PlaneDemand::new(display, 0, iot_core::demand::Sensitivity::Primary),
        );
        SlotAppearance::Diagnostics(Diagnostics {
            demand,
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
}
