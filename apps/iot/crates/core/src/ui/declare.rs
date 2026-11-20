//! A page under construction: the tree, the runs it points into, and the slots it paints.

use super::canvas::Canvas;
use super::geometry::Rect;
use super::layout::{BoxConstraints, NodeId, NodeKind, ScrollId, ScrollInfo, Style, Tree};
use super::paint;
use super::screen::Screen;

/// Bytes of run a single declaration may hold. Enough for a full debug page and two screens of
/// keys.
const TEXT_BYTES: usize = 2048;

/// Custom leaves a page may declare: one per band or band-sized mark.
const PAINTS: usize = 8;

pub struct Ui<'a, 's> {
    canvas: Option<&'s mut Canvas<'a>>,
    screen: Screen,
    tree: Tree,
    text: [u8; TEXT_BYTES],
    used: usize,
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

    pub fn rect(&self, id: NodeId) -> Rect {
        self.tree.rect(id)
    }

    /// How many nodes the declaration holds.
    pub fn node_count(&self) -> usize {
        self.tree.len()
    }

    /// Every node's rectangle, in declaration order.
    pub fn rects(&self) -> impl Iterator<Item = Rect> + '_ {
        self.tree.rects()
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
            canvas, text, used, ..
        } = self;
        let Some(canvas) = canvas.as_deref_mut() else {
            return;
        };
        paint::node(canvas, info, &text[..*used], paint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::layout::PX;

    /// The declaration lives on the panel core's stack for the length of one page. This is
    /// the budget the panel's stack was sized against; a page that needs more nodes must fit
    /// them in it, not grow the array quietly.
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
    fn a_declaration_fits_the_stack_the_panel_core_was_given() {
        let bytes = core::mem::size_of::<Ui<'static, 'static>>();
        assert!(bytes <= 10 * 1024, "a declaration is {bytes} B");
    }
}
