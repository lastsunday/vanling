//! A standing notice with one action; the page decides when it goes away.

use iot_core::ui::panel::TOUCH_MIN;
use iot_core::ui::{NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::ui::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub struct Banner<'a> {
    message: &'a [u8],
    action: &'a [u8],
    tag: u16,
    theme: Option<&'a Theme>,
}

impl<'a> Banner<'a> {
    pub const fn new(message: &'a [u8], action: &'a [u8]) -> Self {
        Self {
            message,
            action,
            tag: 0,
            theme: None,
        }
    }

    pub const fn tag(mut self, tag: u16) -> Self {
        self.tag = tag;
        self
    }

    pub const fn theme(mut self, theme: &'a Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl Widget for Banner<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme.unwrap_or_else(|| Theme::of(ui.screen().theme));
        let ink = ui.color(theme.ink);
        let accent = ui.color(theme.accent);
        let paper = ui.color(theme.selection);
        let mark = Style::new().nowrap().color(accent);
        ui.row(
            Style::new()
                .height(TOUCH_MIN as i16)
                .middle()
                .gap(4)
                .pad(3)
                .radius(3)
                .background(paper),
            |ui| {
                ui.text(b"!", mark);
                ui.text(self.message, Style::new().ellipsis().color(ink));
                ui.node(Style::new().grow(1));
                ui.text(
                    self.action,
                    Style::new()
                        .width(TOUCH_MIN as i16)
                        .height(TOUCH_MIN as i16)
                        .center()
                        .ellipsis()
                        .color(accent)
                        .text_align(TextAlign::Center)
                        .tag(self.tag),
                );
            },
        )
    }
}
