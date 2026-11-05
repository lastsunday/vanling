//! The overlay band: the confirmation, the setup announcement, and the join box.

use crate::controls::DIALOG_GLYPHS;
use crate::state::NetworkPage;
use crate::state::controls as page_controls;
use crate::state::controls::{ControlKind, confirm_verb};
use iot_core::drivers::network::{AccessPoint, LinkStatus, SSID_BYTES};

use crate::ui::components::dialog::Dialog;
use crate::ui::surface::Surface;
use crate::ui::views::network::text::{
    SETUP_NOTICE_TITLE, body, control_label, format_joining_title, format_question,
    format_setup_notice_body, format_setup_pass, joining_body,
};

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
        } else if link.shows_setup_box() {
            self.stamp_notice(link, mac);
        } else {
            self.stamp_joining(shown_ap);
        }
    }

    fn stamp_notice(&mut self, link: LinkStatus, mac: Option<[u8; 6]>) {
        let answers = page_controls::answers(link);
        let mut name = [0u8; SSID_BYTES];
        let mut pass = [0u8; SSID_BYTES];
        Dialog::new(SETUP_NOTICE_TITLE)
            .line(format_setup_notice_body(mac, &mut name))
            .line(format_setup_pass(link.setup_password.as_ref(), &mut pass))
            .answers(
                control_label(answers.keep.kind),
                control_label(answers.act.kind),
            )
            .stamp(self);
    }

    fn stamp_joining(&mut self, shown_ap: Option<AccessPoint>) {
        let mut title = [0u8; DIALOG_GLYPHS];
        Dialog::new(format_joining_title(shown_ap, &mut title))
            .line(joining_body(shown_ap))
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
