//! Who this board is, answered from the chip rather than from the build.
//!
//! A desktop prints this during POST; an embedded board usually does not, and
//! that is a real loss — **a part that is present and broken is invisible**, and
//! a log that reports only failures cannot tell "nothing is wrong" from "nothing
//! was ever tried". [`State`](crate::post::State) splits the two: a device that
//! was never fitted and a device whose init failed are otherwise the same
//! observation.
//!
//! Deliberately *not* here: **flash capacity.** Reading it means the JEDEC
//! `RDID`, which needs the SPI bus — on this board the panel's, held by the panel
//! driver — and `esp-hal`'s SPI master has no `command()` either. The figure comes
//! from `espflash board-info` on the host and lives in the documentation. A
//! number that costs a bus fight to recover is not worth an inventory line.
//!
//! Ungated, unlike the board modules around it: only [`identity`](crate::post::identity) needs a chip and that
//! one function carries its own gate, so the states and the reporting can be built and
//! tested on a host.
// The only part of this module that needs a chip, and gated so the rest can be
// built and tested on a host: everything below the identity reads no
// peripheral, and a host test is the only place `is_ready` gets exercised.
#[cfg(target_os = "none")]
use esp_hal::efuse::{base_mac_address, chip_revision};

/// How a figure in this report was arrived at.
///
/// The point of carrying it is that **the three are not equally trustworthy**, and
/// a line without a provenance is the exact thing this module exists to stop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Provenance {
    /// Read back from the part itself.
    Read,
    /// Written down by the board module and not checkable from hardware.
    ///
    /// Pin numbers, panel geometry, clock rates. True by construction, which is
    /// not the same as verified.
    Asserted,
    /// Wired, but no measurement has confirmed the behaviour.
    Unverified,
}

/// One subsystem's outcome at the end of bring-up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// Brought up and usable.
    Found,
    /// Present but not usable, with the reason it gave.
    Degraded(&'static str),
    /// Not wired in this build, or not fitted on this board.
    Absent,
}

impl State {
    /// Whether this subsystem came up usable, as against being absent or failed.
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Found)
    }
}

/// Prints the chip's own identity: revision and MAC, both read from efuse.
///
/// Neither can be wrong about the part they describe, which is why they are the
/// two figures worth having at the top of the log — everything below them is
/// about the board, and the board is the part that varies.
#[cfg(target_os = "none")]
pub fn identity() {
    let revision = chip_revision();
    let mac = base_mac_address();
    let mac = mac.as_bytes();
    log::info!(
        "[POST] identity chip revision v{}.{}, mac {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        revision.major,
        revision.minor,
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5],
    );
}

/// Prints one subsystem's line.
///
/// The level follows the state rather than being chosen by the caller: a degraded
/// subsystem is a `warn`, because that is the only class here that is both a fact
/// and a fault. A line at `error` for something the product is designed to run
/// without teaches a reader to ignore the ones that matter.
pub fn line(name: &str, state: State, provenance: Provenance) {
    let tag = match provenance {
        Provenance::Read => "read",
        Provenance::Asserted => "asserted",
        Provenance::Unverified => "unverified",
    };
    match state {
        State::Found => log::info!("[POST] {name:<10} found        ({tag})"),
        State::Degraded(reason) => log::warn!("[POST] {name:<10} degraded: {reason} ({tag})"),
        State::Absent => log::info!("[POST] {name:<10} absent       ({tag})"),
    }
}

/// Prints the count of what came up, so the lines above have a bottom line.
pub fn summary(found: usize, degraded: usize, absent: usize) {
    log::info!("[POST] {found} found, {degraded} degraded, {absent} absent");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_subsystem_that_came_up_reads_as_ready() {
        assert!(State::Found.is_ready());
        assert!(!State::Absent.is_ready());
        // The distinction this module exists for: a part that answered and then
        // failed is not ready, and it must not be reported as `Absent` — a
        // reader who sees "absent" concludes the board lacks the hardware,
        // where "degraded" says the hardware is there and broken.
        assert!(!State::Degraded("no chip id").is_ready());
    }

    #[test]
    fn the_three_provenances_stay_three() {
        // They hold no data and are only ever compared, which is exactly the
        // case where a careless edit lets two of them collapse into one.
        assert_ne!(Provenance::Read, Provenance::Asserted);
        assert_ne!(Provenance::Read, Provenance::Unverified);
        assert_ne!(Provenance::Asserted, Provenance::Unverified);
    }
}
