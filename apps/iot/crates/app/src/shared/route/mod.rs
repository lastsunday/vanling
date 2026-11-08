//! The pages as vocabulary: a route table declares, for every page, the name it answers to,
//! what it needs from the board, what it claims of a demand, whether it can be booted on, and
//! how long it lives. The enum is the destination; [`Page::route`] is the one
//! exhaustive map from a destination to its row, so a new page cannot exist without a route.
//!
//! How a page is drawn — its ground, whether frames arrive from a camera, how often it may
//! repaint — belongs to the renderer (`ui::render::policy`), not to this table.
//!
//! The order the button cycles in is a navigation fact, not a route property, and the
//! [`Navigator`] is what walks it (`nav`).

mod ambient;
mod attitude;
mod audio;
mod camera;
mod nav;
mod network;
mod speaker;
mod splash;

#[cfg(test)]
pub(crate) mod test_support;

pub use self::nav::{MAX_DEPTH, Navigator};

pub use self::splash::SPLASH_MS;

use iot_core::demand::{Claim, DemandKey, DemandKind, EffectiveDemand, Mode, Priority};
use iot_core::drivers::board::Backed;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Ambient,
    Attitude,
    Audio,
    Speaker,
    Camera,
    Network,
    /// The transient the boot lands through, never on: it has a timeout and is not bootable.
    Splash,
}

/// One page's row in the route table: the properties that follow from being that page.
struct Route {
    /// The name the page answers to, trimmed and case-insensitive through
    /// [`Page::from_path`].
    path: &'static str,
    /// What the board must wire for this page to be reachable at all.
    needs: fn(Backed) -> bool,
    claim: fn(DemandKind) -> Claim,
    /// Whether a build may come up on this page.
    bootable: bool,
    /// How long a transient page stays up, in milliseconds.
    timeout_ms: Option<u64>,
}

const MOTION_BACKGROUND: Claim = Claim::new(Mode::Continuous, 0, Priority::Secondary);

const MOTION_FOREGROUND: Claim = Claim::new(Mode::Continuous, 0, Priority::Primary);

const ON_SCREEN: Claim = Claim::new(Mode::Continuous, 0, Priority::Primary);

static AMBIENT: Route = Route {
    path: "ambient",
    needs: |_| true,
    claim: ambient::claim,
    bootable: true,
    timeout_ms: None,
};

static ATTITUDE: Route = Route {
    path: "attitude",
    needs: |backed| backed.motion.is_some(),
    claim: attitude::claim,
    bootable: true,
    timeout_ms: None,
};

static AUDIO: Route = Route {
    path: "audio",
    needs: |backed| backed.audio,
    claim: audio::claim,
    bootable: true,
    timeout_ms: None,
};

static SPEAKER: Route = Route {
    path: "speaker",
    needs: |backed| backed.playback,
    claim: speaker::claim,
    bootable: true,
    timeout_ms: None,
};

static CAMERA: Route = Route {
    path: "camera",
    needs: |backed| backed.camera,
    claim: camera::claim,
    bootable: true,
    timeout_ms: None,
};

static NETWORK: Route = Route {
    path: "network",
    needs: |backed| backed.network,
    claim: network::claim,
    bootable: true,
    timeout_ms: None,
};

static SPLASH: Route = Route {
    path: "splash",
    needs: |_| true,
    claim: splash::claim,
    bootable: false,
    timeout_ms: Some(SPLASH_MS),
};

/// The button's order. A prefix of [`Page::ALL`], so the page that leaves the cycle
/// (the splash) stays at the tail and the two lists cannot drift apart silently.
pub(crate) const CYCLE: [Page; 6] = [
    Page::Ambient,
    Page::Attitude,
    Page::Audio,
    Page::Speaker,
    Page::Camera,
    Page::Network,
];

impl Page {
    /// Every page, in `code()` order.
    pub const ALL: [Self; 7] = [
        Self::Ambient,
        Self::Attitude,
        Self::Audio,
        Self::Speaker,
        Self::Camera,
        Self::Network,
        Self::Splash,
    ];

