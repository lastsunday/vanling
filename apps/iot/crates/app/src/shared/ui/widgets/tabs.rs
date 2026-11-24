//! Three tab labels in one row; the selected one takes the accent paper.

use iot_core::ui::panel::TOUCH_MIN;
use iot_core::ui::{NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::ui::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub struct Tabs<'a> {
    labels: [&'a [u8]; 3],
    tags: [u16; 3],
    selected: u8,
    theme: &'a Theme,
}

impl<'a> Tabs<'a> {
    pub const fn new(labels: [&'a [u8]; 3], tags: [u16; 3], selected: u8) -> Self {
        Self {
            labels,
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

impl Widget for Tabs<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme;
        let muted = ui.color(theme.muted);
        let on_paper = ui.color(theme.on_accent);
        let paper = ui.color(theme.accent);
        ui.row(Style::new().height(TOUCH_MIN as i16).gap(2), |ui| {
            for (index, label) in self.labels.into_iter().enumerate() {
                let on = index == usize::from(self.selected);
                let style = Style::new()
                    .grow(1)
                    .basis(0)
                    .center()
                    .nowrap()
                    .radius(3)
                    .tag(self.tags[index])
                    .text_align(TextAlign::Center)
                    .background(if on { paper } else { ui.color(theme.ground) })
                    .color(if on { on_paper } else { muted });
                ui.text(label, style);
            }
        })
    }
}
