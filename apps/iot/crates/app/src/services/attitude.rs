//! The Attitude page: the motion sample, its estimator, and the dial the tilt is drawn as.

use iot_core::drivers::motion::{MotionCapabilities, MotionSample};
use iot_core::horizon::{SCALE, horizon};
use iot_core::ui::text::{format_i32, format_tenths, format_u16};
use iot_core::ui::{Canvas, Rect, Screen, Style, Ui};

use crate::shared::i18n::Msg;
use crate::shared::i18n::words::NOT_AVAILABLE;
use crate::shared::intent::{BusinessIntent, InputIntent};
use crate::shared::model::DeviceState;
use crate::shared::page;
use crate::shared::snapshot::FrameSnapshot;

/// The dial's box in the page's bottom-right corner.
const DIAL: i16 = 96;

/// The attitude dial reads the motion sample and nothing else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttitudePage;

impl super::Service for AttitudePage {}

impl super::LocalPage for AttitudePage {
    fn bind(
        &self,
        _op: &InputIntent,
        _state: &DeviceState,
        _layout: &iot_core::ui::Layout,
    ) -> Option<BusinessIntent> {
        None
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        page::body(ui, |ui| {
            page::title(ui, Msg::AttitudeTitle.bytes(snapshot.lang));
            let Some(sample) = snapshot.motion else {
                page::reading(ui, b"ERR", Msg::Wait.bytes(snapshot.lang));
                return;
            };
            if !sample.valid {
                page::reading(ui, b"ERR", Msg::ReadFailed.bytes(snapshot.lang));
                return;
            }
            ui.stack(Style::new().grow(1), |ui| {
                ui.row(Style::new().gap(8), |ui| {
                    ui.column(Style::new().width(page::READING), |ui| axes(ui, &sample));
                    ui.column(Style::new().width(page::READING), |ui| {
                        estimator(ui, &sample);
                        counters(ui, snapshot, 0, 7);
                    });
                    ui.column(Style::new().width(page::READING), |ui| {
                        counters(ui, snapshot, 7, MOTION_SEMANTIC_ROWS);
                    });
                });
                ui.custom(
                    Style::new()
                        .absolute()
                        .right(page::MARGIN)
                        .bottom(0)
                        .size(DIAL, DIAL),
                    0,
                );
            });
        });
    }

    fn descriptor() -> super::Descriptor {
        super::Descriptor::new(Self::view)
            .with_custom(custom)
            .with_repaints(super::Repaint::Always)
    }
}

/// The dial is the page's only custom leaf: slot 0.
fn custom(
    snapshot: &FrameSnapshot,
    _screen: Screen,
    _slot: u8,
    canvas: &mut Canvas<'_>,
    rect: Rect,
) {
    if let Some(sample) = snapshot.motion.filter(|sample| sample.valid) {
        draw_horizon(canvas, rect, &sample);
    }
}

fn axes(ui: &mut Ui<'_, '_>, sample: &MotionSample) {
    for (group, prefix) in (*b"AMGDR").into_iter().enumerate() {
        for axis in 0..3 {
            let mut buf = [0u8; 12];
            let value: &[u8] = match group {
                0 => format_i32(i32::from(sample.raw_accel[axis]), &mut buf),
                1 => format_i32(sample.accel_mg[axis], &mut buf),
                2 => format_i32(i32::from(sample.raw_gyro[axis]), &mut buf),
                3 => format_tenths(sample.gyro_dps_x10[axis], &mut buf),
                _ => format_tenths(i32::from(sample.tilt_deg_x10[axis]), &mut buf),
            };
            let label = [prefix, b'0' + axis as u8];
            page::reading(ui, &label, value);
        }
    }
}

fn estimator(ui: &mut Ui<'_, '_>, sample: &MotionSample) {
    let counters: [(&[u8], i32); 4] = [
        (b"ST", i32::from(sample.status)),
        (b"LR", sample.gravity_deviation_mg),
        (b"SR", sample.shake_residual_mg),
        (b"TR", sample.tap_residual_mg),
    ];
    for (label, value) in counters {
        if label == b"ST" {
            page::reading(
                ui,
                label,
                format_u16(u16::from(sample.status), &mut [0u8; 6]),
            );
        } else {
            page::reading(ui, label, format_i32(value, &mut [0u8; 12]));
        }
    }
}

fn counters(ui: &mut Ui<'_, '_>, snapshot: &FrameSnapshot, first: usize, last: usize) {
    let rows: [(&[u8], MotionCapabilities, u16); MOTION_SEMANTIC_ROWS] = [
        (
            b"TAP".as_slice(),
            MotionCapabilities::TAP,
            snapshot.motion_counts.taps,
        ),
        (
            b"2T".as_slice(),
            MotionCapabilities::TAP,
            snapshot.motion_counts.double_taps,
        ),
        (
            b"3T".as_slice(),
            MotionCapabilities::TAP,
            snapshot.motion_counts.triple_taps,
        ),
        (
            b"STL".as_slice(),
            MotionCapabilities::STILL,
            snapshot.motion_counts.still,
        ),
        (
            b"MOV".as_slice(),
            MotionCapabilities::MOVING,
            snapshot.motion_counts.moving,
        ),
        (
            b"ACT".as_slice(),
            MotionCapabilities::ACTIVITY,
            snapshot.motion_counts.activity,
        ),
        (
            b"STP".as_slice(),
            MotionCapabilities::STEP,
            snapshot.motion_counts.steps,
        ),
        (
            b"TIN".as_slice(),
            MotionCapabilities::TILT,
            snapshot.motion_counts.tilt_enters,
        ),
        (
            b"TOX".as_slice(),
            MotionCapabilities::TILT,
            snapshot.motion_counts.tilt_exits,
        ),
        (
            b"SHK".as_slice(),
            MotionCapabilities::SHAKE,
            snapshot.motion_counts.shakes,
        ),
        (
            b"LFT".as_slice(),
            MotionCapabilities::LIFT_PLACE,
            snapshot.motion_counts.lifts,
        ),
        (
            b"PLC".as_slice(),
            MotionCapabilities::LIFT_PLACE,
            snapshot.motion_counts.places,
        ),
        (
            b"PRT".as_slice(),
            MotionCapabilities::POSTURE,
            snapshot.motion_counts.portraits,
        ),
        (
            b"LND".as_slice(),
            MotionCapabilities::POSTURE,
            snapshot.motion_counts.landscapes,
        ),
    ];
    for (label, capability, count) in &rows[first..last] {
        let mut buf = [0u8; 6];
        let value: &[u8] = if snapshot.motion_caps.contains(*capability) {
            format_u16(*count, &mut buf)
        } else {
            NOT_AVAILABLE
        };
        page::reading(ui, label, value);
    }
}

