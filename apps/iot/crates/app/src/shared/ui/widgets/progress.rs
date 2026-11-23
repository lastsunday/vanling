//! A track and the share of it a value fills.

use iot_core::ui::{NodeId, Style, Ui, Widget};

use crate::shared::ui::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub struct ProgressBar {
    value: u8,
    width: i16,
    theme: &'static Theme,
}

impl ProgressBar {
    pub const fn new(value: u8) -> Self {
        Self {
            value,
            width: 0,
            theme: &Theme::PANEL,
        }
    }

    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }
}

impl Widget for ProgressBar {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let width = if self.width > 0 {
            self.width
        } else {
            ui.width() as i16
        };
        let filled = i32::from(width) * i32::from(self.value.min(100)) / 100;
        let track = ui.color(self.theme.muted);
        let paper = ui.color(self.theme.accent);
        ui.stack(
            Style::new().width(width).height(6).background(track),
            |ui| {
                ui.node(
                    Style::new()
                        .width(filled as i16)
                        .height(6)
                        .background(paper),
                );
            },
        )
    }
}
