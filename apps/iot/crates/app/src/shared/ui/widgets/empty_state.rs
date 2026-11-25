//! What a box says when it has nothing to show, or the load failed.

use iot_core::ui::{NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::ui::theme::Theme;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Empty,
    Error,
}

#[derive(Clone, Copy, Debug)]
pub struct EmptyState<'a> {
    title: &'a [u8],
    hint: &'a [u8],
    tone: Tone,
    width: i16,
    tag: u16,
    theme: Option<&'a Theme>,
}

impl<'a> EmptyState<'a> {
    pub const fn new(title: &'a [u8], hint: &'a [u8]) -> Self {
        Self {
            title,
            hint,
            tone: Tone::Empty,
            width: 0,
            tag: 0,
            theme: None,
        }
    }

    pub const fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
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

impl Widget for EmptyState<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme.unwrap_or_else(|| Theme::of(ui.screen().theme));
        let paper = if self.tone == Tone::Error {
            ui.color(theme.danger)
        } else {
            ui.color(theme.muted)
        };
        let fg = ui.color(theme.foreground);
        let muted = ui.color(theme.muted);
        let mut style = Style::new().gap(2).center().tag(self.tag);
        if self.width > 0 {
            style = style.width(self.width);
        }
        ui.column(style, |ui| {
            ui.text(
                b"[]",
                Style::new()
                    .nowrap()
                    .color(paper)
                    .text_align(TextAlign::Center),
            );
            ui.text(
                self.title,
                Style::new()
                    .ellipsis()
                    .color(fg)
                    .text_align(TextAlign::Center),
            );
            ui.text(
                self.hint,
                Style::new()
                    .ellipsis()
                    .color(muted)
                    .text_align(TextAlign::Center),
            );
        })
    }
}
