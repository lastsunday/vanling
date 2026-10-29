use super::{MOTION_BACKGROUND, ON_SCREEN};
use crate::demand::{Claim, DemandKind};
use crate::diagnostics::Diagnostics;

pub(super) const fn claim(kind: DemandKind) -> Claim {
    match kind {
        DemandKind::Camera => ON_SCREEN,
        DemandKind::Motion => MOTION_BACKGROUND,
        DemandKind::Display => ON_SCREEN,
        DemandKind::Interaction | DemandKind::Audio => Claim::off(),
    }
}

/// Never: a frame arriving is a data event and not anything a snapshot diff can see. Forcing a
/// repaint here would ship a full frame that is a copy of the last one — ~15 ms of blocking SPI.
pub(super) const fn repaint(_prev: &Diagnostics, _next: &Diagnostics, _overlay: bool) -> bool {
    false
}
