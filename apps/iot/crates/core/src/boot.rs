//! The reset record: how many times in a row this part has come back from a crash.
//!
//! The chip layer stores one word in a register the reset does not clear; this module is
//! the part that decides what that word means, so the rule can be tested without a board.
//!
//! A boot counts as crashed when the previous boot raised a panic (the panic handler sets
//! the flag before resetting) or when this reset's reason says a watchdog fired. A power
//! cycle, a manual reset, or a reflash starts the count over at zero, so a healthy device
//! cannot drift into safe mode just by being reset often.

/// The mark that separates a record this product wrote from whatever a cold register holds.
const MAGIC: u32 = 0x5642;

/// Bit 15: the previous boot ended in a panic.
const PANICKED_BIT: u32 = 1 << 15;

/// Bits 0..=14: crashed boots in a row.
const COUNT_MASK: u32 = 0x7FFF;

/// The magic starts one bit above the panic flag, so decoding shifts by `COUNT_SHIFT + 1`.
const COUNT_SHIFT: u32 = 15;

/// The crash count at which a boot starts in safe mode, so the four attempts before it are the
/// retries a transient fault gets.
pub const MAX_CRASHED_BOOTS: u16 = 5;

/// What the previous boot left behind, as one word in reset-surviving storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetRecord {
    crashed_boots: u16,
    panicked: bool,
}

impl ResetRecord {
    pub const EMPTY: Self = Self {
        crashed_boots: 0,
        panicked: false,
    };

    pub const fn crashed_boots(self) -> u16 {
        self.crashed_boots
    }

    pub const fn panicked(self) -> bool {
        self.panicked
    }

    /// Read a word this module wrote, or the empty record when the word is anything else.
    pub const fn decode(word: u32) -> Self {
        if word >> (COUNT_SHIFT + 1) != MAGIC {
            return Self::EMPTY;
        }
        Self {
            crashed_boots: (word & COUNT_MASK) as u16,
            panicked: word & PANICKED_BIT != 0,
        }
    }

    pub const fn encode(self) -> u32 {
        (MAGIC << (COUNT_SHIFT + 1))
            | (if self.panicked { PANICKED_BIT } else { 0 })
            | (self.crashed_boots as u32 & COUNT_MASK)
    }

    /// The record a panic leaves for the next boot: the count is unchanged, the flag is not.
    pub const fn panicked_after_crash(self) -> Self {
        Self {
            crashed_boots: self.crashed_boots,
            panicked: true,
        }
    }
}

/// How the product should start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootMode {
    Normal,
    Safe,
}

/// Fold this boot's reset into the record the previous boot left.
///
/// `crashed` is true when the previous boot ended in a panic or this reset was a watchdog.
/// Anything else is a fresh start, so the count goes back to zero even if it had climbed.
pub const fn record_boot(previous: ResetRecord, crashed: bool) -> (ResetRecord, BootMode) {
    let crashed_boots = if crashed {
        let raised = previous.crashed_boots.saturating_add(1);
        if raised > COUNT_MASK as u16 {
            COUNT_MASK as u16
        } else {
            raised
        }
    } else {
        0
    };
    let record = ResetRecord {
        crashed_boots,
        panicked: false,
    };
    let mode = if crashed_boots >= MAX_CRASHED_BOOTS {
        BootMode::Safe
    } else {
        BootMode::Normal
    };
    (record, mode)
}

/// What the product has proved about this boot: it can draw a frame, and the radio path (or
/// the deliberate absence of one) has reached its steady state.
///
/// Time alone is not evidence: a boot that hangs after a minute of feeding is exactly the boot
/// safe mode exists for. Both halves are reported by the code that can actually answer them,
/// and the watchdog clears the crash record only once both have been seen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Readiness {
    bits: u8,
}

const FRAME_BIT: u8 = 1 << 0;
const NETWORK_BIT: u8 = 1 << 1;

impl Readiness {
    pub const fn new() -> Self {
        Self { bits: 0 }
    }

    pub fn note_frame(&mut self) {
        self.bits |= FRAME_BIT;
    }

    pub fn note_network(&mut self) {
        self.bits |= NETWORK_BIT;
    }

    pub const fn is_ready(self) -> bool {
        self.bits & (FRAME_BIT | NETWORK_BIT) == FRAME_BIT | NETWORK_BIT
    }
}

/// The boot's readiness, written by the app's frame loop and network worker and read by the
/// watchdog feeder. One byte, because the two reports are independent and idempotent.
static READINESS: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// The first frame has been presented on every wired surface.
pub fn note_first_frame() {
    READINESS.fetch_or(FRAME_BIT, core::sync::atomic::Ordering::Release);
}

