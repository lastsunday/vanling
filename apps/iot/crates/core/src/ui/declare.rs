//! A page under construction: the tree, the runs it points into, and the slots it paints.

use super::canvas::Canvas;
use super::color::{Color, ColorId};
use super::geometry::Rect;
use super::layout::Span;
use super::layout::{
    BoxConstraints, FramePrint, NodeId, NodeInfo, NodeKind, RunStyle, ScrollId, ScrollInfo, Source,
    Style, Tree,
};
use super::paint;
use super::screen::Screen;
use crate::drivers::surface::{DamageRect, FrameDamage};

/// Bytes of run a single declaration may hold. Enough for a full debug page and two screens of
/// keys.
pub const TEXT_BYTES: usize = 2048;

/// Custom leaves a page may declare: one per band or band-sized mark.
const PAINTS: usize = 8;

/// Spans one declaration may recolour.
const SPANS: usize = 8;

pub struct Ui<'a, 's> {
    canvas: Option<&'s mut Canvas<'a>>,
    screen: Screen,
    tree: Tree,
    text: [u8; TEXT_BYTES],
    used: usize,
    spans: [Span; SPANS],
    span_count: u8,
    current: NodeId,
    width: usize,
    height: usize,
}

impl<'a, 's> Ui<'a, 's> {
    pub fn new(canvas: &'s mut Canvas<'a>, screen: Screen) -> Self {
        let (width, height) = (canvas.width(), canvas.height());
        Self::with_size(Some(canvas), screen, width, height)
    }

    /// A page drawn in the first `height` rows of the surface: the footer below them stays the
    /// panel's own.
    pub fn new_with_height(canvas: &'s mut Canvas<'a>, screen: Screen, height: usize) -> Self {
        let width = canvas.width();
        Self::with_size(Some(canvas), screen, width, height)
    }

    /// [`Self::new_with_height`] on the heap: the tree is twelve kilobytes of node array that
    /// the painter's frame cannot spare (`@/records/iot/hardware-constraints.md`).
    pub fn boxed_with_height(
        canvas: &'s mut Canvas<'a>,
        screen: Screen,
        height: usize,
    ) -> alloc::boxed::Box<Self> {
        use core::ptr::addr_of_mut;
        let width = canvas.width();
        let mut slot: alloc::boxed::Box<core::mem::MaybeUninit<Self>> =
            alloc::boxed::Box::new_uninit();
        let ui = slot.as_mut_ptr();
        // SAFETY: `ui` is a fresh allocation of `Self`'s layout; every field is written once,
        // the tree is finished in place, and nothing is read before `assume_init`.
        unsafe {
            addr_of_mut!((*ui).canvas).write(Some(canvas));
            addr_of_mut!((*ui).screen).write(screen);
            Tree::init_in_place(addr_of_mut!((*ui).tree));
            addr_of_mut!((*ui).text).write([0; TEXT_BYTES]);
            addr_of_mut!((*ui).used).write(0);
            addr_of_mut!((*ui).spans).write([Span::default(); SPANS]);
            addr_of_mut!((*ui).span_count).write(0);
            addr_of_mut!((*ui).width).write(width);
            addr_of_mut!((*ui).height).write(height);
            let root = (*addr_of_mut!((*ui).tree)).push(
                Style::new().size(width as i16, height as i16).stretch(),
                NodeKind::Box,
                NodeId::NONE,
            );
            addr_of_mut!((*ui).current).write(root);
            slot.assume_init()
        }
    }

    /// A page laid out but not drawn: the hit test's half of the same declaration.
    pub fn headless(width: usize, height: usize, screen: Screen) -> Self {
        Self::with_size(None, screen, width, height)
    }

    fn with_size(
        canvas: Option<&'s mut Canvas<'a>>,
        screen: Screen,
        width: usize,
        height: usize,
    ) -> Self {
        let mut tree = Tree::new();
        let root = tree.push(
            Style::new().size(width as i16, height as i16).stretch(),
            NodeKind::Box,
            NodeId::NONE,
        );
        Self {
            canvas,
            screen,
            tree,
            text: [0; TEXT_BYTES],
            used: 0,
            spans: [Span::default(); SPANS],
            span_count: 0,
            current: root,
            width,
            height,
        }
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// The surface the page is being drawn into, for the few marks a declaration cannot make.
    pub fn canvas(&self) -> Option<&Canvas<'a>> {
        self.canvas.as_deref()
    }

