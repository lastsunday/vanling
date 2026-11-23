//! The panel's palette and the style each widget starts from.

use iot_core::ui::panel::TOUCH_MIN;
use iot_core::ui::{Color, ColorId, Style, TextAlign, Ui};

use super::state::State;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonKind {
    #[default]
    Filled,
    Outlined,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub ground: Color,
    pub ink: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent_pressed: Color,
    pub on_accent: Color,
    pub danger: Color,
    pub selection: Color,
    pub gap: i16,
}

impl Theme {
    pub const PANEL: Self = Self {
        ground: Color::rgb(0x00, 0x00, 0x00),
        ink: Color::rgb(0xFF, 0xFF, 0xFF),
        muted: Color::rgb(0x88, 0x88, 0x88),
        accent: Color::rgb(0x4C, 0xB8, 0xFF),
        accent_pressed: Color::rgb(0x2C, 0x7C, 0xB8),
        on_accent: Color::rgb(0x00, 0x00, 0x00),
        danger: Color::rgb(0xFF, 0x5A, 0x5A),
        selection: Color::rgba(0xFF, 0xFF, 0xFF, 0x30),
        gap: 4,
    };

    pub fn ink(&self, ui: &mut Ui<'_, '_>) -> ColorId {
        ui.color(self.ink)
    }

    pub fn muted(&self, ui: &mut Ui<'_, '_>) -> ColorId {
        ui.color(self.muted)
    }

    pub fn accent(&self, ui: &mut Ui<'_, '_>) -> ColorId {
        ui.color(self.accent)
    }

    pub fn danger(&self, ui: &mut Ui<'_, '_>) -> ColorId {
        ui.color(self.danger)
    }

    /// The button's own box: paper, outline and the smallest touch target.
    pub fn button(&self, ui: &mut Ui<'_, '_>, kind: ButtonKind, state: State) -> Style {
        let style = Style::new()
            .min_width(TOUCH_MIN as i16)
            .height(TOUCH_MIN as i16)
            .pad(6)
            .center()
            .nowrap();
        if state.has(State::DISABLED) {
            return match kind {
                ButtonKind::Filled => style.background(ui.color(self.muted)),
                ButtonKind::Outlined => style.bordered().color(ui.color(self.muted)),
                ButtonKind::Text => style,
            };
        }
        let active = state.has(State::PRESSED) || state.has(State::SELECTED);
        match kind {
            ButtonKind::Filled => {
                let paper = if active {
                    self.accent_pressed
                } else {
                    self.accent
                };
                style.background(ui.color(paper))
            }
            ButtonKind::Outlined => {
                let style = style.bordered().color(ui.color(self.accent));
                if active {
                    style.background(ui.color(self.selection))
                } else {
                    style
                }
            }
            ButtonKind::Text => {
                if active {
                    style.background(ui.color(self.selection))
                } else {
                    style
                }
            }
        }
    }

    /// The label's colour, resolved against the same state as [`Self::button`].
    pub fn on_button(&self, ui: &mut Ui<'_, '_>, kind: ButtonKind, state: State) -> ColorId {
        if state.has(State::DISABLED) {
            // A disabled filled button sits on `muted`; grey on grey loses its name.
            return match kind {
                ButtonKind::Filled => ui.color(self.on_accent),
                ButtonKind::Outlined | ButtonKind::Text => ui.color(self.muted),
            };
        }
        match kind {
            ButtonKind::Filled => ui.color(self.on_accent),
            ButtonKind::Outlined | ButtonKind::Text => ui.color(self.accent),
        }
    }

    pub fn field(&self, ui: &mut Ui<'_, '_>, state: State) -> Style {
        let style = Style::new().height(TOUCH_MIN as i16).pad(3).nowrap();
        if state.has(State::ERROR) {
            style.bordered().color(ui.color(self.danger))
        } else if state.has(State::FOCUSED) {
            style.bordered().color(ui.color(self.accent))
        } else {
            style.bordered().color(ui.color(self.muted))
        }
    }

    pub fn label(&self, ui: &mut Ui<'_, '_>, state: State) -> ColorId {
        if state.has(State::DISABLED) {
            ui.color(self.muted)
        } else {
            ui.color(self.ink)
        }
    }

    pub fn text_align() -> TextAlign {
        TextAlign::Center
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iot_core::ui::{BoxConstraints, Canvas, PX, Screen};

    #[test]
    fn a_pressed_filled_button_changes_paper_not_ink() {
        let mut frame = [0u8; 80 * 60 * 2];
        let mut canvas = Canvas::new(&mut frame, 80, 60, None);
        let mut ui = Ui::new(&mut canvas, Screen::default());
        let theme = Theme::PANEL;
        let idle = theme.button(&mut ui, ButtonKind::Filled, State::NONE);
        let pressed = theme.button(&mut ui, ButtonKind::Filled, State::PRESSED);
        assert_ne!(idle.background_color, pressed.background_color);
        assert_eq!(
            theme.on_button(&mut ui, ButtonKind::Filled, State::NONE),
            theme.on_button(&mut ui, ButtonKind::Filled, State::PRESSED)
        );
        ui.layout(BoxConstraints::tight(80 * PX, 60 * PX));
    }
}
