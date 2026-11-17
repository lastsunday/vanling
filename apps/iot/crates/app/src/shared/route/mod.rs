//! The pages as vocabulary: a route table declares, for every page, the name it answers to,
//! what it needs from the board, what it claims of a demand, whether it can be booted on, and
//! how long it lives. The enum is the destination; [`PageId::route`] is the one
//! exhaustive map from a destination to its row, so a new page cannot exist without a route.
//!
//! How a page is painted — its ground, whether frames arrive from a camera, how often it may
//! repaint, what a snapshot drift has to move — is the page's own answer (`pages`), not this
//! table's.
//!
//! [`PageId::ALL`] is the button's order too: one list, so a page cannot be listed and still be
//! unreachable from the button. [`Navigator`] walks it (`nav`).

mod ambient;
mod attitude;
mod audio;
mod camera;
mod env;
mod nav;
mod network;
mod speaker;
mod splash;

pub use self::nav::{MAX_DEPTH, Navigator};

pub use self::splash::SPLASH_MS;

use iot_core::demand::{Claim, DemandKey, DemandKind, EffectiveDemand, Mode, Priority};
use iot_core::drivers::board::Backed;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageId {
    #[default]
    Ambient,
    Attitude,
    Audio,
    Speaker,
    Camera,
    Network,
    /// The transient the boot lands through, never on: it has a timeout and is not bootable.
    Splash,
    Env,
}

/// One page's row in the route table: the properties that follow from being that page.
struct Route {
    /// The name the page answers to, trimmed and case-insensitive through
    /// [`PageId::from_path`].
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

static ENV: Route = Route {
    path: "env",
    needs: |_| true,
    claim: env::claim,
    bootable: true,
    timeout_ms: None,
};

impl PageId {
    /// Every page, in the button's order: the wire code is its index, and [`Self::next_page`]
    /// walks it.
    pub const ALL: [Self; 8] = [
        Self::Ambient,
        Self::Attitude,
        Self::Audio,
        Self::Speaker,
        Self::Camera,
        Self::Network,
        Self::Splash,
        Self::Env,
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
            Self::Env => &ENV,
        }
    }

