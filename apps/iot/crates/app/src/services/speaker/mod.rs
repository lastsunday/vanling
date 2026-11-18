//! The Speaker page: what a tap plays, the mute latch, and the readout the output reports.

#[cfg(test)]
mod tests;

use crate::shared::intent::{BusinessIntent, InputIntent, LightAction, PlaybackAction};
use crate::shared::kit::painter::Painter;
use crate::shared::kit::text::{format_u16, sound_label};
use crate::shared::model::{Command, DeviceState, Effects, PlaybackPhase};
use crate::shared::snapshot::FrameSnapshot;
use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent};
use iot_core::overlay::{Block, ROW_PITCH, block_x, overlay_row_top};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpeakerPage;

impl super::Service for SpeakerPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        if let BusinessIntent::Playback(action) = intent {
            state.playback.apply(*action, state.backed.playback);
        }
        (None, Effects::default())
    }
}

impl super::LocalPage for SpeakerPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> Option<BusinessIntent> {
        match op.event {
            InputEvent::Button(ButtonEvent::Click)
            | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
                Some(BusinessIntent::Playback(PlaybackAction::Next))
            }
            InputEvent::Button(ButtonEvent::LongPress)
            | InputEvent::Gesture(GestureEvent::LongPress { .. }) => Some(
                BusinessIntent::Playback(PlaybackAction::SetMuted(!state.playback.muted)),
            ),
            InputEvent::Button(ButtonEvent::DoubleClick) => {
                state.light_intent(op, LightAction::Step)
            }
            InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
                state.light_swipe(op, direction)
            }
            _ => None,
        }
    }

    fn paint(snapshot: &FrameSnapshot, painter: &mut Painter<'_>) {
        let playback = snapshot.playback;
        let fault = playback.health.fault();
        painter.draw_text(b"SPKR", block_x(Block::First), overlay_row_top(0));
        painter.draw_left(
            b"ST",
            match (fault, playback.phase) {
                (Some(_), _) => b"NO OUT",
                (None, PlaybackPhase::Idle) => b"IDLE",
                (None, PlaybackPhase::Playing) => b"PLAY",
            },
            Block::First,
            1,
        );
        painter.draw_left(b"SRC", sound_label(playback.sound), Block::First, 2);
        if playback.muted {
            painter.draw_text(b"MUTE", block_x(Block::Second), overlay_row_top(1));
        }
        painter.draw_left(
            b"PLY",
            format_u16(playback.plays, &mut [0u8; 6]),
            Block::First,
            3,
        );
        painter.draw_left(
            b"DRP",
            format_u16(playback.dropped, &mut [0u8; 6]),
            Block::First,
            4,
        );
        let hint = overlay_row_top(6);
        match fault {
            Some(reason) => {
                painter.draw_text(b"TAP RETRY", block_x(Block::First), hint);
                let reason = reason.as_bytes();
                painter.draw_text(reason, block_x(Block::First), hint + ROW_PITCH);
            }
            None => {
                painter.draw_text(b"TAP PLAY", block_x(Block::First), hint);
                painter.draw_text(b"HOLD MUTE", block_x(Block::First), hint + ROW_PITCH);
            }
        }
    }

    fn descriptor() -> super::Descriptor {
        super::Descriptor::new(Self::paint).with_repaints(super::Repaint::When(playback_moved))
    }
}

fn playback_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.playback != next.playback
}
