//! `PlaybackState` to `PlaybackCommand`: once per intent, so two edges in one frame do not
//! collapse; and once per frame, so a command the queue had no room for is offered again.

use embassy_sync::channel::TrySendError;

use crate::workers::PlaybackQueue;
use iot_core::state::{PlaybackCommand, PlaybackState};

use super::{Frame, System};

pub(crate) struct PlaybackSystem {
    previous: PlaybackState,
}

impl PlaybackSystem {
    pub(crate) fn new(state: PlaybackState) -> Self {
        Self { previous: state }
    }

    /// Queues the edges this state crossed. A command that found no room leaves its field
    /// behind, so the next call offers the same edge again instead of losing it.
    pub(crate) fn dispatch(&mut self, to: &PlaybackQueue, after: PlaybackState) {
        if let Some(command) = after.mute_command(&self.previous)
            && queue(to, command)
        {
            self.previous.muted = after.muted;
        }
        match after.play_command(&self.previous) {
            Some(command) if !queue(to, command) => {}
            _ => self.previous.phase = after.phase,
        }
    }
}

impl System for PlaybackSystem {
    fn tick(&mut self, frame: &mut Frame<'_>) {
        self.dispatch(frame.workers.playback.to, frame.manager.playback_state());
    }
}

fn queue(to: &PlaybackQueue, command: PlaybackCommand) -> bool {
    match to.try_send(command) {
        Ok(()) => true,
        Err(TrySendError::Full(command)) => {
            log::error!("[PLAY] {command:?} had no room, asking again");
            false
        }
    }
}