fn draw_horizon(canvas: &mut Canvas<'_>, rect: Rect, sample: &MotionSample) {
    let geo = horizon(sample.tilt_deg_x10[0], sample.tilt_deg_x10[1]);
    let cx = rect.x as isize + rect.w as isize / 2;
    let cy = rect.y as isize + rect.h as isize / 2;
    let r = (rect.w.min(rect.h) as isize / 2 - 2).max(1);
    canvas.draw_circle(cx, cy, r);
    canvas.draw_line(cx - 4, cy - r + 2, cx + 4, cy - r + 2);
    canvas.draw_line(cx - 10, cy, cx + 10, cy);
    let scale = SCALE as isize;
    let (hx, hy) = (
        geo.unit.0 as isize * r / scale,
        geo.unit.1 as isize * r / scale,
    );
    let offset = geo.offset as isize * r / scale;
    if let Some((a, b)) = chord(hx, hy, offset, r - 1) {
        canvas.draw_line(cx + a.0, cy + offset + a.1, cx + b.0, cy + offset + b.1);
    }
}

/// Both ends of the horizon's chord, as offsets from the line's base point. The line runs
/// through the base point along `(hx, hy)`; where it crosses the circle is the roots of
/// `|base + s·(hx, hy)|² = r²`. `None` when the line misses the circle.
fn chord(
    hx: isize,
    hy: isize,
    offset: isize,
    r: isize,
) -> Option<((isize, isize), (isize, isize))> {
    let (hx, hy, o) = (hx as i64, hy as i64, offset as i64);
    let rr = hx * hx + hy * hy;
    let r = i64::from(r.max(1) as i32);
    let disc = rr * (r * r - o * o) + o * o * hy * hy;
    if rr == 0 || disc <= 0 {
        return None;
    }
    let root = (disc as u64).isqrt() as i64;
    let end = |sign: i64| {
        let along = sign * root - o * hy;
        ((hx * along / rr) as isize, (hy * along / rr) as isize)
    };
    Some((end(1), end(-1)))
}

const MOTION_SEMANTIC_ROWS: usize = 14;

#[cfg(test)]
mod tests {
    use super::super::LocalPage;
    use super::*;
    use crate::shared::route::PageId;
    use alloc::vec::Vec;
    use iot_core::ui::panel::{CONTENT_BOTTOM, MARGIN as EDGE, PANEL_COLUMNS};
    use iot_core::ui::{BoxConstraints, PX, Rect, Screen, Ui};

    fn sample(tilt_deg_x10: i16) -> MotionSample {
        let mg = i32::from(tilt_deg_x10);
        MotionSample {
            raw_accel: [tilt_deg_x10, 0, -tilt_deg_x10],
            raw_gyro: [0, 0, 0],
            accel_mg: [mg, mg, mg],
            gyro_dps_x10: [mg, 0, -mg],
            tilt_deg_x10: [tilt_deg_x10, tilt_deg_x10, 0],
            status: 3,
            tap_residual_mg: mg,
            gravity_deviation_mg: mg,
            shake_residual_mg: mg,
            valid: true,
        }
    }

    fn page(sample: MotionSample) -> Ui<'static, 'static> {
        let snapshot = FrameSnapshot {
            page: PageId::Attitude,
            motion: Some(sample),
            ..FrameSnapshot::default()
        };
        let mut ui = Ui::headless(PANEL_COLUMNS, CONTENT_BOTTOM, Screen::default());
        AttitudePage::view(&snapshot, &mut ui);
        ui.layout(BoxConstraints::tight(
            PANEL_COLUMNS as i32 * PX,
            CONTENT_BOTTOM as i32 * PX,
        ));
        ui
    }

    fn rects(ui: &Ui<'_, '_>) -> Vec<Rect> {
        ui.rects().collect()
    }

    #[test]
    fn the_numbers_do_not_move_the_layout() {
        let quiet = page(sample(0));
        let busy = page(sample(-1799));
        assert!(
            quiet.node_count() <= iot_core::ui::MAX_NODES,
            "the page outgrew one declaration"
        );
        assert_eq!(
            rects(&quiet),
            rects(&busy),
            "a wider reading moved a rectangle"
        );
    }

    #[test]
    fn the_dial_owns_the_bottom_right_corner() {
        let ui = page(sample(400));
        let dial = ui.paint_rect(0).expect("the page declares its dial");
        assert_eq!(dial.right(), PANEL_COLUMNS as i32 - EDGE as i32);
        assert_eq!(dial.bottom(), CONTENT_BOTTOM as i32);
        for rect in ui.text_rects() {
            let overlap = rect.intersect(dial);
            assert!(
                overlap.w == 0 || overlap.h == 0,
                "a reading at {} overlaps the dial",
                rect.y
            );
        }
    }
}
