//! The pages as vocabulary: the order they cycle in, what each needs from the board,
//! what it grounds the panel with, whether it draws frames, and what it claims of a demand.

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayPage {
    #[default]
    Ambient,
    Attitude,
    Audio,
    Speaker,
    Camera,
    Network,
    Splash,
}

pub(crate) const CYCLE: [DisplayPage; 6] = [
    DisplayPage::Ambient,
    DisplayPage::Attitude,
    DisplayPage::Audio,
    DisplayPage::Speaker,
    DisplayPage::Camera,
    DisplayPage::Network,
];

const MOTION_BACKGROUND: Claim = Claim::new(Mode::Continuous, 0, Priority::Secondary);

const MOTION_FOREGROUND: Claim = Claim::new(Mode::Continuous, 0, Priority::Primary);

const ON_SCREEN: Claim = Claim::new(Mode::Continuous, 0, Priority::Primary);

impl DisplayPage {
    /// Every page, in `index()` order: must stay in step with `PAGES` and with `CYCLE`,
    /// which is this list minus the splash.
    pub const ALL: [Self; 7] = [
        Self::Ambient,
        Self::Attitude,
        Self::Audio,
        Self::Speaker,
        Self::Camera,
        Self::Network,
        Self::Splash,
    ];

    pub fn next_page(&self, backed: Backed) -> Self {
        if matches!(self, Self::Splash) {
            return Self::Ambient;
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

    /// Whether the board can show this page: the row's own `needs`, so a page declares what
    /// it takes next to what it draws.
    pub(crate) fn is_backed(&self, backed: Backed) -> bool {
        (PAGES[self.index()].needs)(backed)
    }

    // Out of line on purpose: inlined, the jump table sits beside every caller — 456 B of DRAM
    // for the seventh variant, measured with `nm` against the S3's main stack budget.
    #[inline(never)]
    pub(crate) const fn index(&self) -> usize {
        match self {
            Self::Ambient => 0,
            Self::Attitude => 1,
            Self::Audio => 2,
            Self::Speaker => 3,
            Self::Camera => 4,
            Self::Network => 5,
            Self::Splash => 6,
        }
    }

    /// The name this page answers to. `Splash` has one too; whether it can be booted on is the
    /// caller's decision, not a property of the page.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Ambient => "ambient",
            Self::Attitude => "attitude",
            Self::Audio => "audio",
            Self::Speaker => "speaker",
            Self::Camera => "camera",
            Self::Network => "network",
            Self::Splash => "splash",
        }
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

    /// The fill this page grounds the panel with, or `None` when the page draws over what
    /// the light was already showing. A page that covers the whole panel says so in its row
    /// rather than in each board's screen.
    pub fn ground(&self) -> Option<crate::paint::Rgb> {
        PAGES[self.index()].ground
    }

    /// Whether this page's frames arrive from the camera instead of being drawn.
    pub fn draws_frames(&self) -> bool {
        PAGES[self.index()].draws_frames
    }

    fn claim_for(&self, key: DemandKey) -> Claim {
        (PAGES[self.index()].claim)(key.kind())
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

struct PageSpec {
    /// Only the test can see the label: at runtime a page reads its own row by index, so a
    /// label here would be dead weight the compiler has to be told to ignore.
    #[cfg(test)]
    page: DisplayPage,
    /// What the board must wire for this page to be reachable at all.
    needs: fn(Backed) -> bool,
    /// The fill that covers the panel before the page draws, or `None` when it draws over
    /// what the light already showed.
    ground: Option<crate::paint::Rgb>,
    /// Whether frames arrive from the camera instead of being drawn.
    draws_frames: bool,
    claim: fn(DemandKind) -> Claim,
}

const PAGES: [PageSpec; DisplayPage::ALL.len()] = [
    PageSpec {
        #[cfg(test)]
        page: DisplayPage::Ambient,
        needs: |_| true,
        ground: None,
        draws_frames: false,
        claim: ambient::claim,
    },
    PageSpec {
        #[cfg(test)]
        page: DisplayPage::Attitude,
        needs: |backed| backed.motion.is_some(),
        ground: None,
        draws_frames: false,
        claim: attitude::claim,
    },
    PageSpec {
        #[cfg(test)]
        page: DisplayPage::Audio,
        needs: |backed| backed.audio,
        ground: None,
        draws_frames: false,
        claim: audio::claim,
    },
    PageSpec {
        #[cfg(test)]
        page: DisplayPage::Speaker,
        needs: |backed| backed.playback,
        ground: None,
        draws_frames: false,
        claim: speaker::claim,
    },
    PageSpec {
        #[cfg(test)]
        page: DisplayPage::Camera,
        needs: |backed| backed.camera,
        ground: Some(crate::paint::Rgb(0, 0, 0)),
        draws_frames: true,
        claim: camera::claim,
    },
    PageSpec {
        #[cfg(test)]
        page: DisplayPage::Network,
        needs: |backed| backed.network,
        ground: Some(crate::paint::Rgb(0, 0, 0)),
        draws_frames: false,
        claim: network::claim,
    },
    PageSpec {
        #[cfg(test)]
        page: DisplayPage::Splash,
        needs: |_| true,
        ground: None,
        draws_frames: false,
        claim: splash::claim,
    },
];

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
    fn no_two_pages_share_a_name() {
        for (position, page) in DisplayPage::ALL.iter().enumerate() {
            for other in &DisplayPage::ALL[position + 1..] {
                assert_ne!(page.name(), other.name(), "{page:?} and {other:?}");
            }
        }
    }
}
