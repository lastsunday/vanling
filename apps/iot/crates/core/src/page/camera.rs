use super::{MOTION_BACKGROUND, ON_SCREEN};
use crate::demand::{Claim, DemandKind};

pub(super) const fn claim(kind: DemandKind) -> Claim {
    match kind {
        DemandKind::Camera => ON_SCREEN,
        DemandKind::Motion => MOTION_BACKGROUND,
        DemandKind::Display => ON_SCREEN,
        DemandKind::Interaction | DemandKind::Audio => Claim::off(),
    }
}
