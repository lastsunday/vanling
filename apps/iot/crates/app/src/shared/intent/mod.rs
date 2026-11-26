//! One chain, five words: `Event` → `Intent` → `Action` → `State` (RAM) → `Hardware`. An action
//! only writes state; hardware only reads it. The Network key grid lives in `keyboard`.

mod action;
mod keyboard;

pub use self::action::WidgetsAction;
pub use self::keyboard::{CHARS, Cycling, Key, Layer, MAX_CANDIDATES, MULTITAP_WINDOW_MS};

use crate::shared::i18n::Lang;
use iot_core::drivers::input::{ClickPattern, InputEvent, SwipeDirection};
use iot_core::drivers::network::{AccessPoint, LinkStatus, Ssid};
use iot_core::ui::ThemeId;

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
    /// What a finger asks of a demo gallery: the tag it touched, or the scroll under it. The
    /// gallery is named by its route wire code, the way `NavigationIntent::Go` names a page.
    Gallery {
        wire: u8,
        intent: GalleryIntent,
    },
    /// What a finger asks of the feed: its rows, its two edges, and the band a swipe uncovers.
    List(ListIntent),
    /// What a finger asks of the device's own record rather than of a page.
    Settings(SettingsIntent),
    /// What a finger asks of the lighting page: its scope, or one of its sliders.
    Lighting(LightingIntent),
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
    /// The user picked a page, named by the route table's wire code.
    Choose {
        page: u8,
    },
    /// A contact landed in the box; `tag` is the button or chip under it, zero for a gap.
    Press {
        id: u8,
        tag: u16,
        x: u16,
        y: u16,
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
    /// A control was pressed; `action` is the one the page drew there.
    Press {
        action: WidgetsAction,
        id: u8,
        x: u16,
        y: u16,
    },
    /// The lift landed on the same control: toggle it.
    Tap {
        action: WidgetsAction,
    },
    /// A slider's finger moved to `x` on the panel.
    Slide {
        action: WidgetsAction,
        x: u16,
    },
    /// A key of the field's keyboard.
    Key {
        key: Key,
        at_ms: u32,
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

/// What a settings control asks of the device's own record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsIntent {
    /// The panel's words from now on; a build names the one it starts in.
    SetLang { lang: Lang },
    /// The panel's palette from now on; a build names the one it starts in.
    SetTheme { theme: ThemeId },
    /// How many presses a button may fold into one gesture, and how long it waits to decide.
    SetClicks {
        pattern: ClickPattern,
        window_ms: u16,
    },
}

/// One control on the lighting page. The discriminant is the wire tag, so the view, the hit
/// test and the handler cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum LightingControl {
    Off = 1,
    Page,
    All,
    Period,
    Hue,
    Peak,
    Backlight,
    PatternBreath,
    PatternSteady,
    PatternCycle,
    PatternAurora,
    PatternCandle,
    PatternPulse,
    PatternHeartbeat,
    PatternLightning,
    PatternSparkle,
    PatternStrobe,
    PatternPolice,
    PatternSos,
}

impl LightingControl {
    pub const ALL: [Self; 19] = [
        Self::Off,
        Self::Page,
        Self::All,
        Self::Period,
        Self::Hue,
        Self::Peak,
        Self::Backlight,
        Self::PatternBreath,
        Self::PatternSteady,
        Self::PatternCycle,
        Self::PatternAurora,
        Self::PatternCandle,
        Self::PatternPulse,
        Self::PatternHeartbeat,
        Self::PatternLightning,
        Self::PatternSparkle,
        Self::PatternStrobe,
        Self::PatternPolice,
        Self::PatternSos,
    ];

    pub const fn tag(self) -> u16 {
        self as u16
    }

    pub fn from_tag(tag: u16) -> Option<Self> {
        Self::ALL.into_iter().find(|control| control.tag() == tag)
    }

    /// Whether the control is a slider rather than a cell of the scope strip.
    pub const fn slider(self) -> bool {
        matches!(
            self,
            Self::Period | Self::Hue | Self::Peak | Self::Backlight
        )
    }
}

