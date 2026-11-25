//! A floating line of help; the page places it.

use iot_core::ui::{NodeId, Style, Ui, Widget};

use crate::shared::ui::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub struct Tooltip<'a> {
    text: &'a [u8],
    theme: Option<&'a Theme>,
}

impl<'a> Tooltip<'a> {
    pub const fn new(text: &'a [u8]) -> Self {
        Self { text, theme: None }
    }

    pub const fn theme(mut self, theme: &'a Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl Widget for Tooltip<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme.unwrap_or_else(|| Theme::of(ui.screen().theme));
        let paper = ui.color(theme.ground);
        let edge = ui.color(theme.accent);
        let fg = ui.color(theme.foreground);
        ui.column(
            Style::new()
                .pad(2)
                .center()
                .bordered()
                .background(paper)
                .color(edge),
            |ui| {
                ui.text(self.text, Style::new().nowrap().color(fg));
            },
        )
    }
}
