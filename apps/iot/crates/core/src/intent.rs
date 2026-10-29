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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusinessIntent {
    /// A no-op: the operation recorded its diagnostics but moves nothing
    /// (e.g. a click/tap on `Off`, whose tables have no step to take, or a
    /// pulse/read-back that is diagnostic-only by nature).
    Invalid,
    /// Set the `instance`-th light surface to an absolute target state.
    SetLight { instance: u8, state: LightState },
    /// The button's triple click: the next page in the cycle.
    NextPage,
    /// Start or stop the capture behind the Audio page. Reached from a tap or
    /// the button's click on that page, so whichever one-liner control the user
    /// reaches for drives the recording, and neither shadows a gesture that
    /// means something on the other pages.
    Capture(CaptureAction),
    /// What the Speaker page asks of the output: one sound, the mute latch, or the driver's own
    /// report that a sound ran out.
    Playback(PlaybackAction),
    /// The clock's report that the splash is over. A report, not a target: the page leaves on time,
    /// and the state layer holds the page the build named.
    SplashOver,
    /// The splash's hold: a request to be configured, decided when the splash ends.
    RequestSetup,
    /// Where the link stands, sent by the network task.
    ///
    /// A report rather than a target, like [`PlaybackAction::Finished`]: the radio is the only thing
    /// that has been on the air, so nothing above it can reconstruct an association or a lease.
    /// Never raised by [`translate`].
    LinkReport(LinkStatus),
    /// The radio's answer that a name the panel asked for has no saved copy. It puts the page back
    /// where a passphrase can be typed for that row.
    NoCopy(Ssid),
    /// The board's own address, sent once at boot. It never changes.
    Mac([u8; 6]),
    /// A key of the Network page's keyboard, with the clock at the lift. The state layer decides
    /// what it types; the clock tells a repeat that it is cycling rather than typing.
    Key { key: Key, at_ms: u32 },
    /// Pick the network a finger tapped: the row itself, not the band it occupied. A band is a
    /// page coordinate that moves when the list slides; the access point is what the tap meant.
    Choose(AccessPoint),
    /// Ask about the saved copy of the network a finger held. A hold on a row the store does not
    /// hold asks nothing.
    AskForget(AccessPoint),
    /// Move the scan list by `rows`, positive down. Raised only while the keyboard is down; a
    /// window-sized step cannot land on a partial last page.
    Scroll { rows: i8 },
    /// Leave the link that is up, which is how setup mode is entered.
    Leave,
    /// The action button in a confirmation: the only gesture that says yes. The question travels
    /// with the answer, so a stale tap cannot confirm a newer one. Every other gesture is
    /// [`Self::Dismiss`].
    Answer(Confirm),
    /// Close setup mode and go back to the network this device keeps.
    CloseSetup,
    /// Switch setup mode to its other front end.
    SwitchFront,
    /// Ask the survey for a round now rather than wait out its pause.
    ScanNow,
    /// Put the setup access point up open (`true`) or under its passphrase (`false`). The radio is
    /// the authority on which state it is in; the command names the target.
    SetOpenAccessPoint(bool),
    /// Leave the page's question undone. Every gesture on a page that is asking means this.
    Dismiss,
}

/// The Audio page's one transition: a recording starts, or the one in progress stops and keeps
/// what it captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureAction {
    Start,
    Stop,
}

/// What the Speaker page asks of the output, or the report that comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackAction {
    /// Play the next sound in the catalogue. Whether it is heard is the state layer's call: over a
    /// sounding one it is tallied and dropped, so the command names the request, not the outcome.
    Next,
    /// Move the mute latch to this exact state. A long press on the Speaker page, because muting
    /// is a decision rather than a one-liner verb and should not be reachable by the gesture that
    /// plays.
    SetMuted(bool),
    Finished,
}

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

pub fn translate(op: &OperationIntent, current: &DeviceState) -> BusinessIntent {
    if current.page != DisplayPage::Splash
        && current.link.setup
        && matches!(op.event, InputEvent::Button(ButtonEvent::LongPress))
    {
        return BusinessIntent::SwitchFront;
    }
    page::translate(current.page, op, current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::input::{GestureEvent, SwipeDirection};
    use crate::page::test_support::{long_press, op, state_on};
    use crate::state::LightState;

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
            link: LinkStatus {
                setup: true,
                ..LinkStatus::default()
            },
            ..state_on(DisplayPage::Ambient, LightState::Off)
        };
        assert_eq!(
            translate(&op(long_press()), &setup),
            BusinessIntent::SwitchFront
        );
    }
}
