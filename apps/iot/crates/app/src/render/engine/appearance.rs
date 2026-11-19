//! What a renderer is told to show, and who each appearance is for.

use crate::shared::model::Breath;
use crate::shared::snapshot::FrameSnapshot;
use iot_core::demand::Mode;
use iot_core::light::Rgb;

/// Who an appearance is for: one wired surface's light, the page frame, or the display gate
/// alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topic {
    Surface(u8),
    Frame,
    Display,
}

/// The declared appearance of a light, derived from state; breathing is the schedule, not
/// a color frame — time-driven rendering is the renderer's call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightAppearance {
    Off,
    Color(Rgb),
    Breathing(Breath),
}

/// What one topic should show now. The frame is borrowed: a tick derives it once and every
/// renderer reads it, so only the screen pays for the copy it keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance<'a> {
    Surface {
        instance: u8,
        light: LightAppearance,
    },
    Frame(&'a FrameSnapshot),
    Display(Mode),
}

impl Appearance<'_> {
    pub fn topic(&self) -> Topic {
        match self {
            Self::Surface { instance, .. } => Topic::Surface(*instance),
            Self::Frame(_) => Topic::Frame,
            Self::Display(_) => Topic::Display,
        }
    }
}
