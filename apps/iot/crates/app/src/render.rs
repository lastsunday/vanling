use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::input::IntentBus;
use embassy_futures::select::select;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Timer};
use iot_core::diagnostics::DiagnosticsSink;
use iot_core::drivers::light::{
    Fill, Rgb, RgbLight, backlight_level, group_hue, hsv_to_rgb, should_repaint, smooth_brightness,
};
use iot_core::intent::Intent;
use iot_core::render::{
    Activity, LightAppearance, RenderController, Renderer, Slot, SlotAppearance,
};
use iot_core::state::{Breath, DeviceManager, DeviceState};

/// Frame cadence of the render loop.
pub const STEP_MS: u32 = 20;

/// Capacity of the cross-task render bus.
pub const RENDER_BUS_CAPACITY: usize = 8;

/// Cross-task render messages: renderers register when their resource exists
/// (e.g. a connected Web client) and unregister when it disappears. The
/// render task is the single owner draining this bus.
pub enum RenderMsg {
    /// Cross-task renderers must be `Send`; in-task boot renderers (the light)
    /// register directly with [`Render::register`].
    Register(Box<dyn Renderer + Send>),
    /// P1: runtime renderers (Web/Audio) drop on disconnect.
    Unregister(RenderToken),
}

/// Opaque handle to a registered renderer, assigned by the render layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderToken(pub u64);

/// Cross-task render channel type. The render task consumes it via an explicit
/// argument (never a hidden global); producers (future Web/Audio renderers on
/// other tasks) send to [`RENDER_BUS`].
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
    pub fn tick(&mut self, bus: &RenderBus, state: &DeviceState, now_ms: u32) -> bool {
        self.drain_bus(bus, now_ms);
        let controller = &mut self.controller;
        let entries = &mut self.renderers;
        controller.reconcile(state, |appearance| {
            let slot = appearance.slot();
            for entry in entries.iter_mut() {
                if entry.renderer.slots().contains(&slot) {
                    entry.activity = entry.renderer.on_appearance(appearance, now_ms);
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

/// Renderer for one physical light surface: the WS2812 strip on the DevKitC-1
/// or the ST7789 panel (with its LEDC backlight) on the S3 board. Breathing is
/// time-driven: the render layer steps it every tick to produce frames from
/// the schedule. A surface subscribes to its own light slot plus the
/// device-level diagnostics, forwarding the snapshot to the light bus it
/// sinks (the panel overlays the digits; a plain strip ignores them).
pub struct LightRenderer<R: RgbLight + DiagnosticsSink> {
    instance: u8,
    light: R,
    slots: [Slot; 2],
    last: Option<Rgb>,
    breath: Option<Breath>,
}

impl<R: RgbLight + DiagnosticsSink> LightRenderer<R> {
    pub fn new(instance: u8, light: R) -> Self {
        Self {
            instance,
            light,
            slots: [Slot::Light(instance), Slot::Diagnostics],
            last: None,
            breath: None,
        }
    }

    fn drive(&mut self, fill: Fill, color: Rgb) {
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

    /// Paint one breathing frame and the matching backlight level. `drive`
    /// only rewrites panel RAM when the color moved at least `repaint_step`,
    /// but the backlight tracks the envelope on every tick so the PWM ramps
    /// smoothly.
    fn draw(&mut self, now_ms: u32, breath: Breath) {
        let color = breath_frame(now_ms, breath);
        self.drive(Fill::Uniform, color);
        self.light.set_backlight(backlight_for(now_ms, breath));
    }
}

impl<R: RgbLight + DiagnosticsSink> Renderer for LightRenderer<R> {
    fn slots(&self) -> &[Slot] {
        &self.slots
    }

    fn on_appearance(&mut self, appearance: SlotAppearance, now_ms: u32) -> Activity {
        match appearance {
            // A diagnostics bump must not disturb the animation cadence: keep the
            // breathing activity, and let panel surfaces repaint the digits.
            SlotAppearance::Diagnostics(diagnostics) => {
                self.light.consume(&diagnostics);
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
        match self.breath {
            Some(breath) => {
                self.draw(now_ms, breath);
                Activity::TimeDriven
            }
            None => Activity::Idle,
        }
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

/// Render loop: drains the management pipe, interpreting operation intents
/// against the manager (the pipeline context) and applying business intents
/// wholesale, and parks on the intent/render buses once the surface settles
/// instead of busy-stepping.
pub async fn render_loop(
    intent_bus: &'static IntentBus,
    render_bus: &'static RenderBus,
    mut render: Render,
    mut manager: DeviceManager,
) -> ! {
    let mut elapsed_ms: u32 = 0;
    let mut next_frame: Instant = Instant::now();
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
                }
                // A producer that already speaks business (e.g. a network
                // drive) goes straight to the state.
                Intent::Business(business) => manager.apply_business(business),
            }
        }
        let active = render.tick(render_bus, &manager.state(), elapsed_ms);
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
