//! The Ambient page: the debug overlay over an empty ground, and nothing on a release build.

use crate::shared::view_kit::surface::Surface;

impl Surface<'_> {
    pub(crate) fn stamp_ambient(&mut self) {
        let live = self.live();
        self.stamp_diagnostics(live);
    }
}
