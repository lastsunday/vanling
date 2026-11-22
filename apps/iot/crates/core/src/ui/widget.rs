//! What a page's components implement: a value that declares its own nodes.

use super::declare::Ui;
use super::layout::NodeId;

/// A component that declares itself into a page's tree and hands back its root.
pub trait Widget {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId;
}
