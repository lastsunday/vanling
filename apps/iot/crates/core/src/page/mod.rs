//! The pages as vocabulary: one manifest declares every page and the properties that follow
//! from it — the order they cycle in, what each needs from the board, what it grounds the
//! panel with, whether it draws frames, what it claims of a demand, how often it may repaint,
//! whether it can be booted on, and how long it lives. Everything else is generated from that
//! list, so adding a page is one row plus its view.

mod ambient;
mod attitude;
mod audio;
mod camera;
mod network;
mod speaker;
mod splash;

#[cfg(test)]
pub(crate) mod test_support;

pub use self::splash::SPLASH_MS;

use crate::demand::{Claim, DemandKey, DemandKind, EffectiveDemand, Mode, Priority};
use crate::drivers::board::Backed;

/// The least wall time between diagnostics-driven repaints. A repaint ships the whole 320×240
/// frame down one blocking SPI transfer (on the order of 15 ms), so without a bound the live
/// pages hold the shared cooperative executor in a ~15 ms block almost continuously, starving
/// the playback feed and the capture poll past their ring runway. A page raises it for itself
/// in its manifest row.
pub const DIAG_REPAINT_STEP_MS: u64 = 40;

macro_rules! page_count {
    () => { 0usize };
    ($head:ident $(, $tail:ident)*) => { 1usize + page_count!($($tail),*) };
}

/// Generates the page vocabulary from one row per page. `index` walks `ALL` rather than
/// carrying a second, hand-written position table.
macro_rules! pages {
    (
        default: $default:ident;
        $(
            $variant:ident {
                name: $name:literal,
                needs: $needs:expr,
                ground: $ground:expr,
                frames: $frames:expr,
                claim: $claim:path,
                repaint_step_ms: $repaint:expr,
                cycle: $cycle:literal,
                boot: $boot:literal,
                timeout_ms: $timeout:expr,
            },
        )+
    ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum DisplayPage {
            $( $variant, )+
        }

        impl Default for DisplayPage {
            fn default() -> Self {
                Self::$default
            }
        }

        /// Whether two values are the same variant, in const context: `const fn index` cannot
        /// reach `PartialEq::eq`, and this keeps the comparison generated from the manifest.
        const fn same_page(left: DisplayPage, right: DisplayPage) -> bool {
            match (left, right) {
                $( (DisplayPage::$variant, DisplayPage::$variant) => true, )+
                _ => false,
            }
        }

        struct PageSpec {
            /// Only the test can see the label: at runtime a page reads its own row by index.
            #[cfg(test)]
            page: DisplayPage,
            /// The name the page answers to, trimmed and case-insensitive through
            /// [`DisplayPage::named`].
            name: &'static str,
            /// What the board must wire for this page to be reachable at all.
            needs: fn(Backed) -> bool,
            /// The fill that covers the panel before the page draws, or `None` when it draws
            /// over what the light already showed.
            ground: Option<crate::paint::Rgb>,
            /// Whether frames arrive from the camera instead of being drawn.
            draws_frames: bool,
            claim: fn(DemandKind) -> Claim,
            /// Whether the page is part of the button's cycle.
            cycle: bool,
            /// Whether a build may come up on this page.
            bootable: bool,
            repaint_step_ms: u64,
            /// How long a transient page stays up, in milliseconds.
            timeout_ms: Option<u64>,
        }

        impl DisplayPage {
            /// Every page, in `index()` order.
            pub const ALL: [Self; page_count!($($variant),+)] = [ $( Self::$variant, )+ ];

            /// How many pages the button cycles through.
            const CYCLE_LEN: usize = 0 $(+ { if $cycle { 1usize } else { 0usize } })+;

            pub fn next_page(&self, backed: Backed) -> Self {
                if self.timeout_ms().is_some() {
                    return Self::$default;
                }
                let mut step = 1;
                while step <= CYCLE.len() {
                    let candidate = CYCLE[(self.index() + step) % CYCLE.len()];
                    if candidate.is_backed(backed) {
                        return candidate;
                    }
                    step += 1;
                }
                *self
            }

            /// Whether the board can show this page: the row's own `needs`, so a page declares
            /// what it takes next to what it draws.
            pub(crate) fn is_backed(&self, backed: Backed) -> bool {
                (PAGES[self.index()].needs)(backed)
            }

            // Out of line on purpose: inlined, the lookup is folded into every caller and the
            // seventh variant cost 456 B of DRAM, measured with `nm` against the S3's main
            // stack budget. A linear walk over `ALL` is a few compares and no jump table.
            #[inline(never)]
            pub(crate) const fn index(&self) -> usize {
                let mut position = 0;
                while position < Self::ALL.len() {
                    if same_page(*self, Self::ALL[position]) {
                        return position;
                    }
                    position += 1;
                }
                Self::ALL.len()
            }

            /// The name this page answers to. A transient page has one too; whether it can be
            /// booted on is a row property, not a naming decision.
            pub const fn name(&self) -> &'static str {
                PAGES[self.index()].name
            }

            /// The least wall time this page waits between diagnostics-driven repaints.
            pub fn repaint_step_ms(&self) -> u64 {
                PAGES[self.index()].repaint_step_ms
            }

            /// Whether a build may come up on this page.
            pub const fn bootable(&self) -> bool {
                PAGES[self.index()].bootable
            }

            /// How long this page lives before the device moves on, if it is transient.
            pub const fn timeout_ms(&self) -> Option<u64> {
                PAGES[self.index()].timeout_ms
            }

            /// A stable wire code for tasks that cannot hold the enum, generated from the
            /// manifest order rather than the enum's discriminant.
            pub const fn code(&self) -> u8 {
                self.index() as u8
            }

            /// The inverse of [`Self::code`]; `None` for a code no page answers to.
            pub const fn from_code(code: u8) -> Option<Self> {
                if (code as usize) < Self::ALL.len() {
                    Some(Self::ALL[code as usize])
                } else {
                    None
                }
            }

            fn claim_for(&self, key: DemandKey) -> Claim {
                (PAGES[self.index()].claim)(key.kind())
            }
        }

        const PAGES: [PageSpec; page_count!($($variant),+)] = [
            $(
                PageSpec {
                    #[cfg(test)]
                    page: DisplayPage::$variant,
                    name: $name,
                    needs: $needs,
                    ground: $ground,
                    draws_frames: $frames,
                    claim: $claim,
                    cycle: $cycle,
                    bootable: $boot,
                    repaint_step_ms: $repaint,
                    timeout_ms: $timeout,
                },
            )+
        ];

        pub(crate) const CYCLE: [DisplayPage; DisplayPage::CYCLE_LEN] = {
            let mut cycle = [DisplayPage::$default; DisplayPage::CYCLE_LEN];
            let mut read = 0;
            let mut write = 0;
            while read < DisplayPage::ALL.len() {
                if PAGES[read].cycle {
                    cycle[write] = DisplayPage::ALL[read];
                    write += 1;
                }
                read += 1;
            }
            cycle
        };
    };
}