/// What a finger asks of the lighting page: a press, the slide that follows it, or the lift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightingIntent {
    Press {
        id: u8,
        control: LightingControl,
    },
    Slide {
        control: LightingControl,
        x: u16,
    },
    Tap {
        control: LightingControl,
        x: u16,
    },
    /// The finger left the glass: whatever it held is no longer held.
    LetGo,
}

/// What a finger asks of a demo gallery: the control it touched, or the scroll under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalleryIntent {
    /// A contact landed; `tag` is the control under it, zero for bare canvas.
    Press {
        id: u8,
        tag: u16,
        x: u16,
        y: u16,
    },
    /// The lift landed on the same control.
    Tap {
        tag: u16,
    },
    /// A key of the field's keyboard.
    Key {
        key: Key,
        at_ms: u32,
    },
    /// One cell of the page's own keypad, `0..12`.
    Pad {
        cell: u8,
    },
    /// A horizontal drag over a slide control: `dx` is travel from where the finger landed.
    Slide {
        id: u8,
        tag: u16,
        dx: i16,
    },
    /// The slide's lift, with its total travel: the page snaps to the card that travel picks.
    SlideEnd {
        id: u8,
        tag: u16,
        dx: i16,
    },
    /// A two-dimensional drag over a canvas that reads both axes: the picker's square and strip.
    Point {
        id: u8,
        tag: u16,
        x: u16,
        y: u16,
    },
    /// A vertical drag on one wheel column; `area` names the wheel set, `y` is panel space.
    Wheel {
        id: u8,
        area: u8,
        column: u8,
        y: i16,
        at_ms: u32,
    },
    /// The wheel's lift: a tap on a row when nothing moved, otherwise the fling.
    WheelEnd {
        id: u8,
        area: u8,
        column: u8,
        y: i16,
        at_ms: u32,
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
    /// The lift, with the point it happened at: a list edge reads both.
    Release {
        id: u8,
        area: u8,
        lines: i8,
        speed: u16,
        at_ms: Option<u32>,
        x: u16,
        y: u16,
    },
    Scroll {
        area: u8,
        lines: i8,
        speed: u16,
    },
}

/// What a finger asks of the feed demo: a row, a scroll, or one of the two edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListIntent {
    /// The finger landed in the list.
    Press { id: u8, x: u16, y: u16 },
    /// The finger moved: horizontal travel uncovers an action, vertical travel scrolls.
    Drag { id: u8, x: u16, y: u16, at_ms: u32 },
    /// The finger lifted, with a swipe's step and speed when the classifier named one.
    Release {
        id: u8,
        x: u16,
        y: u16,
        lines: i8,
        speed: u16,
        at_ms: Option<u32>,
    },
    /// A wheel or a key stepped the list.
    Scroll { lines: i8, speed: u16 },
    /// A lift that the classifier reported as a tap.
    Tap { x: u16, y: u16 },
}

/// The lamp's verbs; `Cycle` and `TogglePower` are the ones that can leave `Off`.
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
    /// The next time-based effect; solid mode enters breathing on its default effect.
    NextEffect,
    /// Off ↔ the last lit state's boot default.
    TogglePower,
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
    fn the_lighting_tags_are_dense_and_read_back() {
        for (index, control) in LightingControl::ALL.iter().enumerate() {
            assert_eq!(control.tag(), index as u16 + 1);
            assert_eq!(LightingControl::from_tag(control.tag()), Some(*control));
        }
        assert_eq!(LightingControl::from_tag(0), None);
        assert_eq!(
            LightingControl::from_tag(LightingControl::ALL.len() as u16 + 1),
            None
        );
    }

    #[test]
    fn only_the_three_breath_rows_and_the_backlight_slide() {
        for control in LightingControl::ALL {
            assert_eq!(
                control.slider(),
                matches!(
                    control,
                    LightingControl::Period
                        | LightingControl::Hue
                        | LightingControl::Peak
                        | LightingControl::Backlight
                ),
                "{control:?}"
            );
        }
    }

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
