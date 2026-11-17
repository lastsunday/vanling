//! The Speaker page: what a tap plays, the mute latch, and the readout the output reports.

#[cfg(test)]
mod tests;

use iot_core::paint::Rgb;

use crate::shared::diagnostics::Diagnostics;
use crate::shared::intent::{BusinessIntent, InputIntent, LightAction, PlaybackAction};
use crate::shared::kit::surface::Surface;
use crate::shared::kit::text::{format_u16, sound_label};
use crate::shared::model::{DeviceState, PlaybackPhase};
use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent};
use iot_core::overlay::{Block, ROW_PITCH, block_x, overlay_row_top};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpeakerPage;

pub(super) const GROUND: Option<Rgb> = None;
pub(super) const CAMERA_FRAMES: bool = false;
pub(super) const REPAINT_MS: u64 = super::REPAINT_FLOOR_MS;
pub(super) const REPAINTS: super::Repaint = super::Repaint::When(playback_moved);

impl super::Page for SpeakerPage {}

impl super::LocalPage for SpeakerPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> BusinessIntent {
        match op.event {
            InputEvent::Button(ButtonEvent::Click)
            | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
                BusinessIntent::Playback(PlaybackAction::Next)
            }
            InputEvent::Button(ButtonEvent::LongPress)
            | InputEvent::Gesture(GestureEvent::LongPress { .. }) => {
                BusinessIntent::Playback(PlaybackAction::SetMuted(!state.playback.muted))
            }
            InputEvent::Button(ButtonEvent::DoubleClick) => {
                state.light_intent(op, LightAction::Step)
            }
            InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
                state.light_swipe(op, direction)
            }
            _ => BusinessIntent::Invalid,
        }
    }
}

fn playback_moved(prev: &Diagnostics, next: &Diagnostics) -> bool {
    prev.playback != next.playback
}

impl Surface<'_> {
    pub(crate) fn paint_speaker(&mut self) {
        let playback = self.diagnostics.playback;
        let fault = playback.health.fault();
        self.draw_text(b"SPKR", block_x(Block::First), overlay_row_top(0));
        self.draw_left(
            b"ST",
            match (fault, playback.phase) {
                (Some(_), _) => b"NO OUT",
                (None, PlaybackPhase::Idle) => b"IDLE",
                (None, PlaybackPhase::Playing) => b"PLAY",
            },
            Block::First,
            1,
        );
        self.draw_left(b"SRC", sound_label(playback.sound), Block::First, 2);
        if playback.muted {
            self.draw_text(b"MUTE", block_x(Block::Second), overlay_row_top(1));
        }
        self.draw_left(
            b"PLY",
            format_u16(playback.plays, &mut [0u8; 6]),
            Block::First,
            3,
        );
        self.draw_left(
            b"DRP",
            format_u16(playback.dropped, &mut [0u8; 6]),
            Block::First,
            4,
        );
        let hint = overlay_row_top(6);
        match fault {
            Some(reason) => {
                self.draw_text(b"TAP RETRY", block_x(Block::First), hint);
                let reason = reason.as_bytes();
                self.draw_text(reason, block_x(Block::First), hint + ROW_PITCH);
            }
            None => {
                self.draw_text(b"TAP PLAY", block_x(Block::First), hint);
                self.draw_text(b"HOLD MUTE", block_x(Block::First), hint + ROW_PITCH);
            }
        }
    }
}
