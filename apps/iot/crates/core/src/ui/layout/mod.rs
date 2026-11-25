//! The panel's box model in Flutter's vocabulary: constraints, flex, wrap, stack, scroll.
//! A page declares a tree; layout hands back integer rectangles whose shared edges round to
//! the same pixel, and a hit test runs the same declaration without a canvas.

mod constraints;
mod node;
mod scroll;
mod style;

pub use self::constraints::{BoxConstraints, PX};
pub use self::node::MAX_NODES;
pub use self::node::NodeId;
pub(crate) use self::node::{NodeInfo, NodeKind, Span};
pub use self::scroll::{ScrollId, ScrollInfo};
pub use self::style::{
    Align, Axis, Decoration, DecorationStyle, Flow, FontWeight, Inset, Justify, Length, Overflow,
    RunStyle, Style, TextAlign,
};

use self::constraints::{INFINITY, Point, Size, Subpx, pixels, tight};
use self::node::{Measured, NONE, Node};

use super::typeset;
use crate::ui::Rect;
use crate::ui::color::{Color, ColorId, Palette};

/// The nodes of one declaration, linked in the order they were pushed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tree {
    nodes: [Node; MAX_NODES],
    len: u16,
    root: u16,
    scrolls: [ScrollSlot; MAX_SCROLL_AREAS],
    palette: Palette,
}

/// The named scroll boxes one declaration may report. The text page uses two.
pub const MAX_SCROLL_AREAS: usize = 4;

/// One named scroll box's last layout: the node it belongs to, and what it reported then.
#[derive(Clone, Copy, Debug)]
struct ScrollSlot {
    id: ScrollId,
    node: u16,
    offset: i16,
    content: i16,
    viewport: i16,
}

impl Default for ScrollSlot {
    fn default() -> Self {
        Self {
            id: ScrollId::NONE,
            node: NONE,
            offset: 0,
            content: 0,
            viewport: 0,
        }
    }
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

impl Tree {
    /// Builds a fresh tree straight into `this`, which must be uninitialised: returning one by
    /// value would cost a ten-kilobyte temporary on the paint path.
    ///
    /// # Safety
    ///
    /// `this` must be valid for writes of a whole [`Tree`] and must not be read until the
    /// caller treats it as initialised.
    pub(crate) unsafe fn init_in_place(this: *mut Self) {
        // SAFETY: the caller guarantees room for every field; each is written once, and none
        // of these values borrows anything.
        unsafe {
            core::ptr::addr_of_mut!((*this).nodes).write([Node::default(); MAX_NODES]);
            core::ptr::addr_of_mut!((*this).len).write(0);
            core::ptr::addr_of_mut!((*this).root).write(NONE);
            core::ptr::addr_of_mut!((*this).scrolls)
                .write([ScrollSlot::default(); MAX_SCROLL_AREAS]);
            core::ptr::addr_of_mut!((*this).palette).write(Palette::default());
        }
    }

    pub(crate) fn new() -> Self {
        Self {
            nodes: [Node::default(); MAX_NODES],
            len: 0,
            root: NONE,
            scrolls: [ScrollSlot::default(); MAX_SCROLL_AREAS],
            palette: Palette::default(),
        }
    }

    pub(crate) fn intern(&mut self, color: Color) -> ColorId {
        self.palette.intern(color)
    }

    pub(crate) fn palette(&self) -> Palette {
        self.palette
    }

    pub(crate) fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub(crate) fn parent(&self, id: NodeId) -> Option<NodeId> {
        let parent = self.nodes.get(usize::from(id.0))?.parent;
        (parent != NONE).then_some(NodeId(parent))
    }

