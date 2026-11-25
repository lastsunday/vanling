//! A number with a minus and a plus; each button carries its own tag.

use iot_core::ui::panel::TOUCH_MIN;
use iot_core::ui::text::format_u16;
use iot_core::ui::{NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::ui::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub struct NumberField {
    value: u16,
    tags: [u16; 2],
    width: i16,
    theme: Option<&'static Theme>,
}

impl NumberField {
    pub const fn new(value: u16, tags: [u16; 2]) -> Self {
        Self {
            value,
            tags,
            width: 0,
            theme: None,
        }
    }

    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }
}

impl Widget for NumberField {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme.unwrap_or_else(|| Theme::of(ui.screen().theme));
        let edge = ui.color(theme.muted);
        let fg = ui.color(theme.foreground);
        let mut digits = [0u8; 6];
        let mut style = Style::new()
            .height(TOUCH_MIN as i16)
            .bordered()
            .color(edge)
            .radius(3)
            .middle()
            .gap(2);
        if self.width > 0 {
            style = style.width(self.width);
        }
        ui.row(style, |ui| {
            ui.text(
                b"-",
                Style::new()
                    .width(TOUCH_MIN as i16)
                    .center()
                    .nowrap()
                    .tag(self.tags[0])
                    .text_align(TextAlign::Center)
                    .color(fg),
            );
            ui.text(
                format_u16(self.value, &mut digits),
                Style::new()
                    .grow(1)
                    .basis(0)
                    .nowrap()
                    .text_align(TextAlign::Center)
                    .color(fg),
            );
            ui.text(
                b"+",
                Style::new()
                    .width(TOUCH_MIN as i16)
                    .center()
                    .nowrap()
                    .tag(self.tags[1])
                    .text_align(TextAlign::Center)
                    .color(fg),
            );
        })
    }
}
