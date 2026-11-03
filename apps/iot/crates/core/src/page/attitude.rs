use super::{MOTION_FOREGROUND, ON_SCREEN};
use crate::demand::{Claim, DemandKind};

pub(super) const fn claim(kind: DemandKind) -> Claim {
    match kind {
        DemandKind::Motion => MOTION_FOREGROUND,
        DemandKind::Display => ON_SCREEN,
        DemandKind::Interaction | DemandKind::Audio | DemandKind::Camera => Claim::off(),
    }
}
