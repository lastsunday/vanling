//! The Ambient page: the debug overlay over an empty ground, and nothing on a release build.

use crate::shared::intent::{BusinessIntent, InputIntent};
use crate::shared::kit::chrome;
use crate::shared::kit::text::{format_pair, format_u16, light_mode_word, write_u16};
use crate::shared::model::DeviceState;
use crate::shared::snapshot::FrameSnapshot;
use iot_core::drivers::input::{FINGER_LONG_PRESS, FINGER_SWIPE, FINGER_TAP};
use iot_core::light::{MODE_BREATH, MODE_SOLID, Rgb, brightness_of, rgb_hue};
use iot_core::ui::{Style, Ui};

/// The Ambient page owns no state: the lamp is its whole answer and the overlay its whole view.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AmbientPage;

impl super::Service for AmbientPage {}

impl super::LocalPage for AmbientPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> Option<BusinessIntent> {
        super::lamp(op, state)
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        if !ui.screen().debug {
            return;
        }
        let instance = ui.screen().instance;
        let light = snapshot.lights[usize::from(instance)];
        let live = if light.mode == MODE_BREATH {
            let color = ui
                .canvas()
                .and_then(|canvas| canvas.ground())
                .unwrap_or(Rgb(0, 0, 0));
            (brightness_of(color), rgb_hue(color))
        } else {
            (light.brightness, light.hue)
        };
        chrome::page(ui, |ui| overlay(ui, snapshot, live));
    }

    fn descriptor() -> super::Descriptor {
        super::Descriptor::new(Self::view).with_repaints(super::Repaint::Overlay)
    }
}

fn direction_arrow(dir: u8) -> u8 {
    match dir {
        0 => b'-',
        d @ 1..=9 => b'0' + d,
        _ => b'-',
    }
}

fn gesture_glyph(kind: u8) -> u8 {
    match kind {
        FINGER_TAP => b'1',
        FINGER_LONG_PRESS => b'2',
        FINGER_SWIPE => b'3',
        _ => b'-',
    }
}

fn overlay(ui: &mut Ui<'_, '_>, snapshot: &FrameSnapshot, live: (u8, u8)) {
    let touch = snapshot.touch;
    let light = snapshot.lights[usize::from(ui.screen().instance)];
    ui.row(Style::new().gap(8), |ui| {
        ui.column(Style::new().grow(1), |ui| {
            let mut buf = [0u8; 6];
            let counters: [(&[u8], u16); 6] = [
                (b"GST", u16::from(touch.ghost)),
                (b"2F", u16::from(touch.two_finger_runs)),
                (b"PRS", u16::from(touch.presses)),
                (b"TAP", u16::from(touch.taps)),
                (b"LNG", u16::from(touch.long_presses)),
                (b"SWP", u16::from(touch.swipes)),
            ];
            for (label, value) in counters {
                chrome::reading(ui, label, format_u16(value, &mut buf));
            }
            let mut dir = [b' '; 8];
            dir[0] = direction_arrow(touch.last_swipe_dir);
            dir[1] = b' ';
            let n = write_u16(touch.last_swipe_dist, &mut dir, 2);
            chrome::reading(ui, b"DIR", &dir[..n]);
            let mut xy = [0u8; 12];
            chrome::reading(ui, b"XY", format_pair(touch.last_gesture_origin, &mut xy));
            let mut end = [0u8; 12];
            chrome::reading(ui, b"XY2", format_pair(touch.last_gesture_end, &mut end));
        });
        ui.column(Style::new().grow(1), |ui| {
            let mut buf = [0u8; 6];
            for (slot, labels) in [["P0", "P0G", "P0D", "P0V"], ["P1", "P1G", "P1D", "P1V"]]
                .into_iter()
                .enumerate()
            {
                let mut xy = [0u8; 8];
                let n = match touch.points[slot] {
                    Some((x, y)) => {
                        let n = write_u16(x, &mut xy, 0);
                        xy[n] = b' ';
                        write_u16(y, &mut xy, n + 1)
                    }
                    None => {
                        xy[0] = b'-';
                        1
                    }
                };
                chrome::reading(ui, labels[0].as_bytes(), &xy[..n]);
                let last = touch.finger[slot];
                let mut value = [b'-'; 6];
                let value_len = if last.kind == 0 {
                    1
                } else {
                    let digits = format_u16(last.value, &mut buf);
                    value[..digits.len()].copy_from_slice(digits);
                    digits.len()
                };
                chrome::reading(ui, labels[1].as_bytes(), &[gesture_glyph(last.kind)]);
                chrome::reading(ui, labels[2].as_bytes(), &value[..value_len]);
                chrome::reading(
                    ui,
                    labels[3].as_bytes(),
                    &[direction_arrow(touch.live_dir[slot])],
                );
            }
            chrome::reading(
                ui,
                b"CHP",
                format_u16(u16::from(touch.chip_gesture_id), &mut buf),
            );
            chrome::reading(ui, b"FRM", format_u16(touch.frames, &mut buf));
        });
        ui.column(Style::new().grow(1), |ui| {
            let mut buf = [0u8; 6];
            chrome::reading(ui, b"MOD", light_mode_word(light.mode));
            chrome::reading(ui, b"BRI", format_u16(u16::from(live.0), &mut buf));
            match light.mode {
                MODE_BREATH => {
                    let mut lo_hi = [0u8; 8];
                    let mut n = write_u16(u16::from(light.breath.min_brightness), &mut lo_hi, 0);
                    lo_hi[n] = b' ';
                    n = write_u16(u16::from(light.breath.max_brightness), &mut lo_hi, n + 1);
                    chrome::reading(ui, b"LOHI", &lo_hi[..n]);
                    chrome::reading(ui, b"PER", format_u16(light.breath.period_ms, &mut buf));
                    chrome::reading(ui, b"HUE", format_u16(u16::from(live.1), &mut buf));
                    chrome::reading(ui, b"HPR", format_u16(light.breath.hue_period_ms, &mut buf));
                    chrome::reading(
                        ui,
                        b"SPN",
                        format_u16(u16::from(light.breath.hue_span), &mut buf),
                    );
                    chrome::reading(
                        ui,
                        b"GRP",
                        format_u16(u16::from(light.breath.group_len), &mut buf),
                    );
                    chrome::reading(
                        ui,
                        b"SAT",
                        format_u16(u16::from(light.breath.saturation), &mut buf),
                    );
                }
                MODE_SOLID => {
                    chrome::reading(ui, b"HUE", format_u16(u16::from(live.1), &mut buf));
                }
                _ => {}
            }
        });
    });
}
