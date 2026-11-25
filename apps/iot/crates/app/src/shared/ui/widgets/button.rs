//! A tappable label: filled, outlined or bare.

use iot_core::ui::{FontFamily, Length, NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::ui::state::State;
use crate::shared::ui::theme::{ButtonKind, Theme};

#[derive(Clone, Copy, Debug)]
pub struct Button<'a> {
    label: &'a [u8],
    kind: ButtonKind,
    state: State,
    width: i16,
    tag: u16,
    theme: Option<&'a Theme>,
}

impl<'a> Button<'a> {
    pub const fn new(label: &'a [u8]) -> Self {
        Self {
            label,
            kind: ButtonKind::Filled,
            state: State::NONE,
            width: 0,
            tag: 0,
            theme: None,
        }
    }

    pub const fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }

    pub const fn state(mut self, state: State) -> Self {
        self.state = state;
        self
    }

    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
    }

    /// Marks the box so a page can find it in the laid-out declaration.
    pub const fn tag(mut self, tag: u16) -> Self {
        self.tag = tag;
        self
    }

    pub const fn theme(mut self, theme: &'a Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl Widget for Button<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme.unwrap_or_else(|| Theme::of(ui.screen().theme));
        let mut box_style = theme.button(ui, self.kind, self.state).tag(self.tag);
        if self.width > 0 {
            box_style = box_style.width(self.width);
        }
        let fg = theme.on_button(ui, self.kind, self.state);
        let label = Style::new()
            .ellipsis()
            .color(fg)
            .text_align(TextAlign::Center)
            .font_size(Length::Px(14))
            .font_family(FontFamily::Vector);
        ui.column(box_style, |ui| {
            ui.text(self.label, label);
        })
    }
}
