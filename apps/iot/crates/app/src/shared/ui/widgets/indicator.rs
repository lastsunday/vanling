//! A mark on the corner of another element: Mantine's `Indicator`.

use iot_core::ui::{NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::ui::theme::Theme;

/// The dot's diameter and how far its corner reaches past the child.
const DOT: i16 = 8;
const REACH: i16 = 2;
/// A numbered mark's height: one 12 px line plus a pixel of padding either side.
const PILL: i16 = 14;

#[derive(Clone, Copy, Debug)]
pub struct Indicator<'a, W> {
    child: W,
    label: Option<&'a [u8]>,
    theme: Option<&'a Theme>,
}

impl<'a, W> Indicator<'a, W> {
    pub const fn new(child: W) -> Self {
        Self {
            child,
            label: None,
            theme: None,
        }
    }

    /// The count or word inside the mark; without it the mark is a plain dot.
    pub const fn label(mut self, label: &'a [u8]) -> Self {
        self.label = Some(label);
        self
    }

    pub const fn theme(mut self, theme: &'a Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl<W: Widget> Widget for Indicator<'_, W> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme.unwrap_or_else(|| Theme::of(ui.screen().theme));
        let paper = ui.color(theme.accent);
        ui.stack(Style::new(), |ui| {
            ui.add(self.child);
            let at = Style::new().absolute().right(-REACH).top(-REACH);
            match self.label {
                Some(label) => {
                    let ink = ui.color(theme.on_accent);
                    ui.text(
                        label,
                        at.height(PILL)
                            .min_width(PILL)
                            .pad(1)
                            .radius(PILL as u8 / 2)
                            .nowrap()
                            .text_align(TextAlign::Center)
                            .background(paper)
                            .color(ink),
                    );
                }
                None => {
                    ui.node(at.size(DOT, DOT).radius(DOT as u8 / 2).background(paper));
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::ui::widgets::Avatar;
    use iot_core::ui::{BoxConstraints, Canvas, PX, Screen};

    #[test]
    fn a_numbered_mark_widens_into_a_pill_at_the_same_corner() {
        let mut frame = [0u8; 60 * 40 * 2];
        let mut canvas = Canvas::new(&mut frame, 60, 40, None);
        let mut ui = Ui::new(&mut canvas, Screen::default());
        let mut holder = NodeId::NONE;
        ui.row(Style::new(), |ui| {
            holder = ui.add(Indicator::new(Avatar::new(b"V")).label(b"123"));
        });
        ui.paint(BoxConstraints::tight(60 * PX, 40 * PX), &mut |_, _, _| {});
        let child = ui.rect(holder);
        let pill = ui.rects().last().expect("the mark is the last box");
        let dot = 8;
        assert_eq!(pill.h, 14, "one line plus a pixel either side");
        assert!(
            pill.w > pill.h,
            "a longer count widens the mark into a pill"
        );
        assert!(pill.w > dot, "a number needs more room than a dot");
        assert_eq!(pill.right(), child.right() + 2);
        assert_eq!(pill.y, child.y - 2);
    }

    #[test]
    fn the_dot_sits_past_the_child_s_top_right_corner() {
        let mut frame = [0u8; 60 * 40 * 2];
        let mut canvas = Canvas::new(&mut frame, 60, 40, None);
        let mut ui = Ui::new(&mut canvas, Screen::default());
        let mut holder = NodeId::NONE;
        ui.row(Style::new(), |ui| {
            holder = ui.add(Indicator::new(Avatar::new(b"V")));
        });
        ui.paint(BoxConstraints::tight(60 * PX, 40 * PX), &mut |_, _, _| {});
        let child = ui.rect(holder);
        let dot = ui.rects().last().expect("the dot is the last box");
        assert_eq!((child.w, child.h), (24, 24));
        assert_eq!((dot.w, dot.h), (8, 8), "the dot is its own small box");
        assert_eq!(dot.x, child.right() - 8 + 2);
        assert_eq!(dot.y, child.y - 2);
    }
}
