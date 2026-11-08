use crate::shared::keyboard::Key;
use crate::shared::state::LightState;
use iot_core::drivers::input::InputEvent;
use iot_core::drivers::network::{AccessPoint, LinkStatus, Ssid};

/// The action a confirmation is standing in front of. It is the page's question, kept with
/// the action vocabulary because the answer is an intent like any other.
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

/// What the device is asked to do, grouped by the domain that owns the answer. The manager
/// routes one arm per domain; each domain matches its own group exhaustively, so a new
/// variant cannot be left unanswered and the manager never learns a domain's actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionIntent {
    /// Nothing the panel asked for survived translation.
    Invalid,
    Page(PageIntent),
    /// What the radio reports. Written by the worker, never by a finger.
    Link(LinkIntent),
    /// What a finger asks of the Network page.
    NetworkPage(NetworkPageIntent),
    Audio(AudioIntent),
    Playback(PlaybackAction),
    Light(LightIntent),
}

/// The page's own transitions: the splash ends, the button asks for setup, the page turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageIntent {
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
    LinkReport(LinkStatus),
    Mac([u8; 6]),
}

/// The Audio page's one transition: a recording starts, or the one in progress stops and
/// keeps what it captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioIntent {
    Start,
    Stop,
}

/// An absolute target for one light surface, clamped by the manager to the wired slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightIntent {
    pub instance: u8,
    pub state: LightState,
}

/// What the Speaker page asks of the output, or the report that comes back. A tap while the
/// part is down still reaches the worker: that is the recovery attempt, and `Started` is what
/// confirms a sound actually played.
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
    Action(ActionIntent),
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
    use crate::state::{DeviceState, DisplayPage, LightState, NetworkState};
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
            ..state_on(DisplayPage::Ambient, LightState::Off)
        };
        assert_eq!(
            translate(&op(long_press()), &setup),
            ActionIntent::NetworkPage(NetworkPageIntent::SwitchHost)
        );
    }
}
