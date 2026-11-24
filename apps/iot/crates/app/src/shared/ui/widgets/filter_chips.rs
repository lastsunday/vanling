//! Three filters that latch on their own; the page holds which are on.

use iot_core::ui::panel::TOUCH_MIN;
use iot_core::ui::{NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::ui::theme::{CORNER, Theme};

#[derive(Clone, Copy, Debug)]
pub struct FilterChips<'a> {
    labels: [&'a [u8]; 3],
    tags: [u16; 3],
    on: u8,
    theme: &'a Theme,
}

impl<'a> FilterChips<'a> {
    pub const fn new(labels: [&'a [u8]; 3], tags: [u16; 3], on: u8) -> Self {
        Self {
            labels,
            tags,
            on,
            theme: &Theme::PANEL,
        }
    }

    pub const fn theme(mut self, theme: &'a Theme) -> Self {
        self.theme = theme;
        self
    }
}

impl Widget for FilterChips<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme;
        let edge = ui.color(theme.muted);
        let accent = ui.color(theme.accent);
        let paper = ui.color(theme.selection);
        ui.row(
            Style::new().height(TOUCH_MIN as i16).gap(4).stretch(),
            |ui| {
                for (index, label) in self.labels.into_iter().enumerate() {
                    let on = self.on & (1 << index) != 0;
                    let style = Style::new()
                        .grow(1)
                        .basis(0)
                        .center()
                        .ellipsis()
                        .radius(CORNER)
                        .bordered()
                        .tag(self.tags[index])
                        .text_align(TextAlign::Center)
                        .color(if on { accent } else { edge })
                        .background(if on { paper } else { ui.color(theme.ground) });
                    ui.text(label, style);
                }
            },
        )
    }
}
