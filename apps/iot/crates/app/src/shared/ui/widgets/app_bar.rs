//! A page head: the title, a gap, and one action on the trailing edge.

use iot_core::ui::panel::TOUCH_MIN;
use iot_core::ui::{FontFamily, Length, NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::ui::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub struct AppBar<'a> {
    title: &'a [u8],
    action: &'a [u8],
    tag: u16,
    theme: &'a Theme,
}

impl<'a> AppBar<'a> {
    pub const fn new(title: &'a [u8], action: &'a [u8]) -> Self {
        Self {
            title,
            action,
            tag: 0,
            theme: &Theme::PANEL,
        }
    }

    pub const fn tag(mut self, tag: u16) -> Self {
        self.tag = tag;
        self
    }

    pub const fn theme(mut self, theme: &'a Theme) -> Self {
        self.theme = theme;
        self
    }
}

impl Widget for AppBar<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme;
        let ink = ui.color(theme.ink);
        let muted = ui.color(theme.muted);
        let head = Style::new()
            .nowrap()
            .color(ink)
            .font_size(Length::Px(14))
            .font_family(FontFamily::Vector);
        let action = Style::new()
            .nowrap()
            .color(muted)
            .text_align(TextAlign::Center);
        ui.row(
            Style::new()
                .height(TOUCH_MIN as i16)
                .middle()
                .gap(6)
                .padding(2, 0, 2, 0),
            |ui| {
                ui.text(self.title, head);
                ui.node(Style::new().grow(1));
                ui.column(
                    Style::new()
                        .width(TOUCH_MIN as i16)
                        .height(TOUCH_MIN as i16)
                        .center()
                        .tag(self.tag),
                    |ui| {
                        ui.text(self.action, action);
                    },
                );
            },
        )
    }
}
