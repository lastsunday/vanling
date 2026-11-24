//! Three destinations, the current one tinted: labels only, so the bar costs four nodes.

use iot_core::ui::panel::TOUCH_MIN;
use iot_core::ui::{NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::ui::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub struct NavigationBar<'a> {
    items: [&'a [u8]; 3],
    tags: [u16; 3],
    selected: u8,
    theme: &'a Theme,
}

impl<'a> NavigationBar<'a> {
    pub const fn new(items: [&'a [u8]; 3], tags: [u16; 3], selected: u8) -> Self {
        Self {
            items,
            tags,
            selected,
            theme: &Theme::PANEL,
        }
    }

    pub const fn theme(mut self, theme: &'a Theme) -> Self {
        self.theme = theme;
        self
    }
}

impl Widget for NavigationBar<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme;
        let ink = ui.color(theme.ink);
        let muted = ui.color(theme.muted);
        let paper = ui.color(theme.selection);
        ui.row(Style::new().height(TOUCH_MIN as i16).gap(2), |ui| {
            for (index, item) in self.items.into_iter().enumerate() {
                let on = index == usize::from(self.selected);
                let style = Style::new()
                    .grow(1)
                    .basis(0)
                    .center()
                    .nowrap()
                    .radius(3)
                    .tag(self.tags[index])
                    .color(if on { ink } else { muted })
                    .background(if on { paper } else { ui.color(theme.ground) })
                    .text_align(TextAlign::Center);
                ui.text(item, style);
            }
        })
    }
}
