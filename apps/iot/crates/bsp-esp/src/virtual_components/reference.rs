//! The played stream, shared between the transmit driver that writes it and the capture
//! driver that reads it.
//!
//! A critical-section cell because the two run on different executors: the feed is on a
//! higher-priority interrupt and the capture on the cooperative loop. Taken once per feed
//! and once per poll, never per sample. [`Silent`] is the same trait with no speaker.

use core::cell::RefCell;

use alloc::boxed::Box;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

use iot_core::drivers::aec::{ReferenceLog, required_frames};
use iot_core::drivers::audio::DECIMATION;

/// How many kept samples a log needs, from the two ring geometries.
///
/// A caller passes them rather than this module reading them, because the two
/// numbers belong to two drivers and this one is shared by both: reading either
/// would make the reference a dependency of a module it must not depend on, and
/// `features.md`'s rule that every component stands on its own would fail.
///
/// The division rounds **up**: a log one sample short does not read wrong, it
/// silently laps and starts answering from a lap ago — the fault
/// `ReferenceLog::has_lapped` exists to report, and this sizing exists to make
/// unreachable.
pub const fn samples_for(transmit_runway_frames: usize, receive_latency_frames: usize) -> usize {
    required_frames(transmit_runway_frames, receive_latency_frames).div_ceil(DECIMATION as usize)
}

/// The shared log: one cell, made at bring-up, reached through `&'static`.
pub type Shared<const N: usize> = Mutex<CriticalSectionRawMutex, RefCell<ReferenceLog<N>>>;

/// Makes the cell both drivers will share.
pub fn share<const N: usize>() -> &'static Shared<N> {
    &*Box::leak(Box::new(Mutex::new(RefCell::new(ReferenceLog::new(0)))))
}

/// What the capture side asks the transmit side for.
pub trait Reference {
    fn read_backwards(&self, out: &mut [i16]) -> usize;

    fn depth(&self) -> (usize, usize);
}

/// A transmit side that produces no reference: a board with a microphone and no speaker.
#[derive(Clone, Copy, Default)]
pub struct Silent;

impl Reference for Silent {
    fn read_backwards(&self, _out: &mut [i16]) -> usize {
        0
    }

    fn depth(&self) -> (usize, usize) {
        (0, 0)
    }
}

/// The capture side's end, plus the two answers a board with no speaker needs.
pub enum Side<const N: usize> {
    Wired(&'static Shared<N>),
    Silent,
}

impl<const N: usize> Side<N> {
    pub const fn wired(log: &'static Shared<N>) -> Self {
        Self::Wired(log)
    }

    pub const fn silent() -> Self {
        Self::Silent
    }
}

impl<const N: usize> Clone for Side<N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<const N: usize> Copy for Side<N> {}

impl<const N: usize> Reference for Side<N> {
    fn read_backwards(&self, out: &mut [i16]) -> usize {
        match self {
            Self::Wired(log) => Reader::new(*log).read_backwards(out),
            Self::Silent => 0,
        }
    }

    fn depth(&self) -> (usize, usize) {
        match self {
            Self::Wired(log) => Reader::new(*log).depth(),
            Self::Silent => (0, 0),
        }
    }
}

/// The transmit side's end: fills the log and moves its read cursor.
pub struct Writer<const N: usize> {
    log: &'static Shared<N>,
}

impl<const N: usize> Writer<N> {
    pub const fn new(log: &'static Shared<N>) -> Self {
        Self { log }
    }

    /// Fills the log with one feed's worth of frames, in the order the DMA will send them.
    pub fn write_frames(&self, samples: impl Iterator<Item = i16>) {
        self.log.lock(|log| {
            let mut log = log.borrow_mut();
            for sample in samples {
                log.push(sample);
            }
        });
    }

    /// Moves the read cursor forward by `frames` the DMA has finished sending.
    pub fn advance_played(&self, frames: usize) {
        if frames == 0 {
            return;
        }
        self.log.lock(|log| log.borrow_mut().played(frames));
    }
}

/// The capture side's end.
pub struct Reader<const N: usize> {
    log: &'static Shared<N>,
}

impl<const N: usize> Reader<N> {
    pub const fn new(log: &'static Shared<N>) -> Self {
        Self { log }
    }
}

impl<const N: usize> Reference for Reader<N> {
    fn read_backwards(&self, out: &mut [i16]) -> usize {
        self.log.lock(|log| {
            let log = log.borrow();
            for (index, slot) in out.iter_mut().enumerate() {
                match log.at_play_cursor(index) {
                    Some(sample) => *slot = sample,
                    None => return index,
                }
            }
            out.len()
        })
    }

    fn depth(&self) -> (usize, usize) {
        self.log
            .lock(|log| (log.borrow().held(), log.borrow().capacity()))
    }
}
