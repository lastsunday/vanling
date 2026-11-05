//! The pages as vocabulary: a route table declares, for every page, the name it answers to,
//! what it needs from the board, what it grounds the panel with, whether it draws frames,
//! what it claims of a demand, how often it may repaint, whether it can be booted on, and how
//! long it lives. The enum is the destination; [`DisplayPage::route`] is the one exhaustive
//! map from a destination to its row, so a new page cannot exist without a route.
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

/// The least wall time between diagnostics-driven repaints. A repaint ships the whole 320×240
/// frame down one blocking SPI transfer (on the order of 15 ms), so without a bound the live
/// pages hold the shared cooperative executor in a ~15 ms block almost continuously, starving
/// the playback feed and the capture poll past their ring runway. A page raises it for itself
/// in its route.
pub const DIAG_REPAINT_STEP_MS: u64 = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayPage {
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
    /// [`DisplayPage::from_path`].
    path: &'static str,
    /// What the board must wire for this page to be reachable at all.
    needs: fn(Backed) -> bool,
    /// The fill that covers the panel before the page draws, or `None` when it draws over
    /// what the light already showed.
    ground: Option<iot_core::paint::Rgb>,
    /// Whether frames arrive from the camera instead of being drawn.
    draws_frames: bool,
    claim: fn(DemandKind) -> Claim,
    repaint_step_ms: u64,
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
    ground: None,
    draws_frames: false,
    claim: ambient::claim,
    repaint_step_ms: DIAG_REPAINT_STEP_MS,
    bootable: true,
    timeout_ms: None,
};

static ATTITUDE: Route = Route {
    path: "attitude",
    needs: |backed| backed.motion.is_some(),
    ground: None,
    draws_frames: false,
    claim: attitude::claim,
    repaint_step_ms: DIAG_REPAINT_STEP_MS,
    bootable: true,
    timeout_ms: None,
};

static AUDIO: Route = Route {
    path: "audio",
    needs: |backed| backed.audio,
    ground: None,
    draws_frames: false,
    claim: audio::claim,
    repaint_step_ms: DIAG_REPAINT_STEP_MS * 2,
    bootable: true,
    timeout_ms: None,
};

static SPEAKER: Route = Route {
    path: "speaker",
    needs: |backed| backed.playback,
    ground: None,
    draws_frames: false,
    claim: speaker::claim,
    repaint_step_ms: DIAG_REPAINT_STEP_MS,
    bootable: true,
    timeout_ms: None,
};

static CAMERA: Route = Route {
    path: "camera",
    needs: |backed| backed.camera,
    ground: Some(iot_core::paint::Rgb(0, 0, 0)),
    draws_frames: true,
    claim: camera::claim,
    repaint_step_ms: DIAG_REPAINT_STEP_MS,
    bootable: true,
    timeout_ms: None,
};

static NETWORK: Route = Route {
    path: "network",
    needs: |backed| backed.network,
    ground: Some(iot_core::paint::Rgb(0, 0, 0)),
    draws_frames: false,
    claim: network::claim,
    repaint_step_ms: DIAG_REPAINT_STEP_MS,
    bootable: true,
    timeout_ms: None,
};

static SPLASH: Route = Route {
    path: "splash",
    needs: |_| true,
    ground: None,
    draws_frames: false,
    claim: splash::claim,
    repaint_step_ms: DIAG_REPAINT_STEP_MS,
    bootable: false,
    timeout_ms: Some(SPLASH_MS),
};

/// The button's order. A prefix of [`DisplayPage::ALL`], so the page that leaves the cycle
/// (the splash) stays at the tail and the two lists cannot drift apart silently.
pub(crate) const CYCLE: [DisplayPage; 6] = [
    DisplayPage::Ambient,
    DisplayPage::Attitude,
    DisplayPage::Audio,
    DisplayPage::Speaker,
    DisplayPage::Camera,
    DisplayPage::Network,
];

impl DisplayPage {
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

    /// The fill this page grounds the panel with, or `None` when the page draws over what the
    /// light was already showing.
    pub fn ground(&self) -> Option<iot_core::paint::Rgb> {
        self.route().ground
    }

    /// Whether this page's frames arrive from the camera instead of being drawn.
    pub fn draws_frames(&self) -> bool {
        self.route().draws_frames
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

    /// The least wall time this page waits between diagnostics-driven repaints.
    pub fn repaint_step_ms(&self) -> u64 {
        self.route().repaint_step_ms
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
        for (position, page) in DisplayPage::ALL.iter().enumerate() {
            assert_eq!(usize::from(page.code()), position, "{page:?}");
        }
        for (position, page) in CYCLE.iter().enumerate() {
            assert_eq!(DisplayPage::ALL[position], *page);
        }
        assert_eq!(DisplayPage::ALL[CYCLE.len()], DisplayPage::Splash);
    }

    #[test]
    fn every_page_answers_to_its_own_path() {
        for page in DisplayPage::ALL {
            assert_eq!(DisplayPage::from_path(page.path()), Some(page), "{page:?}");
            assert_eq!(
                DisplayPage::from_path(&page.path().to_ascii_uppercase()),
                Some(page),
                "{page:?} in capitals"
            );
        }
        assert_eq!(
            DisplayPage::from_path("  CAMERA  "),
            Some(DisplayPage::Camera)
        );
        assert_eq!(
            DisplayPage::from_path("\tcamera\n"),
            Some(DisplayPage::Camera)
        );
        assert_eq!(DisplayPage::from_path("photograph"), None);
        assert_eq!(DisplayPage::from_path(""), None);
        assert_eq!(DisplayPage::from_path("   "), None);
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
    fn no_two_pages_share_a_path() {
        for (position, page) in DisplayPage::ALL.iter().enumerate() {
            for other in &DisplayPage::ALL[position + 1..] {
                assert_ne!(page.path(), other.path(), "{page:?} and {other:?}");
            }
        }
    }

    #[test]
    fn every_route_is_either_bootable_or_transient() {
        for page in DisplayPage::ALL {
            assert!(
                page.bootable() || page.timeout_ms().is_some(),
                "{page:?} is neither somewhere the device may come up nor a transient with a \
                 lifetime"
            );
        }
    }
}
