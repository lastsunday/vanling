//! The pages: what each demands, what a gesture means on it, and when it repaints.

mod ambient;
mod attitude;
mod audio;
mod camera;
mod light;
mod network;
mod speaker;
mod splash;

#[cfg(test)]
pub(crate) mod test_support;

pub use self::network::{Part, joining, walk};

pub use self::light::{
    BREATH_PERIODS_MS, BRIGHTNESS_ENVELOPE_STEP, COLOR_GROUPS, ColorGroup, PALETTE,
    SOLID_BRIGHTNESS_STEPS,
};

use crate::demand::{Claim, DemandKey, DemandKind, EffectiveDemand, Mode, Sensitivity};
use crate::diagnostics::Diagnostics;
use crate::drivers::input::{ButtonEvent, InputEvent};
use crate::intent::{BusinessIntent, OperationIntent};
use crate::state::DeviceState;

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

const MOTION_BACKGROUND: Claim = Claim::new(Mode::Continuous, 0, Sensitivity::Secondary);

const MOTION_FOREGROUND: Claim = Claim::new(Mode::Continuous, 0, Sensitivity::Primary);

const ON_SCREEN: Claim = Claim::new(Mode::Continuous, 0, Sensitivity::Primary);

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

    pub const fn next_page(
        &self,
        motion_enabled: bool,
        audio_enabled: bool,
        playback_enabled: bool,
        camera_enabled: bool,
        network_enabled: bool,
    ) -> Self {
        if matches!(self, Self::Splash) {
            return Self::Ambient;
        }
        let mut step = 1;
        while step <= CYCLE.len() {
            let candidate = CYCLE[(self.index() + step) % CYCLE.len()];
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
        *self
    }

    pub(crate) const fn is_backed(
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
            Self::Splash => true,
        }
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

    pub fn demand(&self, keys: &[DemandKey]) -> EffectiveDemand {
        let mut demand = EffectiveDemand::none();
        for key in keys {
            demand.set(*key, self.claim_for(*key));
        }
        demand
    }

    fn claim_for(&self, key: DemandKey) -> Claim {
        (PAGES[self.index()].claim)(key.kind())
    }
}

pub(crate) fn translate(
    page: DisplayPage,
    op: &OperationIntent,
    current: &DeviceState,
) -> BusinessIntent {
    if page != DisplayPage::Splash
        && matches!(op.event, InputEvent::Button(ButtonEvent::TripleClick))
    {
        return BusinessIntent::NextPage;
    }
    (PAGES[page.index()].translate)(op, current)
}

pub(crate) fn repaint_due(
    page: DisplayPage,
    prev: &Diagnostics,
    next: &Diagnostics,
    overlay: bool,
) -> bool {
    (PAGES[page.index()].repaint)(prev, next, overlay)
}

struct PageSpec {
    #[cfg_attr(not(test), allow(dead_code))]
    page: DisplayPage,
    claim: fn(DemandKind) -> Claim,
    translate: fn(&OperationIntent, &DeviceState) -> BusinessIntent,
    repaint: fn(&Diagnostics, &Diagnostics, bool) -> bool,
}

const PAGES: [PageSpec; DisplayPage::ALL.len()] = [
    PageSpec {
        page: DisplayPage::Ambient,
        claim: ambient::claim,
        translate: light::translate,
        repaint: ambient::repaint,
    },
    PageSpec {
        page: DisplayPage::Attitude,
        claim: attitude::claim,
        translate: light::translate,
        repaint: attitude::repaint,
    },
    PageSpec {
        page: DisplayPage::Audio,
        claim: audio::claim,
        translate: audio::translate,
        repaint: audio::repaint,
    },
    PageSpec {
        page: DisplayPage::Speaker,
        claim: speaker::claim,
        translate: speaker::translate,
        repaint: speaker::repaint,
    },
    PageSpec {
        page: DisplayPage::Camera,
        claim: camera::claim,
        translate: light::translate,
        repaint: camera::repaint,
    },
    PageSpec {
        page: DisplayPage::Network,
        claim: network::claim,
        translate: network::translate,
        repaint: network::repaint,
    },
    PageSpec {
        page: DisplayPage::Splash,
        claim: splash::claim,
        translate: splash::translate,
        repaint: splash::repaint,
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
}
