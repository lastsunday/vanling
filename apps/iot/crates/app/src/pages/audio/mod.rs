//! The Audio page: capture control and the meter the capture's envelope is drawn as.

#[cfg(test)]
mod tests;

use iot_core::light::Rgb;

use crate::shared::intent::{AudioIntent, BusinessIntent, InputIntent, LightAction};
use crate::shared::kit::bands::WAVE_TOP;
use crate::shared::kit::painter::Painter;
use crate::shared::kit::placement::PANEL_RIGHT;
use crate::shared::kit::text::{format_dba, format_i32, format_u16};
use crate::shared::model::{AudioPhase, DeviceState};
use crate::shared::snapshot::FrameSnapshot;
use iot_core::drivers::audio::{dbfs, spl};
use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent};
use iot_core::overlay::{Block, ROW_PITCH, block_x, overlay_row_top};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioPage;

pub(super) const GROUND: Option<Rgb> = None;
pub(super) const CAMERA_FRAMES: bool = false;

/// A capture poll closes two envelope columns every 20 ms; at the default step the sweep would
/// hold the executor 40% of the wall time on the S3.
pub(super) const REPAINT_MS: u64 = super::REPAINT_FLOOR_MS * 2;
pub(super) const REPAINTS: super::Repaint = super::Repaint::When(audio_moved);

impl super::Page for AudioPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> BusinessIntent {
        match op.event {
            InputEvent::Button(ButtonEvent::Click)
            | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
                let intent = if state.audio.phase == AudioPhase::Recording {
                    AudioIntent::Stop
                } else {
                    AudioIntent::Start
                };
                BusinessIntent::Audio(intent)
            }
            InputEvent::Button(ButtonEvent::DoubleClick) => {
                state.light_intent(op, LightAction::Step)
            }
            InputEvent::Button(ButtonEvent::LongPress)
            | InputEvent::Gesture(GestureEvent::LongPress { .. }) => {
                state.light_intent(op, LightAction::Cycle)
            }
            InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
                state.light_swipe(op, direction)
            }
            _ => BusinessIntent::Invalid,
        }
    }

    fn paint(snapshot: &FrameSnapshot, painter: &mut Painter<'_>) {
        painter.draw_text(b"AUDIO", block_x(Block::First), overlay_row_top(0));
        let fault = snapshot.audio.health.fault();
        let dba_lsb = snapshot.audio.dba_lsb;
        let offset = painter.spl_offset_db;
        painter.draw_text_right(
            format_dba(spl(dbfs(dba_lsb), offset), &mut [0u8; 12]),
            PANEL_RIGHT,
            overlay_row_top(0),
        );
        painter.draw_level(b"PK", dbfs(snapshot.audio.envelope.loudest()), 1);
        painter.draw_level(b"RMS", dbfs(snapshot.audio.envelope.loudest_rms()), 2);
        painter.draw_left(
            b"ST",
            match (fault, snapshot.audio.phase) {
                (Some(_), _) => b"NO IN",
                (None, AudioPhase::Idle) => b"IDLE",
                (None, AudioPhase::Recording) => b"REC",
                (None, AudioPhase::Stopped) => b"STOP",
            },
            Block::RightEdge,
            1,
        );
        painter.draw_left(
            b"MS",
            format_i32(
                i32::try_from(snapshot.audio.elapsed_ms).unwrap_or(i32::MAX),
                &mut [0u8; 12],
            ),
            Block::RightEdge,
            2,
        );
        painter.draw_left(
            b"COL",
            format_u16(
                u16::from(snapshot.audio.envelope.committed()),
                &mut [0u8; 6],
            ),
            Block::RightEdge,
            3,
        );
        painter.draw_left(
            b"RST",
            format_u16(snapshot.audio.restarts, &mut [0u8; 6]),
            Block::RightEdge,
            4,
        );
        if snapshot.audio.envelope.clipped() {
            painter.draw_text(b"CLIP", block_x(Block::RightEdge), overlay_row_top(5));
        }
        if let Some(reason) = fault {
            painter.draw_text(reason.as_bytes(), block_x(Block::First), WAVE_TOP as usize);
            painter.draw_text(
                b"TAP RETRY",
                block_x(Block::First),
                WAVE_TOP as usize + ROW_PITCH,
            );
            return;
        }
        if snapshot.audio.phase == AudioPhase::Idle {
            painter.draw_text(b"TAP TO REC", block_x(Block::First), WAVE_TOP as usize);
            return;
        }
        let envelope = &snapshot.audio.envelope;
        let peaks = envelope.weighted_released_peaks();
        let levels = envelope.weighted_rms_columns();
        let clip_age = envelope.clipped_age();
        painter.draw_sweep(&peaks, &levels, clip_age);
    }
}

fn audio_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.audio.phase != next.audio.phase
        || prev.audio.envelope != next.audio.envelope
        || prev.audio.restarts != next.audio.restarts
        || prev.audio.dba_lsb != next.audio.dba_lsb
}
