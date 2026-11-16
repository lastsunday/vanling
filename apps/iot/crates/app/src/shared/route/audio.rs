use super::{MOTION_BACKGROUND, ON_SCREEN};
use iot_core::demand::{Claim, DemandKind};

pub(super) const fn claim(kind: DemandKind) -> Claim {
    match kind {
        DemandKind::Audio => ON_SCREEN,
        DemandKind::Motion => MOTION_BACKGROUND,
        DemandKind::Display => ON_SCREEN,
        DemandKind::Interaction | DemandKind::Camera => Claim::off(),
    }
}
