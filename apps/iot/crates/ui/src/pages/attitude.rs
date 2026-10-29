use iot_core::drivers::motion::MotionCapabilities;
use iot_core::overlay::{Block, block_x, overlay_row_top};

use crate::layout::{ESTIMATOR_ROWS, MOTION_SEMANTIC_ROWS, NOT_AVAILABLE};
use crate::pages::draw::{format_i32, format_tenths, format_u16};
use crate::surface::Surface;

impl Surface<'_> {
    pub(crate) fn stamp_attitude(&mut self) {
        self.stamp_text(b"ATTITUDE", block_x(Block::First), overlay_row_top(0));
        let Some(sample) = self.diagnostics.motion else {
            self.stamp_left(b"ERR", b"WAIT", Block::First, 1);
            return;
        };
        if !sample.valid {
            self.stamp_left(b"ERR", b"READ", Block::First, 1);
            return;
        }
        self.stamp_horizon(&sample);

        const BLOCKS: [(u8, usize); 5] = [(b'A', 1), (b'M', 4), (b'G', 7), (b'D', 10), (b'R', 13)];
        for &(prefix, first_row) in &BLOCKS {
            for axis in 0..3 {
                let mut buf = [0u8; 12];
                let value = match prefix {
                    b'A' => format_i32(i32::from(sample.raw_accel[axis]), &mut buf),
                    b'M' => format_i32(sample.accel_mg[axis], &mut buf),
                    b'G' => format_i32(i32::from(sample.raw_gyro[axis]), &mut buf),
                    b'D' => format_tenths(sample.gyro_dps_x10[axis], &mut buf),
                    _ => format_tenths(i32::from(sample.tilt_deg_x10[axis]), &mut buf),
                };
                self.stamp_left(
                    &[prefix, b'0' + axis as u8],
                    value,
                    Block::First,
                    first_row + axis,
                );
            }
        }
        self.stamp_left(
            b"ST",
            format_u16(u16::from(sample.status), &mut [0; 6]),
            Block::Second,
            0,
        );
        self.stamp_left(
            b"LR",
            format_i32(sample.gravity_deviation_mg, &mut [0; 12]),
            Block::Second,
            1,
        );
        self.stamp_left(
            b"SR",
            format_i32(sample.shake_residual_mg, &mut [0; 12]),
            Block::Second,
            2,
        );
        self.stamp_left(
            b"TR",
            format_i32(sample.tap_residual_mg, &mut [0; 12]),
            Block::Second,
            3,
        );
        let counts = self.diagnostics.motion_counts;
        let caps = self.diagnostics.motion_caps;
        let rows: [(&[u8], MotionCapabilities, u16); MOTION_SEMANTIC_ROWS] = [
            (b"TAP".as_slice(), MotionCapabilities::TAP, counts.taps),
            (
                b"2T".as_slice(),
                MotionCapabilities::TAP,
                counts.double_taps,
            ),
            (
                b"3T".as_slice(),
                MotionCapabilities::TAP,
                counts.triple_taps,
            ),
            (b"STL".as_slice(), MotionCapabilities::STILL, counts.still),
            (b"MOV".as_slice(), MotionCapabilities::MOVING, counts.moving),
            (
                b"ACT".as_slice(),
                MotionCapabilities::ACTIVITY,
                counts.activity,
            ),
            (b"STP".as_slice(), MotionCapabilities::STEP, counts.steps),
            (
                b"TIN".as_slice(),
                MotionCapabilities::TILT,
                counts.tilt_enters,
            ),
            (
                b"TOX".as_slice(),
                MotionCapabilities::TILT,
                counts.tilt_exits,
            ),
            (b"SHK".as_slice(), MotionCapabilities::SHAKE, counts.shakes),
            (
                b"LFT".as_slice(),
                MotionCapabilities::LIFT_PLACE,
                counts.lifts,
            ),
            (
                b"PLC".as_slice(),
                MotionCapabilities::LIFT_PLACE,
                counts.places,
            ),
            (
                b"PRT".as_slice(),
                MotionCapabilities::POSTURE,
                counts.portraits,
            ),
            (
                b"LND".as_slice(),
                MotionCapabilities::POSTURE,
                counts.landscapes,
            ),
        ];
        let mut buf = [0u8; 6];
        for (index, (label, capability, count)) in rows.into_iter().enumerate() {
            let value: &[u8] = if caps.contains(capability) {
                format_u16(count, &mut buf)
            } else {
                NOT_AVAILABLE
            };
            self.stamp_left(label, value, Block::Second, ESTIMATOR_ROWS + index);
        }
    }
}
