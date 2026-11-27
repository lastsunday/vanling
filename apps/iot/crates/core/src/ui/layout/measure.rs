//! What a laid-out declaration hands the next input: the rectangles a hit test reads, kept
//! after the frame that produced them is gone.

use super::MAX_SCROLL_AREAS;
use super::node::{NodeId, NodeKind};
use super::scroll::{ScrollId, ScrollInfo};
use crate::ui::Rect;

/// The leaves a page paints itself; one slot each, so a hit test can find them.
pub const PAINTS: usize = 8;

/// The tags one declaration can report; the menu's grid is the widest at twenty-one.
pub const MAX_TAGS: usize = 24;

/// A frame's geometry, in panel pixels: layout has already applied the scroll offsets, so a
/// hit test adds nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    page: u8,
    slots: [Option<Rect>; PAINTS],
    tags: [(u16, Rect); MAX_TAGS],
    tag_count: u8,
    scrolls: [ScrollInfo; MAX_SCROLL_AREAS],
}

impl Default for Layout {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Layout {
    /// No page carries this code, so geometry from before the first paint never matches.
    pub const EMPTY: Self = Self {
        page: u8::MAX,
        slots: [None; PAINTS],
        tags: [(0, Rect::new(0, 0, 0, 0)); MAX_TAGS],
        tag_count: 0,
        scrolls: [ScrollInfo {
            id: ScrollId::NONE,
            rect: Rect::new(0, 0, 0, 0),
            offset: 0,
            content: 0,
            viewport: 0,
        }; MAX_SCROLL_AREAS],
    };

    /// The page this geometry belongs to; a page turn must not leave the old one readable.
    pub const fn page(self) -> u8 {
        self.page
    }

    pub fn set_page(&mut self, page: u8) {
        self.page = page;
    }

    /// Where the leaf painted by `slot` sits, if this frame declared one.
    pub fn slot(&self, slot: u8) -> Option<Rect> {
        self.slots.get(usize::from(slot)).copied().flatten()
    }

    /// Where the node tagged `tag` sits.
    pub fn rect(&self, tag: u16) -> Option<Rect> {
        self.tags[..usize::from(self.tag_count)]
            .iter()
            .find(|(candidate, _)| *candidate == tag)
            .map(|(_, rect)| *rect)
    }

    /// Every tagged node's tag and rectangle, in declaration order.
    pub fn tags(&self) -> impl Iterator<Item = (u16, Rect)> + '_ {
        self.tags[..usize::from(self.tag_count)].iter().copied()
    }

    /// The tag a panel point lands on.
    pub fn at(&self, x: i32, y: i32) -> Option<u16> {
        self.tags[..usize::from(self.tag_count)]
            .iter()
            .find(|(tag, rect)| *tag != 0 && rect.contains(x, y))
            .map(|(tag, _)| *tag)
    }

    /// The last layout of the scroll box named `area`.
    pub fn scroll(&self, area: ScrollId) -> Option<ScrollInfo> {
        if area.is_none() {
            return None;
        }
        self.scrolls.iter().find(|slot| slot.id == area).copied()
    }
}

impl super::Tree {
    /// Reads a laid-out tree's rectangles back out for the next hit test.
    pub(crate) fn geometry(&self) -> Layout {
        let mut layout = Layout::default();
        for index in 0..usize::from(self.len) {
            let info = self.info(NodeId(index as u16));
            if info.style.tag != 0 {
                let tag = info.style.tag;
                if let Some(cell) = layout.tags.get_mut(usize::from(layout.tag_count)) {
                    *cell = (tag, info.rect);
                    layout.tag_count += 1;
                }
            }
            if let NodeKind::Paint { slot, .. } = info.kind
                && let Some(cell) = layout.slots.get_mut(usize::from(slot))
            {
                *cell = Some(info.rect);
            }
        }
        for (index, slot) in self.scrolls.iter().enumerate() {
            if slot.id.is_none() || slot.node == super::node::NONE {
                continue;
            }
            layout.scrolls[index] = ScrollInfo {
                id: slot.id,
                rect: self.rect(NodeId(slot.node)),
                offset: slot.offset,
                content: slot.content,
                viewport: slot.viewport,
            };
        }
        layout
    }
}
