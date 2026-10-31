use crate::drivers::input::{ButtonEvent, InputEvent};
use crate::drivers::network::{AccessPoint, LinkStatus, Ssid};
use crate::keyboard::Key;
use crate::page::{self, DisplayPage};
use crate::state::{Confirm, DeviceState, LightState};

pub use crate::page::{
    BREATH_PERIODS_MS, BRIGHTNESS_ENVELOPE_STEP, COLOR_GROUPS, ColorGroup, PALETTE,
    SOLID_BRIGHTNESS_STEPS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationIntent {
    /// Wiring-order source that raised the event.
    pub source: u8,
    /// The raw/folded input signal, verbatim from the driver.
    pub event: InputEvent,
}

/// What the device is asked to do, grouped by the domain that owns the answer. The manager
/// routes one arm per domain; each domain matches its own group exhaustively, so a new
/// variant cannot be left unanswered and the manager never learns a domain's operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusinessIntent {
    /// Nothing the panel asked for survived translation.
    Invalid,
    Page(PageIntent),
    Network(NetworkIntent),
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

/// What the panel asks of the link, and what the link reports back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkIntent {
    LinkReport(LinkStatus),
    NoCopy(Ssid),
    Mac([u8; 6]),
    Key { key: Key, at_ms: u32 },
    Choose(AccessPoint),
    AskForget(AccessPoint),
    Scroll { rows: i8 },
    Leave,
    Answer(Confirm),
    CloseSetup,
    SwitchFront,
    ScanNow,
    SetOpenAccessPoint(bool),
    Dismiss,
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

/// What the Speaker page asks of the output, or the report that comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackAction {
    Next,
    SetMuted(bool),
    Finished,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Operation(OperationIntent),
    Business(BusinessIntent),
}

/// Stateless recognition: fold a raw input event plus its wiring-order source into an
/// operation intent.
pub fn recognize(event: InputEvent, source: u8) -> OperationIntent {
    OperationIntent { source, event }
}

pub fn translate(op: &OperationIntent, current: &DeviceState) -> BusinessIntent {
    if current.page != DisplayPage::Splash
        && current.network.link.setup
        && matches!(op.event, InputEvent::Button(ButtonEvent::LongPress))
    {
        return BusinessIntent::Network(NetworkIntent::SwitchFront);
    }
    page::translate(current.page, op, current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::input::{GestureEvent, SwipeDirection};
    use crate::page::test_support::{long_press, op, state_on};
    use crate::state::{LightState, NetworkState};

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
                    setup: true,
                    ..LinkStatus::default()
                },
                ..NetworkState::default()
            },
            ..state_on(DisplayPage::Ambient, LightState::Off)
        };
        assert_eq!(
            translate(&op(long_press()), &setup),
            BusinessIntent::Network(NetworkIntent::SwitchFront)
        );
    }
}
