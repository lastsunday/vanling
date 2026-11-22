//! A one-pixel rule between blocks.

use iot_core::ui::{NodeId, Style, Ui, Widget};

use crate::shared::kit::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub struct Divider {
    width: i16,
    theme: &'static Theme,
}

impl Divider {
    pub const fn new() -> Self {
        Self {
            width: 0,
            theme: &Theme::PANEL,
        }
    }

    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }
}

impl Default for Divider {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Divider {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let width = if self.width > 0 {
            self.width
        } else {
            ui.width() as i16
        };
        let ink = ui.color(self.theme.muted);
        ui.node(Style::new().width(width).height(1).background(ink))
    }
}
