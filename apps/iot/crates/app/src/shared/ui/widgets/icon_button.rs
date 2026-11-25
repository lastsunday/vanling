//! A single glyph with a button's touch target.

use iot_core::ui::{NodeId, Style, Ui, Widget};

use crate::shared::ui::state::State;
use crate::shared::ui::theme::{ButtonKind, Theme};

#[derive(Clone, Copy, Debug)]
pub struct IconButton {
    glyph: char,
    state: State,
    tag: u16,
    theme: Option<&'static Theme>,
}

impl IconButton {
    pub const fn new(glyph: char) -> Self {
        Self {
            glyph,
            state: State::NONE,
            tag: 0,
            theme: None,
        }
    }

    pub const fn state(mut self, state: State) -> Self {
        self.state = state;
        self
    }

    pub const fn tag(mut self, tag: u16) -> Self {
        self.tag = tag;
        self
    }
}

impl Widget for IconButton {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme.unwrap_or_else(|| Theme::of(ui.screen().theme));
        let mut buf = [0u8; 4];
        let run = self.glyph.encode_utf8(&mut buf).as_bytes();
        let box_style = theme.button(ui, ButtonKind::Text, self.state).tag(self.tag);
        let fg = theme.on_button(ui, ButtonKind::Text, self.state);
        ui.column(box_style, |ui| {
            ui.text(run, Style::new().nowrap().color(fg));
        })
    }
}
