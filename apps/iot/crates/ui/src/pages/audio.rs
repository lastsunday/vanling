use iot_core::drivers::audio::{dbfs, spl};
use iot_core::overlay::{Block, block_x, overlay_row_top};
use iot_core::state::AudioPhase;

use crate::layout::{PANEL_RIGHT, WAVE_TOP};
use crate::pages::draw::{format_dba, format_i32, format_u16};
use crate::surface::Surface;

impl Surface<'_> {
    pub(crate) fn stamp_audio(&mut self) {
        self.stamp_text(b"AUDIO", block_x(Block::First), overlay_row_top(0));
        let dba_lsb = self.diagnostics.audio.dba_lsb;
        let offset = self.spl_offset_db;
        self.stamp_text_right(
            format_dba(spl(dbfs(dba_lsb), offset), &mut [0u8; 12]),
            PANEL_RIGHT,
            overlay_row_top(0),
        );
        self.stamp_level(b"PK", dbfs(self.diagnostics.audio.envelope.loudest()), 1);
        self.stamp_level(
            b"RMS",
            dbfs(self.diagnostics.audio.envelope.loudest_rms()),
            2,
        );
        self.stamp_left(
            b"ST",
            match self.diagnostics.audio.phase {
                AudioPhase::Idle => b"IDLE",
                AudioPhase::Recording => b"REC",
                AudioPhase::Stopped => b"STOP",
            },
            Block::RightEdge,
            1,
        );
        self.stamp_left(
            b"MS",
            format_i32(
                i32::try_from(self.diagnostics.audio.elapsed_ms).unwrap_or(i32::MAX),
                &mut [0u8; 12],
            ),
            Block::RightEdge,
            2,
        );
        self.stamp_left(
            b"COL",
            format_u16(
                u16::from(self.diagnostics.audio.envelope.committed()),
                &mut [0u8; 6],
            ),
            Block::RightEdge,
            3,
        );
        self.stamp_left(
            b"RST",
            format_u16(self.diagnostics.audio.restarts, &mut [0u8; 6]),
            Block::RightEdge,
            4,
        );
        if self.diagnostics.audio.envelope.clipped() {
            self.stamp_text(b"CLIP", block_x(Block::RightEdge), overlay_row_top(5));
        }
        if self.diagnostics.audio.phase == AudioPhase::Idle {
            self.stamp_text(b"TAP TO REC", block_x(Block::First), WAVE_TOP as usize);
            return;
        }
        let envelope = &self.diagnostics.audio.envelope;
        let peaks = envelope.weighted_released_peaks();
        let levels = envelope.weighted_rms_columns();
        let clip_age = envelope.clipped_age();
        self.stamp_sweep(&peaks, &levels, clip_age);
    }
}
