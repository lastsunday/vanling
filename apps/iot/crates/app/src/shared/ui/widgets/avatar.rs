//! A short mark in a circle: an initial or a role.

use iot_core::ui::{NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::ui::theme::Theme;

const SIZE: i16 = 24;

#[derive(Clone, Copy, Debug)]
pub struct Avatar<'a> {
    text: &'a [u8],
    theme: Option<&'a Theme>,
}

impl<'a> Avatar<'a> {
    pub const fn new(text: &'a [u8]) -> Self {
        Self { text, theme: None }
    }

    pub const fn theme(mut self, theme: &'a Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl Widget for Avatar<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme.unwrap_or_else(|| Theme::of(ui.screen().theme));
        let paper = ui.color(theme.muted);
        let ink = ui.color(theme.ground);
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
