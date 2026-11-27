//! What a frame's pixels depend on, kept between paints so the painter can name the rectangles
//! that changed instead of the page having to.

use super::node::MAX_NODES;
use crate::drivers::surface::FrameDamage;

/// One frame's fingerprint: a hash of the tree's shape, and one of each node's content.
///
/// The structure hash covers what layout decides (kind, rectangle, clip), so a different tree
/// cannot be patched. A node whose content hash moved is repainted inside its own rectangle.
#[derive(Clone, Copy, Debug)]
pub struct FramePrint {
    pub(crate) structure: u32,
    pub(crate) content: [u32; MAX_NODES],
    pub(crate) len: u16,
}

impl FramePrint {
    pub const fn new() -> Self {
        Self {
            structure: 0,
            content: [0; MAX_NODES],
            len: 0,
        }
    }
}

impl Default for FramePrint {
    fn default() -> Self {
        Self::new()
    }
}

/// What the painter already knows about the pixels a declaration is about to make.
#[derive(Clone, Copy)]
pub enum Source<'a> {
    /// The caller laid the frame to the ground; every pixel is drawn again.
    Full,
    /// The caller moved pixels itself and names what it has to draw again (a scroll blit).
    Window(FrameDamage),
    /// Compare the declaration with the last frame's fingerprint.
    Diff {
        prev: Option<&'a FramePrint>,
        /// Whether the snapshot the page draws from moved since the last paint, which is the
        /// only way a custom leaf that does not describe its pixels can have changed.
        moved: bool,
    },
}
