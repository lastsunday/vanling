use crate::drivers::input::{ButtonEvent, GestureEvent, InputEvent, SwipeDirection};
use crate::drivers::light::{GROUP_CAPACITY, Rgb};
use crate::drivers::network::{LinkStatus, Ssid};
use crate::keyboard::{self, Key};
use crate::page::Part;
use crate::state::{Breath, DeviceManager, DeviceState, DisplayPage, LightState};

/// Standard-color palette for solid mode, cycled by a single click or a
/// horizontal swipe: seven hues spanning the wheel
/// (red→orange→yellow→green→cyan→blue→purple), so the readout digit and the
/// eye agree.
pub const PALETTE: [Rgb; 7] = [
    Rgb(255, 64, 48),
    Rgb(255, 160, 48),
    Rgb(255, 224, 64),
    Rgb(80, 200, 96),
    Rgb(32, 200, 220),
    Rgb(72, 128, 255),
    Rgb(190, 96, 255),
];

/// Breathing periods (ms) cycled by the button's double-click advance on
/// breathing mode, from a sub-second shimmer to a slow ambient drift. The boot
/// default (`DeviceManager::DEFAULT_PERIOD_MS`, 3 s) sits in the table so a
/// wrap lands on it.
pub const BREATH_PERIODS_MS: [u32; 10] =
    [125, 250, 375, 500, 750, 1_000, 2_000, 3_000, 5_000, 8_000];

/// Solid-mode brightness steps (8-bit), dark to full, cycled by the button's
/// double-click advance and the vertical swipes on the solid color; the color
/// rides on top unchanged.
pub const SOLID_BRIGHTNESS_STEPS: [u8; 6] = [32, 64, 100, 140, 200, 255];

/// One vertical swipe moves the breathing envelope (`min`/`max` brightness
/// together) this many levels. The same width doubles as the wall guard kept
/// next to either 8-bit edge, so a swipe can never drive `max` down to `min`.
pub const BRIGHTNESS_ENVELOPE_STEP: u8 = 32;

/// A color-group preset for breathing mode, switched by the single click (or a
/// horizontal swipe).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorGroup {
    pub hue_span: u8,
    pub group: [u8; GROUP_CAPACITY],
    pub group_len: u8,
}

impl ColorGroup {
    /// Overlay the group's hue path onto a base breath: a preset only varies
    /// the hue path, so the envelope/saturation ride along unchanged. The one
    /// place the triplet is spread back into a [`Breath`].
    pub fn into_breath(self, base: Breath) -> Breath {
        Breath {
            hue_span: self.hue_span,
            group: self.group,
            group_len: self.group_len,
            ..base
        }
    }
}

/// Color-group presets cycled by the single click's advance on breathing
/// mode; the last entry is the full-hue sweep, the "七彩" finale of the color
/// walk.
pub const COLOR_GROUPS: [ColorGroup; 4] = [
    // Warm sunset: amber, golden, deep rose.
    ColorGroup {
        hue_span: 255,
        group: [16, 32, 3, 0, 0, 0, 0],
        group_len: 3,
    },
    // Rainbow seven: red → orange → yellow → green → cyan → blue → purple,
    // walking the hue wheel the short way around.
    ColorGroup {
        hue_span: 255,
        group: [0, 21, 43, 85, 128, 170, 213],
        group_len: 7,
    },
    // Standalone warm white.
    ColorGroup {
        hue_span: 255,
        group: [28, 0, 0, 0, 0, 0, 0],
        group_len: 1,
    },
    // Full-hue sweep across the whole wheel.
    ColorGroup {
        hue_span: 255,
        group: [0, 0, 0, 0, 0, 0, 0],
        group_len: 0,
    },
];

/// A pure device-level operation: what an input source reported, carrying the
/// wiring-order source that raised it. Recognition attaches no business
/// meaning — no target, no mode, no tables — so the operation plane never reads
/// or encodes device state. [`recognize`] is the only producer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationIntent {
    /// Wiring-order source that raised the event.
    pub source: u8,
    /// The raw/folded input signal, verbatim from the driver.
    pub event: InputEvent,
}

/// A business intent: an absolute target the device should move to. Producers
/// compute it from the pipeline context (the manager holds the current state);
/// the two light-bearing planes never share a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusinessIntent {
    /// A no-op: the operation recorded its diagnostics but moves nothing
    /// (e.g. a click/tap on `Off`, whose tables have no step to take, or a
    /// pulse/read-back that is diagnostic-only by nature).
    Invalid,
    /// Set the `instance`-th light surface to an absolute target state.
    SetLight {
        instance: u8,
        state: LightState,
    },
    TogglePage,
    /// Start or stop the capture behind the Audio page. Reached from a tap or
    /// the button's click on that page, so whichever one-liner control the user
    /// reaches for drives the recording, and neither shadows a gesture that
    /// means something on the other pages.
    ToggleRecord,
    /// Play the next sound in the catalogue, reached from a tap or the button's
    /// click on the Speaker page. Whether it is heard is the state layer's call:
    /// over a sounding one it is tallied and dropped, so the intent names the
    /// request and not the outcome.
    PlayNext,
    /// Flip the output's mute latch. A long press on the Speaker page, because
    /// muting is a decision rather than a one-liner verb and should not be
    /// reachable by the gesture that plays.
    ToggleMute,
    /// The driver's report that a sound ran out, sent by the playback task.
    /// Never raised by [`translate`]: it is a business intent that comes back
    /// from the plane it commands.
    PlaybackFinished,
    /// The clock's report that the splash is over. A report, not a target: the page leaves on time,
    /// and the state layer holds the page the build named.
    SplashOver,
    /// The splash's hold: a request to be configured, decided when the splash ends.
    Setup,
    /// Where the link stands, sent by the network task.
    ///
    /// A report rather than a target, like [`Self::PlaybackFinished`]: the radio is
    /// the only thing that has been on the air, so nothing above it can reconstruct
    /// an association or a lease. Never raised by [`translate`].
    Link(LinkStatus),
    /// The radio's answer that a name the panel asked for has no saved copy. It puts the page back
    /// where a passphrase can be typed for that row.
    NoCopy(Ssid),
    /// The board's own address, sent once at boot. It never changes.
    Mac([u8; 6]),
    /// A key of the Network page's keyboard, with the clock at the lift. The state layer decides
    /// what it types; the clock tells a repeat that it is cycling rather than typing.
    Key {
        key: Key,
        at_ms: u32,
    },
    /// A band of the Network page's scan list; an empty band is background, not a selection.
    Row(u8),
    /// Slide the scan list one row. Raised only while the keyboard is down; a window-sized step
    /// cannot land on a partial last page.
    ListSlide(bool),
    /// A tap on the action strip and its cell. The state layer decides the action from the link;
    /// the cell matters only in setup mode. With a confirmation up, the strip means [`Self::Dismiss`].
    Toolbar(keyboard::ToolbarCell),
    /// The action button in a confirmation: the only gesture that says yes. Every other gesture is
    /// [`Self::Dismiss`].
    Confirm,
    /// Toggle the setup access point between open and passphrase-protected. The radio is the
    /// authority on which state it is in.
    TogglePass,
    /// Leave the page's question undone. Every gesture on a page that is asking means this.
    Dismiss,
    /// A long press on a band: the row a finger held, which asks about the saved copy rather than
    /// picking the network.
    RowHold(u8),
}

/// The management-pipe message: one pipe carries both planes. The operation
/// plane's signals travel as `Operation` and are interpreted into a `Business`
/// intent by the consumer that owns the device state; producers that already
/// speak business (e.g. a network drive) send `Business` directly.
///
/// `Operation` is as wide as `InputEvent` for the same reason that enum is: it
/// crosses the intent bus by value, so a capture envelope stays allocation-free
/// and `Copy` at the cost of some padding on the smaller variants.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Operation(OperationIntent),
    Business(BusinessIntent),
}

/// Stateless recognition: fold a raw input event plus its wiring-order source
/// into an operation intent. Recognition never reads device state or the
/// business tables — the input task only packages what the panel reported.
pub fn recognize(event: InputEvent, source: u8) -> OperationIntent {
    OperationIntent { source, event }
}