/// The network worker owns the radio and the IP stack, or this build deliberately has no
/// radio. Connectivity is not the same question: an access point that cannot be reached is a
/// live device, not a crash.
pub fn note_network_settled() {
    READINESS.fetch_or(NETWORK_BIT, core::sync::atomic::Ordering::Release);
}

/// Whether both halves have reported since reset.
pub fn boot_ready() -> bool {
    READINESS.load(core::sync::atomic::Ordering::Acquire) & (FRAME_BIT | NETWORK_BIT)
        == FRAME_BIT | NETWORK_BIT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cold_register_is_an_empty_record() {
        assert_eq!(ResetRecord::decode(0), ResetRecord::EMPTY);
        assert_eq!(ResetRecord::decode(u32::MAX), ResetRecord::EMPTY);
    }

    #[test]
    fn a_written_record_reads_back_unchanged() {
        let record = ResetRecord {
            crashed_boots: 2,
            panicked: true,
        };
        assert_eq!(ResetRecord::decode(record.encode()), record);
        assert_eq!(
            ResetRecord::decode(ResetRecord::EMPTY.encode()),
            ResetRecord::EMPTY
        );
    }

    #[test]
    fn a_clean_boot_starts_the_count_at_zero() {
        let (record, mode) = record_boot(ResetRecord::EMPTY, false);
        assert_eq!(record.crashed_boots(), 0);
        assert_eq!(mode, BootMode::Normal);
    }

    #[test]
    fn a_manual_reset_after_a_long_run_does_not_accumulate() {
        let (first, _) = record_boot(ResetRecord::EMPTY, false);
        let (second, mode) = record_boot(first, false);
        assert_eq!(second.crashed_boots(), 0);
        assert_eq!(mode, BootMode::Normal);
    }

    #[test]
    fn panics_accumulate_until_safe_mode() {
        let (clean, _) = record_boot(ResetRecord::EMPTY, false);
        let (mut record, mut mode) = record_boot(clean.panicked_after_crash(), true);
        assert_eq!(mode, BootMode::Normal);

        for expected in 2..=MAX_CRASHED_BOOTS {
            assert_eq!(record.crashed_boots(), expected - 1);
            assert_eq!(mode, BootMode::Normal, "retry {}", expected - 1);
            (record, mode) = record_boot(record.panicked_after_crash(), true);
        }
        assert_eq!(record.crashed_boots(), MAX_CRASHED_BOOTS);
        assert_eq!(mode, BootMode::Safe);
    }

    #[test]
    fn safe_mode_takes_four_retries_after_the_count_was_raised() {
        let mut record = ResetRecord::EMPTY;
        for _ in 0..4 {
            let (next, mode) = record_boot(record, true);
            record = next;
            assert_eq!(mode, BootMode::Normal);
        }
        let (record, mode) = record_boot(record, true);
        assert_eq!(record.crashed_boots(), 5);
        assert_eq!(mode, BootMode::Safe);
    }

    #[test]
    fn readiness_needs_both_halves() {
        let mut readiness = Readiness::new();
        assert!(!readiness.is_ready());

        readiness.note_frame();
        assert!(
            !readiness.is_ready(),
            "a frame without a network is half a boot"
        );

        readiness.note_network();
        assert!(readiness.is_ready());

        readiness.note_frame();
        readiness.note_network();
        assert!(readiness.is_ready(), "reports are idempotent");
    }

    #[test]
    fn a_network_report_alone_is_not_a_finished_boot() {
        let mut readiness = Readiness::new();
        readiness.note_network();
        assert!(!readiness.is_ready());
    }

    #[test]
    fn watchdog_resets_accumulate_without_a_panic_flag() {
        let mut record = ResetRecord::EMPTY;
        let mut mode = BootMode::Normal;
        for _ in 0..MAX_CRASHED_BOOTS {
            (record, mode) = record_boot(record, true);
        }
        assert_eq!(record.crashed_boots(), MAX_CRASHED_BOOTS);
        assert_eq!(mode, BootMode::Safe);
    }

    #[test]
    fn a_clean_boot_clears_a_climbed_count() {
        let climbed = ResetRecord {
            crashed_boots: MAX_CRASHED_BOOTS,
            panicked: true,
        };
        let (fresh, mode) = record_boot(climbed, false);
        assert_eq!(fresh.crashed_boots(), 0);
        assert!(!fresh.panicked());
        assert_eq!(mode, BootMode::Normal);
    }

    #[test]
    fn the_count_saturates_instead_of_wrapping() {
        let (record, mode) = record_boot(
            ResetRecord {
                crashed_boots: COUNT_MASK as u16,
                panicked: false,
            },
            true,
        );
        assert_eq!(record.crashed_boots(), COUNT_MASK as u16);
        assert_eq!(mode, BootMode::Safe);
    }
}
