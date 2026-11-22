//! A tappable label: filled, outlined or bare.

use iot_core::ui::{NodeId, Style, TextAlign, Ui, Widget};

use crate::shared::kit::state::State;
use crate::shared::kit::theme::{ButtonKind, Theme};

#[derive(Clone, Copy, Debug)]
pub struct Button<'a> {
    label: &'a [u8],
    kind: ButtonKind,
    state: State,
    width: i16,
    tag: u16,
    theme: &'a Theme,
}

impl<'a> Button<'a> {
    pub const fn new(label: &'a [u8]) -> Self {
        Self {
            label,
            kind: ButtonKind::Filled,
            state: State::NONE,
            width: 0,
            tag: 0,
            theme: &Theme::PANEL,
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
        self.theme = theme;
        self
    }
}

impl Widget for Button<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let mut box_style = self.theme.button(ui, self.kind, self.state).tag(self.tag);
        if self.width > 0 {
            box_style = box_style.width(self.width);
        }
        let ink = self.theme.on_button(ui, self.kind, self.state);
        let label = Style::new()
            .nowrap()
            .color(ink)
            .text_align(TextAlign::Center);
        ui.column(box_style, |ui| {
            ui.text(self.label, label);
        })
    }
}
