//! Three destinations in a standing bar; the current one wears an inset accent pill.

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
        let strip = ui.color(theme.surface);
        let on_pill = ui.color(theme.on_accent);
        let muted = ui.color(theme.muted);
        let accent = ui.color(theme.accent);
        ui.row(
            Style::new()
                .height(TOUCH_MIN as i16)
                .gap(2)
                .pad(4)
                .stretch()
                .background(strip),
            |ui| {
                for (index, item) in self.items.into_iter().enumerate() {
                    let on = index == usize::from(self.selected);
                    let style = Style::new()
                        .grow(1)
                        .basis(0)
                        .center()
                        .nowrap()
                        .radius(6)
                        .tag(self.tags[index])
                        .color(if on { on_pill } else { muted })
                        .background(if on { accent } else { strip })
                        .text_align(TextAlign::Center);
                    ui.text(item, style);
                }
            },
        )
    }
}
