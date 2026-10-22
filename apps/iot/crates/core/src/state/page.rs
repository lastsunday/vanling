//! The pages themselves: what each one is, the order they cycle in, and what each
//! claims from the demand plane. The order lives in one list rather than in a jump per
//! page, so every page a board can back stays reachable from every other.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayPage {
    #[default]
    Ambient,
    Attitude,
    Audio,
    Speaker,
    /// The camera feed, which replaces the background rather than sitting on one: the panel
    /// shows the sensor's picture and prints its readout over it, so there is no fill behind
    /// it and the light's colour is not drawn here.
    Camera,
    /// The link: which access point it is on, the address it holds and — while the build is
    /// unconfigured, which is how the product ships — what a scan can see. The page the
    /// panel will take credentials on; until then it is a readout, and the readout is what
    /// makes "the link came up" a thing the bench can see rather than log.
    Network,
    /// The page a device shows while it is still starting, and the one page whose subject is not a
    /// light: nothing on it moves, no gesture but the button's hold answers, and the only way off it
    /// is the clock.
    ///
    /// The hold is how setup mode is asked for on a board that has no other way in — reset, then
    /// hold — and the page is what keeps that request out of every other long press: a device that
    /// is up and running gives the page's own verb to the same hold, so the way back into the mode
    /// is the deliberate ceremony rather than a gesture any impatient finger could spend on a
    /// working link. Deliberately not in [`CYCLE`], so there is no way back to it: a device that
    /// wrapped round to the splash would be showing a page nobody can use.
    Splash,
}

/// The page cycle, in order.
///
/// The list is the definition of the order rather than a set of jumps written page to page: a
/// cycle spelled as transitions between pairs can close through the wrong page and lose one,
/// which is how `Ambient` stopped being reachable once the camera joined — the camera left for
/// "a page the board can back" that was not the start, and every later page skipped the start
/// too, so nothing returned to it. Walking one list and wrapping cannot skip an entry the
/// board can back, so every page in it is reachable from every other.
pub(super) const CYCLE: [DisplayPage; 6] = [
    DisplayPage::Ambient,
    DisplayPage::Attitude,
    DisplayPage::Audio,
    DisplayPage::Speaker,
    DisplayPage::Camera,
    DisplayPage::Network,
];

impl DisplayPage {
    /// The next page in cycle order, skipping pages the board cannot back.
    ///
    /// Walks `CYCLE` from just after this page and wraps, so the order is the one declared
    /// there and the start is a page like any other. Self-healing: a page the wiring cannot
    /// back is stepped over rather than landed on, and `Ambient` needs no capability, so a
    /// board that lost one mid-cycle always has somewhere to go.
    pub const fn next_page(
        &self,
        motion_enabled: bool,
        audio_enabled: bool,
        playback_enabled: bool,
        camera_enabled: bool,
        network_enabled: bool,
    ) -> Self {
        // The splash is left by its clock and not by a gesture, and it is not a candidate below:
        // a cycle that could wrap round to it would offer a page with nothing on it. The answer
        // is the cycle's fixed point, which every board backs.
        if matches!(self, Self::Splash) {
            return Self::Ambient;
        }
        let mut step = 1;
        while step <= CYCLE.len() {
            let candidate = CYCLE[(self.cycle_index() + step) % CYCLE.len()];
            if candidate.is_backed(
                motion_enabled,
                audio_enabled,
                playback_enabled,
                camera_enabled,
                network_enabled,
            ) {
                return candidate;
            }
            step += 1;
        }
        // Only reachable if the board backs nothing at all, which `Ambient` contradicts.
        *self
    }

    /// Whether the wiring can back this page at all. `Ambient` is the answer for every board,
    /// which is what makes it the cycle's fixed point.
    pub(super) const fn is_backed(
        &self,
        motion_enabled: bool,
        audio_enabled: bool,
        playback_enabled: bool,
        camera_enabled: bool,
        network_enabled: bool,
    ) -> bool {
        match self {
            Self::Ambient => true,
            Self::Attitude => motion_enabled,
            Self::Audio => audio_enabled,
            Self::Speaker => playback_enabled,
            Self::Camera => camera_enabled,
            Self::Network => network_enabled,
            // Every board backs the splash: it asks for no surface, which is why it is also not a
            // page a build may name as its own (see `DeviceManager::with_boot_page`).
            Self::Splash => true,
        }
    }

    /// Position in [`CYCLE`], the order `next_page` walks.
    ///
    /// Out of line on purpose: the match lowers to a jump table, and inlined it sits beside every
    /// caller — 456 B of DRAM for the seventh variant, measured with `nm` against the S3's main
    /// stack budget (`@/records/iot/wifi.md`).
    #[inline(never)]
    pub(super) const fn cycle_index(&self) -> usize {
        match self {
            Self::Ambient => 0,
            Self::Attitude => 1,
            Self::Audio => 2,
            Self::Speaker => 3,
            Self::Camera => 4,
            Self::Network => 5,
            // Never read: `next_page` answers for the splash before it indexes the cycle, and no
            // other caller of this maps a page to a position.
            Self::Splash => 0,
        }
    }

    /// What this page asks of the planes a board declares.
    ///
    /// The plane list is the board's, not this page's: a plane no board wires is never
    /// claimed, so the same page code serves a board with a microphone and one without.
    ///
    /// The mode and the class follow the page, not the plane: a mode says whether a plane
    /// runs, a class says how stale a reading this page can put up with, and only the page
    /// knows that.
    pub fn demand(&self, planes: &[PlaneKey]) -> EffectiveDemand {
        let mut demand = EffectiveDemand::none();
        for key in planes {
            demand.set(*key, self.claim_for(*key));
        }
        demand
    }

    /// What this page asks of one plane. The match is the page's claim and is exhaustive by
    /// construction: a plane this page has no opinion of is claimed off, never unclaimed.
    ///
    /// The picture and the envelope are claimed only by the page that draws them: a page that
    /// reads neither has no use for the part, and on this board both are paid for by polling
    /// (`@/records/iot/input.md`). Motion stays claimed on every page because nothing has yet
    /// exercised the IMU's stop and re-arm.
    fn claim_for(&self, key: PlaneKey) -> PlaneDemand {
        let motion_class = match self {
            Self::Attitude => Sensitivity::Primary,
            _ => Sensitivity::Secondary,
        };
        match key.field() {
            DemandField::Motion => PlaneDemand::new(Mode::Continuous, 0, motion_class),
            DemandField::Audio => match self {
                Self::Audio => PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Primary),
                _ => PlaneDemand::off(),
            },
            DemandField::Camera => match self {
                Self::Camera => PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Primary),
                _ => PlaneDemand::off(),
            },
            DemandField::Display => PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Primary),
            // Nothing gates the interaction path, so no page claims it.
            DemandField::Interaction => PlaneDemand::off(),
        }
    }
}
