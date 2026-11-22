//! One row of a list, drawn as a cell: four text slots and a selected fill.

use iot_core::ui::panel::ROW_PITCH;
use iot_core::ui::{Canvas, Rect};

use crate::shared::kit::slot::Slot;
use crate::shared::kit::state::State;
use crate::shared::kit::theme::Theme;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListTile<'a> {
    pub title: &'a [u8],
    pub subtitle: Option<&'a [u8]>,
    pub leading: Slot<'a>,
    pub trailing: Slot<'a>,
    pub state: State,
    pub dense: bool,
}

impl<'a> ListTile<'a> {
    pub const fn new(title: &'a [u8]) -> Self {
        Self {
            title,
            subtitle: None,
            leading: Slot::None,
            trailing: Slot::None,
            state: State::NONE,
            dense: false,
        }
    }

    pub const fn subtitle(mut self, run: &'a [u8]) -> Self {
        self.subtitle = Some(run);
        self
    }

    pub const fn leading(mut self, slot: Slot<'a>) -> Self {
        self.leading = slot;
        self
    }

    pub const fn trailing(mut self, slot: Slot<'a>) -> Self {
        self.trailing = slot;
        self
    }

    pub const fn state(mut self, state: State) -> Self {
        self.state = state;
        self
    }

    pub const fn dense(mut self, dense: bool) -> Self {
        self.dense = dense;
        self
    }

    pub const fn height(self) -> i32 {
        if self.dense {
            ROW_PITCH as i32
        } else {
            2 * ROW_PITCH as i32
        }
    }

    pub fn draw(self, canvas: &mut Canvas<'_>, rect: Rect, theme: &Theme) {
        if self.state.has(State::SELECTED) {
            canvas.set_color(Some(theme.selection));
            canvas.fill_rect(rect);
        }
        let ink = if self.state.has(State::DISABLED) {
            theme.muted
        } else {
            theme.ink
        };
        let mut left = rect.x + 4;
        if !self.leading.is_none() {
            left += self.leading.draw(canvas, left, rect.y + 2, theme) + 4;
        }
        canvas.set_color(Some(ink));
        canvas.draw_text(
            self.title,
            left.max(0) as usize,
            (rect.y + 1).max(0) as usize,
        );
        if let Some(subtitle) = self.subtitle {
            canvas.set_color(Some(theme.muted));
            canvas.draw_text(
                subtitle,
                left.max(0) as usize,
                (rect.y + ROW_PITCH as i32).max(0) as usize,
            );
        }
        if !self.trailing.is_none() {
            let width = self.trailing.width(canvas);
            self.trailing
                .draw(canvas, rect.right() - 4 - width, rect.y + 2, theme);
        }
        canvas.set_color(None);
    }
}
