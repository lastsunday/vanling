//! The vocabulary a page declares its boxes with: lengths, edges, axes, alignment, and flow.

use super::constraints::{DP, PX, REM, Subpx};
use super::scroll::ScrollId;
use crate::ui::color::ColorId;
use crate::ui::font::Metrics;
use crate::ui::typeset;

/// A length in the unit the page states it in. Four variants, one word: the discriminant is a
/// byte and the payload two, so a fifth unit costs nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Length {
    #[default]
    Auto,
    /// Exact panel pixels.
    Px(i16),
    /// Hundredths of the container's own extent.
    Percent(i16),
    /// Device-independent pixels: 5/4 of a panel pixel on this 200 dpi glass.
    Dp(i16),
    /// Hundredths of the root text size.
    Rem(i16),
}

impl Length {
    pub const fn px(value: i16) -> Self {
        Self::Px(value)
    }

    pub const fn pct(value: i16) -> Self {
        Self::Percent(value)
    }

    pub const fn dp(value: i16) -> Self {
        Self::Dp(value)
    }

    pub const fn rem(value: i16) -> Self {
        Self::Rem(value)
    }

    pub(super) fn resolve(self, base: Subpx) -> Option<Subpx> {
        match self {
            Self::Auto => None,
            Self::Px(value) => Some(Subpx::from(value) * PX),
            Self::Percent(value) => Some(base * Subpx::from(value) / 100),
            Self::Dp(value) => Some(Subpx::from(value) * DP),
            Self::Rem(value) => Some(Subpx::from(value) * REM / 100),
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

/// What a one-line run does when it is wider than its box: Flutter's `TextOverflow`, the half a
/// `nowrap` run can use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Overflow {
    /// Cut the coverage at the box edge; the box itself never grows past what it was given.
    #[default]
    Clip,
    /// Cut at the box edge and end the visible run with an ellipsis.
    Ellipsis,
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

/// A run's weight, on the scale the platform's fonts use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontWeight {
    Thin,
    ExtraLight,
    Light,
    #[default]
    Normal,
    Medium,
    SemiBold,
    Bold,
    ExtraBold,
    Black,
}

impl FontWeight {
    const fn units(self) -> u16 {
        self as u16 + 1
    }

    /// Whether this weight is drawn one column wider than the face's own coverage.
    pub const fn is_heavy(self) -> bool {
        matches!(self, Self::Bold | Self::ExtraBold | Self::Black)
    }
}

/// A line drawn with the text. The bits union, so a run may carry more than one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Decoration(pub u8);

impl Decoration {
    pub const NONE: Self = Self(0);
    pub const UNDERLINE: Self = Self(1);
    pub const OVERLINE: Self = Self(2);
    pub const LINE_THROUGH: Self = Self(4);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn has(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

/// How a decoration line is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DecorationStyle {
    #[default]
    Solid,
    Double,
    Dotted,
    Dashed,
    Wavy,
}

/// Bits of [`Style::flags`], the box's three on/off answers.
mod bits {
    pub const ABSOLUTE: u8 = 1;
    pub const FILL: u8 = 1 << 1;
    pub const BORDER: u8 = 1 << 2;
}

/// Bits of [`Style::text_flags`]. Four bits of weight, three of decoration, three of
/// decoration style, the italic bit, and three of thickness.
mod text {
    pub const WEIGHT: u16 = 0b1111;
    pub const DECORATION: u16 = 0b111 << 6;
    pub const DECORATION_STYLE: u16 = 0b111 << 9;
    pub const ITALIC: u16 = 1 << 12;
    pub const THICKNESS: u16 = 0b111 << 13;
}

/// What a span over a run may change. Metrics stay the node's: a span recolours and
/// redecorates, it never moves a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RunStyle {
    pub color: ColorId,
    pub decoration_color: ColorId,
    pub background_color: ColorId,
    pub decoration: Decoration,
    pub decoration_style: DecorationStyle,
}

impl RunStyle {
    /// Recolours the span; the other slots keep the node's.
    pub const fn color(mut self, color: ColorId) -> Self {
        self.color = color;
        self
    }

    pub const fn decoration(mut self, decoration: Decoration) -> Self {
        self.decoration = decoration;
        self
    }

    pub const fn decoration_style(mut self, style: DecorationStyle) -> Self {
        self.decoration_style = style;
        self
    }

    pub const fn decoration_color(mut self, color: ColorId) -> Self {
        self.decoration_color = color;
        self
    }

    pub const fn background(mut self, color: ColorId) -> Self {
        self.background_color = color;
        self
    }

    /// `node` with this span's non-default slots applied.
    pub const fn over(self, mut node: Style) -> Style {
        if self.color.0 != 0 {
            node.color = self.color;
        }
        if self.decoration_color.0 != 0 {
            node.decoration_color = self.decoration_color;
        }
        if self.background_color.0 != 0 {
            node.background_color = self.background_color;
        }
        if self.decoration.0 != 0 {
            node.text_flags = (node.text_flags & !text::DECORATION)
                | (self.decoration.0 as u16) << 6
                | (self.decoration_style as u16) << 9;
        }
        node
    }
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
    flags: u8,
    pub text_align: TextAlign,
    pub nowrap: bool,
    pub overflow: Overflow,
    pub max_lines: u16,
    pub scroll_offset: i16,
    pub scroll_area: ScrollId,
    pub tag: u16,
    /// The corner radius in px, clamped to half the box; 0 is a square corner.
    pub radius: u8,
    /// The text size in px; 0 is the root size.
    pub font_size: u8,
    /// Weight, family, decoration, decoration style, italic and thickness: see the `text` bits.
    pub text_flags: u16,
    /// The run's colour; [`ColorId::NONE`] keeps the panel's invert-the-ground default.
    pub color: ColorId,
    /// The paper behind the box, drawn before its children.
    pub background_color: ColorId,
    /// The colour of the decoration lines, when they differ from the text.
    pub decoration_color: ColorId,
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
            flags: 0,
            text_align: TextAlign::Start,
            nowrap: false,
            overflow: Overflow::Clip,
            max_lines: typeset::MAX_LINES as u16,
            scroll_offset: 0,
            scroll_area: ScrollId::NONE,
            tag: 0,
            radius: 0,
            font_size: 0,
            text_flags: 0,
            color: ColorId::NONE,
            background_color: ColorId::NONE,
            decoration_color: ColorId::NONE,
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
        self.flags |= bits::ABSOLUTE;
        self
    }

    pub const fn is_absolute(self) -> bool {
        self.flags & bits::ABSOLUTE != 0
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

    pub const fn overflow(mut self, overflow: Overflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// One line that says it was cut: a label in a cell it cannot grow out of.
    pub const fn ellipsis(mut self) -> Self {
        self.nowrap = true;
        self.overflow = Overflow::Ellipsis;
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
        self.flags |= bits::FILL;
        self
    }

    pub const fn has_fill(self) -> bool {
        self.flags & bits::FILL != 0
    }

    /// Draws a one-pixel outline around the box.
    pub const fn bordered(mut self) -> Self {
        self.flags |= bits::BORDER;
        self
    }

    pub const fn has_border(self) -> bool {
        self.flags & bits::BORDER != 0
    }

    /// Rounds the box's painted corners, clamped to half the shorter side and to 16 px, so a
    /// 24 px box written as `12` is a circle. Children are not clipped to it.
    pub const fn radius(mut self, px: u8) -> Self {
        self.radius = px;
        self
    }

    /// Paints the run in `color` instead of the ground turned over.
    pub const fn color(mut self, color: ColorId) -> Self {
        self.color = color;
        self
    }

    /// Lays this colour down behind the box before anything inside it is drawn.
    pub const fn background(mut self, color: ColorId) -> Self {
        self.background_color = color;
        self
    }

    /// Sets the run's size. A length the panel cannot resolve (`Auto`, a percentage of an
    /// unknown parent) falls back to the root size.
    pub const fn font_size(mut self, size: Length) -> Self {
        self.font_size = size_px(size);
        self
    }

    pub const fn font_weight(mut self, weight: FontWeight) -> Self {
        self.text_flags = (self.text_flags & !text::WEIGHT) | weight.units();
        self
    }

    pub const fn decoration(mut self, decoration: Decoration) -> Self {
        self.text_flags = (self.text_flags & !text::DECORATION) | (decoration.0 as u16) << 6;
        self
    }

    pub const fn underline(self) -> Self {
        self.decoration(Decoration::UNDERLINE)
    }

    pub const fn overline(self) -> Self {
        self.decoration(Decoration::OVERLINE)
    }

    pub const fn line_through(self) -> Self {
        self.decoration(Decoration::LINE_THROUGH)
    }

    pub const fn decoration_style(mut self, style: DecorationStyle) -> Self {
        self.text_flags = (self.text_flags & !text::DECORATION_STYLE) | (style as u16) << 9;
        self
    }

    pub const fn italic(mut self) -> Self {
        self.text_flags |= text::ITALIC;
        self
    }

    /// The decoration's thickness in px; 0 keeps the 1 px default.
    pub const fn decoration_thickness(mut self, px: u8) -> Self {
        let px = if px > 7 { 7 } else { px };
        self.text_flags = (self.text_flags & !text::THICKNESS) | (px as u16) << 13;
        self
    }

    pub const fn metrics(&self) -> Metrics {
        Metrics::at(self.font_size as u16)
    }

    pub const fn weight(&self) -> FontWeight {
        match self.text_flags & text::WEIGHT {
            1 => FontWeight::Thin,
            2 => FontWeight::ExtraLight,
            3 => FontWeight::Light,
            5 => FontWeight::Medium,
            6 => FontWeight::SemiBold,
            7 => FontWeight::Bold,
            8 => FontWeight::ExtraBold,
            9 => FontWeight::Black,
            _ => FontWeight::Normal,
        }
    }

    pub const fn decorations(&self) -> Decoration {
        Decoration(((self.text_flags & text::DECORATION) >> 6) as u8)
    }

    pub const fn decoration_style_of(&self) -> DecorationStyle {
        match (self.text_flags & text::DECORATION_STYLE) >> 9 {
            1 => DecorationStyle::Double,
            2 => DecorationStyle::Dotted,
            3 => DecorationStyle::Dashed,
            4 => DecorationStyle::Wavy,
            _ => DecorationStyle::Solid,
        }
    }

    pub const fn thick(&self) -> u8 {
        let px = ((self.text_flags & text::THICKNESS) >> 13) as u8;
        if px == 0 { 1 } else { px }
    }

    pub const fn is_italic(&self) -> bool {
        self.text_flags & text::ITALIC != 0
    }
}

/// The px a size length resolves to; `Auto` and percentages of an unknown parent fall back to
/// the root size.
const fn size_px(size: Length) -> u8 {
    let px = match size {
        Length::Auto => 0,
        Length::Px(px) => px as i32,
        Length::Dp(dp) => dp as i32 * 5 / 4,
        Length::Percent(pct) => pct as i32 * Metrics::ROOT as i32 / 100,
        Length::Rem(rem) => rem as i32 * Metrics::ROOT as i32 / 100,
    };
    if px < 1 {
        Metrics::ROOT
    } else if px > u8::MAX as i32 {
        u8::MAX
    } else {
        px as u8
    }
}