/// Interpret an operation against the current state into an absolute target. Buttons advance
/// their own surface; touch gestures move the first. Pulses and raw snapshots only record
/// diagnostics, so they land on [`BusinessIntent::Invalid`].
pub fn translate(op: &OperationIntent, current: &DeviceState) -> BusinessIntent {
    let slot = light_slot(current, op.source);
    let light = current.lights[slot];
    // The Network page is a setup page rather than a light page: it paints its own black
    // background instead of the light's fill, so a gesture that moves the light has nothing on
    // this page to show for it, and the panel's own controls are the only things a finger can
    // act on. What it answers is therefore exactly what its content owns plus the page switch,
    // and the switch is the hardware button's triple click: touch has no multi-tap gesture to
    // switch pages with, because a single tap that had to wait for a second one would be the
    // latency every key on this page pays.
    //
    // Every page match below therefore carries the Network page as a no-op of its own, which is
    // what keeps a page added later from silently inheriting a gesture here.

    // The splash page is the one page whose subject is not a light: a device on it has just booted
    // and is not doing anything yet, so the button's hold is the way setup mode is asked for — the
    // deliberate ceremony (reset the board, then hold) rather than a press any impatient finger
    // could spend on a working link. Nothing else on the page answers: no page switch, no colour
    // walk, no gesture. That is what makes it a splash rather than a page, and it is also why it is
    // not in the cycle — once the clock has moved the device on, there is no way back to it.
    //
    // The request is not a leave: nothing has been associated yet, because the link waits for this
    // page to close before it reads a credential at all. A board with nothing saved and a board
    // with a network therefore ask the same question here, and the answer is the same page.
    if current.page == DisplayPage::Splash {
        return match op.event {
            InputEvent::Button(ButtonEvent::LongPress) => BusinessIntent::Setup,
            _ => BusinessIntent::Invalid,
        };
    }

    // In setup mode the same hold is the front-end switch, on every page and at any time: the mode
    // is what the press is about while it is on, and it is the one gesture a panel with no working
    // touch has left. It is not on the Network page's own matcher because the reason it exists is a
    // panel that cannot be used — a digit that stopped answering cannot reach that page.
    if current.link.setup && matches!(op.event, InputEvent::Button(ButtonEvent::LongPress)) {
        return BusinessIntent::Toolbar(keyboard::ToolbarCell::Right);
    }
    if current.page == DisplayPage::Network {
        return match op.event {
            InputEvent::Gesture(GestureEvent::Tap { x, y, at_ms, .. }) => {
                network_tap(current, x, y, at_ms)
            }
            InputEvent::Gesture(GestureEvent::LongPress { x, y, .. }) => {
                network_hold(current, x, y)
            }
            // A slide down the list, the way a list of eight rows through a window of three is
            // read everywhere else: the finger pushes the content away from itself, one row at a
            // time so every row is reachable. While the keyboard is up there is no list under the
            // finger, so a slide means nothing — and while the page is asking a question the list
            // behind it is not what the finger is on either, so the slide answers the question.
            InputEvent::Gesture(GestureEvent::Swipe { direction, .. })
                if !current.setup.keyboard =>
            {
                if current.setup.confirm.is_some() {
                    return BusinessIntent::Dismiss;
                }
                if !Part::Content.showing(current.setup, current.link) {
                    return BusinessIntent::Invalid;
                }
                // A list the page is not drawing is not a list to push: the box over it belongs to
                // the radio rather than to the finger, and a slide under it would move rows nobody
                // can see.
                match direction {
                    SwipeDirection::Up => BusinessIntent::ListSlide(true),
                    SwipeDirection::Down => BusinessIntent::ListSlide(false),
                    _ => BusinessIntent::Invalid,
                }
            }
            // The strip's own verb, so a board with a button has the same switch a finger has:
            // `SETUP` on a link, `CANCEL` or the front switch in setup mode, `SCAN` otherwise. The
            // right cell because it is the one a strip with a single verb has, and the click is a
            // one-liner rather than a place to aim at.
            InputEvent::Button(ButtonEvent::Click) => {
                BusinessIntent::Toolbar(keyboard::ToolbarCell::Right)
            }
            InputEvent::Button(ButtonEvent::TripleClick) => BusinessIntent::TogglePage,
            _ => BusinessIntent::Invalid,
        };
    }
    match op.event {
        // A click advances the color; `Off` has none, so it reads as no-op. On
        // the Audio and Speaker pages the button is the page's own one-liner
        // control instead, mirroring the tap below: the panel and the hardware
        // button are the same verb there, and neither takes a gesture away from
        // the other pages.
        InputEvent::Button(ButtonEvent::Click) => match current.page {
            DisplayPage::Audio => BusinessIntent::ToggleRecord,
            DisplayPage::Speaker => BusinessIntent::PlayNext,
            // The camera page draws the sensor's picture rather than a fill, so a colour
            // walk is not visible here — but the state still moves, because the light is
            // still the light's and the page the user leaves onto shows where it landed.
            DisplayPage::Ambient | DisplayPage::Attitude | DisplayPage::Camera => {
                target_for(advance_color(light), slot)
            }
            // Unreachable while the splash rule above stands, and the floor if it ever moves:
            // no gesture on that page is a page's own.
            DisplayPage::Splash => BusinessIntent::Invalid,
            DisplayPage::Network => BusinessIntent::Invalid,
        },
        InputEvent::Button(ButtonEvent::DoubleClick) => target_for(advance_step(light), slot),
        InputEvent::Button(ButtonEvent::TripleClick) => BusinessIntent::TogglePage,
        // Cycles the light mode; always resolves, so any state is the way back
        // on — except on the Speaker page, where the long press is the mute
        // latch instead of a mode move.
        InputEvent::Button(ButtonEvent::LongPress) => match current.page {
            DisplayPage::Speaker => BusinessIntent::ToggleMute,
            // Camera included for the click's reason: the mode changes either way, and a
            // mode with no page to show it on still needs a way back to `Solid`.
            DisplayPage::Ambient
            | DisplayPage::Attitude
            | DisplayPage::Audio
            | DisplayPage::Camera => BusinessIntent::SetLight {
                instance: slot as u8,
                state: cycle_mode(light),
            },
            // Unreachable while the splash rule above stands, and the floor if it ever moves:
            // no gesture on that page is a page's own.
            DisplayPage::Splash => BusinessIntent::Invalid,
            DisplayPage::Network => BusinessIntent::Invalid,
        },
        InputEvent::Gesture(GestureEvent::Press { .. }) => BusinessIntent::Invalid,
        InputEvent::Gesture(GestureEvent::Ghost) => BusinessIntent::Invalid,
        // A raw chip gesture and a motion batch are diagnostic read-backs; they
        // never move a light. (A capture poll never reaches here at all: what the
        // capture produces is a window, and windows do not ride this plane.)
        InputEvent::ChipGesture(_) | InputEvent::Motion(_) => BusinessIntent::Invalid,
        // Classified touch gestures mirror the button moves in their own
        // tallying variants; the device-level panel always drives surface 0.
        // A tap is the one gesture two pages own — the record control on Audio,
        // the play control on Speaker — and both leave the color walk to the
        // pages that have no use for it.
        InputEvent::Gesture(GestureEvent::Tap { .. }) => match current.page {
            DisplayPage::Audio => BusinessIntent::ToggleRecord,
            DisplayPage::Speaker => BusinessIntent::PlayNext,
            // As the button's click: the camera page hides the fill but not the state.
            DisplayPage::Ambient | DisplayPage::Attitude | DisplayPage::Camera => {
                target_for(advance_color(light), 0)
            }
            // Unreachable while the splash rule above stands, and the floor if it ever moves:
            // no gesture on that page is a page's own.
            DisplayPage::Splash => BusinessIntent::Invalid,
            DisplayPage::Network => BusinessIntent::Invalid,
        },
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
            target_for(swipe(light, direction), 0)
        }
        InputEvent::Gesture(GestureEvent::LongPress { .. }) => match current.page {
            DisplayPage::Speaker => BusinessIntent::ToggleMute,
            DisplayPage::Ambient
            | DisplayPage::Attitude
            | DisplayPage::Audio
            | DisplayPage::Camera => BusinessIntent::SetLight {
                instance: 0,
                state: cycle_mode(light),
            },
            // Unreachable while the splash rule above stands, and the floor if it ever moves:
            // no gesture on that page is a page's own.
            DisplayPage::Splash => BusinessIntent::Invalid,
            DisplayPage::Network => BusinessIntent::Invalid,
        },
        InputEvent::Touch(_) => BusinessIntent::Invalid,
    }
}

/// What a tap on the Network page means: a key, an answer, the strip, or a band. All three are read
/// off the geometry the panel draws, so a tap does what it looks like. The state layer decides what
/// a band does. Anything the page is not drawing is not aimed at ([`Part::showing`]).
fn network_tap(current: &DeviceState, x: u16, y: u16, at_ms: u32) -> BusinessIntent {
    // The keyboard is up exactly when there is a draft under it, and it is the page's whole state
    // while it is: a tap anywhere else on this page is a key, or nothing at all.
    if current.setup.keyboard {
        let Some(entry) = current.setup.entry else {
            return BusinessIntent::Invalid;
        };
        return keyboard::key_at(entry.layer(), x, y).map_or(BusinessIntent::Invalid, |key| {
            BusinessIntent::Key { key, at_ms }
        });
    }
    if let Some(answer) = answer_at(current, x, y) {
        return answer;
    }
    // The strip first, because it is the one part of the page that stays offered under the box: it
    // is not read off the list, and it is where the page's own action lives.
    if let Some(cell) = keyboard::toolbar_cell_at(x, y) {
        return BusinessIntent::Toolbar(cell);
    }
    if !Part::Content.showing(current.setup, current.link) {
        return BusinessIntent::Invalid;
    }
    match keyboard::list_band_at(y) {
        Some(band) => BusinessIntent::Row(band as u8),
        None => BusinessIntent::Invalid,
    }
}

