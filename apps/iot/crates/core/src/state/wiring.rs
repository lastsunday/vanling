//! What one boot decides before any state exists: the parts the board wired, what each
//! surface is for, and the page and demand that follow from them.

use alloc::vec::Vec;

use crate::demand::{DemandKey, Override};
use crate::drivers::motion::MotionCapabilities;
use crate::page::DisplayPage;

use super::MAX_LIGHTS;

/// What the board wired, in one value. The page cycle and the boot-page clamp both ask this
/// rather than five positional bools that are one transposition away from a wrong page.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Backed {
    /// `None` = no motion part on this board. `Some` = the part, carrying what it declares it
    /// can report — a part that reports nothing is still a part.
    pub motion: Option<MotionCapabilities>,
    pub audio: bool,
    pub playback: bool,
    pub camera: bool,
    pub network: bool,
}

impl Backed {
    pub const NONE: Self = Self {
        motion: None,
        audio: false,
        playback: false,
        camera: false,
        network: false,
    };

    /// What the motion part declares, and nothing at all when there is no part.
    pub const fn motion_caps(&self) -> MotionCapabilities {
        match self.motion {
            Some(caps) => caps,
            None => MotionCapabilities::EMPTY,
        }
    }

    /// The keys this wiring claims: `DISPLAY` always, and each part only when it was handed
    /// over. `playback` owns no key of its own.
    pub fn demand_keys(&self) -> Vec<DemandKey> {
        let mut keys = Vec::with_capacity(4);
        keys.push(DemandKey::DISPLAY);
        for (present, key) in [
            (self.motion.is_some(), DemandKey::MOTION),
            (self.audio, DemandKey::AUDIO),
            (self.camera, DemandKey::CAMERA),
        ] {
            if present {
                keys.push(key);
            }
        }
        keys
    }
}

/// Everything one boot decides, before the manager exists.
///
/// One record rather than a chain of setters: every field is known when it is written, so the
/// boot page is clamped and the demand derived exactly once, and no call order can change the
/// answer. `..Boot::default()` covers whatever a test does not care about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Boot {
    /// Which parts the board wired, and what the motion part declares.
    pub backed: Backed,
    /// Kept apart from the wiring on purpose: which keys a board claims is a property of the
    /// part it wired, not only of whether it wired one.
    pub demand_keys: Vec<DemandKey>,
    pub surface_roles: [SurfaceRole; MAX_LIGHTS],
    /// The page to land on after the splash; a page the board cannot back falls back to
    /// [`DisplayPage::Ambient`].
    pub boot_page: DisplayPage,
    /// What the build pins beyond the page's claim.
    pub overrides: Override,
}

/// What one wired surface is for: pages, ambient light, or nothing at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SurfaceRole {
    /// Draws the pages and puts the questions on them.
    #[default]
    Pages,
    /// Shows light only; the link's news lands here when there is no page to say it.
    Lamp,
    /// This slot carries no surface.
    Absent,
}
