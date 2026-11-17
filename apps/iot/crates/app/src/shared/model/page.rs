//! What the pages publish for the renderers: the copyable view every client draws from.

use super::NetworkView;

/// One field per page that has something to publish. A page with nothing to say leaves its
/// field out rather than publishing an empty record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PageSnapshot {
    pub network: NetworkView,
}