const MOTION_BACKGROUND: Claim = Claim::new(Mode::Continuous, 0, Priority::Secondary);

const MOTION_FOREGROUND: Claim = Claim::new(Mode::Continuous, 0, Priority::Primary);

const ON_SCREEN: Claim = Claim::new(Mode::Continuous, 0, Priority::Primary);

pages! {
    default: Ambient;
    Ambient {
        name: "ambient",
        needs: |_| true,
        ground: None,
        frames: false,
        claim: ambient::claim,
        repaint_step_ms: DIAG_REPAINT_STEP_MS,
        cycle: true,
        boot: true,
        timeout_ms: None,
    },
    Attitude {
        name: "attitude",
        needs: |backed| backed.motion.is_some(),
        ground: None,
        frames: false,
        claim: attitude::claim,
        repaint_step_ms: DIAG_REPAINT_STEP_MS,
        cycle: true,
        boot: true,
        timeout_ms: None,
    },
    Audio {
        name: "audio",
        needs: |backed| backed.audio,
        ground: None,
        frames: false,
        claim: audio::claim,
        repaint_step_ms: DIAG_REPAINT_STEP_MS * 2,
        cycle: true,
        boot: true,
        timeout_ms: None,
    },
    Speaker {
        name: "speaker",
        needs: |backed| backed.playback,
        ground: None,
        frames: false,
        claim: speaker::claim,
        repaint_step_ms: DIAG_REPAINT_STEP_MS,
        cycle: true,
        boot: true,
        timeout_ms: None,
    },
    Camera {
        name: "camera",
        needs: |backed| backed.camera,
        ground: Some(crate::paint::Rgb(0, 0, 0)),
        frames: true,
        claim: camera::claim,
        repaint_step_ms: DIAG_REPAINT_STEP_MS,
        cycle: true,
        boot: true,
        timeout_ms: None,
    },
    Network {
        name: "network",
        needs: |backed| backed.network,
        ground: Some(crate::paint::Rgb(0, 0, 0)),
        frames: false,
        claim: network::claim,
        repaint_step_ms: DIAG_REPAINT_STEP_MS,
        cycle: true,
        boot: true,
        timeout_ms: None,
    },
    Splash {
        name: "splash",
        needs: |_| true,
        ground: None,
        frames: false,
        claim: splash::claim,
        repaint_step_ms: DIAG_REPAINT_STEP_MS,
        cycle: false,
        boot: false,
        timeout_ms: Some(SPLASH_MS),
    },
}

