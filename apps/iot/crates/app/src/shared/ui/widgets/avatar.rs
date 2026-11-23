//! A short mark in a circle: an initial or a role.

use iot_core::ui::{NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::ui::theme::Theme;

const SIZE: i16 = 24;

#[derive(Clone, Copy, Debug)]
pub struct Avatar<'a> {
    text: &'a [u8],
    theme: &'a Theme,
}

impl<'a> Avatar<'a> {
    pub const fn new(text: &'a [u8]) -> Self {
        Self {
            text,
            theme: &Theme::PANEL,
        }
    }

    pub const fn theme(mut self, theme: &'a Theme) -> Self {
        self.theme = theme;
        self
    }
}

impl Widget for Avatar<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let paper = ui.color(self.theme.muted);
        let ink = ui.color(self.theme.ground);
        ui.text(
            self.text,
            Style::new()
                .size(SIZE, SIZE)
                .radius(SIZE as u8 / 2)
                .text_align(TextAlign::Center)
                .background(paper)
                .nowrap()
                .color(ink),
        )
    }
}
