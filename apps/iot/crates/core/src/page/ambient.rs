use super::{MOTION_BACKGROUND, ON_SCREEN};
use crate::demand::{Claim, DemandKind};
use crate::diagnostics::Diagnostics;

pub(super) const fn claim(kind: DemandKind) -> Claim {
    match kind {
        DemandKind::Motion => MOTION_BACKGROUND,
        DemandKind::Display => ON_SCREEN,
        DemandKind::Interaction | DemandKind::Audio | DemandKind::Camera => Claim::off(),
    }
}

pub(super) const fn repaint(_prev: &Diagnostics, _next: &Diagnostics, overlay: bool) -> bool {
    overlay
}
