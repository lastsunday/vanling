//! A page's tags measured once: the rectangles a hit test reads, taken from the same
//! declaration the painter draws.

use iot_core::ui::{Rect, Ui};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TagTable<const N: usize> {
    entries: [(u16, Rect); N],
}

impl<const N: usize> Default for TagTable<N> {
    fn default() -> Self {
        Self {
            entries: [(0, Rect::default()); N],
        }
    }
}

impl<const N: usize> TagTable<N> {
    /// Measures a laid-out page; tags past `N` are dropped, so `N` is the page's own count.
    pub fn measure(ui: &Ui<'_, '_>) -> Self {
        let mut table = Self::default();
        for (slot, tag) in table.entries.iter_mut().zip(ui.tags()) {
            *slot = tag;
        }
        table
    }

    pub fn rect(&self, tag: u16) -> Option<Rect> {
        self.entries
            .iter()
            .find(|(candidate, _)| *candidate == tag)
            .map(|(_, rect)| *rect)
    }

    /// The tag a panel point lands on inside a scrolling box, or zero.
    pub fn at(&self, x: i32, y: i32, offset: i16) -> u16 {
        let content_y = y + i32::from(offset);
        self.entries
            .iter()
            .find(|(tag, rect)| *tag != 0 && rect.contains(x, content_y))
            .map_or(0, |(tag, _)| *tag)
    }

    pub fn iter(&self) -> impl Iterator<Item = (u16, Rect)> + '_ {
        self.entries.iter().copied().filter(|(tag, _)| *tag != 0)
    }
}
