//! A labelled field with a hint line; the hint turns red when the value is rejected.

use iot_core::ui::{NodeId, Style, Ui, Widget};

use crate::shared::ui::state::State;
use crate::shared::ui::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub struct FormField<'a> {
    label: &'a [u8],
    value: &'a [u8],
    hint: &'a [u8],
    state: State,
    width: i16,
    tag: u16,
    theme: &'a Theme,
}

impl<'a> FormField<'a> {
    pub const fn new(label: &'a [u8], value: &'a [u8], hint: &'a [u8]) -> Self {
        Self {
            label,
            value,
            hint,
            state: State::NONE,
            width: 0,
            tag: 0,
            theme: &Theme::PANEL,
        }
    }

    pub const fn state(mut self, state: State) -> Self {
        self.state = state;
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
        self.theme = theme;
        self
    }
}

impl Widget for FormField<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme;
        let ink = theme.label(ui, self.state);
        let muted = ui.color(theme.muted);
        let hint = if self.state.has(State::ERROR) {
            ui.color(theme.danger)
        } else {
            muted
        };
        let mut input = theme.field(ui, self.state).tag(self.tag);
        if self.width > 0 {
            input = input.width(self.width);
        }
        ui.column(Style::new().gap(1), |ui| {
            ui.text(self.label, Style::new().nowrap().color(muted));
            ui.text(self.value, input.color(ink));
            ui.text(self.hint, Style::new().nowrap().color(hint));
        })
    }
}
