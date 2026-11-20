//! The overlay band: the confirmation, the setup announcement, and the join box.

use crate::services::network::controller::{ControlKind, answers, confirm_verb};
use crate::services::network::shape::{
    DIALOG_BODY_TOP, DIALOG_H, DIALOG_INSET, DIALOG_TITLE_TOP, DIALOG_TOP, DIALOG_W, DIALOG_X,
    DialogChoice, dialog_answer_box,
};
use crate::services::network::view::text::{
    DIALOG_BYTES, NAME_BYTES, SETUP_NOTICE_TITLE, SETUP_STARTING_BODY, body, control_label,
    format_joining_title, format_question, format_setup_notice_body, format_setup_pass,
    joining_body,
};
use crate::shared::model::{NetworkView, SetupWait};
use iot_core::drivers::network::{AccessPoint, LinkStatus};
use iot_core::ui::Canvas;
use iot_core::ui::Rect;
use iot_core::ui::panel::ROW_PITCH;

/// A box over the list: a title, up to two lines under it, and optionally two answers along
/// its bottom edge. It owns where each part sits, so a caller hands it words only.
struct Dialog<'a> {
    title: &'a [u8],
    lines: [&'a [u8]; 2],
    line_count: usize,
    answers: Option<(&'a [u8], &'a [u8])>,
}

impl<'a> Dialog<'a> {
    fn new(title: &'a [u8]) -> Self {
        Self {
            title,
            lines: [b"", b""],
            line_count: 0,
            answers: None,
        }
    }

    /// One line under the title, in the order the box stacks them.
    fn line(mut self, line: &'a [u8]) -> Self {
        debug_assert!(
            self.line_count < self.lines.len(),
            "a dialog holds two lines"
        );
        self.lines[self.line_count] = line;
        self.line_count += 1;
        self
    }

    /// The two answers along the bottom edge: what keeps things as they are, and what acts.
    fn answers(mut self, keep: &'a [u8], act: &'a [u8]) -> Self {
        self.answers = Some((keep, act));
        self
    }

    fn draw(self, canvas: &mut Canvas<'_>) {
        canvas.fill_rect(Rect::new(
            DIALOG_X as i32,
            DIALOG_TOP as i32,
            DIALOG_W as i32,
            DIALOG_H as i32,
        ));
        let left = DIALOG_X + DIALOG_INSET;
        canvas.draw_text(self.title, left, DIALOG_TITLE_TOP);
        for (index, line) in self.lines[..self.line_count].iter().enumerate() {
            canvas.draw_text(line, left, DIALOG_BODY_TOP + index * ROW_PITCH);
        }
        let Some((keep, act)) = self.answers else {
            return;
        };
        for (choice, label) in [(DialogChoice::Keep, keep), (DialogChoice::Act, act)] {
            super::draw_cell(canvas, dialog_answer_box(choice), label);
        }
    }
}

pub(crate) fn draw(
    canvas: &mut Canvas<'_>,
    draft: NetworkView,
    link: LinkStatus,
    shown_ap: Option<AccessPoint>,
    mac: Option<[u8; 6]>,
) {
    if draft.confirm.is_some() {
        draw_confirm(canvas, draft, link);
    } else if draft.setup_box_pending() {
        draw_setup_starting(canvas, mac);
    } else if link.shows_setup_box() {
        draw_notice(canvas, draft, link, mac);
    } else {
        draw_joining(canvas, link, shown_ap);
    }
}

/// The box is on the glass the moment the panel asks for the access point, before the
/// radio has been reconfigured for it.
fn draw_setup_starting(canvas: &mut Canvas<'_>, mac: Option<[u8; 6]>) {
    let mut name = [0u8; NAME_BYTES];
    Dialog::new(SETUP_NOTICE_TITLE)
        .line(format_setup_notice_body(mac, &mut name))
        .line(SETUP_STARTING_BODY)
        .draw(canvas);
}

fn draw_notice(
    canvas: &mut Canvas<'_>,
    draft: NetworkView,
    link: LinkStatus,
    mac: Option<[u8; 6]>,
) {
    let answers = answers(link);
    let mut name = [0u8; NAME_BYTES];
    let mut pass = [0u8; NAME_BYTES];
    let asked = match draft.setup_wait {
        Some(SetupWait::Open(open)) => Some(open),
        _ => None,
    };
    let keep = if asked.is_some() {
        ControlKind::Wait
    } else {
        answers.keep.kind
    };
    let line = match asked {
        Some(true) => format_setup_pass(None, &mut pass),
        Some(false) => SETUP_STARTING_BODY,
        None => format_setup_pass(link.setup_password.as_ref(), &mut pass),
    };
    Dialog::new(SETUP_NOTICE_TITLE)
        .line(format_setup_notice_body(mac, &mut name))
        .line(line)
        .answers(control_label(keep), control_label(answers.act.kind))
        .draw(canvas);
}

fn draw_joining(canvas: &mut Canvas<'_>, link: LinkStatus, shown_ap: Option<AccessPoint>) {
    let mut title = [0u8; DIALOG_BYTES];
    let associated = link.ap.is_some();
    Dialog::new(format_joining_title(associated, shown_ap, &mut title))
        .line(joining_body(associated))
        .draw(canvas);
}

fn draw_confirm(canvas: &mut Canvas<'_>, draft: NetworkView, link: LinkStatus) {
    let Some(confirm) = draft.confirm else {
        return;
    };
    let mut question = [0u8; DIALOG_BYTES];
    let title = format_question(confirm, link, &mut question);
    Dialog::new(title)
        .line(body(confirm))
        .answers(
            control_label(ControlKind::Keep),
            control_label(confirm_verb(confirm)),
        )
        .draw(canvas);
}
