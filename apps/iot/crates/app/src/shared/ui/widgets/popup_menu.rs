//! A short list of verbs that hangs under its anchor; the page decides where and why.

use iot_core::ui::{NodeId, Style, Ui, Widget};

use crate::shared::ui::theme::{CORNER, Theme};

#[derive(Clone, Copy, Debug)]
pub struct PopupMenu<'a> {
    items: [&'a [u8]; 3],
    tags: [u16; 3],
    selected: u8,
    width: i16,
    theme: &'a Theme,
}

impl<'a> PopupMenu<'a> {
    pub const fn new(items: [&'a [u8]; 3], tags: [u16; 3], selected: u8) -> Self {
        Self {
            items,
            tags,
            selected,
            width: 0,
            theme: &Theme::PANEL,
        }
    }

    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    pub const fn theme(mut self, theme: &'a Theme) -> Self {
        self.theme = theme;
        self
    }
}

impl Widget for PopupMenu<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme;
        let edge = ui.color(theme.muted);
        let ink = ui.color(theme.ink);
        let paper = ui.color(theme.selection);
        let ground = ui.color(theme.ground);
        let mut style = Style::new()
            .bordered()
            .color(edge)
            .radius(CORNER)
            .pad(2)
            .gap(1);
        if self.width > 0 {
            style = style.width(self.width);
        }
        ui.column(style, |ui| {
            for (index, item) in self.items.into_iter().enumerate() {
                let on = index == usize::from(self.selected);
                ui.text(
                    item,
                    Style::new()
                        .nowrap()
                        .pad(2)
                        .tag(self.tags[index])
                        .background(if on { paper } else { ground })
                        .color(ink),
                );
            }
        })
    }
}