/// What a long press means: the held row, or an answer. A hold asks about the saved copy rather
/// than picking. An answer comes first, and a box over the list covers the rows it hides.
fn network_hold(current: &DeviceState, x: u16, y: u16) -> BusinessIntent {
    if current.setup.keyboard {
        return BusinessIntent::Invalid;
    }
    if let Some(answer) = answer_at(current, x, y) {
        return answer;
    }
    if !Part::Content.showing(current.setup, current.link) {
        return BusinessIntent::Invalid;
    }
    match keyboard::list_band_at(y) {
        Some(band) => BusinessIntent::RowHold(band as u8),
        None => BusinessIntent::Invalid,
    }
}

/// The answer a gesture gives to the page's question, or `None` when it asks nothing. The action
/// button says yes; every other point leaves it undone, which is the point of asking.
fn answer_at(current: &DeviceState, x: u16, y: u16) -> Option<BusinessIntent> {
    // The box a phone's setup is read in asks one question — "with a passphrase, or without?" —
    // and its other answer is the way back to the panel. A tap that lands on neither is nothing:
    // this is a mode rather than a question, and a stray finger must not close the access point a
    // phone is being walked onto.
    if current.link.shows_setup_box() {
        return match keyboard::dialog_choice_at(x, y) {
            Some(keyboard::DialogChoice::Keep) => Some(BusinessIntent::TogglePass),
            Some(keyboard::DialogChoice::Act) => {
                Some(BusinessIntent::Toolbar(keyboard::ToolbarCell::Right))
            }
            // Not an answer: the strip under the box is still the page's own control, and the
            // caller looks there next.
            None => None,
        };
    }
    // A page that is asking nothing answers nothing: the question is the only thing that makes a
    // tap an answer.
    current.setup.confirm?;
    Some(match keyboard::dialog_choice_at(x, y) {
        Some(keyboard::DialogChoice::Act) => BusinessIntent::Confirm,
        Some(keyboard::DialogChoice::Keep) | None => BusinessIntent::Dismiss,
    })
}

/// Fold an optional table step into a business intent: `None` means the mode
/// has no state to advance (an `Off` surface), which is an `Invalid` no-op.
fn target_for(step: Option<LightState>, instance: usize) -> BusinessIntent {
    step.map_or(BusinessIntent::Invalid, |state| BusinessIntent::SetLight {
        instance: instance as u8,
        state,
    })
}

/// The light surface a wiring-order source drives: its own button instance,
/// or the first surface for the device-level panel's gestures (`source == 0`).
/// Out-of-range sources clamp to the last installed surface rather than panic
/// the interpreter.
fn light_slot(current: &DeviceState, source: u8) -> usize {
    usize::from(source).min(current.lights.len() - 1)
}

/// Cycle the light mode one step: `Off` wakes to boot breathing, breathing
/// hands the wheel to the first solid color at full brightness, solid powers
/// off. The long-press's action — the most deliberate input — owns the
/// heaviest transition and never needs a no-op arm.
fn cycle_mode(current: LightState) -> LightState {
    match current {
        LightState::Off => LightState::default(),
        LightState::Breath { .. } => LightState::Solid {
            color: PALETTE[0],
            brightness: DeviceManager::SOLID_DEFAULT_BRIGHTNESS,
        },
        LightState::Solid { .. } => LightState::Off,
    }
}

/// Advance the color one slot (wrapping): the color group in breath, the
/// palette in solid. Exactly a right-swipe, so the two land on the same next
/// color.
fn advance_color(current: LightState) -> Option<LightState> {
    swipe_color(current, true)
}

/// Step the current mode's table by one slot (wrapping): breathing period for
/// breath, brightness for solid.
fn advance_step(current: LightState) -> Option<LightState> {
    match current {
        LightState::Off => None,
        LightState::Breath(breath) => {
            let next_period = advance(&BREATH_PERIODS_MS, breath.period_ms);
            Some(LightState::Breath(Breath {
                period_ms: next_period,
                ..breath
            }))
        }
        LightState::Solid { color, brightness } => {
            let next = advance_u8(&SOLID_BRIGHTNESS_STEPS, brightness);
            Some(LightState::Solid {
                color,
                brightness: next,
            })
        }
    }
}

/// A resolved swipe's light move: horizontal steps the color, vertical the
/// brightness (the breathing envelope as a whole, the solid step table
/// otherwise); the diagonals resolve to an identity target so they tally
/// without moving a light. `None` on `Off`.
fn swipe(current: LightState, direction: SwipeDirection) -> Option<LightState> {
    match direction {
        SwipeDirection::Left => swipe_color(current, false),
        SwipeDirection::Right => swipe_color(current, true),
        SwipeDirection::Down => swipe_brightness(current, false),
        SwipeDirection::Up => swipe_brightness(current, true),
        SwipeDirection::UpLeft
        | SwipeDirection::UpRight
        | SwipeDirection::DownLeft
        | SwipeDirection::DownRight => Some(current),
    }
}

fn swipe_color(current: LightState, forward: bool) -> Option<LightState> {
    match current {
        LightState::Off => None,
        LightState::Breath(breath) => {
            let next_group = if forward {
                advance_color_group(&breath)
            } else {
                retreat_color_group(&breath)
            };
            Some(LightState::Breath(next_group.into_breath(breath)))
        }
        LightState::Solid { color, brightness } => {
            let next_color = if forward {
                advance_palette(color)
            } else {
                retreat_palette(color)
            };
            Some(LightState::Solid {
                color: next_color,
                brightness,
            })
        }
    }
}

fn swipe_brightness(current: LightState, up: bool) -> Option<LightState> {
    match current {
        LightState::Off => None,
        LightState::Breath(breath) => Some(LightState::Breath(shift_envelope(breath, up))),
        LightState::Solid { color, brightness } => {
            let next = if up {
                advance_u8(&SOLID_BRIGHTNESS_STEPS, brightness)
            } else {
                retreat_u8(&SOLID_BRIGHTNESS_STEPS, brightness)
            };
            Some(LightState::Solid {
                color,
                brightness: next,
            })
        }
    }
}

/// Slide a breathing envelope's floor and ceiling together by
/// [`BRIGHTNESS_ENVELOPE_STEP`], guarding either 8-bit wall — both bounds move
/// together, so `max` always stays a full step above `min` and an equal
/// envelope (which would make `LO HI` meaningless) is impossible.
fn shift_envelope(breath: Breath, up: bool) -> Breath {
    let step = if up {
        i16::from(BRIGHTNESS_ENVELOPE_STEP)
    } else {
        -i16::from(BRIGHTNESS_ENVELOPE_STEP)
    };
    let guard = i16::from(BRIGHTNESS_ENVELOPE_STEP);
    let min = (i16::from(breath.min_brightness) + step).clamp(0, 255 - guard);
    let max = (i16::from(breath.max_brightness) + step).clamp(guard, 255);
    Breath {
        min_brightness: min as u8,
        max_brightness: max as u8,
        ..breath
    }
}

/// Look up `current` in `table` and move `dir` slots (wrapping), where `dir`
/// is `+1` forward and `-1` backward. Falls back to the first entry if
/// `current` is not found, then still moves, so an unknown value walks off
/// position zero rather than staying stuck there.
fn table_step<T: Copy + PartialEq>(table: &[T], current: T, dir: isize) -> T {
    let idx = table.iter().position(|&v| v == current).unwrap_or(0);
    let len = table.len() as isize;
    table[((idx as isize + dir).rem_euclid(len)) as usize]
}

fn advance(table: &[u32], current: u32) -> u32 {
    table_step(table, current, 1)
}

fn advance_u8(table: &[u8], current: u8) -> u8 {
    table_step(table, current, 1)
}

fn retreat_u8(table: &[u8], current: u8) -> u8 {
    table_step(table, current, -1)
}

fn advance_palette(current: Rgb) -> Rgb {
    table_step(&PALETTE, current, 1)
}

fn retreat_palette(current: Rgb) -> Rgb {
    table_step(&PALETTE, current, -1)
}

/// Index of `breath`'s color group in `COLOR_GROUPS`, `0` if not found.
/// Matching keys on `group`/`group_len` — the hue span rides along with every
/// preset, so an unknown span must not hide a known trajectory.
fn color_group_idx(breath: &Breath) -> usize {
    COLOR_GROUPS
        .iter()
        .position(|c| c.group == breath.group && c.group_len == breath.group_len)
        .unwrap_or(0)
}

