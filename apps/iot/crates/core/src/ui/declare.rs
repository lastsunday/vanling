//! A page under construction: the tree, the runs it points into, and the slots it paints.

use super::canvas::Canvas;
use super::color::{Color, ColorId};
use super::geometry::Rect;
use super::layout::Span;
use super::layout::{
    BoxConstraints, NodeId, NodeKind, RunStyle, ScrollId, ScrollInfo, Style, Tree,
};
use super::paint;
use super::screen::Screen;

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
    pub fn custom(&mut self, style: Style, slot: u8) -> NodeId {
        if usize::from(slot) >= PAINTS {
            debug_assert!(
                false,
                "the page wants a custom slot the panel does not hold"
            );
            return self.node(style);
        }
        self.tree
            .push(style, NodeKind::Paint { slot }, self.current)
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
    /// is the page's own ink for the custom slots it declared.
    pub fn paint(
        &mut self,
        constraints: BoxConstraints,
        paint: &mut dyn FnMut(u8, &mut Canvas<'_>, Rect),
    ) {
        self.layout(constraints);
        if self.canvas.is_none() {
            return;
        }
        for index in 0..self.tree.len() {
            let info = self.tree.info(NodeId(index as u16));
            self.draw(info, paint);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::layout::PX;
    use crate::ui::{DecorationStyle, FontFamily, FontWeight, Length};

    /// The declaration lives on the panel core's stack (24 KiB on the S3) for the length of
    /// one page, so it is budgeted separately from that stack's other work. Every page's
    /// boxes are part of it: the 25-line Wrap window and its one box per line are why the
    /// ceiling is 12 KiB rather than the 10 KiB the 14-line page needed.
    #[test]
    fn a_declaration_fits_the_stack_the_panel_core_was_given() {
        let bytes = core::mem::size_of::<Ui<'static, 'static>>();
        assert!(bytes <= 12 * 1024, "a declaration is {bytes} B");
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
        let ink = |x: usize, y: usize| {
            let at = (y * W + x) * 2;
            frame[at] != 0 || frame[at + 1] != 0
        };
        assert!((0..40).any(|y| (0..W).any(|x| ink(x, y))), "in the box");
        assert!(
            !(41..H).any(|y| (0..W).any(|x| ink(x, y))),
            "below the box is empty"
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
        let ink = |x: usize, y: usize| {
            let at = (y * W + x) * 2;
            frame[at] != 0 || frame[at + 1] != 0
        };
        assert!(
            (0..30).any(|x| (0..12).any(|y| ink(x, y))),
            "the box is inked"
        );
        assert!(
            !(30..W).any(|x| (0..12).any(|y| ink(x, y))),
            "nothing leaves the box"
        );
    }

    #[test]
    fn a_descender_is_not_cut_by_its_own_line_box() {
        const W: usize = 64;
        const H: usize = 60;
        let mut frame = [0u8; W * H * 2];
        let mut canvas = Canvas::new(&mut frame, W, H, None);
        let mut ui = Ui::new(&mut canvas, Screen::default());
        // A box one line tall. `g` reaches below the baseline, past a 24 px box: only an
        // ancestor clip may stop it, never the box that placed it.
        ui.text(
            b"g",
            Style::new()
                .nowrap()
                .height(24)
                .font_size(Length::Px(24))
                .font_family(FontFamily::Vector),
        );
        ui.paint(BoxConstraints::tight(64 * PX, 60 * PX), &mut |_, _, _| {});
        let canvas = ui.into_canvas().expect("a canvas was handed in");
        let frame = canvas.frame();
        let ink = |x: usize, y: usize| {
            let at = (y * W + x) * 2;
            frame[at] != 0 || frame[at + 1] != 0
        };
        assert!(
            (24..30).any(|y| (0..W).any(|x| ink(x, y))),
            "the descender shows below the line box"
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

    fn ink(run: &[u8], style: Style, spans: &[(usize, usize, RunStyle)]) -> alloc::vec::Vec<u8> {
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
        let frame = ink(b"AB", Style::new(), &[(0, 1, RunStyle::default())]);
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
    fn a_heavy_weight_lays_down_more_ink_and_a_taller_size_more_still() {
        let plain = count(&ink(b"HHHH", Style::new(), &[]));
        let bold = count(&ink(
            b"HHHH",
            Style::new().font_weight(FontWeight::Bold),
            &[],
        ));
        let large = count(&ink(b"HHHH", Style::new().font_size(Length::Px(24)), &[]));
        assert!(bold > plain, "bold {bold} vs plain {plain}");
        assert!(large > bold, "24 px {large} vs bold {bold}");
    }

    #[test]
    fn an_underline_draws_a_rule_below_the_baseline() {
        let plain = row_count(&ink(b"HHHH", Style::new(), &[]), 11);
        let underlined = row_count(&ink(b"HHHH", Style::new().underline(), &[]), 11);
        assert_eq!(plain, 0);
        assert!(underlined >= 20, "the rule spans the run: {underlined} px");
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
            let frame = ink(
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
