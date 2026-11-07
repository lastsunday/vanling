use crate::model::state::PlaybackPhase;
use iot_core::overlay::{Block, ROW_PITCH, block_x, overlay_row_top};

use crate::ui::pages::surface::Surface;
use crate::ui::pages::text::{format_u16, sound_label};

impl Surface<'_> {
    pub(crate) fn stamp_speaker(&mut self) {
        let playback = self.diagnostics.playback;
        let fault = playback.health.fault();
        self.stamp_text(b"SPKR", block_x(Block::First), overlay_row_top(0));
        self.stamp_left(
            b"ST",
            match (fault, playback.phase) {
                (Some(_), _) => b"NO OUT",
                (None, PlaybackPhase::Idle) => b"IDLE",
                (None, PlaybackPhase::Playing) => b"PLAY",
            },
            Block::First,
            1,
        );
        self.stamp_left(b"SRC", sound_label(playback.sound), Block::First, 2);
        if playback.muted {
            self.stamp_text(b"MUTE", block_x(Block::Second), overlay_row_top(1));
        }
        self.stamp_left(
            b"PLY",
            format_u16(playback.plays, &mut [0u8; 6]),
            Block::First,
            3,
        );
        self.stamp_left(
            b"DRP",
            format_u16(playback.dropped, &mut [0u8; 6]),
            Block::First,
            4,
        );
        let hint = overlay_row_top(6);
        match fault {
            Some(reason) => {
                self.stamp_text(b"TAP RETRY", block_x(Block::First), hint);
                let reason = reason.as_bytes();
                self.stamp_text(reason, block_x(Block::First), hint + ROW_PITCH);
            }
            None => {
                self.stamp_text(b"TAP PLAY", block_x(Block::First), hint);
                self.stamp_text(b"HOLD MUTE", block_x(Block::First), hint + ROW_PITCH);
            }
        }
    }
}
