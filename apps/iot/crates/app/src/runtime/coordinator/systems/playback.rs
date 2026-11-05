//! `PlaybackState` to `PlaybackCommand`: once per intent, so two edges in one frame do not
//! collapse; and once per frame, so a command the queue had no room for is offered again.

use embassy_sync::channel::TrySendError;

use crate::model::state::{Command, PlaybackCommand, PlaybackState};
use crate::runtime::workers::{Playback, PlaybackQueue};

use crate::runtime::system::{Frame, System};

pub(crate) struct PlaybackSystem {
    playback: Playback,
    previous: PlaybackState,
}

impl PlaybackSystem {
    pub(crate) fn new(state: PlaybackState, playback: Playback) -> Self {
        Self {
            playback,
            previous: state,
        }
    }

    /// Queues the edges this state crossed. A command that found no room leaves its field
    /// behind, so the next call offers the same edge again instead of losing it.
    fn dispatch(&mut self, after: PlaybackState) {
        if let Some(command) = after.mute_command(&self.previous)
            && queue(self.playback.to, command)
        {
            self.previous.muted = after.muted;
        }
        match after.play_command(&self.previous) {
            Some(command) if !queue(self.playback.to, command) => {}
            _ => self.previous.phase = after.phase,
        }
    }
}

impl System for PlaybackSystem {
    fn tick(&mut self, frame: &mut Frame<'_>) {
        self.dispatch(frame.manager.playback_state());
    }

    fn intent(&mut self, frame: &mut Frame<'_>, _command: &mut Option<Command>) {
        self.dispatch(frame.manager.playback_state());
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
