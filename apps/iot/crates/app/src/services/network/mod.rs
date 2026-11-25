//! The Network page: the link's list and setup box, the edit session the user moves through
//! them, and the drawing that reads both.

pub mod controller;
pub mod shape;
mod view;

#[cfg(test)]
mod tests;

use iot_core::light::Rgb;
use iot_core::ui::panel::{CONTENT_BOTTOM, PANEL_COLUMNS};
use iot_core::ui::{Canvas, Rect, Screen, Style, Ui};

use crate::shared::intent::{BusinessIntent, InputIntent, LinkIntent};
use crate::shared::model::{Command, DeviceState, Effects, NetworkView};
use crate::shared::snapshot::FrameSnapshot;
use crate::shared::snapshot::PageSnapshot;

/// The page owns the one edit session the link never writes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkPage {
    draft: NetworkView,
}

impl super::Service for NetworkPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        match intent {
            BusinessIntent::Link(intent) => {
                let effects = state.network.apply(*intent);
                match intent {
                    LinkIntent::Status(status) => self.draft.on_link(*status),
                    LinkIntent::Mac(_) => {}
                    LinkIntent::Scan => self.draft.clamp_top(state.network.len()),
                }
                (None, effects)
            }
            BusinessIntent::Network(intent) => (
                self.draft
                    .apply(*intent, &state.network, state.has_screen()),
                Effects::default(),
            ),
            _ => (None, Effects::default()),
        }
    }
}

impl super::LocalPage for NetworkPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> Option<BusinessIntent> {
        controller::bind(op, state, self.draft)
    }

    fn publish(&self, out: &mut PageSnapshot) {
        out.network = self.draft;
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        let link = snapshot.link;
        let draft = snapshot.pages.network;

        ui.stack(
            Style::new().size(PANEL_COLUMNS as i16, CONTENT_BOTTOM as i16),
            |ui| {
                ui.custom(
                    Style::new()
                        .absolute()
                        .left(0)
                        .top(0)
                        .size(PANEL_COLUMNS as i16, shape::LIST_TOP as i16),
                    view::HEADER,
                );

                if shape::Band::Content.showing(draft, link) {
                    let bottom = if draft.keyboard {
                        CONTENT_BOTTOM
                    } else {
                        shape::TOOLBAR_TOP
                    };
                    ui.custom(
                        Style::new()
                            .absolute()
                            .left(0)
                            .top(shape::LIST_TOP as i16)
                            .size(PANEL_COLUMNS as i16, (bottom - shape::LIST_TOP) as i16),
                        view::CONTENT,
                    );
                }

                if shape::Band::Controls.showing(draft, link) {
                    ui.custom(
                        Style::new()
                            .absolute()
                            .left(0)
                            .top(shape::TOOLBAR_TOP as i16)
                            .size(PANEL_COLUMNS as i16, shape::TOOLBAR_H as i16),
                        view::STRIP,
                    );
                }

                if shape::Band::Overlay.showing(draft, link) {
                    ui.custom(
                        Style::new()
                            .absolute()
                            .left(0)
                            .top(0)
                            .size(PANEL_COLUMNS as i16, CONTENT_BOTTOM as i16),
                        view::OVERLAY,
                    );
                }
            },
        );
    }

    fn on_hide(&mut self) {
        self.draft.collapse();
    }

    fn descriptor() -> super::Descriptor {
        super::Descriptor::new(Self::view)
            .with_ground(Some(Rgb(0, 0, 0)))
            .with_custom(custom)
            .with_repaints(super::Repaint::When(network_moved))
    }
}

/// The four bands, each a custom leaf of its own.
fn custom(
    snapshot: &FrameSnapshot,
    _screen: Screen,
    slot: u8,
    canvas: &mut Canvas<'_>,
    _rect: Rect,
) {
    let link = snapshot.link;
    let aps = snapshot.aps;
    let draft = snapshot.pages.network;
    let mac = snapshot.mac;
    let shown_ap = if draft.keyboard {
        draft.selected
    } else {
        link.ap.or(draft.selected)
    };
    match slot {
        view::HEADER => {
            let list_len = aps.filter(|list| !list.is_empty()).map(|list| list.len());
            view::header::draw(canvas, snapshot.lang, draft, link, shown_ap, mac, list_len);
        }
        view::CONTENT => view::content::draw(canvas, draft, link, aps),
        view::STRIP => view::draw_strip(canvas, draft, link),
        _ => view::overlay::draw(canvas, draft, link, shown_ap, mac),
    }
}

fn network_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.lang != next.lang
        || prev.link != next.link
        || prev.aps != next.aps
        || prev.pages != next.pages
        || prev.mac != next.mac
}