    /// The page's row. One exhaustive match: a variant with no route is a compile error.
    // Out of line on purpose: inlined, the lookup is folded into every caller and the seventh
    // variant cost 456 B of DRAM, measured with `nm` against the S3's main stack budget.
    #[inline(never)]
    const fn route(&self) -> &'static Route {
        match self {
            Self::Ambient => &AMBIENT,
            Self::Attitude => &ATTITUDE,
            Self::Audio => &AUDIO,
            Self::Speaker => &SPEAKER,
            Self::Camera => &CAMERA,
            Self::Network => &NETWORK,
            Self::Splash => &SPLASH,
        }
    }

    /// The page the button turns to: the next backed page in [`CYCLE`], wrapping. A transient
    /// has no place in the cycle and stays until its lifetime ends.
    pub fn next_page(&self, backed: Backed) -> Self {
        if self.timeout_ms().is_some() {
            return *self;
        }
        let Some(here) = CYCLE.iter().position(|page| page == self) else {
            return *self;
        };
        for step in 1..=CYCLE.len() {
            let candidate = CYCLE[(here + step) % CYCLE.len()];
            if candidate.is_backed(backed) {
                return candidate;
            }
        }
        *self
    }

    /// Whether the board can show this page: the route's own `needs`, so a page declares what
    /// it takes next to what it draws.
    pub(crate) fn is_backed(&self, backed: Backed) -> bool {
        (self.route().needs)(backed)
    }

    /// The name this page answers to. A transient page has one too; whether it can be booted
    /// on is a route property, not a naming decision.
    pub const fn path(&self) -> &'static str {
        self.route().path
    }

    /// Case- and whitespace-insensitive lookup by [`Self::path`]. Const so a build-time page
    /// never becomes a run-time string — `@/development/iot/flashing.md`.
    pub const fn from_path(path: &str) -> Option<Self> {
        let wanted = path.trim_ascii().as_bytes();
        let mut index = 0;
        while index < Self::ALL.len() {
            let page = Self::ALL[index];
            if eq_ignore_ascii_case(page.path().as_bytes(), wanted) {
                return Some(page);
            }
            index += 1;
        }
        None
    }

    /// Whether a build may come up on this page.
    pub const fn bootable(&self) -> bool {
        self.route().bootable
    }

    /// How long this page lives before the device moves on, if it is transient.
    pub const fn timeout_ms(&self) -> Option<u64> {
        self.route().timeout_ms
    }

    /// A stable wire code for tasks that cannot hold the enum: the position in [`Self::ALL`],
    /// written out so a policy call site stays `const`.
    pub const fn code(&self) -> u8 {
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

    /// The inverse of [`Self::code`]; `None` for a code no page answers to.
    pub const fn from_code(code: u8) -> Option<Self> {
        if (code as usize) < Self::ALL.len() {
            Some(Self::ALL[code as usize])
        } else {
            None
        }
    }

    fn claim_for(&self, key: DemandKey) -> Claim {
        (self.route().claim)(key.kind())
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
    fn the_table_is_in_code_order_and_the_cycle_is_its_prefix() {
        for (position, page) in Page::ALL.iter().enumerate() {
            assert_eq!(usize::from(page.code()), position, "{page:?}");
        }
        for (position, page) in CYCLE.iter().enumerate() {
            assert_eq!(Page::ALL[position], *page);
        }
        assert_eq!(Page::ALL[CYCLE.len()], Page::Splash);
    }

    #[test]
    fn every_page_answers_to_its_own_path() {
        for page in Page::ALL {
            assert_eq!(Page::from_path(page.path()), Some(page), "{page:?}");
            assert_eq!(
                Page::from_path(&page.path().to_ascii_uppercase()),
                Some(page),
                "{page:?} in capitals"
            );
        }
        assert_eq!(Page::from_path("  CAMERA  "), Some(Page::Camera));
        assert_eq!(Page::from_path("\tcamera\n"), Some(Page::Camera));
        assert_eq!(Page::from_path("photograph"), None);
        assert_eq!(Page::from_path(""), None);
        assert_eq!(Page::from_path("   "), None);
    }

    #[test]
    fn every_page_has_a_stable_wire_code() {
        for (position, page) in Page::ALL.iter().enumerate() {
            assert_eq!(usize::from(page.code()), position, "{page:?}");
            assert_eq!(Page::from_code(page.code()), Some(*page), "{page:?}");
        }
        assert_eq!(
            Page::from_code(Page::ALL.len() as u8),
            None,
            "one past the last page"
        );
        assert_eq!(Page::from_code(u8::MAX), None, "a code no page owns");
    }

    #[test]
    fn no_two_pages_share_a_path() {
        for (position, page) in Page::ALL.iter().enumerate() {
            for other in &Page::ALL[position + 1..] {
                assert_ne!(page.path(), other.path(), "{page:?} and {other:?}");
            }
        }
    }

    #[test]
    fn every_route_is_either_bootable_or_transient() {
        for page in Page::ALL {
            assert!(
                page.bootable() || page.timeout_ms().is_some(),
                "{page:?} is neither somewhere the device may come up nor a transient with a \
                 lifetime"
            );
        }
    }
}
