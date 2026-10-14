//! The room the pairing tests in this module are built from, because they all
//! need the same property: **broadband, and a dominant direct reflection**.
//!
//! A near-periodic reference gives an adaptive filter more than one correct
//! answer, so a pairing test built on one can pass without the pairing being
//! right. And a path whose taps are all the same size has no peak for the delay
//! search to lock onto — the record's own finding, that adding a dominant direct
//! reflection took a generated room from 0.4 dB to 61 dB. A test room without one
//! measures a filter pointed at noise.
//!
//! Only the room is local here; the broadband source and the sum of squares are
//! [`super::signal`]'s, shared rather than copied so a seed names one signal
//! everywhere in the driver.
use alloc::vec::Vec;

/// The echo path, in ppm per tap: a dominant direct reflection then a decaying
/// tail, which is the shape a room's impulse response has.
pub const PATH: [(usize, i32); 4] = [(9, 700_000), (14, -300_000), (21, 160_000), (30, -70_000)];

/// What a room does to a reference: [`PATH`] convolved in behind `delay`.
pub fn echo(played: &[i16], delay: usize) -> Vec<i16> {
    (0..played.len())
        .map(|n| {
            let total: i64 = PATH
                .iter()
                .filter(|(tap, _)| n >= delay + tap)
                .map(|&(tap, ppm)| i64::from(played[n - delay - tap]) * i64::from(ppm) / 1_000_000)
                .sum();
            total as i16
        })
        .collect()
}

/// A room's gain as a multiple of the reference's own power, so a test can
/// assert the room it built is the room it meant to build rather than
/// discovering later that it was nearly silent.
pub fn room_gain(played: &[i16], heard: &[i16], delay: usize) -> f64 {
    let signal: i64 = heard.iter().map(|s| i64::from(*s) * i64::from(*s)).sum();
    let reference: i64 = played[delay..]
        .iter()
        .map(|s| i64::from(*s) * i64::from(*s))
        .sum();
    signal as f64 / reference as f64
}
