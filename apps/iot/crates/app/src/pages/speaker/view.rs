use crate::shared::model::PlaybackPhase;
use iot_core::overlay::{Block, ROW_PITCH, block_x, overlay_row_top};

use crate::shared::kit::surface::Surface;
use crate::shared::kit::text::{format_u16, sound_label};

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