/// The preset after the current group, wrapping.
fn advance_color_group(breath: &Breath) -> ColorGroup {
    let idx = color_group_idx(breath);
    COLOR_GROUPS[(idx + 1) % COLOR_GROUPS.len()]
}

/// The preset before the current group, wrapping.
fn retreat_color_group(breath: &Breath) -> ColorGroup {
    let idx = color_group_idx(breath);
    COLOR_GROUPS[(idx + COLOR_GROUPS.len() - 1) % COLOR_GROUPS.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::input::{
        FingerLast, MAX_TOUCH_POINTS, MAX_TRACKED_POINTS, TouchEvent, TouchPoint, TouchStatus,
    };
    use crate::drivers::light::MAX_LIGHTS;
    use crate::drivers::motion::{MotionCapabilities, MotionCounts};
    use crate::drivers::network::Front;
    use crate::state::{AudioState, Panel, PlaybackState};

    const DEFAULT_BREATH: LightState = LightState::Breath(BREATH_BASE);

    const BREATH_BASE: Breath = Breath {
        period_ms: DeviceManager::DEFAULT_PERIOD_MS,
        hue_period_ms: DeviceManager::DEFAULT_HUE_PERIOD_MS,
        hue_span: DeviceManager::DEFAULT_HUE_SPAN,
        min_brightness: DeviceManager::DEFAULT_MIN_BRIGHTNESS,
        max_brightness: DeviceManager::DEFAULT_MAX_BRIGHTNESS,
        saturation: DeviceManager::DEFAULT_SATURATION,
        hue: DeviceManager::DEFAULT_HUE,
        group: DeviceManager::DEFAULT_GROUP,
        group_len: DeviceManager::DEFAULT_GROUP_LEN,
    };

    fn state(light: LightState) -> DeviceState {
        state_on(DisplayPage::Ambient, light)
    }

    fn state_on(page: DisplayPage, light: LightState) -> DeviceState {
        DeviceState {
            lights: [light; MAX_LIGHTS],
            page,
            link: LinkStatus::default(),
            mac: None,
            aps: None,
            setup: crate::state::NetworkSetup::default(),
            setup_wanted: false,
            join: None,
            leave: false,
            setup_cancel: false,
            switch_front: false,
            toggle_pass: false,
            scan_now: false,
            forget: None,
            motion_enabled: true,
            motion: None,
            motion_counts: MotionCounts::default(),
            motion_caps: MotionCapabilities::EMPTY,
            audio_enabled: true,
            audio: AudioState::default(),
            playback_enabled: true,
            playback: PlaybackState::default(),
            camera_enabled: true,
            network_enabled: true,
            panel: Panel::Wired,
            touch: None,
            touch_points: [None; MAX_TRACKED_POINTS],
            live_dir: [0; MAX_TRACKED_POINTS],
            two_finger_runs: 0,
            touch_frames: 0,
            finger: [FingerLast::default(); MAX_TRACKED_POINTS],
            touch_held_ms: 0,
            tap_count: 0,
            press_count: 0,
            long_press_count: 0,
            ghost_count: 0,
            swipe_count: 0,
            last_swipe: None,
            last_gesture_origin: None,
            last_gesture_end: None,
            chip_gesture_id: 0,
        }
    }

    fn op(event: InputEvent) -> OperationIntent {
        recognize(event, 0)
    }

    /// The button's long press, as the pages that own a verb read it. What it means on the splash
    /// is decided by the page the state is on, not by the event.
    fn long_press() -> InputEvent {
        InputEvent::Button(ButtonEvent::LongPress)
    }

    /// The `SetLight` target `translate` yields for surface 0, or `None` when
    /// the gesture is an `Invalid` no-op.
    fn target(event: InputEvent, current: LightState) -> Option<LightState> {
        target_in(event, &state(current))
    }

    fn target_in(event: InputEvent, current: &DeviceState) -> Option<LightState> {
        match translate(&op(event), current) {
            BusinessIntent::SetLight { state, .. } => Some(state),
            BusinessIntent::Invalid
            | BusinessIntent::TogglePage
            | BusinessIntent::ToggleRecord
            | BusinessIntent::PlayNext
            | BusinessIntent::ToggleMute
            | BusinessIntent::PlaybackFinished
            | BusinessIntent::SplashOver
            | BusinessIntent::Setup
            | BusinessIntent::Link(_)
            | BusinessIntent::NoCopy(_)
            | BusinessIntent::Mac(_)
            | BusinessIntent::Key { .. }
            | BusinessIntent::Row(_)
            | BusinessIntent::ListSlide(_)
            | BusinessIntent::Toolbar(_)
            | BusinessIntent::Confirm
            | BusinessIntent::TogglePass
            | BusinessIntent::Dismiss
            | BusinessIntent::RowHold(_) => None,
        }
    }

    fn tap_gesture(held_ms: u16) -> InputEvent {
        InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: 0,
            y: 0,
            held_ms,
            at_ms: 0,
        })
    }

    fn swipe_event(direction: SwipeDirection) -> InputEvent {
        InputEvent::Gesture(GestureEvent::Swipe {
            id: 0,
            direction,
            x: 0,
            y: 0,
            end_x: 0,
            end_y: 0,
            held_ms: 0,
            distance_px: 80,
        })
    }

    mod recognize {
        use super::*;

        #[test]
        fn wraps_event_and_source() {
            let event = InputEvent::Button(ButtonEvent::Click);
            let intent = recognize(event, 3);
            assert_eq!(intent.source, 3);
            assert_eq!(intent.event, event);
        }

        #[test]
        fn carries_swipe_fields_verbatim() {
            let event = InputEvent::Gesture(GestureEvent::Swipe {
                id: 0,
                direction: SwipeDirection::Up,
                x: 10,
                y: 20,
                end_x: 10,
                end_y: 5,
                held_ms: 75,
                distance_px: 99,
            });
            let intent = recognize(event, 0);
            assert_eq!(intent.source, 0);
            assert_eq!(
                intent.event, event,
                "recognition keeps the gesture's tracking fields byte-for-byte"
            );
        }
    }

    mod buttons {
        use super::*;

        #[test]
        fn click_advances_color_in_every_mode() {
            // Off has no color a click can advance; only a long press turns it on.
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state(LightState::Off)
                ),
                BusinessIntent::Invalid
            );
            // Breath walks the color groups; solid walks the palette.
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state(DEFAULT_BREATH)
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)),
                }
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state(LightState::Solid {
                        color: PALETTE[2],
                        brightness: 140,
                    })
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Solid {
                        color: PALETTE[3],
                        brightness: 140,
                    },
                }
            );
        }

        #[test]
        fn a_button_click_on_the_audio_page_records_and_elsewhere_moves_the_color() {
            let on_audio = state_on(DisplayPage::Audio, DEFAULT_BREATH);
            assert_eq!(
                translate(&op(InputEvent::Button(ButtonEvent::Click)), &on_audio),
                BusinessIntent::ToggleRecord,
                "on the audio page the button is the record control, the same as the tap"
            );
            // The camera page has no one-liner control, so it keeps the colour walk even
            // though it does not draw a fill: the state moves either way, and the page the
            // user leaves onto is where the new colour shows up.
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state_on(DisplayPage::Camera, DEFAULT_BREATH)
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)),
                },
                "the camera page is not a control page, so the colour still walks"
            );
            // Off that page the click still advances the color, so taking the
            // record control on the Audio page costs no gesture anywhere else.
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state(DEFAULT_BREATH)
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)),
                }
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state_on(DisplayPage::Attitude, DEFAULT_BREATH)
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)),
                }
            );
        }

        #[test]
        fn long_press_cycles_the_mode_ring() {
            // Off wakes to the default breath.
            assert_eq!(
                translate(&op(long_press()), &state(LightState::Off)),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: DEFAULT_BREATH,
                }
            );
            // The outgoing state's period and brightness never leak into solid.
            let slow = Breath {
                period_ms: 5_000,
                ..BREATH_BASE
            };
            assert_eq!(
                translate(&op(long_press()), &state(LightState::Breath(slow))),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Solid {
                        color: PALETTE[0],
                        brightness: DeviceManager::SOLID_DEFAULT_BRIGHTNESS,
                    },
                }
            );
            // Solid powers back off from any brightness.
            assert_eq!(
                translate(
                    &op(long_press()),
                    &state(LightState::Solid {
                        color: PALETTE[0],
                        brightness: 255,
                    })
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Off,
                }
            );
            // A full ring returns home: off → breathing → solid → off.
            let mut light = LightState::Off;
            light = target(long_press(), light).unwrap();
            assert!(matches!(light, LightState::Breath(_)));
            light = target(long_press(), light).unwrap();
            assert!(matches!(light, LightState::Solid { .. }));
            light = target(long_press(), light).unwrap();
            assert_eq!(light, LightState::Off);
        }

        #[test]
        fn double_click_steps_period_and_brightness() {
            // Off is a no-op.
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::DoubleClick)),
                    &state(LightState::Off)
                ),
                BusinessIntent::Invalid
            );
            // Breathing period steps 3000 → 5000...
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::DoubleClick)),
                    &state(DEFAULT_BREATH)
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Breath(Breath {
                        period_ms: 5_000,
                        ..BREATH_BASE
                    }),
                }
            );
            // ...then one full lap from the head (125): every slot in order,
            // wrapping back to the head.
            let mut breath = Breath {
                period_ms: BREATH_PERIODS_MS[0],
                ..BREATH_BASE
            };
            for i in 0..BREATH_PERIODS_MS.len() {
                let business = translate(
                    &op(InputEvent::Button(ButtonEvent::DoubleClick)),
                    &state(LightState::Breath(breath)),
                );
                let BusinessIntent::SetLight { state, .. } = business else {
                    panic!("double click must stay in breathing");
                };
                assert!(
                    matches!(state, LightState::Breath(_)),
                    "double click must stay in breathing"
                );
                let LightState::Breath(next) = state else {
                    unreachable!()
                };
                assert_eq!(
                    next.period_ms,
                    BREATH_PERIODS_MS[(i + 1) % BREATH_PERIODS_MS.len()],
                    "step {i}: wrong period reached"
                );
                breath = next;
            }
            // Solid brightness steps 140 → 200 and wraps 255 → 32, color intact.
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::DoubleClick)),
                    &state(LightState::Solid {
                        color: PALETTE[0],
                        brightness: 140,
                    })
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Solid {
                        color: PALETTE[0],
                        brightness: 200,
                    },
                }
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::DoubleClick)),
                    &state(LightState::Solid {
                        color: PALETTE[0],
                        brightness: 255,
                    })
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Solid {
                        color: PALETTE[0],
                        brightness: 32,
                    },
                }
            );
        }
    }

    mod gestures {
        use super::*;

        #[test]
        fn pulses_tally_without_moving_the_light() {
            // A raw touch snapshot is diagnostic-only, whatever the light mode.
            let touch = InputEvent::Touch(TouchEvent {
                points: [TouchPoint {
                    id: 0,
                    x: 10,
                    y: 20,
                    status: TouchStatus::Down,
                }; MAX_TOUCH_POINTS],
                len: 1,
                contacts: 2,
            });
            assert_eq!(
                translate(&op(touch), &state(LightState::Off)),
                BusinessIntent::Invalid
            );
            let release = InputEvent::Touch(TouchEvent {
                points: [TouchPoint {
                    id: 3,
                    x: 1,
                    y: 2,
                    status: TouchStatus::Release,
                }; MAX_TOUCH_POINTS],
                len: 1,
                contacts: 0,
            });
            assert_eq!(
                translate(&op(release), &state(DEFAULT_BREATH)),
                BusinessIntent::Invalid
            );
            // Press-down, ghost anomalies and chip read-backs tally on the
            // operation plane without ever moving a light.
            assert_eq!(
                translate(
                    &op(InputEvent::Gesture(GestureEvent::Press {
                        id: 0,
                        x: 3,
                        y: 4
                    })),
                    &state(DEFAULT_BREATH)
                ),
                BusinessIntent::Invalid
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Gesture(GestureEvent::Ghost)),
                    &state(DEFAULT_BREATH)
                ),
                BusinessIntent::Invalid
            );
            assert_eq!(
                translate(&op(InputEvent::ChipGesture(0x10)), &state(DEFAULT_BREATH)),
                BusinessIntent::Invalid
            );
        }

        #[test]
        fn tap_mirrors_the_button_click() {
            // No color, period or brightness to step on Off.
            assert_eq!(
                translate(&op(tap_gesture(0)), &state(LightState::Off)),
                BusinessIntent::Invalid
            );
            // A tap walks the same color the button's click does: the color
            // group in breathing, the palette in solid.
            assert_eq!(
                target(tap_gesture(0), DEFAULT_BREATH),
                Some(LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)))
            );
            assert_eq!(
                target(
                    tap_gesture(0),
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 255,
                    }
                ),
                Some(LightState::Solid {
                    color: PALETTE[1],
                    brightness: 255,
                })
            );
        }

        #[test]
        fn a_tap_on_the_audio_page_records_and_elsewhere_moves_the_color() {
            let on_audio = state_on(DisplayPage::Audio, DEFAULT_BREATH);
            assert_eq!(
                translate(&op(tap_gesture(0)), &on_audio),
                BusinessIntent::ToggleRecord,
                "on the audio page the tap is the record control"
            );
            // The same tap off that page is still the color move, so the
            // record control never shadows an existing gesture.
            assert_eq!(
                translate(&op(tap_gesture(0)), &state(LightState::Off)),
                BusinessIntent::Invalid
            );
            assert_eq!(
                target_in(
                    tap_gesture(0),
                    &state_on(DisplayPage::Attitude, DEFAULT_BREATH)
                ),
                target(tap_gesture(0), DEFAULT_BREATH)
            );
        }

        #[test]
        fn long_press_mirrors_the_button_mode_ring() {
            // A long press on Off is never a no-op: it is the way back on.
            assert_eq!(
                translate(
                    &op(InputEvent::Gesture(GestureEvent::LongPress {
                        id: 0,
                        x: 0,
                        y: 0,
                        held_ms: 0
                    })),
                    &state(LightState::Off)
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: DEFAULT_BREATH,
                }
            );
            // From breathing it hands off to the same solid the button's long
            // press does.
            assert_eq!(
                target(
                    InputEvent::Gesture(GestureEvent::LongPress {
                        id: 0,
                        x: 1,
                        y: 2,
                        held_ms: 0
                    }),
                    DEFAULT_BREATH
                ),
                Some(LightState::Solid {
                    color: PALETTE[0],
                    brightness: DeviceManager::SOLID_DEFAULT_BRIGHTNESS,
                })
            );
        }
    }

    mod speaker_page {
        use super::*;

        fn on_speaker() -> DeviceState {
            state_on(DisplayPage::Speaker, DEFAULT_BREATH)
        }

        fn long_press_gesture() -> InputEvent {
            InputEvent::Gesture(GestureEvent::LongPress {
                id: 0,
                x: 1,
                y: 2,
                held_ms: 800,
            })
        }

        #[test]
        fn a_tap_plays_and_a_long_press_mutes() {
            // The two verbs the page owns, from the panel...
            assert_eq!(
                translate(&op(tap_gesture(0)), &on_speaker()),
                BusinessIntent::PlayNext
            );
            assert_eq!(
                translate(&op(long_press_gesture()), &on_speaker()),
                BusinessIntent::ToggleMute
            );
            // ...and from the hardware button, which is the same verb on this
            // page exactly as the record control is on the Audio page.
            assert_eq!(
                translate(&op(InputEvent::Button(ButtonEvent::Click)), &on_speaker()),
                BusinessIntent::PlayNext
            );
            assert_eq!(
                translate(&op(long_press()), &on_speaker()),
                BusinessIntent::ToggleMute
            );
        }

        #[test]
        fn the_page_never_moves_a_light() {
            // Playback takes a tap and a long press, but leaves the light verbs
            // it did not claim — the double-click and the swipes — alone, so the
            // Speaker page costs no light control and never sits on a stale
            // color.
            assert_eq!(
                target_in(InputEvent::Button(ButtonEvent::DoubleClick), &on_speaker()),
                target(InputEvent::Button(ButtonEvent::DoubleClick), DEFAULT_BREATH),
                "the double-click still steps the table"
            );
            for direction in [
                SwipeDirection::Up,
                SwipeDirection::Down,
                SwipeDirection::Left,
                SwipeDirection::Right,
            ] {
                assert_eq!(
                    target_in(swipe_event(direction), &on_speaker()),
                    target(swipe_event(direction), DEFAULT_BREATH),
                    "{direction:?} still moves the light"
                );
            }
            // The page turn is what the button's triple click still means, or
            // there would be no way back to the page you can hear the answer on.
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::TripleClick)),
                    &on_speaker()
                ),
                BusinessIntent::TogglePage
            );
        }

        #[test]
        fn a_tap_off_the_speaker_page_never_plays() {
            // The play control belongs to the page, not to the gesture: a tap
            // anywhere else is still the color move.
            assert_eq!(
                target(tap_gesture(0), DEFAULT_BREATH),
                Some(LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)))
            );
        }

        #[test]
        fn a_finished_play_is_never_recognized_from_an_event() {
            // It is the one intent that comes back from the plane it commands,
            // so no input can raise it and the driver owns the only path in.
            for event in [
                InputEvent::Button(ButtonEvent::Click),
                long_press(),
                tap_gesture(0),
                long_press_gesture(),
            ] {
                assert_ne!(
                    translate(&op(event), &on_speaker()),
                    BusinessIntent::PlaybackFinished,
                    "{event:?} must not fake a driver report"
                );
            }
        }
    }

    mod swipes {
        use super::*;

        #[test]
        fn step_color_and_brightness_per_axis() {
            let last = *PALETTE.last().unwrap();
            let breathe = LightState::Breath(BREATH_BASE);
            let cases: [(SwipeDirection, LightState, LightState, &str); 8] = [
                (
                    SwipeDirection::Right,
                    breathe,
                    LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)),
                    "step right walks into the rainbow-seven group",
                ),
                (
                    SwipeDirection::Left,
                    breathe,
                    LightState::Breath(COLOR_GROUPS[3].into_breath(BREATH_BASE)),
                    "step left from head wraps to the full-hue sweep",
                ),
                (
                    SwipeDirection::Right,
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 140,
                    },
                    LightState::Solid {
                        color: PALETTE[1],
                        brightness: 140,
                    },
                    "step right advances the palette",
                ),
                (
                    SwipeDirection::Left,
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 60,
                    },
                    LightState::Solid {
                        color: last,
                        brightness: 60,
                    },
                    "step left from head wraps to the last palette entry",
                ),
                (
                    SwipeDirection::Up,
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 60,
                    },
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 64,
                    },
                    "an unknown 60 falls back to head 32 and advances to 64",
                ),
                (
                    SwipeDirection::Up,
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 255,
                    },
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 32,
                    },
                    "the top step wraps to the floor",
                ),
                (
                    SwipeDirection::Down,
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 140,
                    },
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 100,
                    },
                    "step down the brightness ladder",
                ),
                (
                    SwipeDirection::Down,
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 32,
                    },
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 255,
                    },
                    "the floor wraps to the top step",
                ),
            ];
            for (direction, current, expected, why) in cases {
                assert_eq!(
                    target(swipe_event(direction), current),
                    Some(expected),
                    "{why} ({direction:?})"
                );
            }
            // Every axis on Off is a no-op: no color or brightness to step.
            for direction in [
                SwipeDirection::Right,
                SwipeDirection::Left,
                SwipeDirection::Up,
                SwipeDirection::Down,
            ] {
                assert_eq!(
                    translate(&op(swipe_event(direction)), &state(LightState::Off)),
                    BusinessIntent::Invalid,
                    "{direction:?} on Off has no table to step"
                );
            }
        }

        #[test]
        fn shift_the_brightness_envelope_together() {
            // Lifting and sinking move both bounds together from the defaults.
            assert_eq!(
                target(swipe_event(SwipeDirection::Up), DEFAULT_BREATH),
                Some(LightState::Breath(Breath {
                    min_brightness: 24 + BRIGHTNESS_ENVELOPE_STEP,
                    max_brightness: 80 + BRIGHTNESS_ENVELOPE_STEP,
                    ..BREATH_BASE
                }))
            );
            assert_eq!(
                target(swipe_event(SwipeDirection::Down), DEFAULT_BREATH),
                Some(LightState::Breath(Breath {
                    min_brightness: 0,
                    max_brightness: 48,
                    ..BREATH_BASE
                }))
            );
            // At the walls the bounds clamp while the guards keep the span.
            let at_bottom = Breath {
                min_brightness: 8,
                max_brightness: 40,
                ..BREATH_BASE
            };
            let sunk = shift_envelope(at_bottom, false);
            assert_eq!(sunk.min_brightness, 0, "floor clamps at 0");
            assert_eq!(
                sunk.max_brightness, BRIGHTNESS_ENVELOPE_STEP,
                "ceiling rides the wall guard so the envelope keeps its span"
            );
            let at_top = Breath {
                min_brightness: 240,
                max_brightness: 255,
                ..BREATH_BASE
            };
            let lifted = shift_envelope(at_top, true);
            assert_eq!(
                lifted.min_brightness,
                255 - BRIGHTNESS_ENVELOPE_STEP,
                "floor parks at 255-guard so the envelope keeps its span"
            );
            assert_eq!(lifted.max_brightness, 255, "ceiling clamps at 255");
            // Hammer both walls from a mid envelope: a full step of span must
            // survive every press, so `LO HI` never reads equal.
            let mut breath = Breath {
                min_brightness: 24,
                max_brightness: 80,
                ..BREATH_BASE
            };
            for _ in 0..32 {
                breath = shift_envelope(breath, true);
                assert!(
                    breath.max_brightness as u16 - breath.min_brightness as u16
                        >= u16::from(BRIGHTNESS_ENVELOPE_STEP),
                    "upward shifts keep at least a full step of span"
                );
            }
            for _ in 0..32 {
                breath = shift_envelope(breath, false);
                assert!(
                    breath.max_brightness as u16 - breath.min_brightness as u16
                        >= u16::from(BRIGHTNESS_ENVELOPE_STEP),
                    "downward shifts keep at least a full step of span"
                );
            }
        }

        #[test]
        fn diagonal_axis_targets_identity() {
            // The diagonals are recognized and tallied by the input stack and
            // resolve to an identity target — the current light unchanged — so
            // the tally sees them while a straight axis still owns each
            // adjustment.
            for direction in [
                SwipeDirection::UpLeft,
                SwipeDirection::UpRight,
                SwipeDirection::DownLeft,
                SwipeDirection::DownRight,
            ] {
                assert_eq!(
                    target(swipe_event(direction), DEFAULT_BREATH),
                    Some(DEFAULT_BREATH),
                    "{direction:?} must not move the light"
                );
            }
        }
    }

    mod routing {
        use super::*;

        fn two_lights() -> DeviceState {
            DeviceState {
                lights: [
                    LightState::Off,
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 140,
                    },
                ],
                ..state(LightState::Off)
            }
        }

        #[test]
        fn button_source_routes_to_its_light_surface() {
            // A click from wiring-order source 1 advances only surface 1.
            assert_eq!(
                translate(
                    &recognize(InputEvent::Button(ButtonEvent::Click), 1),
                    &two_lights()
                ),
                BusinessIntent::SetLight {
                    instance: 1,
                    state: LightState::Solid {
                        color: PALETTE[1],
                        brightness: 140,
                    },
                },
                "each button drives the surface its wiring order names"
            );
            // A long press from source 1 cycles surface 1's own mode: solid off.
            assert_eq!(
                translate(&recognize(long_press(), 1), &two_lights()),
                BusinessIntent::SetLight {
                    instance: 1,
                    state: LightState::Off,
                },
                "the mode cycle walks surface 1 from its own solid head"
            );
        }

        #[test]
        fn out_of_range_source_clamps_to_last_surface() {
            // A source beyond the installed boards (e.g. a driver token
            // mismatch) must never panic the interpreter: it drives the last
            // surface.
            assert_eq!(
                translate(
                    &recognize(InputEvent::Button(ButtonEvent::DoubleClick), 7),
                    &two_lights()
                ),
                BusinessIntent::SetLight {
                    instance: 1,
                    state: LightState::Solid {
                        color: PALETTE[0],
                        brightness: 200,
                    },
                },
                "out-of-range sources clamp to the last surface"
            );
        }

        #[test]
        fn panel_touches_only_the_first_surface() {
            // The device-level panel drives surface 0: a tap on an `Off` first
            // surface is a no-op there, never leaking onto surface 1.
            assert_eq!(
                translate(&op(tap_gesture(0)), &two_lights()),
                BusinessIntent::Invalid,
                "a tap on the panel touches only the first surface"
            );
        }
    }

    mod tables {
        use super::*;

        #[test]
        fn advance_falls_back_to_head_when_current_not_in_table() {
            assert_eq!(advance(&BREATH_PERIODS_MS, 9999), BREATH_PERIODS_MS[1]);
            assert_eq!(advance_palette(Rgb(0, 0, 0)), PALETTE[1]);
            assert_eq!(
                advance_u8(&SOLID_BRIGHTNESS_STEPS, 7),
                SOLID_BRIGHTNESS_STEPS[1]
            );
            let unknown = Breath {
                group: [200, 250, 30, 0, 0, 0, 0],
                group_len: 3,
                ..BREATH_BASE
            };
            assert_eq!(advance_color_group(&unknown), COLOR_GROUPS[1]);
        }

        #[test]
        fn retreat_and_table_step_wrap_backwards() {
            let last = *PALETTE.last().unwrap();
            assert_eq!(retreat_palette(PALETTE[0]), last);
            assert_eq!(advance_palette(last), PALETTE[0]);
            assert_eq!(retreat_u8(&SOLID_BRIGHTNESS_STEPS, 32), 255);
            assert_eq!(retreat_u8(&SOLID_BRIGHTNESS_STEPS, 140), 100);
            assert_eq!(table_step(&SOLID_BRIGHTNESS_STEPS, 7, -1), 255);
        }

        #[test]
        fn seven_color_preset_uses_every_hue_slot() {
            let expected: [u8; GROUP_CAPACITY] = [0, 21, 43, 85, 128, 170, 213];
            let rainbow = COLOR_GROUPS[1];
            assert_eq!(rainbow.group_len, expected.len() as u8);
            assert_eq!(rainbow.group, expected);
        }
    }

    mod network_page {
        use super::*;
        use crate::drivers::network::{AccessPoint, ApList, Ssid};
        use crate::keyboard::{self, Key};

        fn ap(ssid: &[u8], rssi: i8) -> AccessPoint {
            AccessPoint {
                ssid: Ssid::from_octets(ssid),
                bssid: [1, 2, 3, 4, 5, 6],
                channel: 6,
                rssi,
                secured: true,
            }
        }

        /// The page with one network in range and the link still off, which is how the product
        /// ships: the list is the only content, and the keyboard is what a band opens.
        fn on_network() -> DeviceState {
            DeviceState {
                aps: Some(ApList::from_ranked(&[ap(b"neighbour", -55)])),
                ..state_on(DisplayPage::Network, DEFAULT_BREATH)
            }
        }

        fn typing() -> DeviceState {
            DeviceState {
                setup: crate::state::NetworkSetup {
                    selected: Some(ap(b"neighbour", -55)),
                    entry: Some(crate::state::Entry::default()),
                    keyboard: true,
                    ..Default::default()
                },
                ..on_network()
            }
        }

        fn tap_at(x: u16, y: u16) -> InputEvent {
            InputEvent::Gesture(GestureEvent::Tap {
                id: 0,
                x,
                y,
                held_ms: 40,
                at_ms: 1_000,
            })
        }

        /// A finger that stays where it landed, which is the gesture the page reads as "about this
        /// row" rather than "this row".
        fn long_press_at(x: u16, y: u16) -> InputEvent {
            InputEvent::Gesture(GestureEvent::LongPress {
                id: 0,
                x,
                y,
                held_ms: u16::from(crate::drivers::input::FINGER_LONG_PRESS) + 100,
            })
        }

        /// The page with a question on it, which is its third state: the list and the strip are
        /// behind the box, and only the box's own two answers mean anything.
        fn asking() -> DeviceState {
            DeviceState {
                setup: crate::state::NetworkSetup {
                    confirm: Some(crate::state::Confirm::Forget(ap(b"neighbour", -55))),
                    ..Default::default()
                },
                ..on_network()
            }
        }

        fn swipe(direction: SwipeDirection) -> InputEvent {
            InputEvent::Gesture(GestureEvent::Swipe {
                id: 0,
                direction,
                x: 100,
                y: 100,
                end_x: 100,
                end_y: 40,
                held_ms: 60,
                distance_px: 60,
            })
        }

        /// The middle of the key in `cell`, as a finger would land on it. The geometry does not
        /// depend on the layer — a key is one cell of the grid wherever the labels have moved to —
        /// which is the half of "a label and its target are the same cell" a tap can assert.
        fn key_tap(cell: usize) -> InputEvent {
            let rect = keyboard::key_box(cell).expect("a cell of the grid");
            tap_at((rect.x + rect.w / 2) as u16, (rect.y + rect.h / 2) as u16)
        }

        #[test]
        fn a_tap_on_a_band_is_that_band_of_the_list() {
            // Every row from the first band to the bottom of the page is a band of some kind: the
            // pixels below the last row's glyphs are part of it, because a touch target with dead
            // strips in it is a list a thumb keeps missing.
            let state = on_network();
            let first = keyboard::LIST_TOP as u16;
            assert_eq!(
                translate(&op(tap_at(30, first)), &state),
                BusinessIntent::Row(0),
                "the list's first band is band 0"
            );
            assert_eq!(
                translate(&op(tap_at(30, first + keyboard::LIST_ROW_H as u16)), &state),
                BusinessIntent::Row(1),
                "a band down is the next band"
            );
            assert_eq!(
                translate(
                    &op(tap_at(30, first + keyboard::LIST_ROW_H as u16 - 1)),
                    &state
                ),
                BusinessIntent::Row(0),
                "and the pixels below a band's glyphs still belong to that band"
            );
            assert_eq!(
                translate(&op(tap_at(30, 5)), &state),
                BusinessIntent::Invalid,
                "while the page's own header is not a band"
            );
        }

        #[test]
        fn a_slide_moves_the_list_and_only_while_the_keyboard_is_down() {
            let state = on_network();
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Up)), &state),
                BusinessIntent::ListSlide(true),
                "a finger pushing the list up walks down it"
            );
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Down)), &state),
                BusinessIntent::ListSlide(false)
            );
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Left)), &state),
                BusinessIntent::Invalid,
                "the list is one column, so there is nothing to slide sideways to"
            );
            // A passphrase has the keyboard up, and the list is not what the finger is on: a
            // slide there must not page a list the user cannot see.
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Up)), &typing()),
                BusinessIntent::Invalid
            );
        }

        #[test]
        fn the_strip_is_the_page_s_own_control_and_the_bands_stop_above_it() {
            // One band under the list, and every column of it is the strip: the page's own action
            // is a band rather than a button in a band. The bands above it stop at it, so a tap
            // that means the strip can never be read as a row. The strip is split only while setup
            // mode carries two verbs; with one, both halves are the same control.
            let state = on_network();
            let strip = keyboard::toolbar_box();
            for (x, cell) in [
                (0, keyboard::ToolbarCell::Left),
                (strip.w / 2 - 1, keyboard::ToolbarCell::Left),
                (strip.w / 2, keyboard::ToolbarCell::Right),
                (strip.w - 1, keyboard::ToolbarCell::Right),
            ] {
                for y in [strip.y, strip.y + strip.h / 2, strip.y + strip.h - 1] {
                    assert_eq!(
                        translate(&op(tap_at(x as u16, y as u16)), &state),
                        BusinessIntent::Toolbar(cell),
                        "({x}, {y}) is the strip"
                    );
                }
            }
            assert_eq!(
                translate(&op(tap_at(160, (keyboard::TOOLBAR_TOP - 1) as u16)), &state),
                BusinessIntent::Row(keyboard::LIST_ROWS as u8 - 1),
                "the row above the strip is still the list's own last band"
            );
        }

        #[test]
        fn the_button_is_the_strip_verb_and_the_setup_box_asks_about_its_passphrase() {
            // The page paints no colour fill, so a click has nothing to walk: it is the strip's
            // verb instead, which is the switch a board with a button and no thumb needs.
            assert_eq!(
                translate(&op(InputEvent::Button(ButtonEvent::Click)), &on_network()),
                BusinessIntent::Toolbar(keyboard::ToolbarCell::Right)
            );

            // The box a phone's setup is read in asks one question, and both of its answers are
            // buttons: `CLOSE` goes back to the panel, the other puts the access point up open. A
            // tap on the box's own paper is nothing — the access point a phone is walking onto
            // must not close under a stray finger.
            let announcing = DeviceState {
                link: LinkStatus {
                    setup: true,
                    front: Front::AccessPoint,
                    setup_password: Some(crate::drivers::network::SetupPass::from_array(
                        *b"k3mq9t7w2x",
                    )),
                    ..Default::default()
                },
                ..on_network()
            };
            let keep = keyboard::dialog_answer_box(keyboard::DialogChoice::Keep);
            let act = keyboard::dialog_answer_box(keyboard::DialogChoice::Act);
            assert_eq!(
                translate(
                    &op(tap_at(keep.x as u16 + 1, keep.y as u16 + 1)),
                    &announcing
                ),
                BusinessIntent::TogglePass
            );
            assert_eq!(
                translate(&op(tap_at(act.x as u16 + 1, act.y as u16 + 1)), &announcing),
                BusinessIntent::Toolbar(keyboard::ToolbarCell::Right)
            );
            assert_eq!(
                translate(&op(tap_at(10, 10)), &announcing),
                BusinessIntent::Invalid
            );
            assert_eq!(
                translate(&op(long_press_at(10, 10)), &announcing),
                BusinessIntent::Invalid
            );
        }

        #[test]
        fn a_long_press_offers_the_access_points_front_whatever_page_is_up() {
            // The reason it is not on the Network page's own matcher: a panel whose touch stopped
            // answering cannot reach that page, and setup mode is the one state where a button is
            // the only way in.
            let setup = DeviceState {
                link: LinkStatus {
                    setup: true,
                    ..LinkStatus::default()
                },
                ..state_on(DisplayPage::Ambient, LightState::Off)
            };
            assert_eq!(
                translate(&op(long_press()), &setup),
                BusinessIntent::Toolbar(keyboard::ToolbarCell::Right)
            );
        }

        #[test]
        fn the_splash_page_is_where_the_button_asks_for_setup_mode() {
            // The page is the whole of the window: a device on it has just booted, so a hold there
            // is the request rather than the page's own verb — on every board, because whether the
            // link is up is not a question this page can answer (it waits for the page to close
            // before it reads a credential at all).
            let booting = state_on(DisplayPage::Splash, LightState::Off);
            assert_eq!(
                translate(&op(long_press()), &booting),
                BusinessIntent::Setup
            );
        }

        #[test]
        fn the_splash_page_answers_nothing_but_the_hold() {
            // No page switch, no colour walk, no gesture: the page is a splash, and the one thing
            // a finger can do on it is ask for setup mode. The triple click is here because it is
            // the page switch, and the point is that a device that just started cannot be walked
            // into a page before its own clock has said so.
            let booting = state_on(DisplayPage::Splash, LightState::Off);
            for event in [
                InputEvent::Button(ButtonEvent::Click),
                InputEvent::Button(ButtonEvent::DoubleClick),
                InputEvent::Button(ButtonEvent::TripleClick),
                tap_gesture(0),
                swipe_event(SwipeDirection::Up),
                InputEvent::Gesture(GestureEvent::LongPress {
                    id: 0,
                    x: 0,
                    y: 0,
                    held_ms: 800,
                }),
            ] {
                assert_eq!(
                    translate(&op(event), &booting),
                    BusinessIntent::Invalid,
                    "{event:?} on the splash"
                );
            }
            // And the hold is the same request whether the board has a link, nothing saved, or no
            // radio at all: the link is not read until this page is over.
            let with_link = DeviceState {
                link: LinkStatus {
                    phase: crate::drivers::network::LinkPhase::Connected,
                    ..LinkStatus::default()
                },
                ..state_on(DisplayPage::Splash, LightState::Off)
            };
            assert_eq!(
                translate(&op(long_press()), &with_link),
                BusinessIntent::Setup
            );
        }

        #[test]
        fn a_long_press_once_the_splash_is_over_is_the_pages_own_verb() {
            // The window is the page and nothing else: a device that has moved on gives the same
            // hold to the page, so a working link is never spent by an idle finger.
            let on_air = DeviceState {
                link: LinkStatus {
                    phase: crate::drivers::network::LinkPhase::Connected,
                    ..LinkStatus::default()
                },
                ..state_on(DisplayPage::Ambient, LightState::Off)
            };
            assert_eq!(
                translate(&op(long_press()), &on_air),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: DEFAULT_BREATH,
                }
            );
        }

        #[test]
        fn a_hold_on_a_band_is_that_band_asked_about() {
            // The second verb for a row: the same geometry as a tap, because the finger is aiming
            // at the page either way — what differs is the question, and the state layer's.
            let state = on_network();
            let first = keyboard::LIST_TOP as u16;
            assert_eq!(
                translate(&op(long_press_at(30, first)), &state),
                BusinessIntent::RowHold(0)
            );
            assert_eq!(
                translate(
                    &op(long_press_at(30, first + keyboard::LIST_ROW_H as u16)),
                    &state
                ),
                BusinessIntent::RowHold(1)
            );
            assert_eq!(
                translate(&op(long_press_at(30, 5)), &state),
                BusinessIntent::Invalid,
                "the header is not a row to ask about"
            );
            assert_eq!(
                translate(
                    &op(long_press_at(160, keyboard::TOOLBAR_TOP as u16)),
                    &state
                ),
                BusinessIntent::Invalid,
                "and neither is the strip"
            );
            assert_eq!(
                translate(&op(long_press_at(30, first)), &typing()),
                BusinessIntent::Invalid,
                "while the keyboard is up there is no list under the finger to hold"
            );
        }

        #[test]
        fn a_question_is_answered_by_where_a_finger_lands_and_by_nothing_else() {
            // The one gesture that does what the page is asking about is the tap on the box's own
            // action button. Everything else a finger can do to the page means the action is left
            // undone, which is what makes the mistake cheap enough to ask at all.
            let state = asking();
            let act = keyboard::dialog_answer_box(keyboard::DialogChoice::Act);
            let keep = keyboard::dialog_answer_box(keyboard::DialogChoice::Keep);
            assert_eq!(
                translate(
                    &op(tap_at(
                        (act.x + act.w / 2) as u16,
                        (act.y + act.h / 2) as u16
                    )),
                    &state
                ),
                BusinessIntent::Confirm,
                "the action's own button is the whole of the yes"
            );
            assert_eq!(
                translate(
                    &op(tap_at(
                        (keep.x + keep.w / 2) as u16,
                        (keep.y + keep.h / 2) as u16
                    )),
                    &state
                ),
                BusinessIntent::Dismiss,
                "and the button that says no says no"
            );
            for (x, y) in [
                (30, 5),
                (30, keyboard::LIST_TOP as u16 + 2),
                (160, keyboard::TOOLBAR_TOP as u16 + 2),
                (
                    (keyboard::DIALOG_X + 2) as u16,
                    (keyboard::DIALOG_TOP + 2) as u16,
                ),
            ] {
                assert_eq!(
                    translate(&op(tap_at(x, y)), &state),
                    BusinessIntent::Dismiss,
                    "({x}, {y}) leaves the question undone"
                );
            }
            // A gesture that is not a tap answers the same way: the question is answered by the
            // answer rather than by the shape of the finger's travel.
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Up)), &state),
                BusinessIntent::Dismiss,
                "a swipe under the box does not slide a list it cannot see"
            );
            assert_eq!(
                translate(
                    &op(long_press_at(30, keyboard::LIST_TOP as u16 + 2)),
                    &state
                ),
                BusinessIntent::Dismiss
            );
            // And the strip's own intent is not raised while the page is asking: the answer that
            // is not the action is a dismissal rather than a second action.
            assert_eq!(
                translate(&op(tap_at(160, keyboard::TOOLBAR_TOP as u16 + 2)), &state),
                BusinessIntent::Dismiss
            );
        }

        #[test]
        fn a_tap_off_the_page_s_own_content_is_nothing() {
            // The page draws a readout on a black fill instead of the light's colour, so a tap
            // that misses every band and every key does nothing at all: there is no colour walk to
            // fall back on where the page never shows the colour. The gesture that leaves the
            // page is the button's triple click (see `intent`'s page gate).
            let header = tap_at(30, 5);
            assert_eq!(
                translate(&op(header), &on_network()),
                BusinessIntent::Invalid,
                "the page's header is not a control and not a colour walk"
            );
            let state = typing();
            assert_eq!(
                translate(&op(tap_at(30, 5)), &state),
                BusinessIntent::Invalid,
                "and the keyboard's own header is not a key"
            );
        }

        #[test]
        fn a_tap_while_the_keyboard_is_up_types_the_key_under_it() {
            let state = typing();
            // A character key, a control, and the way out: all three read back through the
            // geometry the panel draws with, so a label and the target that types it are the same
            // cell.
            assert_eq!(
                translate(&op(key_tap(1)), &state),
                BusinessIntent::Key {
                    key: Key::Char(b'2'),
                    at_ms: 1_000,
                },
                "the phone layer's second key is the one printed 2"
            );
            assert_eq!(
                translate(
                    &op(key_tap(
                        keyboard::cell_of(Key::Shift).expect("a control's own cell")
                    )),
                    &state
                ),
                BusinessIntent::Key {
                    key: Key::Shift,
                    at_ms: 1_000,
                }
            );
            assert_eq!(
                translate(
                    &op(key_tap(
                        keyboard::cell_of(Key::Cancel).expect("a control's own cell")
                    )),
                    &state
                ),
                BusinessIntent::Key {
                    key: Key::Cancel,
                    at_ms: 1_000,
                }
            );
            assert_eq!(
                translate(
                    &op(key_tap(
                        keyboard::cell_of(Key::Reveal).expect("a control's own cell")
                    )),
                    &state
                ),
                BusinessIntent::Key {
                    key: Key::Reveal,
                    at_ms: 1_000,
                }
            );
            // Above the keyboard is the page's own text, which is not a key: a tap that missed
            // types nothing rather than walking the colour under a keyboard in use.
            assert_eq!(
                translate(&op(tap_at(30, 5)), &state),
                BusinessIntent::Invalid
            );
            // Nor is anything below the last row, which the FPS strip owns.
            assert_eq!(
                translate(
                    &op(tap_at(
                        30,
                        (keyboard::KEYBOARD_TOP + keyboard::ROWS * keyboard::KEY_H) as u16
                    )),
                    &state
                ),
                BusinessIntent::Invalid
            );
        }

        #[test]
        fn the_clock_at_the_lift_is_what_the_key_carries() {
            // The multi-tap window is measured from one tap to the next, so the tap's own clock
            // reading has to travel with the key: a key that arrived without it would be typed as
            // the first candidate of every character.
            assert_eq!(
                translate(
                    &op(InputEvent::Gesture(GestureEvent::Tap {
                        id: 0,
                        x: keyboard::KEY_W as u16 / 2,
                        y: (keyboard::KEYBOARD_TOP + keyboard::KEY_H / 2) as u16,
                        held_ms: 40,
                        at_ms: 4_321,
                    })),
                    &typing()
                ),
                BusinessIntent::Key {
                    key: Key::Char(b'1'),
                    at_ms: 4_321,
                }
            );
        }
    }
}
