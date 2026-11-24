//! A search box: a mark, the query, and the border a focused field takes.

use iot_core::ui::{NodeId, Style, Ui, Widget};

use crate::shared::ui::state::State;
use crate::shared::ui::theme::{CORNER, Theme};

const RUN: usize = 96;

#[derive(Clone, Copy, Debug)]
pub struct SearchField<'a> {
    value: &'a [u8],
    placeholder: &'a [u8],
    focused: bool,
    width: i16,
    tag: u16,
    theme: &'a Theme,
}

impl<'a> SearchField<'a> {
    pub const fn new(value: &'a [u8], placeholder: &'a [u8]) -> Self {
        Self {
            value,
            placeholder,
            focused: false,
            width: 0,
            tag: 0,
            theme: &Theme::PANEL,
        }
    }

    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
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

impl Widget for SearchField<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let state = State::NONE.then(State::FOCUSED, self.focused);
        let theme = self.theme;
        let mut box_style = theme.field(ui, state).tag(self.tag).radius(CORNER);
        if self.width > 0 {
            box_style = box_style.width(self.width);
        }
        let ink = theme.label(ui, state);
        let muted = ui.color(theme.muted);
        let mut buf = [0u8; RUN];
        let text = if self.value.is_empty() {
            self.placeholder
        } else {
            self.value
        };
        let mut len = text.len().min(RUN - 1);
        buf[..len].copy_from_slice(&text[..len]);
        if self.focused {
            buf[len] = b'|';
            len += 1;
        }
        ui.row(box_style.middle().gap(4), |ui| {
            ui.text(b"?", Style::new().nowrap().color(muted));
            ui.text(&buf[..len], Style::new().nowrap().color(ink));
        })
    }
}
