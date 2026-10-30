//! `PlaybackState` to `PlaybackCommand`, once per pass over the intent bus.

use embassy_sync::channel::TrySendError;

use super::PlaybackBus;
use iot_core::state::{PlaybackCommand, PlaybackState};

pub(crate) struct PlaybackDispatch {
    previous: PlaybackState,
}

impl PlaybackDispatch {
    pub(crate) fn new(state: PlaybackState) -> Self {
        Self { previous: state }
    }

    /// Queues the edges this state crossed. A command that found no room leaves its field
    /// behind, so the next pass offers the same edge again instead of losing it.
    pub(crate) fn dispatch(&mut self, bus: &PlaybackBus, after: PlaybackState) {
        if let Some(command) = after.mute_command(&self.previous)
            && queue(bus, command)
        {
            self.previous.muted = after.muted;
        }
        match after.play_command(&self.previous) {
            Some(command) if !queue(bus, command) => {}
            _ => self.previous.phase = after.phase,
        }
    }
}

fn queue(bus: &PlaybackBus, command: PlaybackCommand) -> bool {
    match bus.try_send(command) {
        Ok(()) => true,
        Err(TrySendError::Full(command)) => {
            log::error!("[PLAY] {command:?} had no room, asking again");
            false
        }
    }
}