    pub fn into_canvas(self) -> Option<&'s mut Canvas<'a>> {
        self.canvas
    }

    pub fn node(&mut self, style: Style) -> NodeId {
        self.tree.push(style, NodeKind::Box, self.current)
    }

    /// Declares `widget` and returns its root.
    pub fn add<W: super::widget::Widget>(&mut self, widget: W) -> NodeId {
        widget.build(self)
    }

    pub fn open(&mut self, style: Style) -> NodeId {
        let id = self.node(style);
        self.current = id;
        id
    }

    pub fn close(&mut self) -> NodeId {
        let closed = self.current;
        if let Some(parent) = self.tree.parent(closed) {
            self.current = parent;
        }
        closed
    }

    pub fn column(&mut self, style: Style, body: impl FnOnce(&mut Self)) -> NodeId {
        let id = self.open(style.column());
        body(self);
        self.close();
        id
    }

    pub fn row(&mut self, style: Style, body: impl FnOnce(&mut Self)) -> NodeId {
        let id = self.open(style.row());
        body(self);
        self.close();
        id
    }

    pub fn stack(&mut self, style: Style, body: impl FnOnce(&mut Self)) -> NodeId {
        let id = self.open(style.stack());
        body(self);
        self.close();
        id
    }

    pub fn wrap(&mut self, style: Style, body: impl FnOnce(&mut Self)) -> NodeId {
        let id = self.open(style.wrap());
        body(self);
        self.close();
        id
    }

    pub fn scroll(&mut self, style: Style, body: impl FnOnce(&mut Self)) -> NodeId {
        let id = self.open(style.scrolling());
        body(self);
        self.close();
        id
    }

    /// Declares a run of text; the bytes are copied so a formatted buffer can be reused at once.
    pub fn text(&mut self, run: &[u8], style: Style) -> NodeId {
        let room = TEXT_BYTES - self.used;
        debug_assert!(
            run.len() <= room,
            "the declaration ran out of text: {} B for {room} B of room",
            run.len()
        );
        let mut len = run.len().min(room);
        if len < run.len() {
            len = core::str::from_utf8(&run[..len])
                .map(|_| len)
                .unwrap_or_else(|error| error.valid_up_to());
        }
        let start = self.used;
        self.text[start..start + len].copy_from_slice(&run[..len]);
        self.used += len;
        self.tree.push(
            style,
            NodeKind::Text {
                start: start as u16,
                len: len as u16,
            },
            self.current,
        )
    }

    /// Declares a leaf the page paints itself: a sweep, a dial, a band. The `slot` is what the
    /// page's own painter switches on when [`Self::paint`] reaches the leaf.
    ///
    /// The page does not say where its pixels changed. A frame repaints the leaf whenever the
    /// snapshot moved; [`Self::custom_state`] narrows that to the frames the leaf's own picture
    /// moved.
    pub fn custom(&mut self, style: Style, slot: u8) -> NodeId {
        self.custom_state(style, slot, super::layout::UNKNOWN_PAINT)
    }

    /// The same, with a fingerprint of what the leaf draws. `state` must cover every input of
    /// the page's drawing for `slot`: a frame whose state and style are unchanged leaves the
    /// leaf's pixels alone.
    pub fn custom_state(&mut self, style: Style, slot: u8, state: u32) -> NodeId {
        if usize::from(slot) >= PAINTS {
            debug_assert!(
                false,
                "the page wants a custom slot the panel does not hold"
            );
            return self.node(style);
        }
        self.tree
            .push(style, NodeKind::Paint { slot, state }, self.current)
    }

    /// The slot `color` is held in for this declaration, so a style can name it in one byte.
    pub fn color(&mut self, color: Color) -> ColorId {
        self.tree.intern(color)
    }

    /// Recolours bytes `start..start + len` of the run `node` was declared with. A span is
    /// paint only: the line it sits on is still the one the node's own metrics broke.
    pub fn span(&mut self, node: NodeId, start: usize, len: usize, style: RunStyle) {
        let Some(base) = self.tree.text_start(node) else {
            debug_assert!(false, "a span needs a run of text");
            return;
        };
        let Some(slot) = self.spans.get_mut(usize::from(self.span_count)) else {
            debug_assert!(false, "the declaration ran out of spans");
            return;
        };
        *slot = Span {
            start: base + start as u16,
            end: base + (start + len) as u16,
            style,
        };
        self.span_count += 1;
    }