    pub(crate) fn push(&mut self, style: Style, kind: NodeKind, parent: NodeId) -> NodeId {
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

    pub(crate) fn rect(&self, id: NodeId) -> Rect {
        self.nodes
            .get(usize::from(id.0))
            .map_or_else(Rect::default, |node| node.rect)
    }

    /// Where the run of text node `id` begins in the declaration's buffer.
    pub(crate) fn text_start(&self, id: NodeId) -> Option<u16> {
        match self.info(id).kind {
            NodeKind::Text { start, .. } => Some(start),
            _ => None,
        }
    }

    pub(crate) fn info(&self, id: NodeId) -> NodeInfo {
        let node = &self.nodes[usize::from(id.0)];
        NodeInfo {
            kind: node.kind,
            style: node.style,
            rect: node.rect,
            clip: node.clip,
            clipped: node.clipped,
        }
    }

    pub(crate) fn rects(&self) -> impl Iterator<Item = Rect> + '_ {
        self.nodes[..usize::from(self.len)]
            .iter()
            .map(|node| node.rect)
    }

    pub(crate) fn text_rects(&self) -> impl Iterator<Item = Rect> + '_ {
        self.nodes[..usize::from(self.len)]
            .iter()
            .filter(|node| matches!(node.kind, NodeKind::Text { .. }))
            .map(|node| node.rect)
    }

