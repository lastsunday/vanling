//! The vocabulary a page declares its boxes with: lengths, edges, axes, alignment, and flow.

use super::constraints::{PX, Subpx};
use super::scroll::ScrollId;
use crate::ui::typeset;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Length {
    #[default]
    Auto,
    Px(i16),
    Percent(i16),
}

impl Length {
    pub const fn px(value: i16) -> Self {
        Self::Px(value)
    }

    pub const fn pct(value: i16) -> Self {
        Self::Percent(value)
    }

    pub(super) fn resolve(self, base: Subpx) -> Option<Subpx> {
        match self {
            Self::Auto => None,
            Self::Px(value) => Some(Subpx::from(value) * PX),
            Self::Percent(value) => Some(base * Subpx::from(value) / 100),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edges {
    pub left: i16,
    pub top: i16,
    pub right: i16,
    pub bottom: i16,
}

impl Edges {
    pub const fn all(value: i16) -> Self {
        Self {
            left: value,
            top: value,
            right: value,
            bottom: value,
        }
    }

    pub(super) fn horizontal(self) -> Subpx {
        (self.left as Subpx + self.right as Subpx) * PX
    }

    pub(super) fn vertical(self) -> Subpx {
        (self.top as Subpx + self.bottom as Subpx) * PX
    }

    pub(super) fn left(self) -> Subpx {
        self.left as Subpx * PX
    }

    pub(super) fn top(self) -> Subpx {
        self.top as Subpx * PX
    }
}

/// Where an absolutely positioned child sits: a side that was named, or none. `None` and
/// `Some(0)` are different answers, which is why this is not [`Edges`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Inset {
    pub left: Option<i16>,
    pub top: Option<i16>,
    pub right: Option<i16>,
    pub bottom: Option<i16>,
}

impl Inset {
    pub(super) const NONE: Self = Self {
        left: None,
        top: None,
        right: None,
        bottom: None,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Axis {
    #[default]
    Column,
    Row,
}

/// Cross-axis placement of a flex child.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

/// Main-axis distribution of a flex container.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// Inline placement of a text run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
    /// Widens the spaces of every line but the last until it fills the box.
    Justify,
}

/// How a box arranges its children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Flow {
    #[default]
    Flex,
    Wrap,
    Stack,
    Scroll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Style {
    pub width: Length,
    pub height: Length,
    pub min_width: Length,
    pub min_height: Length,
    pub max_width: Length,
    pub max_height: Length,
    pub padding: Edges,
    pub inset: Inset,
    pub gap: i16,
    pub flow: Flow,
    pub axis: Axis,
    pub justify: Justify,
    pub align: Align,
    pub align_self: Align,
    pub align_content: Justify,
    pub grow: u16,
    pub shrink: u16,
    pub basis: Length,
    pub absolute: bool,
    pub text_align: TextAlign,
    pub nowrap: bool,
    pub max_lines: u16,
    pub scroll_offset: i16,
    pub scroll_area: ScrollId,
    pub tag: u16,
    pub fill: bool,
    pub border: bool,
}

impl Style {
    pub const fn new() -> Self {
        Self {
            width: Length::Auto,
            height: Length::Auto,
            min_width: Length::Auto,
            min_height: Length::Auto,
            max_width: Length::Auto,
            max_height: Length::Auto,
            padding: Edges::all(0),
            inset: Inset::NONE,
            gap: 0,
            flow: Flow::Flex,
            axis: Axis::Column,
            justify: Justify::Start,
            align: Align::Start,
            align_self: Align::Start,
            align_content: Justify::Start,
            grow: 0,
            shrink: 0,
            basis: Length::Auto,
            absolute: false,
            text_align: TextAlign::Start,
            nowrap: false,
            max_lines: typeset::MAX_LINES as u16,
            scroll_offset: 0,
            scroll_area: ScrollId::NONE,
            tag: 0,
            fill: false,
            border: false,
        }
    }

    pub const fn row(mut self) -> Self {
        self.axis = Axis::Row;
        self
    }

    pub const fn column(mut self) -> Self {
        self.axis = Axis::Column;
        self
    }

    pub const fn wrap(mut self) -> Self {
        self.flow = Flow::Wrap;
        self
    }

    pub const fn stack(mut self) -> Self {
        self.flow = Flow::Stack;
        self
    }

    pub const fn scrolling(mut self) -> Self {
        self.flow = Flow::Scroll;
        self
    }

    pub const fn size(mut self, width: i16, height: i16) -> Self {
        self.width = Length::Px(width);
        self.height = Length::Px(height);
        self
    }

    pub const fn width(mut self, value: i16) -> Self {
        self.width = Length::Px(value);
        self
    }

    pub const fn height(mut self, value: i16) -> Self {
        self.height = Length::Px(value);
        self
    }

    pub const fn min_width(mut self, value: i16) -> Self {
        self.min_width = Length::Px(value);
        self
    }

    pub const fn max_width(mut self, value: i16) -> Self {
        self.max_width = Length::Px(value);
        self
    }

    pub const fn grow(mut self, value: u16) -> Self {
        self.grow = value;
        self
    }

    pub const fn shrink(mut self, value: u16) -> Self {
        self.shrink = value;
        self
    }

    pub const fn basis(mut self, value: i16) -> Self {
        self.basis = Length::Px(value);
        self
    }

    pub const fn pad(mut self, value: i16) -> Self {
        self.padding = Edges::all(value);
        self
    }

    pub const fn padding(mut self, left: i16, top: i16, right: i16, bottom: i16) -> Self {
        self.padding = Edges {
            left,
            top,
            right,
            bottom,
        };
        self
    }

    pub const fn gap(mut self, value: i16) -> Self {
        self.gap = value;
        self
    }

    pub const fn center(mut self) -> Self {
        self.justify = Justify::Center;
        self.align = Align::Center;
        self
    }

    pub const fn middle(mut self) -> Self {
        self.align = Align::Center;
        self
    }

    pub const fn stretch(mut self) -> Self {
        self.align = Align::Stretch;
        self
    }

    pub const fn end(mut self) -> Self {
        self.align = Align::End;
        self
    }

    pub const fn between(mut self) -> Self {
        self.justify = Justify::SpaceBetween;
        self
    }

    pub const fn content(mut self, value: Justify) -> Self {
        self.align_content = value;
        self
    }

    pub const fn absolute(mut self) -> Self {
        self.absolute = true;
        self
    }

    pub const fn left(mut self, value: i16) -> Self {
        self.inset.left = Some(value);
        self
    }

    pub const fn top(mut self, value: i16) -> Self {
        self.inset.top = Some(value);
        self
    }

    pub const fn right(mut self, value: i16) -> Self {
        self.inset.right = Some(value);
        self
    }

    pub const fn bottom(mut self, value: i16) -> Self {
        self.inset.bottom = Some(value);
        self
    }

    pub const fn text_align(mut self, align: TextAlign) -> Self {
        self.text_align = align;
        self
    }

    pub const fn nowrap(mut self) -> Self {
        self.nowrap = true;
        self
    }

    pub const fn lines(mut self, value: u16) -> Self {
        self.max_lines = value;
        self
    }

    /// How far a scroll box's content is shifted against its main axis.
    pub const fn scroll_offset(mut self, value: i16) -> Self {
        self.scroll_offset = value;
        self
    }

    /// Names a scroll box so layout reports its rectangle and travel.
    pub const fn scroll_area(mut self, area: ScrollId) -> Self {
        self.scroll_area = area;
        self
    }

    pub const fn tag(mut self, value: u16) -> Self {
        self.tag = value;
        self
    }

    /// Paints the box's own paper: the ground turned over, under everything inside it.
    pub const fn filled(mut self) -> Self {
        self.fill = true;
        self
    }

    /// Draws a one-pixel outline around the box.
    pub const fn bordered(mut self) -> Self {
        self.border = true;
        self
    }
}
