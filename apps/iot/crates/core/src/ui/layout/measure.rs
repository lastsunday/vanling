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

/// The control under a panel point: `slide` says a sideways drag belongs to the box, not the
/// scroll region it sits in, and `clip` names that region when the box hangs out of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub tag: u16,
    pub slide: bool,
    pub rect: Rect,
    clip: u8,
}

impl Hit {
    const NONE: Self = Self {
        tag: 0,
        slide: false,
        rect: Rect::new(0, 0, 0, 0),
        clip: 0,
    };
}

/// A frame's geometry, in panel pixels: layout has already applied the scroll offsets, so a
/// hit test adds nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    page: u8,
    slots: [Option<Rect>; PAINTS],
    hits: [Hit; MAX_TAGS],
    hit_count: u8,
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
        hits: [Hit::NONE; MAX_TAGS],
        hit_count: 0,
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

    /// Where the node tagged `tag` sits; the last declaration wins, the one drawn on top.
    pub fn rect(&self, tag: u16) -> Option<Rect> {
        self.lookup(tag).map(|hit| hit.rect)
    }

    fn lookup(&self, tag: u16) -> Option<Hit> {
        self.hits[..usize::from(self.hit_count)]
            .iter()
            .rev()
            .find(|hit| hit.tag == tag)
            .copied()
    }

    /// Every tagged node's tag and rectangle, in declaration order.
    pub fn tags(&self) -> impl Iterator<Item = (u16, Rect)> + '_ {
        self.hits[..usize::from(self.hit_count)]
            .iter()
            .map(|hit| (hit.tag, hit.rect))
    }

    /// The control a panel point lands on: the topmost tagged box that contains it and is not
    /// clipped away at that point.
    pub fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        self.hits[..usize::from(self.hit_count)]
            .iter()
            .rev()
            .find(|hit| hit.tag != 0 && hit.rect.contains(x, y) && self.through(hit, x, y))
            .copied()
    }

    /// Whether a hit's box is still inside the region that clips it.
    fn through(&self, hit: &Hit, x: i32, y: i32) -> bool {
        hit.clip == 0
            || self
                .scrolls
                .get(usize::from(hit.clip) - 1)
                .is_some_and(|slot| slot.rect.contains(x, y))
    }

    /// The tag a panel point lands on.
    pub fn at(&self, x: i32, y: i32) -> Option<u16> {
        self.hit(x, y).map(|hit| hit.tag)
    }

    /// Whether a sideways drag at `x`, `y` belongs to the control under it.
    pub fn slides(&self, x: i32, y: i32) -> bool {
        self.hit(x, y).is_some_and(|hit| hit.slide)
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
            if info.style.tag != 0
                && let Some(cell) = layout.hits.get_mut(usize::from(layout.hit_count))
            {
                *cell = Hit {
                    tag: info.style.tag,
                    slide: info.style.slides(),
                    rect: info.rect,
                    clip: if info.clipped {
                        self.clip_of(info.clip)
                    } else {
                        0
                    },
                };
                layout.hit_count += 1;
            }
            if let NodeKind::Paint { slot, .. } = info.kind
                && let Some(cell) = layout.slots.get_mut(usize::from(slot))
            {
                *cell = Some(info.rect);
            }
        }
        for (index, slot) in self.scrolls.iter().enumerate() {
            if slot.node == super::node::NONE {
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

    /// The scroll region whose box is `clip`, as a one-based slot number; zero when none.
    fn clip_of(&self, clip: Rect) -> u8 {
        self.scrolls
            .iter()
            .position(|slot| slot.node != super::node::NONE && self.rect(NodeId(slot.node)) == clip)
            .and_then(|index| u8::try_from(index + 1).ok())
            .unwrap_or(0)
    }
}