    /// The page the button turns to: the next page in [`Self::ALL`] the board backs, wrapping.
    /// A transient has no place in the lap and stays until its lifetime ends.
    pub fn next_page(&self, backed: Backed) -> Self {
        if self.timeout_ms().is_some() {
            return *self;
        }
        let Some(here) = Self::ALL.iter().position(|page| page == self) else {
            return *self;
        };
        for step in 1..=Self::ALL.len() {
            let candidate = Self::ALL[(here + step) % Self::ALL.len()];
            if candidate.timeout_ms().is_none() && candidate.is_backed(backed) {
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
            Self::Env => 7,
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
    use alloc::vec::Vec;
    use iot_core::drivers::motion::MotionCapabilities;

    #[test]
    fn every_page_answers_to_its_own_path() {
        for page in PageId::ALL {
            assert_eq!(PageId::from_path(page.path()), Some(page), "{page:?}");
            assert_eq!(
                PageId::from_path(&page.path().to_ascii_uppercase()),
                Some(page),
                "{page:?} in capitals"
            );
        }
        assert_eq!(PageId::from_path("  CAMERA  "), Some(PageId::Camera));
        assert_eq!(PageId::from_path("\tcamera\n"), Some(PageId::Camera));
        assert_eq!(PageId::from_path("photograph"), None);
        assert_eq!(PageId::from_path(""), None);
        assert_eq!(PageId::from_path("   "), None);
    }

    #[test]
    fn the_wire_code_is_the_position_in_all() {
        for (position, page) in PageId::ALL.iter().enumerate() {
            assert_eq!(usize::from(page.code()), position, "{page:?}");
        }
    }

    #[test]
    fn no_two_pages_share_a_path() {
        for (position, page) in PageId::ALL.iter().enumerate() {
            for other in &PageId::ALL[position + 1..] {
                assert_ne!(page.path(), other.path(), "{page:?} and {other:?}");
            }
        }
    }

    #[test]
    fn every_route_is_either_bootable_or_transient() {
        for page in PageId::ALL {
            assert!(
                page.bootable() || page.timeout_ms().is_some(),
                "{page:?} is neither somewhere the device may come up nor a transient with a \
                 lifetime"
            );
        }
    }

    /// One tap at a time from the default page, until the lap closes back on it.
    fn lap(board: Backed) -> Vec<PageId> {
        let mut walked = Vec::new();
        let mut page = PageId::Ambient;
        for _ in 0..PageId::ALL.len() {
            page = page.next_page(board);
            if page == PageId::Ambient {
                break;
            }
            walked.push(page);
        }
        walked
    }

    /// The board's capabilities decide which pages exist; the lap is the table's own order.
    #[test]
    fn one_lap_visits_exactly_the_pages_the_board_backs() {
        let board =
            |motion: bool, audio: bool, playback: bool, camera: bool, network: bool| Backed {
                motion: motion.then_some(MotionCapabilities::EMPTY),
                audio,
                playback,
                camera,
                network,
            };
        let env = PageId::Env;
        assert_eq!(lap(Backed::NONE), [env]);
        assert_eq!(
            lap(board(true, false, false, false, false)),
            [PageId::Attitude, env]
        );
        assert_eq!(
            lap(board(false, true, false, false, false)),
            [PageId::Audio, env]
        );
        assert_eq!(
            lap(board(false, false, true, false, false)),
            [PageId::Speaker, env]
        );
        assert_eq!(
            lap(board(false, false, false, true, false)),
            [PageId::Camera, env]
        );
        assert_eq!(
            lap(board(false, false, false, false, true)),
            [PageId::Network, env]
        );
        assert_eq!(
            lap(board(true, true, true, true, true)),
            [
                PageId::Attitude,
                PageId::Audio,
                PageId::Speaker,
                PageId::Camera,
                PageId::Network,
                env
            ]
        );
    }

    #[test]
    fn every_page_the_board_can_back_is_shown_on_the_way_round() {
        for (motion, audio, playback, camera, network) in [
            (false, false, false, false, false),
            (true, false, false, false, false),
            (false, true, false, false, false),
            (false, false, true, false, false),
            (false, false, false, true, false),
            (false, false, false, false, true),
            (true, true, true, true, true),
            (false, true, true, true, true),
            (true, false, false, true, true),
            (true, true, true, true, false),
        ] {
            let wired = Backed {
                motion: motion.then_some(MotionCapabilities::EMPTY),
                audio,
                playback,
                camera,
                network,
            };
            let cycle: Vec<PageId> = PageId::ALL
                .into_iter()
                .filter(|page| page.timeout_ms().is_none())
                .collect();
            let backed: Vec<PageId> = cycle
                .iter()
                .copied()
                .filter(|page| page.is_backed(wired))
                .collect();
            assert!(
                !backed.is_empty(),
                "Ambient needs no capability, so there is always somewhere to go"
            );
            for start in cycle {
                let start_is_backed = backed.contains(&start);
                let mut seen = Vec::new();
                let mut page = start;
                for _ in 0..backed.len() {
                    page = page.next_page(wired);
                    seen.push(page);
                }
                let mut expected = backed.clone();
                let mut walked = seen.clone();
                expected.sort_by_key(|page| page.code());
                walked.sort_by_key(|page| page.code());
                assert_eq!(
                    walked, expected,
                    "a full turn from {start:?} did not visit each backed page once \
                     (motion={motion} audio={audio} playback={playback} camera={camera} \
                     network={network})"
                );
                if start_is_backed {
                    assert_eq!(
                        page, start,
                        "and a full turn from a page the board backs comes back to it, so \
                         the toggle is a cycle rather than a drift"
                    );
                } else {
                    assert!(
                        backed.contains(&seen[0]),
                        "one tap off {start:?} enters the cycle rather than staying put"
                    );
                }
            }
        }
    }
}
