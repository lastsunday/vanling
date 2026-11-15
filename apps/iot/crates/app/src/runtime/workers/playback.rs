//! The playback worker's side: commands go out, and a sound's completion comes back as an
//! intent, so there is no `from` side here.

use crate::state::PlaybackCommand;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;

pub type PlaybackQueue = Channel<CriticalSectionRawMutex, PlaybackCommand, 4>;

/// The feeder's poke to the control loop: a sound ended, so the next command may start one.
pub type FeedWake = Signal<CriticalSectionRawMutex, ()>;

/// The coordinator's handle on the playback worker.
pub struct Playback {
    pub to: PlaybackQueue,
}

impl Playback {
    pub(crate) const fn new() -> Self {
        Self { to: Channel::new() }
    }
}
