//! A track and a knob the page places by value.

use iot_core::ui::{NodeId, Style, Ui, Widget};

use crate::shared::kit::state::State;
use crate::shared::kit::theme::Theme;

const KNOB: i16 = 8;

#[derive(Clone, Copy, Debug)]
pub struct Slider {
    value: u8,
    width: i16,
    state: State,
    tag: u16,
    theme: &'static Theme,
}

impl Slider {
    pub const fn new(value: u8) -> Self {
        Self {
            value,
            width: 0,
            state: State::NONE,
            tag: 0,
            theme: &Theme::PANEL,
        }
    }

    pub const fn width(mut self, width: i16) -> Self {
        self.width = width;
        self
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

impl Widget for Slider {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let width = if self.width > 0 {
            self.width
        } else {
            ui.width() as i16
        };
        let travel = (width - KNOB).max(1);
        let left = (i32::from(travel) * i32::from(self.value.min(100)) / 100) as i16;
        let track = ui.color(self.theme.muted);
        let knob = ui.color(if self.state.has(State::PRESSED) {
            self.theme.accent_pressed
        } else {
            self.theme.accent
        });
        ui.stack(Style::new().width(width).height(20).tag(self.tag), |ui| {
            ui.node(
                Style::new()
                    .absolute()
                    .left(0)
                    .top(9)
                    .width(width)
                    .height(2)
                    .background(track),
            );
            ui.node(
                Style::new()
                    .absolute()
                    .left(left)
                    .top(2)
                    .size(KNOB, 16)
                    .background(knob),
            );
        })
    }
}
