use iot_core::overlay::{Block, ROW_PITCH, block_x, overlay_row_top};
use iot_core::state::PlaybackPhase;

use crate::pages::draw::format_u16;
use crate::surface::Surface;
use crate::text::sound_label;

impl Surface<'_> {
    pub(crate) fn stamp_speaker(&mut self) {
        let playback = self.diagnostics.playback;
        self.stamp_text(b"SPKR", block_x(Block::First), overlay_row_top(0));
        self.stamp_left(
            b"ST",
            match playback.phase {
                PlaybackPhase::Idle => b"IDLE",
                PlaybackPhase::Playing => b"PLAY",
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
        self.stamp_text(b"TAP PLAY", block_x(Block::First), hint);
        self.stamp_text(b"HOLD MUTE", block_x(Block::First), hint + ROW_PITCH);
    }
}
