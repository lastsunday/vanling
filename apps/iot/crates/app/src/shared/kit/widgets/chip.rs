//! A rounded tag, optionally selected.

use iot_core::ui::{FontFamily, Length, NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::kit::theme::{CORNER, Theme};

#[derive(Clone, Copy, Debug)]
pub struct Chip<'a> {
    text: &'a [u8],
    selected: bool,
    width: i16,
    height: i16,
    tag: u16,
    theme: &'a Theme,
}

impl<'a> Chip<'a> {
    pub const fn new(text: &'a [u8]) -> Self {
        Self {
            text,
            selected: false,
            width: 0,
            height: 0,
            tag: 0,
            theme: &Theme::PANEL,
        }
    }

    pub const fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    pub const fn height(mut self, height: i16) -> Self {
        self.height = height;
        self
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

impl Widget for Chip<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let edge = if self.selected {
            self.theme.accent
        } else {
            self.theme.muted
        };
        let edge = ui.color(edge);
        let mut box_style = Style::new()
            .radius(CORNER)
            .pad(3)
            .center()
            .bordered()
            .color(edge)
            .tag(self.tag);
        if self.width > 0 {
            box_style = box_style.width(self.width);
        }
        if self.height > 0 {
            box_style = box_style.height(self.height);
        }
        if self.selected {
            let paper = ui.color(self.theme.selection);
            box_style = box_style.background(paper);
        }
        let ink = ui.color(self.theme.ink);
        let label = Style::new()
            .nowrap()
            .color(ink)
            .text_align(TextAlign::Center)
            .font_size(Length::Px(14))
            .font_family(FontFamily::Vector);
        ui.column(box_style, |ui| {
            ui.text(self.text, label);
        })
    }
}
