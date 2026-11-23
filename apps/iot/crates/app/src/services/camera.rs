//! The Camera page: the sensor's frame, and the readout drawn over it.

use iot_core::drivers::camera::CameraSnapshot;
use iot_core::light::Rgb;
use iot_core::ui::text::{format_pair, format_u32, frame_fingerprint, write_ratio};
use iot_core::ui::{Style, Ui};

use crate::shared::i18n::Msg;
use crate::shared::i18n::words::light_mode_word;
use crate::shared::intent::{BusinessIntent, InputIntent};
use crate::shared::model::DeviceState;
use crate::shared::page;
use crate::shared::snapshot::FrameSnapshot;

/// The camera page draws the sensor's frames and its own readout over them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CameraPage;

impl super::Service for CameraPage {}

impl super::LocalPage for CameraPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> Option<BusinessIntent> {
        super::lamp(op, state)
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        Self::view_frame(snapshot, ui, CameraSnapshot::STARTING);
    }

    /// A frame arriving is a data event and not anything a snapshot diff can see. Forcing a
    /// repaint here would ship a full frame that is a copy of the last one — ~15 ms of
    /// blocking SPI.
    fn descriptor() -> super::Descriptor {
        super::Descriptor::new(Self::view)
            .with_ground(Some(Rgb(0, 0, 0)))
            .with_camera_frames()
    }
}

impl CameraPage {
    /// The camera's own frame, after the sensor has filled it.
    pub(crate) fn view_frame(
        snapshot: &FrameSnapshot,
        ui: &mut Ui<'_, '_>,
        readout: CameraSnapshot,
    ) {
        let lang = snapshot.lang;
        let fault = readout.fault;
        let fingerprint = (!readout.starting && fault.is_none())
            .then(|| ui.canvas().map(|canvas| frame_fingerprint(canvas.frame())))
            .flatten();

        page::body(ui, |ui| {
            ui.row(Style::new().between(), |ui| {
                page::title(ui, Msg::CameraTitle.bytes(lang));
                match (fault, readout.starting) {
                    (Some(_), _) => ui.text(Msg::NoCam.bytes(lang), Style::new().nowrap()),
                    (None, true) => ui.text(Msg::Wait.bytes(lang), Style::new().nowrap()),
                    (None, false) => ui.text(b"", Style::new()),
                };
            });
            if fault.is_none() {
                if let Some(geometry) = readout.geometry {
                    page::reading(ui, b"WIN", write_ratio(&mut [0u8; 6], geometry.subsample));
                    page::reading(
                        ui,
                        b"OUT",
                        format_pair(
                            Some((geometry.out_width, geometry.out_height)),
                            &mut [0u8; 12],
                        ),
                    );
                    page::reading(
                        ui,
                        b"RD",
                        format_pair(
                            Some((geometry.win_width, geometry.win_height)),
                            &mut [0u8; 12],
                        ),
                    );
                } else if readout.starting {
                    page::reading(ui, b"BUS", b"----");
                } else {
                    page::reading(ui, b"BUS", b"NOBUS");
                }
                ui.text(b"", Style::new());
                page::reading(
                    ui,
                    b"FRM",
                    format_u32(readout.counters.frames, &mut [0u8; 12]),
                );
                page::reading(
                    ui,
                    b"RM",
                    format_u32(readout.counters.restarts, &mut [0u8; 12]),
                );
                page::reading(ui, b"MOD", light_mode_word(snapshot.lights[0].mode));
                if let Some(fingerprint) = fingerprint {
                    page::reading(ui, b"FP", format_u32(fingerprint, &mut [0u8; 12]));
                }
            } else if let Some(reason) = fault {
                ui.text(reason.as_bytes(), Style::new().nowrap());
                ui.text(Msg::Retrying.bytes(lang), Style::new().nowrap());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::snapshot::FrameSnapshot;
    use crate::state::PageId;
    use alloc::vec;
    use iot_core::drivers::camera::CameraCounters;
    use iot_core::ui::panel::{CONTENT_BOTTOM, PANEL_COLUMNS};
    use iot_core::ui::{BoxConstraints, Canvas, PX, Screen, Ui};

    const WIDTH: usize = PANEL_COLUMNS;
    const HEIGHT: usize = CONTENT_BOTTOM + iot_core::ui::panel::FOOTER_ROWS;

    fn render(frame: &mut [u8], snapshot: &FrameSnapshot, readout: CameraSnapshot) -> Vec<u8> {
        {
            let mut canvas = Canvas::new(frame, WIDTH, HEIGHT, None);
            let mut ui = Ui::new_with_height(&mut canvas, Screen::default(), CONTENT_BOTTOM);
            CameraPage::view_frame(snapshot, &mut ui, readout);
            ui.paint(
                BoxConstraints::tight(PANEL_COLUMNS as i32 * PX, CONTENT_BOTTOM as i32 * PX),
                &mut |_, _, _| {},
            );
        }
        frame.to_vec()
    }

    #[test]
    fn the_camera_readout_reports_a_fingerprint_only_for_a_real_frame() {
        let mut frame = vec![0u8; WIDTH * HEIGHT * 2];
        for (index, byte) in frame.iter_mut().enumerate() {
            *byte = (index % 251) as u8;
        }
        let snapshot = FrameSnapshot {
            page: PageId::Camera,
            ..FrameSnapshot::default()
        };
        let mut scratch = frame.clone();
        let starting = render(&mut scratch, &snapshot, CameraSnapshot::STARTING);
        assert_ne!(
            starting, frame,
            "the waiting readout is drawn over the frame"
        );
        let mut scratch = frame.clone();
        let real = render(
            &mut scratch,
            &snapshot,
            CameraSnapshot {
                counters: CameraCounters::ZERO,
                geometry: None,
                starting: false,
                fault: None,
            },
        );
        assert_ne!(starting, real, "a real frame adds its fingerprint");
    }

    #[test]
    fn a_camera_that_cannot_start_says_so_and_offers_the_retry() {
        let frame = vec![0u8; WIDTH * HEIGHT * 2];
        let snapshot = FrameSnapshot {
            page: PageId::Camera,
            ..FrameSnapshot::default()
        };
        let mut failed_frame = frame.clone();
        let failed = render(
            &mut failed_frame,
            &snapshot,
            CameraSnapshot {
                counters: CameraCounters::ZERO,
                geometry: None,
                starting: false,
                fault: Some("the sensor did not answer"),
            },
        );
        let mut healthy_frame = frame;
        let healthy = render(
            &mut healthy_frame,
            &snapshot,
            CameraSnapshot {
                counters: CameraCounters::ZERO,
                geometry: None,
                starting: false,
                fault: None,
            },
        );
        assert_ne!(
            failed, healthy,
            "the fault readout is not the no-sensor readout"
        );
    }
}
