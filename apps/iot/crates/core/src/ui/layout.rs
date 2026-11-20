//! The panel's box model.
//!
//! A page declares a tree while it runs; this lays it out once, in 1/64 px, and hands back
//! integer rectangles whose shared edges round to the same pixel. Layout is a pure function of
//! the tree, so a hit test can run the same declaration without a canvas and land on the same
//! boxes.
//!
//! The vocabulary is Flutter's: [`BoxConstraints`], `Flex` with grow/shrink/basis, `Wrap`
//! runs, `Stack` positioning, and a scroll box that clips. Measurement is top-down and each
//! container hands its children the constraints its own box leaves.

use super::typeset;

/// Boxes one declaration may hold. The fullest page (the Attitude debug grid) uses 41, and
/// every node is 160 B of the panel core's stack, so the arena is sized to the page with room
/// to grow rather than to a round number.
pub const MAX_NODES: usize = 48;
pub const NONE: u16 = u16::MAX;

/// Subpixels per pixel. Fractions exist only between a container's edges and its children's;
/// the final rectangles are whole pixels.
pub const UNIT: i32 = 64;
/// A finite stand-in for "no limit", far enough from overflow for a 320 px panel.
pub const INFINITY: i32 = 1 << 24;

pub type Sub = i32;

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

    fn resolve(self, base: Sub) -> Option<Sub> {
        match self {
            Self::Auto => None,
            Self::Px(value) => Some(Sub::from(value) * UNIT),
            Self::Percent(value) => Some(base / 100 * Sub::from(value)),
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

    fn horizontal(self) -> Sub {
        (self.left as Sub + self.right as Sub) * UNIT
    }

    fn vertical(self) -> Sub {
        (self.top as Sub + self.bottom as Sub) * UNIT
    }

    fn left(self) -> Sub {
        self.left as Sub * UNIT
    }

    fn top(self) -> Sub {
        self.top as Sub * UNIT
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
    const NONE: Self = Self {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Display {
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
    pub margin: Edges,
    pub inset: Inset,
    pub gap: i16,
    pub display: Display,
    pub axis: Axis,
    pub justify: Justify,
    pub align: Align,
    pub align_self: Align,
    pub align_content: Justify,
    pub grow: u16,
    pub shrink: u16,
    pub basis: Length,
    pub absolute: bool,
    pub text_align: Align,
    pub nowrap: bool,
    pub max_lines: u16,
    pub offset: i16,
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
            margin: Edges::all(0),
            inset: Inset::NONE,
            gap: 0,
            display: Display::Flex,
            axis: Axis::Column,
            justify: Justify::Start,
            align: Align::Start,
            align_self: Align::Start,
            align_content: Justify::Start,
            grow: 0,
            shrink: 0,
            basis: Length::Auto,
            absolute: false,
            text_align: Align::Start,
            nowrap: false,
            max_lines: typeset::MAX_LINES as u16,
            offset: 0,
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
        self.display = Display::Wrap;
        self
    }

    pub const fn stack(mut self) -> Self {
        self.display = Display::Stack;
        self
    }

    pub const fn scroll(mut self) -> Self {
        self.display = Display::Scroll;
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

    pub const fn text(mut self, align: Align) -> Self {
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

    pub const fn offset(mut self, value: i16) -> Self {
        self.offset = value;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoxConstraints {
    pub min_w: Sub,
    pub max_w: Sub,
    pub min_h: Sub,
    pub max_h: Sub,
}

impl BoxConstraints {
    pub const fn loose(width: Sub, height: Sub) -> Self {
        Self {
            min_w: 0,
            max_w: width,
            min_h: 0,
            max_h: height,
        }
    }

    pub const fn tight(width: Sub, height: Sub) -> Self {
        Self {
            min_w: width,
            max_w: width,
            min_h: height,
            max_h: height,
        }
    }

    fn deflate(self, padding: Edges) -> Self {
        let horizontal = padding.horizontal();
        let vertical = padding.vertical();
        Self {
            min_w: (self.min_w - horizontal).max(0),
            max_w: (self.max_w - horizontal).max(0),
            min_h: (self.min_h - vertical).max(0),
            max_h: (self.max_h - vertical).max(0),
        }
    }

    fn enforce(self, style: &Style) -> Self {
        let mut constraints = self;
        if let Some(width) = style.width.resolve(self.max_w) {
            constraints.min_w = width.max(0);
            constraints.max_w = width.max(0);
        }
        if let Some(height) = style.height.resolve(self.max_h) {
            constraints.min_h = height.max(0);
            constraints.max_h = height.max(0);
        }
        if let Some(value) = style.min_width.resolve(self.max_w) {
            constraints.min_w = constraints.min_w.max(value);
        }
        if let Some(value) = style.min_height.resolve(self.max_h) {
            constraints.min_h = constraints.min_h.max(value);
        }
        if let Some(value) = style.max_width.resolve(self.max_w) {
            constraints.max_w = constraints.max_w.min(value);
        }
        if let Some(value) = style.max_height.resolve(self.max_h) {
            constraints.max_h = constraints.max_h.min(value);
        }
        constraints.min_w = constraints.min_w.clamp(0, constraints.max_w);
        constraints.min_h = constraints.min_h.clamp(0, constraints.max_h);
        constraints
    }

    fn constrain(self, width: Sub, height: Sub) -> Size {
        Size {
            w: width.clamp(self.min_w, self.max_w),
            h: height.clamp(self.min_h, self.max_h),
        }
    }

    fn main(self, horizontal: bool) -> (Sub, Sub) {
        if horizontal {
            (self.min_w, self.max_w)
        } else {
            (self.min_h, self.max_h)
        }
    }

    fn cross(self, horizontal: bool) -> (Sub, Sub) {
        if horizontal {
            (self.min_h, self.max_h)
        } else {
            (self.min_w, self.max_w)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub w: Sub,
    pub h: Sub,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: Sub,
    pub y: Sub,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Box,
    Text { start: u16, len: u16 },
    Custom { slot: u8 },
}

/// The width a text node was measured at. `Default` is a pair no constraint produces, so a
/// freshly declared node never reads as already measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Measured {
    min_w: Sub,
    max_w: Sub,
}

impl Default for Measured {
    fn default() -> Self {
        Self {
            min_w: Sub::MIN,
            max_w: Sub::MIN,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Node {
    style: Style,
    kind: Kind,
    parent: u16,
    first: u16,
    last: u16,
    next: u16,
    offset: Point,
    measured: Size,
    /// The constraints `measured` came from. A text node is asked for its size many times in
    /// one declaration — flex measures, then places, then measures again — and every ask is a
    /// full pass of glyph lookups in flash unless the answer is kept.
    measured_at: Measured,
    alloc: Sub,
    rect: super::Rect,
    clip: super::Rect,
    clipped: bool,
}

impl Default for Node {
    fn default() -> Self {
        Self {
            style: Style::new(),
            kind: Kind::Box,
            parent: NONE,
            first: NONE,
            last: NONE,
            next: NONE,
            offset: Point::default(),
            measured: Size::default(),
            measured_at: Measured::default(),
            alloc: 0,
            rect: super::Rect::default(),
            clip: super::Rect::default(),
            clipped: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeInfo {
    pub kind: Kind,
    pub style: Style,
    pub rect: super::Rect,
    pub clip: super::Rect,
    pub clipped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u16);

impl NodeId {
    pub const NONE: Self = Self(NONE);

    pub fn is_none(self) -> bool {
        self.0 == NONE
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Tree {
    nodes: [Node; MAX_NODES],
    len: u16,
    root: u16,
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

impl Tree {
    pub fn new() -> Self {
        Self {
            nodes: [Node::default(); MAX_NODES],
            len: 0,
            root: NONE,
        }
    }

    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn root(&self) -> NodeId {
        NodeId(self.root)
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        let parent = self.nodes.get(usize::from(id.0))?.parent;
        (parent != NONE).then_some(NodeId(parent))
    }

    pub fn push(&mut self, style: Style, kind: Kind, parent: NodeId) -> NodeId {
        if usize::from(self.len) >= MAX_NODES {
            debug_assert!(false, "the page ran out of nodes");
            return NodeId::NONE;
        }
        let id = self.len;
        self.len += 1;
        self.nodes[usize::from(id)] = Node {
            style,
            kind,
            parent: parent.0,
            ..Node::default()
        };
        if parent.is_none() {
            if self.root == NONE {
                self.root = id;
            }
        } else {
            let parent_index = usize::from(parent.0);
            let last = self.nodes[parent_index].last;
            if last == NONE {
                self.nodes[parent_index].first = id;
            } else {
                self.nodes[usize::from(last)].next = id;
            }
            self.nodes[parent_index].last = id;
        }
        NodeId(id)
    }

    pub fn rect(&self, id: NodeId) -> super::Rect {
        self.nodes
            .get(usize::from(id.0))
            .map_or_else(super::Rect::default, |node| node.rect)
    }

    pub fn info(&self, id: NodeId) -> NodeInfo {
        let node = &self.nodes[usize::from(id.0)];
        NodeInfo {
            kind: node.kind,
            style: node.style,
            rect: node.rect,
            clip: node.clip,
            clipped: node.clipped,
        }
    }

    pub fn tag_of(&self, id: NodeId) -> u16 {
        self.nodes
            .get(usize::from(id.0))
            .map_or(0, |node| node.style.tag)
    }

    /// The last tagged node under `(x, y)`: later declarations are on top.
    pub fn tag_at(&self, x: i32, y: i32) -> Option<u16> {
        let mut found = None;
        for id in 0..self.len {
            let node = &self.nodes[usize::from(id)];
            if node.style.tag != 0 && node.rect.contains(x, y) {
                found = Some(node.style.tag);
            }
        }
        found
    }

    /// Lays the tree out into `constraints` and fixes every node's pixel rectangle.
    pub fn finish(&mut self, constraints: BoxConstraints, text: &[u8]) {
        if self.root == NONE {
            return;
        }
        self.layout_node(NodeId(self.root), constraints, text);
        let root = NodeId(self.root);
        let size = self.nodes[usize::from(root.0)].measured;
        self.nodes[usize::from(root.0)].rect = super::Rect {
            x: 0,
            y: 0,
            w: pixels(size.w),
            h: pixels(size.h),
        };
        self.absolute(root, Point::default());
    }

    fn layout_node(&mut self, id: NodeId, constraints: BoxConstraints, text: &[u8]) -> Size {
        let node = self.nodes[usize::from(id.0)];
        let constraints = constraints.enforce(&node.style);
        let at = Measured {
            min_w: constraints.min_w,
            max_w: constraints.max_w,
        };
        if self.nodes[usize::from(id.0)].measured_at == at {
            return self.nodes[usize::from(id.0)].measured;
        }
        let size = match node.kind {
            Kind::Text { start, len } => self.layout_text(&node, start, len, constraints, text),
            Kind::Box | Kind::Custom { .. } => self.layout_box(id, &node, constraints, text),
        };
        let size = constraints.constrain(size.w, size.h);
        let slot = &mut self.nodes[usize::from(id.0)];
        slot.measured = size;
        slot.measured_at = at;
        size
    }

    fn layout_text(
        &self,
        node: &Node,
        start: u16,
        len: u16,
        constraints: BoxConstraints,
        text: &[u8],
    ) -> Size {
        let run = &text[usize::from(start)..usize::from(start) + usize::from(len)];
        let line_height = typeset::line_height();
        let available = pixels(constraints.max_w).max(1);
        let natural = typeset::max_content(run).min(available as usize);
        let width = if constraints.min_w == constraints.max_w {
            available as usize
        } else {
            natural
        };
        let paragraph = typeset::wrap(
            run,
            width,
            usize::from(node.style.max_lines).min(typeset::MAX_LINES),
        );
        Size {
            w: width as Sub * UNIT,
            h: (paragraph.count * line_height) as Sub * UNIT,
        }
    }

    fn layout_box(
        &mut self,
        id: NodeId,
        node: &Node,
        constraints: BoxConstraints,
        text: &[u8],
    ) -> Size {
        let inner = constraints.deflate(node.style.padding);
        let size = match node.style.display {
            Display::Flex => self.layout_flex(id, node, inner, text, false),
            Display::Wrap => self.layout_flex(id, node, inner, text, true),
            Display::Stack => self.layout_stack(id, node, constraints, inner, text),
            Display::Scroll => self.layout_scroll(id, node, constraints, inner, text),
        };
        Size {
            w: size.w + node.style.padding.horizontal(),
            h: size.h + node.style.padding.vertical(),
        }
    }

    fn layout_flex(
        &mut self,
        id: NodeId,
        node: &Node,
        inner: BoxConstraints,
        text: &[u8],
        wraps: bool,
    ) -> Size {
        let _ = &text;
        let horizontal = node.style.axis == Axis::Row;
        let (main_min, main_max) = inner.main(horizontal);
        let (cross_min, cross_max) = inner.cross(horizontal);
        let gap = node.style.gap as Sub * UNIT;

        let mut count = 0usize;
        let mut grow = 0u32;
        let mut shrink = 0u32;
        let mut base_total = 0i32;
        let mut cross_natural = 0i32;
        let mut child = self.nodes[usize::from(id.0)].first;
        while child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let child_node = self.nodes[usize::from(child)];
            if !child_node.style.absolute {
                let loose = if horizontal {
                    BoxConstraints {
                        min_w: 0,
                        max_w: INFINITY,
                        min_h: 0,
                        max_h: cross_max.max(0),
                    }
                } else {
                    BoxConstraints {
                        min_w: 0,
                        max_w: cross_max.max(0),
                        min_h: 0,
                        max_h: INFINITY,
                    }
                };
                let measured = self.layout_node(NodeId(child), loose, text);
                let (main_size, cross_size) = if horizontal {
                    (measured.w, measured.h)
                } else {
                    (measured.h, measured.w)
                };
                self.nodes[usize::from(child)].alloc = main_size;
                base_total += main_size;
                cross_natural = cross_natural.max(cross_size);
                grow += u32::from(child_node.style.grow);
                shrink += u32::from(child_node.style.shrink);
                count += 1;
            }
            child = next;
        }
        if count == 0 {
            return Size::default();
        }
        let gaps = gap * (count as Sub - 1);
        let definite = length_of(
            &node.style,
            !horizontal,
            if horizontal { inner.max_w } else { inner.max_h },
        );
        let main = definite.unwrap_or_else(|| {
            if main_min == main_max && main_max < INFINITY {
                main_max
            } else {
                base_total + gaps
            }
        });
        let free = main - base_total - gaps;

        if !wraps {
            if free > 0 && grow > 0 {
                distribute(&mut self.nodes, id, free, true);
            } else if free < 0 && (shrink > 0 || grow > 0) {
                distribute(&mut self.nodes, id, -free, false);
            }
        }

        if wraps {
            return self.place_wrap(id, node, inner, text, horizontal, main);
        }

        // Lay each child out at its allotted main size and learn the cross extent.
        let mut cross = 0i32;
        let mut used = 0i32;
        let mut child = self.nodes[usize::from(id.0)].first;
        while child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let child_node = self.nodes[usize::from(child)];
            if !child_node.style.absolute {
                let main_size = self.nodes[usize::from(child)].alloc;
                let constraints = if horizontal {
                    BoxConstraints {
                        min_w: main_size,
                        max_w: main_size,
                        min_h: 0,
                        max_h: cross_max.max(0),
                    }
                } else {
                    BoxConstraints {
                        min_w: 0,
                        max_w: cross_max.max(0),
                        min_h: main_size,
                        max_h: main_size,
                    }
                };
                let measured = self.layout_node(NodeId(child), constraints, text);
                let cross_size = if horizontal { measured.h } else { measured.w };
                cross = cross.max(cross_size);
                used += main_size + gap;
            }
            child = next;
        }
        used = used.saturating_sub(gap);
        let definite_cross = length_of(
            &node.style,
            horizontal,
            if horizontal { inner.max_h } else { inner.max_w },
        );
        let cross = definite_cross.unwrap_or(cross).max(cross_min);
        if node.style.align == Align::Stretch {
            self.stretch_cross(id, horizontal, cross, text);
        }

        let leftover = if grow > 0 { 0 } else { main - used };
        let (mut lead, spacing) = justify(node.style.justify, leftover, count);
        if lead < 0 {
            lead = 0;
        }
        let mut child = self.nodes[usize::from(id.0)].first;
        while child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let child_node = self.nodes[usize::from(child)];
            if !child_node.style.absolute {
                let size = self.nodes[usize::from(child)].measured;
                let cross_size = if horizontal { size.h } else { size.w };
                let cross_offset = align_offset(
                    node.style.align,
                    child_node.style.align_self,
                    cross_size,
                    cross,
                );
                if horizontal {
                    self.nodes[usize::from(child)].offset = Point {
                        x: lead,
                        y: cross_offset,
                    };
                } else {
                    self.nodes[usize::from(child)].offset = Point {
                        x: cross_offset,
                        y: lead,
                    };
                }
                lead += (if horizontal { size.w } else { size.h }) + gap + spacing;
            }
            child = next;
        }
        if horizontal {
            Size { w: main, h: cross }
        } else {
            Size { w: cross, h: main }
        }
    }

    fn stretch_cross(&mut self, id: NodeId, horizontal: bool, cross: Sub, text: &[u8]) {
        let mut child = self.nodes[usize::from(id.0)].first;
        while child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let node = self.nodes[usize::from(child)];
            if !node.style.absolute {
                let main_size = node.alloc;
                let constraints = if horizontal {
                    BoxConstraints {
                        min_w: main_size,
                        max_w: main_size,
                        min_h: cross,
                        max_h: cross,
                    }
                } else {
                    BoxConstraints {
                        min_w: cross,
                        max_w: cross,
                        min_h: main_size,
                        max_h: main_size,
                    }
                };
                self.layout_node(NodeId(child), constraints, text);
            }
            child = next;
        }
    }

    fn place_wrap(
        &mut self,
        id: NodeId,
        node: &Node,
        inner: BoxConstraints,
        _text: &[u8],
        horizontal: bool,
        main: Sub,
    ) -> Size {
        let gap = node.style.gap as Sub * UNIT;
        let (cross_min, cross_max) = inner.cross(horizontal);
        let mut child = self.nodes[usize::from(id.0)].first;
        let mut run_main = 0i32;
        let mut run_cross = 0i32;
        let mut content_cross = 0i32;
        let mut runs = 0usize;
        while child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let child_node = self.nodes[usize::from(child)];
            if !child_node.style.absolute {
                let measured = self.nodes[usize::from(child)].measured;
                let (main_size, cross_size) = if horizontal {
                    (measured.w, measured.h)
                } else {
                    (measured.h, measured.w)
                };
                if run_main > 0 && run_main + gap + main_size > main {
                    content_cross += run_cross + gap;
                    run_main = 0;
                    run_cross = 0;
                    runs += 1;
                }
                self.nodes[usize::from(child)].alloc = main_size;
                self.nodes[usize::from(child)].offset = Point { x: run_main, y: 0 };
                run_main += main_size + gap;
                run_cross = run_cross.max(cross_size);
            }
            child = next;
        }
        content_cross += run_cross;
        let runs = runs + 1;
        let _ = cross_max;
        let definite_cross = length_of(
            &node.style,
            horizontal,
            if horizontal { inner.max_h } else { inner.max_w },
        );
        let cross = definite_cross.unwrap_or(content_cross).max(cross_min);
        let (lead, spacing) = justify(node.style.align_content, cross - content_cross, runs);
        // A second walk assigns run and item offsets now that run heights and the leading
        // space are known.
        let mut child = self.nodes[usize::from(id.0)].first;
        let mut run_main = 0i32;
        let mut run_cross = 0i32;
        let mut y = lead.max(0);
        let mut x = 0i32;
        while child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let child_node = self.nodes[usize::from(child)];
            if !child_node.style.absolute {
                let size = self.nodes[usize::from(child)].measured;
                let (main_size, cross_size) = if horizontal {
                    (size.w, size.h)
                } else {
                    (size.h, size.w)
                };
                if run_main > 0 && run_main + gap + main_size > main {
                    y += run_cross + gap + spacing;
                    run_main = 0;
                    run_cross = 0;
                    x = 0;
                }
                let inner_cross = align_offset(
                    node.style.align,
                    child_node.style.align_self,
                    cross_size,
                    run_cross,
                );
                let offset = if horizontal {
                    Point {
                        x: x + run_main,
                        y: y + inner_cross,
                    }
                } else {
                    Point {
                        x: y + inner_cross,
                        y: x + run_main,
                    }
                };
                self.nodes[usize::from(child)].offset = offset;
                run_main += main_size + gap;
                run_cross = run_cross.max(cross_size);
            }
            child = next;
        }
        let _ = (run_cross, x);
        if horizontal {
            Size { w: main, h: cross }
        } else {
            Size { w: cross, h: main }
        }
    }

    fn layout_stack(
        &mut self,
        id: NodeId,
        node: &Node,
        constraints: BoxConstraints,
        inner: BoxConstraints,
        text: &[u8],
    ) -> Size {
        let definite_w = length_of(&node.style, false, constraints.max_w);
        let definite_h = length_of(&node.style, true, constraints.max_h);
        let mut width = 0;
        let mut height = 0;
        let mut child = self.nodes[usize::from(id.0)].first;
        while child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let child_node = self.nodes[usize::from(child)];
            if !child_node.style.absolute {
                let size = self.layout_node(
                    NodeId(child),
                    BoxConstraints {
                        min_w: 0,
                        max_w: inner.max_w.max(0),
                        min_h: 0,
                        max_h: inner.max_h.max(0),
                    },
                    text,
                );
                width = width.max(size.w);
                height = height.max(size.h);
            }
            child = next;
        }
        let width = definite_w.unwrap_or_else(|| tight(inner.min_w, inner.max_w).unwrap_or(width));
        let height =
            definite_h.unwrap_or_else(|| tight(inner.min_h, inner.max_h).unwrap_or(height));
        let mut child = self.nodes[usize::from(id.0)].first;
        while child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let child_node = self.nodes[usize::from(child)];
            if child_node.style.absolute {
                let size =
                    self.layout_node(NodeId(child), BoxConstraints::loose(width, height), text);
                let (x, y) = absolute_offset(&child_node.style, size, width, height);
                self.nodes[usize::from(child)].offset = Point { x, y };
            } else {
                let size = self.nodes[usize::from(child)].measured;
                self.nodes[usize::from(child)].offset = Point {
                    x: align_offset(node.style.align, child_node.style.align_self, size.w, width),
                    y: align_offset(
                        node.style.align,
                        child_node.style.align_self,
                        size.h,
                        height,
                    ),
                };
            }
            child = next;
        }
        Size {
            w: width,
            h: height,
        }
    }

    fn layout_scroll(
        &mut self,
        id: NodeId,
        node: &Node,
        constraints: BoxConstraints,
        inner: BoxConstraints,
        text: &[u8],
    ) -> Size {
        let horizontal = node.style.axis == Axis::Row;
        let unbounded = if horizontal {
            BoxConstraints {
                min_w: 0,
                max_w: INFINITY,
                min_h: 0,
                max_h: inner.max_h.max(0),
            }
        } else {
            BoxConstraints {
                min_w: 0,
                max_w: inner.max_w.max(0),
                min_h: 0,
                max_h: INFINITY,
            }
        };
        let mut content = 0;
        let mut cross = 0;
        let mut child = self.nodes[usize::from(id.0)].first;
        while child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let child_node = self.nodes[usize::from(child)];
            if !child_node.style.absolute {
                let size = self.layout_node(NodeId(child), unbounded, text);
                if horizontal {
                    content += size.w;
                    cross = cross.max(size.h);
                } else {
                    content += size.h;
                    cross = cross.max(size.w);
                }
            }
            child = next;
        }
        let width =
            length_of(&node.style, false, constraints.max_w).unwrap_or(if inner.max_w < INFINITY {
                inner.max_w
            } else {
                cross
            });
        let height =
            length_of(&node.style, true, constraints.max_h).unwrap_or(if inner.max_h < INFINITY {
                inner.max_h
            } else {
                content
            });
        let viewport = if horizontal { width } else { height };
        let shift = -(i32::from(node.style.offset) * UNIT).min((content - viewport).max(0));
        let mut child = self.nodes[usize::from(id.0)].first;
        while child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let child_node = self.nodes[usize::from(child)];
            if !child_node.style.absolute {
                self.nodes[usize::from(child)].offset = if horizontal {
                    Point { x: shift, y: 0 }
                } else {
                    Point { x: 0, y: shift }
                };
            }
            child = next;
        }
        let _ = cross;
        Size {
            w: width,
            h: height,
        }
    }

    fn absolute(&mut self, id: NodeId, origin: Point) {
        let node = self.nodes[usize::from(id.0)];
        let scroll = node.style.display == Display::Scroll;
        let clip = if scroll { node.rect } else { node.clip };
        let clipped = node.clipped || scroll;
        let content = Point {
            x: origin.x + node.style.padding.left(),
            y: origin.y + node.style.padding.top(),
        };
        let mut child = node.first;
        while child != NONE {
            let child_node = self.nodes[usize::from(child)];
            let position = Point {
                x: content.x + child_node.offset.x,
                y: content.y + child_node.offset.y,
            };
            let size = child_node.measured;
            let rect = super::Rect {
                x: pixels(position.x),
                y: pixels(position.y),
                w: pixels(position.x + size.w) - pixels(position.x),
                h: pixels(position.y + size.h) - pixels(position.y),
            };
            self.nodes[usize::from(child)].rect = rect;
            self.nodes[usize::from(child)].clip = clip;
            self.nodes[usize::from(child)].clipped = clipped;
            self.absolute(NodeId(child), position);
            child = child_node.next;
        }
    }
}

fn distribute(nodes: &mut [Node; MAX_NODES], parent: NodeId, amount: Sub, grow: bool) {
    let weight = |node: &Node| {
        if grow {
            u32::from(node.style.grow)
        } else if node.style.shrink > 0 {
            u32::from(node.style.shrink)
        } else {
            u32::from(node.style.grow > 0)
        }
    };
    let mut total = 0u32;
    let mut child = nodes[usize::from(parent.0)].first;
    while child != NONE {
        total += weight(&nodes[usize::from(child)]);
        child = nodes[usize::from(child)].next;
    }
    if total == 0 {
        return;
    }
    let mut left = amount;
    let mut child = nodes[usize::from(parent.0)].first;
    while child != NONE {
        let node = &mut nodes[usize::from(child)];
        let weight = weight(node);
        if weight > 0 {
            let share = amount * weight as Sub / total as Sub;
            if grow {
                node.alloc += share;
                left -= share;
            } else {
                let taken = share.min(node.alloc);
                node.alloc -= taken;
                left -= taken;
            }
        }
        child = node.next;
    }
    // Whole subpixels left after the division go to the children in declaration order, which
    // keeps the sum exact and the rounding at the end collision-free.
    let mut child = nodes[usize::from(parent.0)].first;
    while left > 0 && child != NONE {
        let node = &mut nodes[usize::from(child)];
        if node.style.grow > 0 || node.style.shrink > 0 {
            if grow {
                node.alloc += 1;
                left -= 1;
            } else if node.alloc > 0 {
                node.alloc -= 1;
                left -= 1;
            }
        }
        child = node.next;
    }
}

/// The one value a tight axis allows, so a child absolutely positioned against the edge can
/// find that edge before the parent's own size is clamped.
fn tight(min: Sub, max: Sub) -> Option<Sub> {
    (min == max && max < INFINITY).then_some(max)
}

fn length_of(style: &Style, vertical: bool, base: Sub) -> Option<Sub> {
    if vertical {
        style.height.resolve(base)
    } else {
        style.width.resolve(base)
    }
}

fn align_offset(align: Align, own: Align, size: Sub, space: Sub) -> Sub {
    let align = if own == Align::Start { align } else { own };
    match align {
        Align::Start | Align::Stretch => 0,
        Align::Center => (space - size).max(0) / 2,
        Align::End => (space - size).max(0),
    }
}

fn justify(justify: Justify, leftover: Sub, count: usize) -> (Sub, Sub) {
    let leftover = leftover.max(0);
    match justify {
        Justify::Start => (0, 0),
        Justify::Center => (leftover / 2, 0),
        Justify::End => (leftover, 0),
        Justify::SpaceBetween => (
            0,
            if count > 1 {
                leftover / (count as Sub - 1)
            } else {
                0
            },
        ),
        Justify::SpaceAround => {
            let each = leftover / count as Sub;
            (each / 2, each)
        }
        Justify::SpaceEvenly => {
            let each = leftover / (count as Sub + 1);
            (each, each)
        }
    }
}

fn absolute_offset(style: &Style, size: Size, width: Sub, height: Sub) -> (Sub, Sub) {
    let horizontal = match (style.inset.left, style.inset.right) {
        (Some(left), _) => left as Sub * UNIT,
        (None, Some(right)) => width - size.w - right as Sub * UNIT,
        (None, None) => 0,
    };
    let vertical = match (style.inset.top, style.inset.bottom) {
        (Some(top), _) => top as Sub * UNIT,
        (None, Some(bottom)) => height - size.h - bottom as Sub * UNIT,
        (None, None) => 0,
    };
    (horizontal, vertical)
}

fn pixels(value: Sub) -> i32 {
    (value + UNIT / 2).div_euclid(UNIT)
}

#[cfg(test)]
mod tests;
