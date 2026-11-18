//! The overlay band: the confirmation, the setup announcement, and the join box.

use crate::pages::network::controller::{ControlKind, answers, confirm_verb};
use crate::pages::network::layout::{
    DIALOG_BODY_TOP, DIALOG_GLYPHS, DIALOG_H, DIALOG_INSET, DIALOG_TITLE_TOP, DIALOG_TOP, DIALOG_W,
    DIALOG_X, DialogChoice, dialog_answer_box,
};
use crate::pages::network::view::text::{
    SETUP_NOTICE_TITLE, SETUP_STARTING_BODY, body, control_label, format_joining_title,
    format_question, format_setup_notice_body, format_setup_pass, joining_body,
};
use crate::shared::kit::painter::Painter;
use crate::shared::model::{NetworkView, SetupWait};
use iot_core::drivers::network::{AccessPoint, LinkStatus, SSID_BYTES};
use iot_core::geometry::ControlRect;
use iot_core::overlay::ROW_PITCH;

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

    fn draw(self, painter: &mut Painter<'_>) {
        painter.fill(ControlRect {
            x: DIALOG_X,
            y: DIALOG_TOP,
            w: DIALOG_W,
            h: DIALOG_H,
        });
        let left = DIALOG_X + DIALOG_INSET;
        painter.draw_text(self.title, left, DIALOG_TITLE_TOP);
        for (index, line) in self.lines[..self.line_count].iter().enumerate() {
            painter.draw_text(line, left, DIALOG_BODY_TOP + index * ROW_PITCH);
        }
        let Some((keep, act)) = self.answers else {
            return;
        };
        for (choice, label) in [(DialogChoice::Keep, keep), (DialogChoice::Act, act)] {
            painter.draw_cell(dialog_answer_box(choice), label);
        }
    }
}

impl Painter<'_> {
    pub(crate) fn draw_box(
        &mut self,
        draft: NetworkView,
        link: LinkStatus,
        shown_ap: Option<AccessPoint>,
        mac: Option<[u8; 6]>,
    ) {
        if draft.confirm.is_some() {
            self.draw_confirm(draft, link);
        } else if draft.setup_box_pending() {
            self.draw_setup_starting(mac);
        } else if link.shows_setup_box() {
            self.draw_notice(draft, link, mac);
        } else {
            self.draw_joining(link, shown_ap);
        }
    }

    /// The box is on the glass the moment the panel asks for the access point, before the
    /// radio has been reconfigured for it.
    fn draw_setup_starting(&mut self, mac: Option<[u8; 6]>) {
        let mut name = [0u8; SSID_BYTES];
        Dialog::new(SETUP_NOTICE_TITLE)
            .line(format_setup_notice_body(mac, &mut name))
            .line(SETUP_STARTING_BODY)
            .draw(self);
    }

    fn draw_notice(&mut self, draft: NetworkView, link: LinkStatus, mac: Option<[u8; 6]>) {
        let answers = answers(link);
        let mut name = [0u8; SSID_BYTES];
        let mut pass = [0u8; SSID_BYTES];
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
            .draw(self);
    }

    fn draw_joining(&mut self, link: LinkStatus, shown_ap: Option<AccessPoint>) {
        let mut title = [0u8; DIALOG_GLYPHS];
        let associated = link.ap.is_some();
        Dialog::new(format_joining_title(associated, shown_ap, &mut title))
            .line(joining_body(associated))
            .draw(self);
    }

    fn draw_confirm(&mut self, draft: NetworkView, link: LinkStatus) {
        let Some(confirm) = draft.confirm else {
            return;
        };
        let mut question = [0u8; DIALOG_GLYPHS];
        let title = format_question(confirm, link, &mut question);
        Dialog::new(title)
            .line(body(confirm))
            .answers(
                control_label(ControlKind::Keep),
                control_label(confirm_verb(confirm)),
            )
            .draw(self);
    }
}
