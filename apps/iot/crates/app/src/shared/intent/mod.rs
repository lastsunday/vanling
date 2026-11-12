//! Three words, two paths: a driver `Event` folds into an `Intent` on the bus (a finger's
//! request, a worker's report, a boot decision), and a record answers with its own `Action`
//! table. Where an intent is already that table, the record applies it as is.
//!
//! The Network key grid lives in `keyboard`.

mod keyboard;

pub use self::keyboard::{CHARS, Cycling, Key, Layer, MAX_CANDIDATES, MULTITAP_WINDOW_MS};

use iot_core::drivers::input::{InputEvent, SwipeDirection};
use iot_core::drivers::network::{AccessPoint, LinkStatus, Ssid};

/// The action a confirmation is standing in front of. It is the page's question, kept with
/// the business vocabulary because the answer is an intent like any other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    Leave,
    Forget(AccessPoint),
}

impl Confirm {
    /// The network this question is about, which is the row the page highlights behind it.
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
    /// The raw/folded input signal, verbatim from the driver.
    pub event: InputEvent,
}

impl InputIntent {
    /// The surface this input drives: a button carries its wiring order, and a touch is the
    /// panel's own surface.
    pub const fn surface(&self) -> u8 {
        match self.event {
            InputEvent::Button(_) => self.source,
            _ => 0,
        }
    }
}

/// What the device must act on, grouped by the domain that owns the answer. The manager
/// routes one arm per domain; each domain matches its own group exhaustively, so a new
/// variant cannot be left unanswered and the manager never learns a domain's own variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusinessIntent {
    /// Nothing the panel asked for survived translation.
    Invalid,
    /// Where the device goes: the boot splash, the page cycle, the setup request.
    Navigation(NavigationIntent),
    /// What the radio reports. Written by the worker, never by a finger.
    Link(LinkIntent),
    /// What a finger asks of the Network page.
    NetworkPage(NetworkPageIntent),
    Audio(AudioIntent),
    Playback(PlaybackAction),
    /// One light surface and the verb it is asked for. The instance is routing, not part of
    /// the verb: the record's `apply` only ever sees the action.
    Light {
        instance: u8,
        action: LightAction,
    },
}

/// The device's own transitions: the splash ends, a hold asks for setup, the button turns the
/// page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationIntent {
    NextPage,
    SplashOver,
    RequestSetup,
}

/// What the panel's fingers ask of the Network page: the draft, the list, the box. The link
/// is not asked anything here — the page turns these into [`NetworkCommand`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPageIntent {
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
}

/// The Audio page's one transition: a recording starts, or the one in progress stops and
/// keeps what it captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioIntent {
    Start,
    Stop,
}

/// The lamp's verbs. `Cycle` is the only one that leaves `Off`; the rest need a mode to act
/// in, which is what `LightState::can` answers.
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
/// `Finished`/`Started`/`Ready`/`Refused`. One table, because both move one state machine. A
/// tap while the part is down still reaches the worker: that is the recovery attempt, and
/// `Started` is what confirms a sound actually played.
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

/// Stateless recognition: fold a raw input event plus its wiring-order source into an
/// input intent.
pub fn recognize(event: InputEvent, source: u8) -> InputIntent {
    InputIntent { source, event }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::route::test_support::{long_press, op, state_on};
    use crate::state::translate::translate;
    use crate::state::{DeviceState, LightState, NetworkState, Page};
    use iot_core::drivers::input::{ButtonEvent, GestureEvent, SwipeDirection};

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
    #[test]
    fn a_long_press_offers_the_access_points_front_whatever_page_is_up() {
        let setup = DeviceState {
            network: NetworkState {
                link: LinkStatus {
                    mode: iot_core::drivers::network::NetworkMode::Setup {
                        host: iot_core::drivers::network::SetupHost::Panel,
                        cancellable: false,
                    },
                    ..LinkStatus::default()
                },
                ..NetworkState::default()
            },
            ..state_on(Page::Ambient, LightState::Off)
        };
        assert_eq!(
            translate(&op(long_press()), &setup),
            BusinessIntent::NetworkPage(NetworkPageIntent::SwitchHost)
        );
    }
}
