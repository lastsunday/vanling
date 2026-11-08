//! The overlay band: the confirmation, the setup announcement, and the join box.

use crate::features::network::controller::{self as page_controls, ControlKind, confirm_verb};
use crate::features::network::layout::DIALOG_GLYPHS;
use crate::features::network::model::{NetworkPage, SetupWait};
use iot_core::drivers::network::{AccessPoint, LinkStatus, SSID_BYTES};

use super::dialog_box::Dialog;
use crate::features::network::view::text::{
    SETUP_NOTICE_TITLE, SETUP_STARTING_BODY, body, control_label, format_joining_title,
    format_question, format_setup_notice_body, format_setup_pass, joining_body,
};
use crate::shared::view_kit::surface::Surface;

impl Surface<'_> {
    pub(super) fn stamp_box(
        &mut self,
        draft: NetworkPage,
        link: LinkStatus,
        shown_ap: Option<AccessPoint>,
        mac: Option<[u8; 6]>,
    ) {
        if draft.confirm.is_some() {
            self.stamp_confirm(draft, link);
        } else if draft.setup_box_pending() {
            self.stamp_setup_starting(mac);
        } else if link.shows_setup_box() {
            self.stamp_notice(draft, link, mac);
        } else {
            self.stamp_joining(link, shown_ap);
        }
    }

    /// The box is on the glass the moment the panel asks for the access point, before the
    /// radio has been reconfigured for it.
    fn stamp_setup_starting(&mut self, mac: Option<[u8; 6]>) {
        let mut name = [0u8; SSID_BYTES];
        Dialog::new(SETUP_NOTICE_TITLE)
            .line(format_setup_notice_body(mac, &mut name))
            .line(SETUP_STARTING_BODY)
            .stamp(self);
    }

    fn stamp_notice(&mut self, draft: NetworkPage, link: LinkStatus, mac: Option<[u8; 6]>) {
        let answers = page_controls::answers(link);
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
            .stamp(self);
    }

    fn stamp_joining(&mut self, link: LinkStatus, shown_ap: Option<AccessPoint>) {
        let mut title = [0u8; DIALOG_GLYPHS];
        let associated = link.ap.is_some();
        Dialog::new(format_joining_title(associated, shown_ap, &mut title))
            .line(joining_body(associated))
            .stamp(self);
    }

    fn stamp_confirm(&mut self, draft: NetworkPage, link: LinkStatus) {
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
            .stamp(self);
    }
}
