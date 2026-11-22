//! A short mark in a square: an initial or a role.

use iot_core::ui::{NodeId, Style, Ui, Widget};

use crate::shared::kit::theme::Theme;

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
        let edge = ui.color(self.theme.muted);
        let ink = ui.color(self.theme.ink);
        ui.column(
            Style::new().size(24, 24).center().bordered().color(edge),
            |ui| {
                ui.text(self.text, Style::new().nowrap().color(ink));
            },
        )
    }
}
