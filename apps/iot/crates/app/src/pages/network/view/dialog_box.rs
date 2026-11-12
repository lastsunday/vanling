//! The box a page-wide question is drawn in.

use iot_core::geometry::ControlRect;
use iot_core::overlay::ROW_PITCH;

use crate::pages::network::layout::{
    DIALOG_BODY_TOP, DIALOG_H, DIALOG_INSET, DIALOG_TITLE_TOP, DIALOG_TOP, DIALOG_W, DIALOG_X,
    DialogChoice, dialog_answer_box,
};
use crate::shared::kit::surface::Surface;

/// A box over the list: a title, up to two lines under it, and optionally two answers along
/// its bottom edge. It owns where each part sits, so a caller hands it words only.
pub(crate) struct Dialog<'a> {
    title: &'a [u8],
    lines: [&'a [u8]; 2],
    line_count: usize,
    answers: Option<(&'a [u8], &'a [u8])>,
}

impl<'a> Dialog<'a> {
    pub(crate) fn new(title: &'a [u8]) -> Self {
        Self {
            title,
            lines: [b"", b""],
            line_count: 0,
            answers: None,
        }
    }

    /// One line under the title, in the order the box stacks them.
    pub(crate) fn line(mut self, line: &'a [u8]) -> Self {
        debug_assert!(
            self.line_count < self.lines.len(),
            "a dialog holds two lines"
        );
        self.lines[self.line_count] = line;
        self.line_count += 1;
        self
    }

    /// The two answers along the bottom edge: what keeps things as they are, and what acts.
    pub(crate) fn answers(mut self, keep: &'a [u8], act: &'a [u8]) -> Self {
        self.answers = Some((keep, act));
        self
    }

    pub(crate) fn stamp(self, surface: &mut Surface<'_>) {
        surface.fill(ControlRect {
            x: DIALOG_X,
            y: DIALOG_TOP,
            w: DIALOG_W,
            h: DIALOG_H,
        });
        let left = DIALOG_X + DIALOG_INSET;
        surface.stamp_text(self.title, left, DIALOG_TITLE_TOP);
        for (index, line) in self.lines[..self.line_count].iter().enumerate() {
            surface.stamp_text(line, left, DIALOG_BODY_TOP + index * ROW_PITCH);
        }
        let Some((keep, act)) = self.answers else {
            return;
        };
        for (choice, label) in [(DialogChoice::Keep, keep), (DialogChoice::Act, act)] {
            surface.stamp_cell(dialog_answer_box(choice), label);
        }
    }
}
