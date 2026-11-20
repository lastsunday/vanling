//! Pixels, text, and the boxes a page declares: the panel's whole drawing surface.
//!
//! A page states what it wants while its `view` runs; [`Ui`] builds the tree, lays it out once,
//! then draws its text and custom marks into the canvas it was handed. The same declaration can
//! run without a canvas, which is how a hit test uses the picture's own rectangles.

pub mod canvas;
pub mod font;
pub mod layout;
pub mod panel;
pub mod text;
pub mod typeset;

pub use canvas::Canvas;
pub use layout::{Align, Axis, BoxConstraints, Display, Justify, Length, Style, Tree};
pub use layout::{NodeId, UNIT};

use font::Face;
use layout::NodeInfo;

/// Lines a single declaration may hold. Enough for a full debug page and two screens of keys.
const TEXT_BYTES: usize = 2048;
/// Custom leaves a page may declare: one per band or band-sized mark.
const PAINTS: usize = 8;

/// A rectangle in panel pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub const fn right(self) -> i32 {
        self.x + self.w
    }

    pub const fn bottom(self) -> i32 {
        self.y + self.h
    }

    pub const fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    pub fn intersect(self, other: Rect) -> Rect {
        let x = if self.x > other.x { self.x } else { other.x };
        let y = if self.y > other.y { self.y } else { other.y };
        let right = if self.right() < other.right() {
            self.right()
        } else {
            other.right()
        };
        let bottom = if self.bottom() < other.bottom() {
            self.bottom()
        } else {
            other.bottom()
        };
        Rect {
            x,
            y,
            w: (right - x).max(0),
            h: (bottom - y).max(0),
        }
    }
}

/// What the panel draws with that is not the picture: whose surface it is, the frame rate to
/// report, and the microphone's calibration.
#[derive(Clone, Copy, Debug, Default)]
pub struct Screen {
    pub instance: u8,
    pub fps: u8,
    pub debug: bool,
    pub spl_offset_db: i16,
    /// The platform's stack reading, taken where the paint path is deepest.
    pub probe: Option<fn()>,
}

/// A page under construction: the tree, the text pool its runs point into, and the slots the
/// page will paint itself once the rectangles are known.
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
    pub fn measure(width: usize, height: usize, screen: Screen) -> Self {
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
            layout::Kind::Box,
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

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// The surface the page is being drawn into, for the few marks a declaration cannot make.
    pub fn canvas(&self) -> Option<&Canvas<'a>> {
        self.canvas.as_deref()
    }

    pub fn into_canvas(self) -> Option<&'s mut Canvas<'a>> {
        self.canvas
    }

    pub fn node(&mut self, style: Style) -> NodeId {
        self.tree.push(style, layout::Kind::Box, self.current)
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
        let id = self.open(style.scroll());
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
            layout::Kind::Text {
                start: start as u16,
                len: len as u16,
            },
            self.current,
        )
    }

    /// Declares a leaf the page paints itself: a sweep, a dial, a band. The `slot` is what
    /// the page's own painter switches on when [`Self::finish`] reaches the leaf.
    pub fn custom(&mut self, style: Style, slot: u8) -> NodeId {
        if usize::from(slot) >= PAINTS {
            debug_assert!(
                false,
                "the page wants a custom slot the panel does not hold"
            );
            return self.node(style);
        }
        self.tree
            .push(style, layout::Kind::Custom { slot }, self.current)
    }

    pub fn rect(&self, id: NodeId) -> Rect {
        self.tree.rect(id)
    }

    /// The topmost tagged box under a panel point.
    pub fn tag_at(&self, x: i32, y: i32) -> Option<u16> {
        self.tree.tag_at(x, y)
    }

    /// Lays the declaration out and draws it, when there is a canvas to draw into. `paint`
    /// is the page's own ink for the custom slots it declared.
    pub fn finish(
        &mut self,
        constraints: BoxConstraints,
        paint: &mut dyn FnMut(u8, &mut Canvas<'_>, Rect),
    ) {
        self.layout(constraints);
        if self.canvas.is_none() {
            return;
        }
        for index in 0..self.tree.len() {
            let id = NodeId(index as u16);
            let info = self.tree.info(id);
            self.draw(info, paint);
        }
    }

    /// Lays the declaration out without drawing: the same rectangles a hit test reads.
    pub fn layout(&mut self, constraints: BoxConstraints) {
        self.tree.finish(constraints, &self.text[..self.used]);
    }

    fn draw(&mut self, info: NodeInfo, paint: &mut dyn FnMut(u8, &mut Canvas<'_>, Rect)) {
        let Self { canvas, text, .. } = self;
        let Some(canvas) = canvas.as_deref_mut() else {
            return;
        };
        if info.clipped {
            canvas.push_clip(info.clip);
        }
        match info.kind {
            layout::Kind::Text { start, len } => {
                let run = &text[usize::from(start)..usize::from(start + len)];
                draw_text(canvas, run, info.rect, info.style);
            }
            layout::Kind::Custom { slot } => paint(slot, canvas, info.rect),
            layout::Kind::Box => {
                if info.style.fill {
                    canvas.fill_rect(info.rect);
                }
                if info.style.border {
                    canvas.stroke_rect(info.rect);
                }
            }
        }
        if info.clipped {
            canvas.pop_clip();
        }
    }
}

fn draw_text(canvas: &mut Canvas<'_>, run: &[u8], rect: Rect, style: Style) {
    if rect.w <= 0 {
        return;
    }
    // A left-aligned run that fits needs no breaking: the panel's pages are mostly readings,
    // and running the breaker over one of them every frame measures the same run twice.
    if style.text_align == Align::Start && style.max_lines >= 1 && !run.contains(&b'\n') {
        let width = canvas.measure(run);
        let height = typeset::line_height();
        let top = rect.y + (rect.h - height as i32).max(0) / 2;
        if width <= rect.w as usize && rect.x >= 0 && top >= 0 {
            canvas.draw_text(run, rect.x as usize, top as usize);
            return;
        }
    }
    let paragraph = typeset::wrap(run, rect.w as usize, usize::from(style.max_lines));
    let height = paragraph.count * typeset::line_height();
    let top = rect.y + (rect.h - height as i32).max(0) / 2;
    for (index, line) in paragraph.lines[..paragraph.count].iter().enumerate() {
        let last = index + 1 == paragraph.count;
        let mut x = match style.text_align {
            Align::Center => rect.x + (rect.w - line.width as i32) / 2,
            Align::End => rect.x + rect.w - line.width as i32,
            _ => rect.x,
        };
        let gaps = line.gaps.max(1) as i32;
        let extra = if style.text_align == Align::Stretch && !last {
            (rect.w - line.width as i32).max(0) / gaps
        } else {
            0
        };
        let y = top + index as i32 * typeset::line_height() as i32;
        let mut at = line.start;
        while at < line.end {
            let Some((ch, len)) = typeset::next_char(run, at) else {
                break;
            };
            canvas.draw_char(ch, x as isize, y as usize);
            x += Face::new().advance(ch) as i32;
            if ch == ' ' {
                x += extra;
            }
            at += len;
        }
        if paragraph.truncated && last {
            canvas.draw_char('…', x as isize, y as usize);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The declaration lives on the panel core's stack for the length of one page. This is
    /// the budget the panel's stack was sized against; a page that needs more nodes must fit
    /// them in it, not grow the array quietly.
    #[test]
    fn a_declaration_fits_the_stack_the_panel_core_was_given() {
        let bytes = core::mem::size_of::<Ui<'static, 'static>>();
        assert!(bytes <= 10 * 1024, "a declaration is {bytes} B");
    }
}
