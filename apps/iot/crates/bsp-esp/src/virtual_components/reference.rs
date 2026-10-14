//! The played stream, shared between the transmit driver that writes it and the
//! capture driver that reads it.
//!
//! **A shared cell because the two run on different executors.** The speaker's
//! feed is on a higher-priority interrupt executor — it has a hard cadence, and a
//! late feed is a gap in the sound *and* a stalled microphone clock — while the
//! capture is polled from the cooperative input loop. The reference therefore
//! crosses an executor boundary that neither driver may own: the write side would
//! have to be reachable from the read side's task and the read side from the
//! write side's.
//!
//! One direction only, which is what keeps it simple: the transmit driver fills
//! the log and the capture driver reads it. The speaker never learns a microphone
//! exists, so nothing on the playback path changes shape and the feed still meets
//! its own contract — bounded, infallible, no logging, no allocation.
//!
//! [`CriticalSectionRawMutex`] because it never spins: the feed takes it from an
//! interrupt and a bounded wait is the one thing it cannot afford. Taken **once
//! per feed** and once per poll, never per sample — a lock per sample at 48 kHz
//! would cost more than the canceller it is feeding.
//!
//! **A board with no speaker** is a real configuration, and [`Silent`] is the
//! same trait answering from nothing, so a capture-only board compiles and runs
//! unchanged. It is deliberately **not** an empty log: that would answer `None`
//! from the log's own API, which a capture reads as a *lapsed* reference to be
//! reported as a fault — and a board with no speaker has nothing to fault.

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
///
/// Leaked rather than put in a `static` for the same reason `playback::share()`
/// leaks its handle — the cell holds a [`ReferenceLog`], which has no `const`
/// constructor, and the two drivers that read and write it outlive the function
/// that makes it. Written once, before either driver starts.
pub type Shared<const N: usize> = Mutex<CriticalSectionRawMutex, RefCell<ReferenceLog<N>>>;

/// Makes the cell both drivers will share. Phase 0, because a board that arms its
/// transmit unit before its capture pairs frame 0 to frame 0 exactly; one that
/// does the reverse has to say so here and to the capture's decimator, and the
/// delay estimator covers the rest.
pub fn share<const N: usize>() -> &'static Shared<N> {
    &*Box::leak(Box::new(Mutex::new(RefCell::new(ReferenceLog::new(0)))))
}

/// What the capture side asks the transmit side for.
pub trait Reference {
    /// Fills `out` from the play cursor **backwards** — `out[0]` is the frame the
    /// wire is carrying now, `out[n]` the frame `n` frames before it — and answers
    /// how many it could reach.
    ///
    /// **Newest first, deliberately.** The log is indexed from the newest frame
    /// backwards, and the newest frame is the one the newest microphone sample
    /// heard. A caller whose microphone run is chronological has to reverse this
    /// before zipping; see `Es7210Rx::pair_and_cancel`, which is where that
    /// reconciliation is done and why.
    ///
    /// Fewer than `out.len()` means the log has lapped, or the wire has not
    /// started, or the log is shorter than the ring's runway. The caller pairs
    /// what it got and reports the rest: cancelling a run against a different
    /// stretch of reference would be worse than cancelling nothing.
    fn read_backwards(&self, out: &mut [i16]) -> usize;

    /// Kept samples held unread and the log's capacity — the two numbers that
    /// say whether the pairing is still inside the log or reading from a lap.
    fn depth(&self) -> (usize, usize);
}

/// A transmit side that produces no reference: a board with a microphone and no
/// speaker.
///
/// Kept as a unit type rather than only as [`Side::Silent`] so a capture driver
/// can be built with it directly and a board with no speaker needs no cell at
/// all.
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
///
/// One type rather than a trait object, so a capture driver can hand out
/// `&'static Reader` and borrow it for the length of a poll without an
/// allocation and without a self-referential borrow.
pub enum Side<const N: usize> {
    /// A board with a speaker: the shared log.
    Wired(&'static Shared<N>),
    /// A board with a microphone and no speaker.
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

/// Copyable because it holds nothing but a `&'static` handle — a copy is a
/// pointer, so a poll can take one out of the driver and borrow the driver
/// mutably at the same time. That is the whole reason this is an enum of
/// handles rather than a trait object: a `Box<dyn>` cannot be copied out of a
/// struct that is being borrowed mutably.
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

    /// Fills the log with one feed's worth of frames, in the order the DMA will
    /// send them.
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

/// The capture side's end. Copyable, so a probe can hold one to report from
/// without owning the driver.
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