    /// Every tagged node's tag and rectangle, in declaration order.
    pub(crate) fn tags(&self) -> impl Iterator<Item = (u16, Rect)> + '_ {
        self.nodes[..usize::from(self.len)]
            .iter()
            .filter(|node| node.style.tag != 0)
            .map(|node| (node.style.tag, node.rect))
    }

    pub(crate) fn paint_rect(&self, slot: u8) -> Option<Rect> {
        self.nodes[..usize::from(self.len)]
            .iter()
            .find(|node| node.kind == NodeKind::Paint { slot })
            .map(|node| node.rect)
    }

    /// The last layout of the scroll box named `area`, if the declaration made one.
    pub(crate) fn scroll_info(&self, area: ScrollId) -> Option<ScrollInfo> {
        if area.is_none() {
            return None;
        }
        self.scrolls
            .iter()
            .find(|slot| slot.id == area && slot.node != NONE)
            .map(|slot| ScrollInfo {
                id: slot.id,
                rect: self.rect(NodeId(slot.node)),
                offset: slot.offset,
                content: slot.content,
                viewport: slot.viewport,
            })
    }

    /// Keeps what one named scroll box reported. A box measured twice in one layout — loose
    /// by its parent, then at its allotted size — is recorded by its placement, so the slot is
    /// found by node rather than appended.
    fn record_scroll(&mut self, id: NodeId, area: ScrollId, content: Subpx, viewport: Subpx) {
        if area.is_none() {
            return;
        }
        let node = self.nodes[usize::from(id.0)];
        let slot = ScrollSlot {
            id: area,
            node: id.0,
            offset: node.style.scroll_offset,
            content: pixels(content) as i16,
            viewport: pixels(viewport) as i16,
        };
        if let Some(existing) = self.scrolls.iter_mut().find(|slot| slot.node == id.0) {
            *existing = slot;
            return;
        }
        if let Some(free) = self.scrolls.iter_mut().find(|slot| slot.node == NONE) {
            *free = slot;
            return;
        }
        debug_assert!(false, "the page ran out of scroll areas");
    }

    /// The last tagged node under `(x, y)`: later declarations are on top.
    pub(crate) fn tag_at(&self, x: i32, y: i32) -> Option<u16> {
        self.nodes[..usize::from(self.len)]
            .iter()
            .filter(|node| node.style.tag != 0 && node.rect.contains(x, y))
            .map(|node| node.style.tag)
            .next_back()
    }

    /// Lays the tree out into `constraints` and fixes every node's pixel rectangle.
    pub(crate) fn layout(&mut self, constraints: BoxConstraints, text: &[u8]) {
        if self.root == NONE {
            return;
        }
        self.layout_node(NodeId(self.root), constraints, text);
        let root = NodeId(self.root);
        let size = self.nodes[usize::from(root.0)].measured;
        self.nodes[usize::from(root.0)].rect = Rect {
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
        let run = matches!(node.kind, NodeKind::Text { .. });
        let at = if run {
            Measured::of_text(constraints, 0)
        } else {
            Measured::of(constraints)
        };
        let cached = self.nodes[usize::from(id.0)].measured_at;
        if run && cached.answers_text(at) {
            let h = cached.natural().clamp(constraints.min_h, constraints.max_h);
            let size = Size {
                w: self.nodes[usize::from(id.0)].measured.w,
                h,
            };
            self.nodes[usize::from(id.0)].measured = size;
            return size;
        }
        if !run && cached == at {
            return self.nodes[usize::from(id.0)].measured;
        }
        let size = match node.kind {
            NodeKind::Text { start, len } => {
                let inner = constraints.deflate(node.style.padding);
                let measured = self.layout_text(&node, start, len, inner, text);
                Size {
                    w: measured.w + node.style.padding.horizontal(),
                    h: measured.h + node.style.padding.vertical(),
                }
            }
            NodeKind::Box | NodeKind::Paint { .. } => self.layout_box(id, &node, constraints, text),
        };
        let slot = &mut self.nodes[usize::from(id.0)];
        slot.measured = constraints.constrain(size.w, size.h);
        slot.measured_at = if run {
            Measured::of_text(constraints, size.h)
        } else {
            at
        };
        slot.measured
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
        let metrics = node.style.metrics();
        let line_height = typeset::line_height(metrics);
        if node.style.nowrap {
            // A definite width is the whole answer for a run that cannot wrap; measuring it
            // would walk the face once per glyph for a width the caller already fixed.
            let width = if constraints.min_w == constraints.max_w && constraints.max_w < INFINITY {
                constraints.max_w
            } else {
                typeset::max_content(run, metrics) as Subpx * PX
            };
            return Size {
                w: width,
                h: line_height as Subpx * PX,
            };
        }
        let natural = typeset::max_content(run, metrics);
        let available = pixels(constraints.max_w).max(1) as usize;
        let width = if constraints.min_w == constraints.max_w {
            available
        } else {
            natural.min(available)
        };
        let paragraph = typeset::wrap(
            run,
            width,
            usize::from(node.style.max_lines).min(typeset::MAX_LINES),
            metrics,
        );
        Size {
            w: width as Subpx * PX,
            h: (paragraph.count * line_height) as Subpx * PX,
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
        let size = match node.style.flow {
            Flow::Flex => self.layout_flex(id, node, inner, text, false),
            Flow::Wrap => self.layout_flex(id, node, inner, text, true),
            Flow::Stack => self.layout_stack(id, node, constraints, inner, text),
            Flow::Scroll => self.layout_scroll(id, node, constraints, inner, text),
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
        let horizontal = node.style.axis == Axis::Row;
        let (main_min, main_max) = inner.main(horizontal);
        let (cross_min, cross_max) = inner.cross(horizontal);
        let gap = node.style.gap as Subpx * PX;
        let basis_base = if horizontal { inner.max_w } else { inner.max_h };

        let mut count = 0usize;
        let mut grow = 0u32;
        let mut shrink = 0u32;
        let mut base_total = 0i32;
        let mut child = self.nodes[usize::from(id.0)].first;
        while child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let child_node = self.nodes[usize::from(child)];
            if !child_node.style.is_absolute() {
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
                // A child with an explicit basis discards whatever its natural main size would
                // have been, so measuring it here is a full text pass thrown away — and a scroll
                // box with `basis(0)` would be measured at the unbounded width the loose
                // constraints hand it. The placement pass below measures it once, at the size it
                // was actually given. A wrapping flex still needs every natural size to break
                // its runs.
                let explicit = if wraps {
                    None
                } else {
                    child_node.style.basis.resolve(basis_base)
                };
                let main_size = match explicit {
                    Some(basis) => basis,
                    None => {
                        let measured = self.layout_node(NodeId(child), loose, text);
                        if horizontal { measured.w } else { measured.h }
                    }
                };
                self.nodes[usize::from(child)].alloc = main_size;
                base_total += main_size;
                grow += u32::from(child_node.style.grow);
                shrink += u32::from(child_node.style.shrink);
                count += 1;
            }
            child = next;
        }
        if count == 0 {
            return Size::default();
        }
        let gaps = gap * (count as Subpx - 1);
        let definite = content_length_of(
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
            if !child_node.style.is_absolute() {
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
        let definite_cross = content_length_of(
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
            if !child_node.style.is_absolute() {
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
        let (w, h) = if horizontal {
            (main, cross)
        } else {
            (cross, main)
        };
        self.place_absolute(id, w, h, Point::default(), text);
        Size { w, h }
    }

    fn stretch_cross(&mut self, id: NodeId, horizontal: bool, cross: Subpx, text: &[u8]) {
        let mut child = self.nodes[usize::from(id.0)].first;
        while child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let node = self.nodes[usize::from(child)];
            if !node.style.is_absolute() {
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
        text: &[u8],
        horizontal: bool,
        main: Subpx,
    ) -> Size {
        let gap = node.style.gap as Subpx * PX;
        let (cross_min, _) = inner.cross(horizontal);
        let mut content_cross = 0i32;
        let mut runs = 0usize;
        let mut child = self.nodes[usize::from(id.0)].first;
        let mut run_main = 0i32;
        let mut run_cross = 0i32;
        while child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let child_node = self.nodes[usize::from(child)];
            if !child_node.style.is_absolute() {
                let measured = self.nodes[usize::from(child)].measured;
                let (main_size, cross_size) = if horizontal {
                    (measured.w, measured.h)
                } else {
                    (measured.h, measured.w)
                };
                if run_main > 0 && run_main + gap + main_size > main {
                    content_cross += run_cross + gap;
                    runs += 1;
                    run_main = 0;
                    run_cross = 0;
                }
                run_main += main_size + gap;
                run_cross = run_cross.max(cross_size);
            }
            child = next;
        }
        content_cross += run_cross;
        let runs = runs + 1;
        let definite_cross = content_length_of(
            &node.style,
            horizontal,
            if horizontal { inner.max_h } else { inner.max_w },
        );
        let cross = definite_cross.unwrap_or(content_cross).max(cross_min);
        let (lead, spacing) = justify(node.style.align_content, cross - content_cross, runs);
        let mut y = lead.max(0);
        let mut cursor = self.nodes[usize::from(id.0)].first;
        while cursor != NONE {
            // One run at a time: find where it ends and how wide it is across the axis.
            let mut end = NONE;
            let mut run_cross = 0i32;
            let mut probe = 0i32;
            let mut child = cursor;
            while child != NONE {
                let next = self.nodes[usize::from(child)].next;
                let child_node = self.nodes[usize::from(child)];
                if !child_node.style.is_absolute() {
                    let measured = self.nodes[usize::from(child)].measured;
                    let (main_size, cross_size) = if horizontal {
                        (measured.w, measured.h)
                    } else {
                        (measured.h, measured.w)
                    };
                    if probe > 0 && probe + gap + main_size > main {
                        end = child;
                        break;
                    }
                    probe += main_size + gap;
                    run_cross = run_cross.max(cross_size);
                }
                child = next;
            }
            let mut x = 0i32;
            let mut child = cursor;
            while child != end {
                let next = self.nodes[usize::from(child)].next;
                let child_node = self.nodes[usize::from(child)];
                if !child_node.style.is_absolute() {
                    let measured = self.nodes[usize::from(child)].measured;
                    let (main_size, cross_size) = if horizontal {
                        (measured.w, measured.h)
                    } else {
                        (measured.h, measured.w)
                    };
                    let cross_size = if node.style.align == Align::Stretch {
                        let constraints = if horizontal {
                            BoxConstraints {
                                min_w: main_size,
                                max_w: main_size,
                                min_h: run_cross,
                                max_h: run_cross,
                            }
                        } else {
                            BoxConstraints {
                                min_w: run_cross,
                                max_w: run_cross,
                                min_h: main_size,
                                max_h: main_size,
                            }
                        };
                        let stretched = self.layout_node(NodeId(child), constraints, text);
                        if horizontal { stretched.h } else { stretched.w }
                    } else {
                        cross_size
                    };
                    let inner_cross = align_offset(
                        node.style.align,
                        child_node.style.align_self,
                        cross_size,
                        run_cross,
                    );
                    let offset = if horizontal {
                        Point {
                            x,
                            y: y + inner_cross,
                        }
                    } else {
                        Point {
                            x: y + inner_cross,
                            y: x,
                        }
                    };
                    self.nodes[usize::from(child)].offset = offset;
                    x += main_size + gap;
                }
                child = next;
            }
            cursor = end;
            y += run_cross + gap + spacing;
        }
        let (w, h) = if horizontal {
            (main, cross)
        } else {
            (cross, main)
        };
        self.place_absolute(id, w, h, Point::default(), text);
        Size { w, h }
    }

    /// Lays out a box's out-of-flow children once its content size is known; unplaced, their
    /// rectangles stay empty.
    fn place_absolute(
        &mut self,
        id: NodeId,
        width: Subpx,
        height: Subpx,
        drift: Point,
        text: &[u8],
    ) {
        let mut child = self.nodes[usize::from(id.0)].first;
        while child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let style = self.nodes[usize::from(child)].style;
            if style.is_absolute() {
                let size =
                    self.layout_node(NodeId(child), BoxConstraints::loose(width, height), text);
                let (x, y) = absolute_offset(&style, size, width, height);
                self.nodes[usize::from(child)].offset = Point {
                    x: x + drift.x,
                    y: y + drift.y,
                };
            }
            child = next;
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
        let definite_w = content_length_of(&node.style, false, constraints.max_w);
        let definite_h = content_length_of(&node.style, true, constraints.max_h);
        let mut width = 0;
        let mut height = 0;
        let mut child = self.nodes[usize::from(id.0)].first;
        while child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let child_node = self.nodes[usize::from(child)];
            if !child_node.style.is_absolute() {
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
            if !child_node.style.is_absolute() {
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
        self.place_absolute(id, width, height, Point::default(), text);
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
        // A stretched list gives each row the viewport's whole cross extent; without it a row
        // that only grows has no width to grow into and collapses to its gaps.
        let stretched = node.style.align == Align::Stretch;
        let across_max = if horizontal { inner.max_h } else { inner.max_w };
        let unbounded = if horizontal {
            BoxConstraints {
                min_w: 0,
                max_w: INFINITY,
                min_h: if stretched { across_max.max(0) } else { 0 },
                max_h: across_max.max(0),
            }
        } else {
            BoxConstraints {
                min_w: if stretched { across_max.max(0) } else { 0 },
                max_w: across_max.max(0),
                min_h: 0,
                max_h: INFINITY,
            }
        };
        let mut content = 0;
        let mut cross = 0;
        let mut count = 0;
        let gap = node.style.gap as Subpx * PX;
        // A box with no room across has no content to measure: wrapping a paragraph into a
        // zero-width box reports one line per glyph, and a loose pass asks for that width
        // before the parent hands out the real one.
        let across = if horizontal { inner.max_h } else { inner.max_w };
        let mut child = self.nodes[usize::from(id.0)].first;
        while across > 0 && child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let child_node = self.nodes[usize::from(child)];
            if !child_node.style.is_absolute() {
                let size = self.layout_node(NodeId(child), unbounded, text);
                if count > 0 {
                    content += gap;
                }
                if horizontal {
                    content += size.w;
                    cross = cross.max(size.h);
                } else {
                    content += size.h;
                    cross = cross.max(size.w);
                }
                count += 1;
            }
            child = next;
        }
        let width = content_length_of(&node.style, false, constraints.max_w).unwrap_or(
            if inner.max_w < INFINITY {
                inner.max_w
            } else {
                cross
            },
        );
        let height = content_length_of(&node.style, true, constraints.max_h).unwrap_or(
            if inner.max_h < INFINITY {
                inner.max_h
            } else {
                content
            },
        );
        let viewport = if horizontal { width } else { height };
        let shift = -(i32::from(node.style.scroll_offset) * PX);
        self.record_scroll(id, node.style.scroll_area, content, viewport);
        let room = if horizontal { height } else { width };
        let mut cursor = 0;
        let mut child = self.nodes[usize::from(id.0)].first;
        while child != NONE {
            let next = self.nodes[usize::from(child)].next;
            let child_node = self.nodes[usize::from(child)];
            if !child_node.style.is_absolute() {
                let (main, across) = if horizontal {
                    (child_node.measured.w, child_node.measured.h)
                } else {
                    (child_node.measured.h, child_node.measured.w)
                };
                let cross_at = match node.style.align {
                    Align::Center => (room - across).max(0) / 2,
                    Align::End => (room - across).max(0),
                    _ => 0,
                };
                self.nodes[usize::from(child)].offset = if horizontal {
                    Point {
                        x: shift + cursor,
                        y: cross_at,
                    }
                } else {
                    Point {
                        x: cross_at,
                        y: shift + cursor,
                    }
                };
                cursor += main + gap;
            }
            child = next;
        }
        // An out-of-flow child rides the content, so it takes the same shift the rows did.
        let drift = if horizontal {
            Point { x: shift, y: 0 }
        } else {
            Point { x: 0, y: shift }
        };
        self.place_absolute(id, width, height, drift, text);
        Size {
            w: width,
            h: height,
        }
    }

    fn absolute(&mut self, id: NodeId, origin: Point) {
        let node = self.nodes[usize::from(id.0)];
        let scroll = node.style.flow == Flow::Scroll;
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
            let rect = Rect {
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

fn distribute(nodes: &mut [Node; MAX_NODES], parent: NodeId, amount: Subpx, grow: bool) {
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
            let share = amount * weight as Subpx / total as Subpx;
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

fn length_of(style: &Style, vertical: bool, base: Subpx) -> Option<Subpx> {
    if vertical {
        style.height.resolve(base)
    } else {
        style.width.resolve(base)
    }
}

/// An explicit extent is the border box; flex and scroll math place children in the content
/// box, so the padding comes off here and is added back once in `absolute`.
fn content_length_of(style: &Style, vertical: bool, base: Subpx) -> Option<Subpx> {
    let length = length_of(style, vertical, base)?;
    let pad = if vertical {
        style.padding.vertical()
    } else {
        style.padding.horizontal()
    };
    Some((length - pad).max(0))
}

fn align_offset(align: Align, own: Align, size: Subpx, space: Subpx) -> Subpx {
    let align = if own == Align::Start { align } else { own };
    match align {
        Align::Start | Align::Stretch => 0,
        Align::Center => (space - size).max(0) / 2,
        Align::End => (space - size).max(0),
    }
}

fn justify(justify: Justify, leftover: Subpx, count: usize) -> (Subpx, Subpx) {
    let leftover = leftover.max(0);
    match justify {
        Justify::Start => (0, 0),
        Justify::Center => (leftover / 2, 0),
        Justify::End => (leftover, 0),
        Justify::SpaceBetween => (
            0,
            if count > 1 {
                leftover / (count as Subpx - 1)
            } else {
                0
            },
        ),
        Justify::SpaceAround => {
            let each = leftover / count as Subpx;
            (each / 2, each)
        }
        Justify::SpaceEvenly => {
            let each = leftover / (count as Subpx + 1);
            (each, each)
        }
    }
}

fn absolute_offset(style: &Style, size: Size, width: Subpx, height: Subpx) -> (Subpx, Subpx) {
    let horizontal = match (style.inset.left, style.inset.right) {
        (Some(left), _) => left as Subpx * PX,
        (None, Some(right)) => width - size.w - right as Subpx * PX,
        (None, None) => 0,
    };
    let vertical = match (style.inset.top, style.inset.bottom) {
        (Some(top), _) => top as Subpx * PX,
        (None, Some(bottom)) => height - size.h - bottom as Subpx * PX,
        (None, None) => 0,
    };
    (horizontal, vertical)
}

#[cfg(test)]
mod tests;
