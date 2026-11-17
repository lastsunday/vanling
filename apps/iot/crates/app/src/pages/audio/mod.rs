//! The Audio page: capture control and the meter the capture's envelope is drawn as.

#[cfg(test)]
mod tests;

use iot_core::paint::Rgb;

use crate::shared::diagnostics::Diagnostics;
use crate::shared::intent::{AudioIntent, BusinessIntent, InputIntent, LightAction};
use crate::shared::kit::bands::WAVE_TOP;
use crate::shared::kit::placement::PANEL_RIGHT;
use crate::shared::kit::surface::Surface;
use crate::shared::kit::text::{format_dba, format_i32, format_u16};
use crate::shared::model::{AudioPhase, DeviceState};
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

impl super::Page for AudioPage {}

impl super::LocalPage for AudioPage {
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
}

fn audio_moved(prev: &Diagnostics, next: &Diagnostics) -> bool {
    prev.audio.phase != next.audio.phase
        || prev.audio.envelope != next.audio.envelope
        || prev.audio.restarts != next.audio.restarts
        || prev.audio.dba_lsb != next.audio.dba_lsb
}

impl Surface<'_> {
    pub(crate) fn paint_audio(&mut self) {
        self.draw_text(b"AUDIO", block_x(Block::First), overlay_row_top(0));
        let fault = self.diagnostics.audio.health.fault();
        let dba_lsb = self.diagnostics.audio.dba_lsb;
        let offset = self.spl_offset_db;
        self.draw_text_right(
            format_dba(spl(dbfs(dba_lsb), offset), &mut [0u8; 12]),
            PANEL_RIGHT,
            overlay_row_top(0),
        );
        self.draw_level(b"PK", dbfs(self.diagnostics.audio.envelope.loudest()), 1);
        self.draw_level(
            b"RMS",
            dbfs(self.diagnostics.audio.envelope.loudest_rms()),
            2,
        );
        self.draw_left(
            b"ST",
            match (fault, self.diagnostics.audio.phase) {
                (Some(_), _) => b"NO IN",
                (None, AudioPhase::Idle) => b"IDLE",
                (None, AudioPhase::Recording) => b"REC",
                (None, AudioPhase::Stopped) => b"STOP",
            },
            Block::RightEdge,
            1,
        );
        self.draw_left(
            b"MS",
            format_i32(
                i32::try_from(self.diagnostics.audio.elapsed_ms).unwrap_or(i32::MAX),
                &mut [0u8; 12],
            ),
            Block::RightEdge,
            2,
        );
        self.draw_left(
            b"COL",
            format_u16(
                u16::from(self.diagnostics.audio.envelope.committed()),
                &mut [0u8; 6],
            ),
            Block::RightEdge,
            3,
        );
        self.draw_left(
            b"RST",
            format_u16(self.diagnostics.audio.restarts, &mut [0u8; 6]),
            Block::RightEdge,
            4,
        );
        if self.diagnostics.audio.envelope.clipped() {
            self.draw_text(b"CLIP", block_x(Block::RightEdge), overlay_row_top(5));
        }
        if let Some(reason) = fault {
            self.draw_text(reason.as_bytes(), block_x(Block::First), WAVE_TOP as usize);
            self.draw_text(
                b"TAP RETRY",
                block_x(Block::First),
                WAVE_TOP as usize + ROW_PITCH,
            );
            return;
        }
        if self.diagnostics.audio.phase == AudioPhase::Idle {
            self.draw_text(b"TAP TO REC", block_x(Block::First), WAVE_TOP as usize);
            return;
        }
        let envelope = &self.diagnostics.audio.envelope;
        let peaks = envelope.weighted_released_peaks();
        let levels = envelope.weighted_rms_columns();
        let clip_age = envelope.clipped_age();
        self.draw_sweep(&peaks, &levels, clip_age);
    }
}
