//! A short notice that sits over the page until the page takes it down.

use iot_core::ui::{NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::ui::theme::{CORNER, Theme};

#[derive(Clone, Copy, Debug)]
pub struct Toast<'a> {
    text: &'a [u8],
    width: i16,
    height: i16,
    tag: u16,
    theme: Option<&'a Theme>,
}

impl<'a> Toast<'a> {
    pub const fn new(text: &'a [u8]) -> Self {
        Self {
            text,
            width: 0,
            height: 0,
            tag: 0,
            theme: None,
        }
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
        self.theme = Some(theme);
        self
    }
}

impl Widget for Toast<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme.unwrap_or_else(|| Theme::of(ui.screen().theme));
        let paper = ui.color(theme.muted);
        let ink = ui.color(theme.ground);
        let mut style = Style::new()
            .nowrap()
            .radius(CORNER)
            .pad(4)
            .center()
            .tag(self.tag)
            .text_align(TextAlign::Center)
            .background(paper)
            .color(ink);
        if self.width > 0 {
            style = style.width(self.width);
        }
        if self.height > 0 {
            style = style.height(self.height);
        }
        ui.text(self.text, style)
    }
}