impl DisplayPage {
    /// The fill this page grounds the panel with, or `None` when the page draws over what the
    /// light was already showing.
    pub fn ground(&self) -> Option<crate::paint::Rgb> {
        PAGES[self.index()].ground
    }

    /// Whether this page's frames arrive from the camera instead of being drawn.
    pub fn draws_frames(&self) -> bool {
        PAGES[self.index()].draws_frames
    }

    /// Case- and whitespace-insensitive lookup by [`Self::name`]. Const so a build-time page
    /// never becomes a run-time string — `@/development/iot/flashing.md`.
    pub const fn named(name: &str) -> Option<Self> {
        let wanted = name.trim_ascii().as_bytes();
        let mut index = 0;
        while index < Self::ALL.len() {
            let page = Self::ALL[index];
            if eq_ignore_ascii_case(page.name().as_bytes(), wanted) {
                return Some(page);
            }
            index += 1;
        }
        None
    }

    pub fn demand(&self, keys: &[DemandKey]) -> EffectiveDemand {
        let mut demand = EffectiveDemand::none();
        for key in keys {
            demand.set(*key, self.claim_for(*key));
        }
        demand
    }
}

const fn eq_ignore_ascii_case(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if !left[index].eq_ignore_ascii_case(&right[index]) {
            return false;
        }
        index += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_is_indexed_once_and_the_table_is_in_that_order() {
        assert_eq!(DisplayPage::ALL.len(), PAGES.len());
        for (position, page) in DisplayPage::ALL.iter().enumerate() {
            assert_eq!(page.index(), position, "{page:?}");
            assert_eq!(PAGES[position].page, *page, "{page:?}");
        }
        for (position, page) in CYCLE.iter().enumerate() {
            assert_eq!(DisplayPage::ALL[position], *page);
        }
        assert_eq!(DisplayPage::ALL[CYCLE.len()], DisplayPage::Splash);
    }

    #[test]
    fn every_page_answers_to_its_own_name() {
        for page in DisplayPage::ALL {
            assert_eq!(DisplayPage::named(page.name()), Some(page), "{page:?}");
            assert_eq!(
                DisplayPage::named(&page.name().to_ascii_uppercase()),
                Some(page),
                "{page:?} in capitals"
            );
        }
        assert_eq!(DisplayPage::named("  CAMERA  "), Some(DisplayPage::Camera));
        assert_eq!(DisplayPage::named("\tcamera\n"), Some(DisplayPage::Camera));
        assert_eq!(DisplayPage::named("photograph"), None);
        assert_eq!(DisplayPage::named(""), None);
        assert_eq!(DisplayPage::named("   "), None);
    }

    #[test]
    fn every_page_has_a_stable_wire_code() {
        for (position, page) in DisplayPage::ALL.iter().enumerate() {
            assert_eq!(usize::from(page.code()), position, "{page:?}");
            assert_eq!(DisplayPage::from_code(page.code()), Some(*page), "{page:?}");
        }
        assert_eq!(
            DisplayPage::from_code(DisplayPage::ALL.len() as u8),
            None,
            "one past the last page"
        );
        assert_eq!(DisplayPage::from_code(u8::MAX), None, "a code no page owns");
    }

    #[test]
    fn no_two_pages_share_a_name() {
        for (position, page) in DisplayPage::ALL.iter().enumerate() {
            for other in &DisplayPage::ALL[position + 1..] {
                assert_ne!(page.name(), other.name(), "{page:?} and {other:?}");
            }
        }
    }
}
