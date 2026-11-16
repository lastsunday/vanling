use super::{MOTION_BACKGROUND, ON_SCREEN};
use iot_core::demand::{Claim, DemandKind};

/// How long the boot page stays up before the build's page takes over. A page property:
/// the splash owns its claim; this is how long it exists.
pub const SPLASH_MS: u64 = 1_500;

pub(super) const fn claim(kind: DemandKind) -> Claim {
    match kind {
        DemandKind::Motion => MOTION_BACKGROUND,
        DemandKind::Display => ON_SCREEN,
        DemandKind::Interaction | DemandKind::Audio | DemandKind::Camera => Claim::off(),
    }
}
