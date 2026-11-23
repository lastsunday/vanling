//! A one-line value with a border, and the same box with a choice arrow.

use iot_core::ui::{NodeId, Style, Ui, Widget};

use crate::shared::ui::state::State;
use crate::shared::ui::theme::Theme;

const RUN: usize = 128;

#[derive(Clone, Copy, Debug)]
pub struct TextField<'a> {
    value: &'a [u8],
    focused: bool,
    width: i16,
    tag: u16,
    theme: &'a Theme,
}

impl<'a> TextField<'a> {
    pub const fn new(value: &'a [u8]) -> Self {
        Self {
            value,
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

impl Widget for TextField<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let state = State::NONE.then(State::FOCUSED, self.focused);
        let mut box_style = self.theme.field(ui, state).tag(self.tag);
        if self.width > 0 {
            box_style = box_style.width(self.width);
        }
        let ink = self.theme.label(ui, state);
        let mut buf = [0u8; RUN];
        let mut len = self.value.len().min(RUN - 1);
        buf[..len].copy_from_slice(&self.value[..len]);
        if self.focused {
            buf[len] = b'|';
            len += 1;
        }
        ui.column(box_style, |ui| {
            ui.text(&buf[..len], Style::new().nowrap().color(ink));
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Select<'a> {
    value: &'a [u8],
    width: i16,
    tag: u16,
    theme: &'a Theme,
}

impl<'a> Select<'a> {
    pub const fn new(value: &'a [u8]) -> Self {
        Self {
            value,
            width: 0,
            tag: 0,
            theme: &Theme::PANEL,
        }
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

impl Widget for Select<'_> {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let mut box_style = self.theme.field(ui, State::NONE).tag(self.tag);
        if self.width > 0 {
            box_style = box_style.width(self.width);
        }
        let ink = self.theme.label(ui, State::NONE);
        let mut buf = [0u8; RUN];
        let mut len = self.value.len().min(RUN - 2);
        buf[..len].copy_from_slice(&self.value[..len]);
        buf[len..len + 2].copy_from_slice(b" v");
        len += 2;
        ui.column(box_style, |ui| {
            ui.text(&buf[..len], Style::new().nowrap().color(ink));
        })
    }
}
