//! One chain, five words: `Event` → `Intent` → `Action` → `State` (RAM) → `Hardware`. An action
//! only writes state; hardware only reads it. The Network key grid lives in `keyboard`.

mod keyboard;

pub use self::keyboard::{CHARS, Cycling, Key, Layer, MAX_CANDIDATES, MULTITAP_WINDOW_MS};

use iot_core::drivers::input::{InputEvent, SwipeDirection};
use iot_core::drivers::network::{AccessPoint, LinkStatus, Ssid};

/// The page's question, kept with the business vocabulary because the answer is an intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    Leave,
    Forget(AccessPoint),
}

impl Confirm {
    /// The network this question is about: the row the page highlights behind it.
    pub fn network(&self, link: LinkStatus) -> Option<AccessPoint> {
        match self {
            Confirm::Leave => link.ap,
            Confirm::Forget(ap) => Some(*ap),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputIntent {
    /// Wiring-order source that raised the event.
    pub source: u8,
    /// The input signal, verbatim from the driver.
    pub event: InputEvent,
}

impl InputIntent {
    /// A button carries its wiring order; a touch is the panel's own surface.
    pub const fn surface(&self) -> u8 {
        match self.event {
            InputEvent::Button(_) => self.source,
            _ => 0,
        }
    }
}

/// What the device must act on, grouped by the record it writes; each owner matches its group
/// exhaustively, so a new variant cannot be left unanswered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusinessIntent {
    /// Where the device goes: the boot splash, the page cycle, the setup request.
    Navigation(NavigationIntent),
    /// What the radio reports. Written by the worker, never by a finger.
    Link(LinkIntent),
    /// What a finger asks of the network record: the draft, the list, the box.
    Network(NetworkIntent),
    Audio(AudioIntent),
    Playback(PlaybackAction),
    /// What a finger asks of a page's own scroll regions.
    Text(TextIntent),
    /// What a finger asks of the Menu's button grid.
    Menu(MenuIntent),
    /// What a finger asks of the Widgets page: a control, its slider, or the gallery's scroll.
    Widgets(WidgetsIntent),
    /// One light surface and the verb it is asked for; the instance is routing, not part of the verb.
    Light {
        instance: u8,
        action: LightAction,
    },
}

/// The device's own transitions: the splash ends, a hold asks for setup, the button turns the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationIntent {
    NextPage,
    /// A destination the user picked, by the route table's wire code: this layer names what
    /// is asked, not which page answers.
    Go(u8),
    SplashOver,
    RequestSetup,
}

/// What the panel's fingers ask of the network record: the draft, the list, the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkIntent {
    NoCopy(Ssid),
    Key { key: Key, at_ms: u32 },
    Choose(AccessPoint),
    AskForget(AccessPoint),
    Scroll { rows: i8 },
    Leave,
    Answer(Confirm),
    CloseSetup,
    SwitchHost,
    ScanNow,
    SetOpenAccessPoint(bool),
    Dismiss,
}

/// What the radio reports, and nothing the page decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkIntent {
    Status(LinkStatus),
    Mac([u8; 6]),
    /// A scan round landed; the record already holds the list, so this is only the notice.
    Scan,
}

/// The Audio page's one transition: start a recording, or stop and keep what it captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioIntent {
    Start,
    Stop,
}

/// What one swipe asks of a scroll region: which region, how many lines, and the release speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextIntent {
    /// A finger landed in a region: the region is now held.
    Press {
        id: u8,
        area: u8,
        y: i16,
    },
    /// That finger moved, in panel pixels.
    Drag {
        id: u8,
        x: u16,
        y: u16,
        at_ms: u32,
    },
    /// The lift. Falls back to `lines`/`speed` without a drag sample; `at_ms` is `None` for a hold.
    Release {
        id: u8,
        area: u8,
        lines: i8,
        speed: u16,
        at_ms: Option<u32>,
    },
    Scroll {
        area: u8,
        lines: i8,
        speed: u16,
    },
}

/// What the Menu's grid asks of its one scroll region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuIntent {
    /// A finger landed in the box: a glide in the region stops where it is.
    Press {
        area: u8,
    },
    Drag {
        id: u8,
        x: u16,
        y: u16,
        at_ms: u32,
    },
    /// The lift. Falls back to `lines`/`speed` without a drag sample; `at_ms` is `None` for a hold.
    Release {
        id: u8,
        area: u8,
        lines: i8,
        speed: u16,
        at_ms: Option<u32>,
    },
    Scroll {
        area: u8,
        lines: i8,
        speed: u16,
    },
}

/// What the Widgets page's controls and gallery ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetsIntent {
    /// A control was pressed; `tag` is the one the page drew there.
    Press {
        tag: u16,
    },
    /// The lift landed on the same control: toggle it.
    Tap {
        tag: u16,
    },
    /// A slider's finger moved to `x` on the panel.
    Slide {
        tag: u16,
        x: u16,
    },
    /// A finger landed in the gallery's box rather than on a control.
    Grab {
        id: u8,
        area: u8,
        y: i16,
    },
    Drag {
        id: u8,
        x: u16,
        y: u16,
        at_ms: u32,
    },
    Release {
        id: u8,
        area: u8,
        lines: i8,
        speed: u16,
        at_ms: Option<u32>,
    },
    Scroll {
        area: u8,
        lines: i8,
        speed: u16,
    },
}

/// The lamp's verbs; `Cycle` is the only one that leaves `Off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightAction {
    /// Off → breath → solid → off.
    Cycle,
    /// The next colour: the palette in solid, the hue group in breath.
    NextColor { forward: bool },
    /// The next step of the mode's own scalar: breath period, or solid brightness.
    Step,
    /// Shift the breathing envelope, or step solid brightness.
    Brightness { up: bool },
}

impl LightAction {
    /// The step a resolved swipe asks for. A diagonal has no axis, so it asks for nothing.
    pub const fn from_swipe(direction: SwipeDirection) -> Option<Self> {
        match direction {
            SwipeDirection::Left => Some(Self::NextColor { forward: false }),
            SwipeDirection::Right => Some(Self::NextColor { forward: true }),
            SwipeDirection::Up => Some(Self::Brightness { up: true }),
            SwipeDirection::Down => Some(Self::Brightness { up: false }),
            SwipeDirection::UpLeft
            | SwipeDirection::UpRight
            | SwipeDirection::DownLeft
            | SwipeDirection::DownRight => None,
        }
    }
}

/// The output's trigger table: the page asks with `Next`/`SetMuted`, the worker answers with
/// `Finished`/`Started`/`Ready`/`Refused`. A tap while the part is down is the recovery attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackAction {
    Next,
    SetMuted(bool),
    Finished,
    /// The worker started the sound the page asked for.
    Started,
    /// The worker found the output answering, with no sound involved.
    Ready,
    /// The worker could not carry out the request; the board's own words say why.
    Refused(&'static str),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Input(InputIntent),
    Business(BusinessIntent),
}

/// Stateless recognition: fold a raw input event and its source into an input intent.
pub fn recognize(event: InputEvent, source: u8) -> InputIntent {
    InputIntent { source, event }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iot_core::drivers::input::{ButtonEvent, GestureEvent};

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
            at_ms: 75,
        });
        let intent = recognize(event, 0);
        assert_eq!(intent.source, 0);
        assert_eq!(
            intent.event, event,
            "recognition keeps the gesture's tracking fields byte-for-byte"
        );
    }
}
