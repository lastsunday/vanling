//! Switch, checkbox and radio: the same two-node shape with a different mark.

use iot_core::ui::{NodeId, Style, Ui, Widget};

use crate::shared::ui::state::State;
use crate::shared::ui::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub struct Switch {
    on: bool,
    state: State,
    tag: u16,
    theme: Option<&'static Theme>,
}

impl Switch {
    pub const fn new(on: bool) -> Self {
        Self {
            on,
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

impl Widget for Switch {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme.unwrap_or_else(|| Theme::of(ui.screen().theme));
        let off = self.state.has(State::DISABLED);
        let track = if off {
            theme.muted
        } else if self.on {
            theme.accent
        } else {
            theme.muted
        };
        let track = ui.color(track);
        let knob = ui.color(if off { theme.muted } else { theme.ink });
        let left = if self.on { 20 } else { 2 };
        ui.stack(
            Style::new()
                .width(36)
                .height(20)
                .background(track)
                .bordered()
                .tag(self.tag),
            |ui| {
                ui.node(
                    Style::new()
                        .absolute()
                        .left(left)
                        .top(2)
                        .size(14, 14)
                        .background(knob),
                );
            },
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Checkbox {
    checked: bool,
    state: State,
    tag: u16,
    theme: Option<&'static Theme>,
}

impl Checkbox {
    pub const fn new(checked: bool) -> Self {
        Self {
            checked,
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

impl Widget for Checkbox {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme.unwrap_or_else(|| Theme::of(ui.screen().theme));
        let disabled = self.state.has(State::DISABLED);
        let edge = ui.color(if disabled {
            theme.muted
        } else if self.checked {
            theme.accent
        } else {
            theme.muted
        });
        let mark = ui.color(if disabled { theme.muted } else { theme.accent });
        ui.stack(
            Style::new()
                .size(16, 16)
                .bordered()
                .color(edge)
                .tag(self.tag),
            |ui| {
                if self.checked {
                    ui.text(
                        b"x",
                        Style::new().absolute().left(4).top(1).nowrap().color(mark),
                    );
                }
            },
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Radio {
    selected: bool,
    state: State,
    tag: u16,
    theme: Option<&'static Theme>,
}

impl Radio {
    pub const fn new(selected: bool) -> Self {
        Self {
            selected,
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

impl Widget for Radio {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let theme = self.theme.unwrap_or_else(|| Theme::of(ui.screen().theme));
        let disabled = self.state.has(State::DISABLED);
        let edge = ui.color(if disabled {
            theme.muted
        } else if self.selected {
            theme.accent
        } else {
            theme.muted
        });
        let dot = ui.color(if disabled { theme.muted } else { theme.accent });
        ui.stack(
            Style::new()
                .size(16, 16)
                .bordered()
                .color(edge)
                .tag(self.tag),
            |ui| {
                if self.selected {
                    ui.node(
                        Style::new()
                            .absolute()
                            .left(5)
                            .top(5)
                            .size(6, 6)
                            .background(dot),
                    );
                }
            },
        )
    }
}
