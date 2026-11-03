//! What one boot decides before any state exists: the parts the board wired, what each
//! surface is for, and the page and demand that follow from them.

use alloc::vec::Vec;

use crate::demand::{DemandKey, Override};
use crate::drivers::board::Backed;
use crate::drivers::surface::{MAX_LIGHTS, SurfaceRole};
use crate::page::DisplayPage;

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