    pub fn rect(&self, id: NodeId) -> Rect {
        self.tree.rect(id)
    }

    /// How many nodes the declaration holds.
    pub fn node_count(&self) -> usize {
        self.tree.len()
    }

    /// How many bytes of text the declaration copied, against [`TEXT_BYTES`].
    pub fn text_bytes(&self) -> usize {
        self.used
    }

    /// Every node's rectangle, in declaration order.
    pub fn rects(&self) -> impl Iterator<Item = Rect> + '_ {
        self.tree.rects()
    }

    /// Every tagged node's tag and rectangle, in declaration order.
    pub fn tags(&self) -> impl Iterator<Item = (u16, Rect)> + '_ {
        self.tree.tags()
    }

    /// The rectangles of the declared runs, in declaration order.
    pub fn text_rects(&self) -> impl Iterator<Item = Rect> + '_ {
        self.tree.text_rects()
    }

    /// The rectangle of the paint leaf declared for `slot`.
    pub fn paint_rect(&self, slot: u8) -> Option<Rect> {
        self.tree.paint_rect(slot)
    }

    /// Where the scroll box named `area` sits and how far it can travel, as last laid out.
    pub fn scroll_info(&self, area: ScrollId) -> Option<ScrollInfo> {
        self.tree.scroll_info(area)
    }

    /// Whether the box at `id` is inside a clipping ancestor.
    pub fn clipped(&self, id: NodeId) -> bool {
        self.tree.info(id).clipped
    }

    /// The topmost tagged box under a panel point.
    pub fn tag_at(&self, x: i32, y: i32) -> Option<u16> {
        self.tree.tag_at(x, y)
    }

    /// Lays the declaration out and draws it, when there is a canvas to draw into. `paint`
    /// is the page's own drawing for the custom slots it declared.
    pub fn paint(
        &mut self,
        constraints: BoxConstraints,
        paint: &mut dyn FnMut(u8, &mut Canvas<'_>, Rect),
    ) {
        self.layout(constraints);
        self.draw_all(paint);
    }

    /// Lays the declaration out, records its fingerprints in `next`, and draws only what
    /// `source` says the frame still needs. `extra` names pixels the caller draws outside the
    /// tree — the debug badge — which are repainted with every partial frame.
    ///
    /// The answer is the rectangle set the canvas was narrowed to. The caller ships it to the
    /// panel; a full answer means every pixel was drawn.
    pub fn paint_measured(
        &mut self,
        constraints: BoxConstraints,
        next: &mut FramePrint,
        source: Source<'_>,
        extra: FrameDamage,
        paint: &mut dyn FnMut(u8, &mut Canvas<'_>, Rect),
    ) -> FrameDamage {
        self.layout(constraints);
        self.record(next);
        let mut damage = match source {
            Source::Full => FrameDamage::FULL,
            Source::Window(window) => window,
            Source::Diff { prev, moved } => prev.map_or(FrameDamage::FULL, |prev| {
                self.damage_since(prev, next, moved)
            }),
        };
        if damage.is_full() || extra.is_full() {
            damage = FrameDamage::FULL;
        } else {
            for rect in extra.rects() {
                damage.add(*rect);
            }
        }
        // A partial frame starts from the ground: page text is the ground turned over, so a
        // band drawn twice would toggle. The caller filled the frame for `Source::Full`.
        if !matches!(source, Source::Full)
            && let Some(canvas) = self.canvas.as_deref_mut()
        {
            canvas.clear(damage);
            canvas.set_damage(damage);
        }
        self.draw_all(paint);
        damage
    }

    fn draw_all(&mut self, paint: &mut dyn FnMut(u8, &mut Canvas<'_>, Rect)) {
        if self.canvas.is_none() {
            return;
        }
        let damage = self
            .canvas
            .as_deref()
            .and_then(|canvas| canvas.damage())
            .unwrap_or(FrameDamage::FULL);
        let mut covers = 0u64;
        for index in 0..self.tree.len() {
            if self.tree.info(NodeId(index as u16)).style.covers() {
                covers |= 1 << index;
            }
        }
        for index in 0..self.tree.len() {
            let info = self.tree.info(NodeId(index as u16));
            if self
                .canvas
                .as_deref()
                .is_some_and(|canvas| !canvas.touches(info.rect))
            {
                continue;
            }
            if self.covered(index, info.rect, damage, covers) {
                continue;
            }
            self.draw(info, paint);
        }
    }

    /// Whether every damaged pixel this node could draw is inside a later node that paints its
    /// whole rectangle: the cover redraws them, so this node's are hidden.
    fn covered(&self, index: usize, rect: Rect, damage: FrameDamage, covers: u64) -> bool {
        if damage.is_full() {
            return false;
        }
        let later = covers & (u64::MAX << (index + 1));
        damage
            .rects()
            .iter()
            .filter(|area| {
                i32::from(area.x) < rect.right()
                    && i32::from(area.right()) > rect.x
                    && i32::from(area.y) < rect.bottom()
                    && i32::from(area.bottom()) > rect.y
            })
            .all(|area| {
                let mut rest = later;
                while rest != 0 {
                    let at = rest.trailing_zeros() as usize;
                    rest &= rest - 1;
                    let cover = self.tree.info(NodeId(at as u16)).rect;
                    if cover.x <= i32::from(area.x)
                        && cover.y <= i32::from(area.y)
                        && cover.right() >= i32::from(area.right())
                        && cover.bottom() >= i32::from(area.bottom())
                    {
                        return true;
                    }
                }
                false
            })
    }

    /// Hashes the tree's shape and every node's content into `next`.
    fn record(&self, next: &mut FramePrint) {
        let mut structure = 0x811C_9DC5 ^ self.tree.len() as u32;
        next.len = self.tree.len() as u16;
        for index in 0..self.tree.len() {
            let info = self.tree.info(NodeId(index as u16));
            let kind = match info.kind {
                NodeKind::Box => 0,
                NodeKind::Text { .. } => 1,
                NodeKind::Paint { slot, .. } => 2 + u32::from(slot),
            };
            structure = mix(structure, kind);
            for value in [info.rect.x, info.rect.y, info.rect.w, info.rect.h] {
                structure = mix(structure, value as u32);
            }
            for value in [info.clip.x, info.clip.y, info.clip.w, info.clip.h] {
                structure = mix(structure, value as u32);
            }
            structure = mix(structure, u32::from(info.clipped));
            next.content[index] = self.content(&info);
        }
        next.structure = structure;
    }

    fn content(&self, info: &NodeInfo) -> u32 {
        let seed = info.style.fingerprint();
        match info.kind {
            NodeKind::Box => seed,
            NodeKind::Text { start, len } => {
                let mut hash = seed;
                for &byte in &self.text[usize::from(start)..usize::from(start + len)] {
                    hash = mix(hash, u32::from(byte));
                }
                let end = start + len;
                for span in &self.spans[..usize::from(self.span_count)] {
                    if span.end > start && span.start < end {
                        hash = mix(hash, u32::from(span.start));
                        hash = mix(hash, u32::from(span.end));
                        hash = mix(hash, span.style.fingerprint());
                    }
                }
                hash
            }
            NodeKind::Paint { slot, state } => mix(mix(seed, u32::from(slot)), state),
        }
    }

    /// The pixels `prev` and `next` disagree on. A different shape cannot be patched, so it is
    /// the whole frame.
    fn damage_since(&self, prev: &FramePrint, next: &FramePrint, moved: bool) -> FrameDamage {
        if prev.structure != next.structure || prev.len != next.len {
            return FrameDamage::FULL;
        }
        let mut damage = FrameDamage::NONE;
        for index in 0..self.tree.len() {
            let info = self.tree.info(NodeId(index as u16));
            let unknown = moved
                && matches!(info.kind, NodeKind::Paint { state, .. } if state == super::layout::UNKNOWN_PAINT);
            if prev.content[index] != next.content[index] || unknown {
                // `clip` only means something for a node inside a scroll box; an unclipped
                // node's clip is the empty default.
                let rect = if info.clipped {
                    info.rect.intersect(info.clip)
                } else {
                    info.rect
                };
                self.mark_damage(&mut damage, rect);
            }
        }
        damage
    }

    fn mark_damage(&self, damage: &mut FrameDamage, rect: Rect) {
        let (x0, y0) = (
            rect.x.clamp(0, self.width as i32),
            rect.y.clamp(0, self.height as i32),
        );
        let (x1, y1) = (
            rect.right().clamp(0, self.width as i32),
            rect.bottom().clamp(0, self.height as i32),
        );
        if x1 > x0 && y1 > y0 {
            damage.add(DamageRect::new(
                x0 as u16,
                y0 as u16,
                (x1 - x0) as u16,
                (y1 - y0) as u16,
            ));
        }
    }

    /// Lays the declaration out without drawing: the same rectangles a hit test reads.
    pub fn layout(&mut self, constraints: BoxConstraints) {
        self.tree.layout(constraints, &self.text[..self.used]);
    }

    fn draw(
        &mut self,
        info: super::layout::NodeInfo,
        paint: &mut dyn FnMut(u8, &mut Canvas<'_>, Rect),
    ) {
        let Self {
            canvas,
            text,
            used,
            tree,
            spans,
            span_count,
            ..
        } = self;
        let Some(canvas) = canvas.as_deref_mut() else {
            return;
        };
        let palette = tree.palette();
        paint::node(
            canvas,
            info,
            &text[..*used],
            &palette,
            &spans[..usize::from(*span_count)],
            paint,
        );
    }
}

/// One step of the 32-bit FNV-1a every fingerprint uses.
fn mix(seed: u32, value: u32) -> u32 {
    (seed ^ value).wrapping_mul(0x0100_0193)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::surface::DamageRect;
    use crate::ui::layout::PX;
    use crate::ui::{DecorationStyle, FontWeight, Length, Overflow, TextAlign};

    /// The declaration lives on the main stack (about 80 KiB on the S3) for the length of one
    /// page, so it is budgeted separately from that stack's other work. Every page's boxes are
    /// part of it: the 25-line Wrap window and its one box per line are why the ceiling is
    /// 13 KiB rather than the 10 KiB the 14-line page needed. The menu's 63-node declaration,
    /// scroll thumb included, is the fullest page.
    #[test]
    fn a_declaration_fits_the_stack_the_panel_core_was_given() {
        let bytes = core::mem::size_of::<Ui<'static, 'static>>();
        assert!(bytes <= 13 * 1024, "a declaration is {bytes} B");
    }

    #[test]
    fn a_scroll_box_clips_the_lines_below_it() {
        const W: usize = 100;
        const H: usize = 100;
        let mut frame = [0u8; W * H * 2];
        let mut canvas = Canvas::new(&mut frame, W, H, None);
        let mut ui = Ui::new(&mut canvas, Screen::default());
        ui.scroll(Style::new().height(40), |ui| {
            for _ in 0..8 {
                ui.text(b"WWWWWWWW", Style::new().nowrap());
            }
        });
        ui.paint(BoxConstraints::tight(100 * PX, 100 * PX), &mut |_, _, _| {});
        let canvas = ui.into_canvas().expect("a canvas was handed in");
        let frame = canvas.frame();
        let drawn = |x: usize, y: usize| {
            let at = (y * W + x) * 2;
            frame[at] != 0 || frame[at + 1] != 0
        };
        assert!((0..40).any(|y| (0..W).any(|x| drawn(x, y))), "in the box");
        assert!(
            !(41..H).any(|y| (0..W).any(|x| drawn(x, y))),
            "below the box is empty"
        );
    }

    #[test]
    fn a_run_wider_than_its_box_is_cut_and_can_be_marked() {
        const W: usize = 80;
        const H: usize = 20;
        let drawn = |overflow: Overflow| -> usize {
            let mut frame = [0u8; W * H * 2];
            let mut canvas = Canvas::new(&mut frame, W, H, None);
            let mut ui = Ui::new(&mut canvas, Screen::default());
            ui.column(Style::new().width(30), |ui| {
                ui.text(b"WWWWWWWW", Style::new().nowrap().overflow(overflow));
            });
            ui.paint(BoxConstraints::tight(80 * PX, 20 * PX), &mut |_, _, _| {});
            let frame = ui.into_canvas().expect("a canvas was handed in").frame();
            (0..W * H)
                .filter(|at| frame[at * 2] != 0 || frame[at * 2 + 1] != 0)
                .count()
        };
        let clip = drawn(Overflow::Clip);
        assert!(clip > 0, "the cut run still draws");
        assert!(
            drawn(Overflow::Ellipsis) < clip,
            "the mark is drawn, but one glyph less of the run is"
        );
    }

    #[test]
    fn a_marked_run_keeps_its_alignment() {
        const W: usize = 40;
        const H: usize = 12;
        let first = |align: TextAlign| -> usize {
            let mut frame = [0u8; W * H * 2];
            let mut canvas = Canvas::new(&mut frame, W, H, None);
            let mut ui = Ui::new(&mut canvas, Screen::default());
            ui.column(Style::new().width(32), |ui| {
                ui.text(
                    b"WWWWWWWW",
                    Style::new().nowrap().ellipsis().text_align(align),
                );
            });
            ui.paint(BoxConstraints::tight(40 * PX, 12 * PX), &mut |_, _, _| {});
            let frame = ui.into_canvas().expect("a canvas was handed in").frame();
            (0..W)
                .find(|x| {
                    (0..H).any(|y| frame[(y * W + x) * 2] != 0 || frame[(y * W + x) * 2 + 1] != 0)
                })
                .expect("the run draws")
        };
        let start = first(TextAlign::Start);
        assert_eq!(
            first(TextAlign::Center) - start,
            1,
            "two pixels of slack, one either side"
        );
        assert_eq!(first(TextAlign::End) - start, 2);
    }

    #[test]
    fn a_text_node_lays_its_own_paper() {
        const W: usize = 40;
        const H: usize = 20;
        let mut frame = [0u8; W * H * 2];
        let mut canvas = Canvas::new(&mut frame, W, H, None);
        let mut ui = Ui::new(&mut canvas, Screen::default());
        let paper = ui.color(super::super::color::Color::rgb(0xFF, 0xFF, 0xFF));
        ui.text(
            b"X",
            Style::new()
                .width(24)
                .height(16)
                .radius(4)
                .background(paper)
                .text_align(TextAlign::Center),
        );
        ui.paint(BoxConstraints::tight(40 * PX, 20 * PX), &mut |_, _, _| {});
        let frame = ui.into_canvas().expect("a canvas was handed in").frame();
        let pixel = |x: usize, y: usize| {
            u16::from_be_bytes([frame[(y * W + x) * 2], frame[(y * W + x) * 2 + 1]])
        };
        assert_eq!(pixel(0, 0), 0, "the rounded corner is cut away");
        assert_eq!(pixel(12, 8), 0xFFFF, "the paper covers the box");
        assert!(
            (0..W).any(|x| (0..H).any(|y| pixel(x, y) == 0)),
            "the run is still drawn over the paper"
        );
    }

    #[test]
    fn a_nowrap_run_honours_centre_and_end() {
        const W: usize = 40;
        const H: usize = 24;
        let left_of = |align: TextAlign| {
            let mut frame = [0u8; W * H * 2];
            let mut canvas = Canvas::new(&mut frame, W, H, None);
            let mut ui = Ui::new(&mut canvas, Screen::default());
            ui.text(
                b"V",
                Style::new().width(24).height(24).nowrap().text_align(align),
            );
            ui.paint(BoxConstraints::tight(40 * PX, 24 * PX), &mut |_, _, _| {});
            let frame = ui.into_canvas().expect("a canvas was handed in").frame();
            (0..W)
                .find(|&x| {
                    (0..H).any(|y| frame[(y * W + x) * 2] != 0 || frame[(y * W + x) * 2 + 1] != 0)
                })
                .expect("the run left a mark")
        };
        assert_eq!(left_of(TextAlign::Start), 0);
        assert!(left_of(TextAlign::Center) > 4, "the run sits in the middle");
        assert!(
            left_of(TextAlign::End) > left_of(TextAlign::Center),
            "the run ends at the box's right edge"
        );
    }

    #[test]
    fn nowrap_text_paints_only_inside_its_box() {
        const W: usize = 100;
        const H: usize = 24;
        let mut frame = [0u8; W * H * 2];
        let mut canvas = Canvas::new(&mut frame, W, H, None);
        let mut ui = Ui::new(&mut canvas, Screen::default());
        ui.text(b"WWWWWWWWWW", Style::new().width(30).nowrap());
        ui.paint(BoxConstraints::tight(100 * PX, 24 * PX), &mut |_, _, _| {});
        let canvas = ui.into_canvas().expect("a canvas was handed in");
        let frame = canvas.frame();
        let drawn = |x: usize, y: usize| {
            let at = (y * W + x) * 2;
            frame[at] != 0 || frame[at + 1] != 0
        };
        assert!(
            (0..30).any(|x| (0..12).any(|y| drawn(x, y))),
            "the box is drawn"
        );
        assert!(
            !(30..W).any(|x| (0..12).any(|y| drawn(x, y))),
            "nothing leaves the box"
        );
    }

    #[test]
    fn a_descender_reaches_the_bottom_of_its_line_box() {
        const W: usize = 64;
        const H: usize = 60;
        let mut frame = [0u8; W * H * 2];
        let mut canvas = Canvas::new(&mut frame, W, H, None);
        let mut ui = Ui::new(&mut canvas, Screen::default());
        // `g` reaches below the baseline; the 24 px line box is the face's ascent plus descent.
        ui.text(
            b"g",
            Style::new().nowrap().height(24).font_size(Length::Px(24)),
        );
        ui.paint(BoxConstraints::tight(64 * PX, 60 * PX), &mut |_, _, _| {});
        let canvas = ui.into_canvas().expect("a canvas was handed in");
        let frame = canvas.frame();
        let drawn = |x: usize, y: usize| {
            let at = (y * W + x) * 2;
            frame[at] != 0 || frame[at + 1] != 0
        };
        assert!(
            (20..24).any(|y| (0..W).any(|x| drawn(x, y))),
            "the descender fills its own line box"
        );
    }

    #[test]
    fn a_run_naming_a_colour_is_painted_in_it() {
        const W: usize = 40;
        const H: usize = 24;
        let mut frame = [0u8; W * H * 2];
        let mut canvas = Canvas::new(&mut frame, W, H, None);
        let mut ui = Ui::new(&mut canvas, Screen::default());
        let red = ui.color(Color::rgb(0xFF, 0, 0));
        ui.text(b"X", Style::new().nowrap().color(red));
        ui.paint(BoxConstraints::tight(40 * PX, 24 * PX), &mut |_, _, _| {});
        let canvas = ui.into_canvas().expect("a canvas was handed in");
        let frame = canvas.frame();
        let red = crate::light::rgb565(crate::light::Rgb(0xFF, 0, 0)).to_be_bytes();
        assert!(
            frame.as_chunks::<2>().0.contains(&red),
            "the run is in the declared colour"
        );
    }

    const W: usize = 48;
    const H: usize = 24;

    fn raster(run: &[u8], style: Style, spans: &[(usize, usize, RunStyle)]) -> alloc::vec::Vec<u8> {
        let mut frame = alloc::vec![0u8; W * H * 2];
        {
            let mut canvas = Canvas::new(&mut frame, W, H, None);
            let mut ui = Ui::new(&mut canvas, Screen::default());
            let red = ui.color(Color::rgb(0xF0, 0x20, 0x20));
            let id = ui.text(run, style.nowrap());
            for (start, len, span) in spans {
                let span = if span.color.0 == 0 {
                    span.color(red)
                } else {
                    *span
                };
                ui.span(id, *start, *len, span);
            }
            ui.paint(BoxConstraints::tight(48 * PX, 24 * PX), &mut |_, _, _| {});
        }
        frame
    }

    fn count(frame: &[u8]) -> usize {
        frame
            .as_chunks::<2>()
            .0
            .iter()
            .filter(|pixel| pixel.iter().any(|byte| *byte != 0))
            .count()
    }

    fn row_count(frame: &[u8], row: usize) -> usize {
        (0..W)
            .filter(|&x| {
                let at = (row * W + x) * 2;
                frame[at] != 0 || frame[at + 1] != 0
            })
            .count()
    }

    #[test]
    fn a_span_recolours_only_its_own_bytes() {
        let frame = raster(b"AB", Style::new(), &[(0, 1, RunStyle::default())]);
        let red = crate::light::rgb565(crate::light::Rgb(0xF0, 0x20, 0x20)).to_be_bytes();
        let red_in = |from: usize, to: usize| {
            (from..to).any(|x| {
                (0..H).any(|y| {
                    let at = (y * W + x) * 2;
                    frame[at..at + 2] == red
                })
            })
        };
        assert!(red_in(0, 6), "the first glyph takes the span's colour");
        assert!(!red_in(7, 13), "the next keeps the inverted ground");
    }

    #[test]
    fn a_heavy_weight_lays_down_more_coverage_and_a_taller_size_more_still() {
        let plain = count(&raster(b"HHHH", Style::new(), &[]));
        let bold = count(&raster(
            b"HHHH",
            Style::new().font_weight(FontWeight::Bold),
            &[],
        ));
        let large = count(&raster(
            b"HHHH",
            Style::new().font_size(Length::Px(24)),
            &[],
        ));
        assert!(bold > plain, "bold {bold} vs plain {plain}");
        assert!(large > bold, "24 px {large} vs bold {bold}");
    }

    #[test]
    fn an_underline_draws_a_rule_below_the_baseline() {
        let plain = row_count(&raster(b"HHHH", Style::new(), &[]), 11);
        let underlined = row_count(&raster(b"HHHH", Style::new().underline(), &[]), 11);
        assert_eq!(plain, 0);
        assert!(underlined >= 20, "the rule spans the run: {underlined} px");
    }

    /// One declaration of `build`, measured headless, and the damage it needs against
    /// `prev`.
    fn damage_of(
        prev: Option<&FramePrint>,
        moved: bool,
        build: impl FnOnce(&mut Ui<'_, '_>),
    ) -> (FramePrint, FrameDamage) {
        let mut print = FramePrint::new();
        let mut ui = Ui::headless(40, 30, Screen::default());
        build(&mut ui);
        let mut ignore = |_: u8, _: &mut Canvas<'_>, _: Rect| {};
        let damage = ui.paint_measured(
            BoxConstraints::tight(40 * PX, 30 * PX),
            &mut print,
            Source::Diff { prev, moved },
            FrameDamage::NONE,
            &mut ignore,
        );
        (print, damage)
    }

    fn leaf(ui: &mut Ui<'_, '_>) {
        ui.custom(Style::new().width(10).height(10), 0);
    }

    #[test]
    fn an_unstated_custom_slot_repaints_when_the_snapshot_moves() {
        let (first, full) = damage_of(None, false, leaf);
        assert!(
            full.is_full(),
            "the first frame has nothing to compare with"
        );

        let (_, still) = damage_of(Some(&first), false, leaf);
        assert_eq!(still, FrameDamage::NONE, "a still snapshot leaves it alone");

        let (_, moved) = damage_of(Some(&first), true, leaf);
        assert_eq!(moved.rects(), &[DamageRect::new(0, 0, 10, 10)]);
    }

    #[test]
    fn a_stated_custom_slot_repaints_only_when_its_state_moves() {
        let state = |ui: &mut Ui<'_, '_>, state: u32| {
            ui.custom_state(Style::new().width(10).height(10), 0, state);
        };
        let (first, _) = damage_of(None, false, |ui| state(ui, 1));
        let (_, same) = damage_of(Some(&first), true, |ui| state(ui, 1));
        assert_eq!(same, FrameDamage::NONE, "the picture did not move");
        let (_, moved) = damage_of(Some(&first), false, |ui| state(ui, 2));
        assert_eq!(moved.rects(), &[DamageRect::new(0, 0, 10, 10)]);
    }

    #[test]
    fn a_different_tree_cannot_be_patched() {
        let (first, _) = damage_of(None, false, leaf);
        let (_, grown) = damage_of(Some(&first), false, |ui| {
            leaf(ui);
            ui.custom(Style::new().width(5).height(5), 1);
        });
        assert!(grown.is_full(), "one more node moves the pixels under it");
    }

    #[test]
    fn a_run_that_changes_its_bytes_repaints_its_rectangle() {
        let run = |ui: &mut Ui<'_, '_>, text: &'static [u8]| {
            ui.text(text, Style::new().width(20).height(10).nowrap());
        };
        let (first, _) = damage_of(None, false, |ui| run(ui, b"AB"));
        let (_, changed) = damage_of(Some(&first), false, |ui| run(ui, b"AC"));
        assert_eq!(changed.rects(), &[DamageRect::new(0, 0, 20, 10)]);
    }

    #[test]
    fn a_leaf_that_moves_the_layout_is_a_whole_frame() {
        let styled = |ui: &mut Ui<'_, '_>, width: i16| {
            ui.custom(Style::new().width(width).height(10), 0);
        };
        let (first, _) = damage_of(None, false, |ui| styled(ui, 10));
        let (_, resized) = damage_of(Some(&first), false, |ui| styled(ui, 12));
        assert!(
            resized.is_full(),
            "a leaf that grew moves the boxes after it"
        );
    }

    #[test]
    fn every_decoration_style_marks_the_line() {
        for style in [
            DecorationStyle::Solid,
            DecorationStyle::Double,
            DecorationStyle::Dotted,
            DecorationStyle::Dashed,
            DecorationStyle::Wavy,
        ] {
            let frame = raster(
                b"HHHH",
                Style::new().underline().decoration_style(style),
                &[],
            );
            assert!(
                row_count(&frame, 11) + row_count(&frame, 12) > 0,
                "{style:?}"
            );
        }
    }
}
