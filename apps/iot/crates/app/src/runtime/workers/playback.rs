//! The playback worker's side: commands go out, and a sound's completion comes back as an
//! intent, so there is no `from` side here.

use crate::state::PlaybackCommand;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;

pub type PlaybackQueue = Channel<CriticalSectionRawMutex, PlaybackCommand, 4>;

static PLAYBACK_QUEUE: PlaybackQueue = PlaybackQueue::new();

/// The coordinator's handle on the playback worker.
#[derive(Clone, Copy)]
pub struct Playback {
    pub to: &'static PlaybackQueue,
}

pub(crate) static PLAYBACK: Playback = Playback {
    to: &PLAYBACK_QUEUE,
};
