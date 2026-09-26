use super::input::{InputEvent, InputSource};
use libm::{atan2f, sqrtf};

mod event;
mod recognizer;

pub use event::{
    MOTION_COOLDOWN_MS, MOTION_EVENT_CAPACITY, MotionArbiter, MotionBatch, MotionCapabilities,
    MotionCounts, MotionEvent, MotionEvents, MotionGroup, RECOGNIZER_CAPABILITIES, TiltDir,
};
pub use recognizer::{
    DWELL_MS, GRAVITY_MG, MotionRecognizer, POSTURE_FLAT_MG, SHAKE_GRAVITY_TAU_MS, SHAKE_ON_MG,
    SHAKE_PEAKS, SHAKE_REFRACTORY_MS, SHAKE_WINDOW_MS, STILL_GRAVITY_DEV_MG, TAP_BASELINE_TAU_MS,
    TAP_DOUBLE_WINDOW_MS, TAP_PEAK_MAG_MG2, TAP_PEAK_WINDOW_MS, TAP_QUIET_MG2, TAP_QUIET_WINDOW_MS,
    TAP_SETTLED_MOTION_MS, TILT_ENTER_DEG_X10, TILT_EXIT_DEG_X10,
};

/// Poll cadence of the semantic/data plane. Every source shares the app's 5 ms
/// base tick, so this is four of those ticks: fast enough that a knock or a
/// lift is not missed between polls, slow enough to leave the shared I2C bus
/// to the touch and button scanners.
pub const MOTION_SCAN_MS: u64 = 20;

/// A quiet device must still refresh the readout, but a moving one need not
/// publish every intermediate frame. Emitting only past these deltas keeps a
/// 50 Hz stream from saturating the intent bus while the recognizer below still
/// sees every poll.
const EMIT_DELTA_MG: i32 = 20;
const EMIT_DELTA_DPS_X10: i32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionError {
    NotReady,
    Bus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MotionSample {
    pub raw_accel: [i16; 3],
    pub raw_gyro: [i16; 3],
    pub accel_mg: [i32; 3],
    pub gyro_dps_x10: [i32; 3],
    /// Roll, pitch, then an integrated yaw. Without a magnetometer the yaw
    /// drifts without bound, so it is a diagnostic readout only and no semantic
    /// or business decision may read it.
    pub tilt_deg_x10: [i16; 3],
    pub status: u8,
    /// The gravity-removed accelerometer magnitude's root against the tap
    /// baseline, in mG. This is the number the squared-domain peak bar
    /// (`TAP_PEAK_MAG_MG2`) decides on, published so the threshold can be read
    /// off the device instead of inferred from a flash, mirroring `SR`.
    pub tap_residual_mg: i32,
    /// How far the measured magnitude sits from one g, in mG. This is the
    /// number the lift/place still band measures in the squared domain via
    /// [`crate::drivers::motion::recognizer`], published so a threshold can be
    /// read off the device instead of inferred from a flash.
    pub gravity_deviation_mg: i32,
    /// The gravity-removed accelerometer magnitude against the shake estimate,
    /// taken as a vector length. This is the number [`SHAKE_ON_MG`] decides on.
    pub shake_residual_mg: i32,
    pub valid: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionScale {
    pub accel_mg_per_lsb: f32,
    pub gyro_dps_x10_per_lsb: f32,
}

impl MotionScale {
    pub const fn from_ranges(accel_lsb_per_g: i32, gyro_lsb_per_dps: i32) -> Self {
        Self {
            accel_mg_per_lsb: 1_000.0 / accel_lsb_per_g as f32,
            gyro_dps_x10_per_lsb: 10.0 / gyro_lsb_per_dps as f32,
        }
    }
}

impl MotionSample {
    pub const fn error() -> Self {
        Self {
            raw_accel: [0; 3],
            raw_gyro: [0; 3],
            accel_mg: [0; 3],
            gyro_dps_x10: [0; 3],
            tilt_deg_x10: [0; 3],
            status: 0,
            tap_residual_mg: 0,
            gravity_deviation_mg: 0,
            shake_residual_mg: 0,
            valid: false,
        }
    }

    pub fn from_raw(
        raw_accel: [i16; 3],
        raw_gyro: [i16; 3],
        scale: MotionScale,
        status: u8,
        yaw_deg_x10: i16,
    ) -> Self {
        let accel_mg = raw_accel.map(|value| scaled_i32(value as f32, scale.accel_mg_per_lsb));
        let gyro_dps_x10 =
            raw_gyro.map(|value| scaled_i32(value as f32, scale.gyro_dps_x10_per_lsb));
        let ax = f32::from(raw_accel[0]);
        let ay = f32::from(raw_accel[1]);
        let az = f32::from(raw_accel[2]);
        let roll = atan2f(ay, az) * 57.29578;
        let pitch = atan2f(ax, sqrtf(ay * ay + az * az)) * 57.29578;
        let yaw = wrap_tenths(yaw_deg_x10);
        Self {
            raw_accel,
            raw_gyro,
            accel_mg,
            gyro_dps_x10,
            tilt_deg_x10: [scaled_i16(roll * 10.0), scaled_i16(pitch * 10.0), yaw],
            status,
            tap_residual_mg: 0,
            gravity_deviation_mg: 0,
            shake_residual_mg: 0,
            valid: true,
        }
    }

    /// Whether a poll moved far enough to be worth publishing. Validity drives
    /// this alone, so a fault surfaces on the frame it appears and clears on
    /// the frame it clears.
    fn materially_changed(&self, previous: &Self) -> bool {
        self.valid != previous.valid
            || axis_spread(self.accel_mg, previous.accel_mg) > EMIT_DELTA_MG
            || axis_spread(self.gyro_dps_x10, previous.gyro_dps_x10) > EMIT_DELTA_DPS_X10
    }
}

fn axis_spread(current: [i32; 3], previous: [i32; 3]) -> i32 {
    (0..3)
        .map(|axis| (current[axis] - previous[axis]).abs())
        .max()
        .unwrap_or_default()
}

fn scaled_i32(value: f32, scale: f32) -> i32 {
    if value >= 0.0 {
        (value * scale + 0.5) as i32
    } else {
        (value * scale - 0.5) as i32
    }
}

fn scaled_i16(value: f32) -> i16 {
    scaled_i32(value, 1.0).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

fn wrap_tenths(value: i16) -> i16 {
    let mut wrapped = i32::from(value) % 3_600;
    if wrapped > 1_800 {
        wrapped -= 3_600;
    } else if wrapped < -1_800 {
        wrapped += 3_600;
    }
    wrapped as i16
}

/// One successful poll: the data plane plus whatever the source's own hardware
/// engines reported. The core-side recognizer adds its semantics on top, and
/// both sets are arbitrated together before leaving the driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MotionReading {
    pub sample: MotionSample,
    pub hardware: MotionEvents,
}

impl MotionReading {
    pub fn new(sample: MotionSample) -> Self {
        Self {
            sample,
            hardware: MotionEvents::new(),
        }
    }
}

/// A motion source in whatever position it occupies on the bus. An
/// implementation may report a hardware engine's verdict, raw telemetry, or
/// both; the framework never assumes which.
pub trait MotionSource {
    /// `Ok(None)` means the sensor had no new frame, which is ordinary at this
    /// cadence and must not be reported as a fault.
    fn sample(&mut self, now_ms: u64) -> Result<Option<MotionReading>, MotionError>;
}

pub struct MotionInput<S> {
    source: S,
    recognizer: MotionRecognizer,
    arbiter: MotionArbiter,
    last: Option<MotionSample>,
}

impl<S> MotionInput<S> {
    pub const fn new(source: S) -> Self {
        Self {
            source,
            recognizer: MotionRecognizer::new(),
            arbiter: MotionArbiter::new(),
            last: None,
        }
    }
}

impl<S: MotionSource> InputSource for MotionInput<S> {
    fn poll(&mut self, now_ms: u64) -> Option<InputEvent> {
        let reading = match self.source.sample(now_ms) {
            Ok(Some(reading)) => reading,
            Ok(None) => return None,
            Err(_) => MotionReading::new(MotionSample::error()),
        };
        let mut sample = reading.sample;
        let mut events = reading.hardware;
        events.extend_from(self.recognizer.recognize(&mut sample, now_ms));
        self.arbiter.resolve(now_ms, &mut events);

        let publishable = match self.last {
            Some(previous) => sample.materially_changed(&previous),
            None => true,
        };
        self.last = Some(sample);
        if !publishable && events.is_empty() {
            return None;
        }
        Some(InputEvent::Motion(MotionBatch { sample, events }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scaled(accel: [i16; 3], gyro: [i16; 3], yaw: i16) -> MotionSample {
        MotionSample::from_raw(accel, gyro, MotionScale::from_ranges(8_192, 64), 3, yaw)
    }

    fn reading(sample: MotionSample) -> MotionReading {
        MotionReading::new(sample)
    }

    fn telemetry(
        accel_mg: [i32; 3],
        gyro_dps_x10: [i32; 3],
        tilt_deg_x10: [i16; 3],
    ) -> MotionSample {
        MotionSample {
            accel_mg,
            gyro_dps_x10,
            tilt_deg_x10,
            valid: true,
            ..MotionSample::error()
        }
    }

    struct Scripted {
        frames: Vec<Result<Option<MotionReading>, MotionError>>,
        index: usize,
    }

    impl MotionSource for Scripted {
        fn sample(&mut self, _now_ms: u64) -> Result<Option<MotionReading>, MotionError> {
            let frame = self.frames[self.index % self.frames.len()];
            self.index += 1;
            frame
        }
    }

    fn scripted(frames: Vec<Result<Option<MotionReading>, MotionError>>) -> Scripted {
        Scripted { frames, index: 0 }
    }

    #[test]
    fn raw_sample_scales_accel_gyro_and_tilt() {
        let sample = scaled([0, 8192, 0], [64, -64, 0], 0);
        assert_eq!(sample.accel_mg, [0, 1000, 0]);
        assert_eq!(sample.gyro_dps_x10, [10, -10, 0]);
        assert_eq!(sample.tilt_deg_x10, [900, 0, 0]);
        assert!(sample.valid);
    }

    #[test]
    fn an_idle_sensor_reports_nothing_and_never_a_fault() {
        let mut input = MotionInput::new(scripted(vec![Ok(None)]));
        for poll in 0..20 {
            assert_eq!(
                input.poll(poll * MOTION_SCAN_MS),
                None,
                "a stale frame is not an error"
            );
        }
    }

    #[test]
    fn a_quiet_device_publishes_once_then_goes_silent() {
        let still = reading(scaled([0, 0, 8192], [0, 0, 0], 0));
        let mut input = MotionInput::new(scripted(vec![Ok(Some(still))]));
        let first = input
            .poll(0)
            .expect("the first frame has nothing to compare against");
        let InputEvent::Motion(batch) = first else {
            panic!("a motion source must publish a batch");
        };
        assert!(batch.events.is_empty());
        assert_eq!(batch.sample, still.sample);
        assert_eq!(
            input.poll(MOTION_SCAN_MS),
            None,
            "an unchanged frame is not news"
        );
    }

    #[test]
    fn a_bounded_wobble_stays_off_the_bus_but_a_real_move_gets_through() {
        let drift = scaled([1, 1, 8192], [0, 0, 0], 0);
        let mut input = MotionInput::new(scripted(vec![Ok(Some(reading(drift)))]));
        assert!(input.poll(0).is_some(), "the first frame always publishes");
        assert_eq!(
            input.poll(MOTION_SCAN_MS),
            None,
            "sub-threshold jitter must not publish"
        );

        let still = scaled([0, 0, 8192], [0, 0, 0], 0);
        let moved = scaled([400, 0, 7900], [0, 0, 0], 0);
        let mut input = MotionInput::new(scripted(vec![
            Ok(Some(reading(still))),
            Ok(Some(reading(moved))),
        ]));
        assert!(input.poll(0).is_some());
        assert!(
            input.poll(MOTION_SCAN_MS).is_some(),
            "a real move clears the delta floor"
        );
    }

    #[test]
    fn a_fault_surfaces_once_and_clears_on_recovery() {
        let healthy = scaled([0, 0, 8192], [0, 0, 0], 0);
        let mut input = MotionInput::new(scripted(vec![
            Ok(Some(reading(healthy))),
            Err(MotionError::Bus),
            Err(MotionError::Bus),
            Ok(Some(reading(healthy))),
        ]));
        assert!(input.poll(0).is_some());
        let fault = input
            .poll(MOTION_SCAN_MS)
            .expect("a bus fault must surface");
        let InputEvent::Motion(batch) = fault else {
            panic!("a fault still travels as a batch")
        };
        assert!(!batch.sample.valid);
        assert_eq!(
            input.poll(MOTION_SCAN_MS * 2),
            None,
            "a held fault is not re-published"
        );
        let recovered = input
            .poll(MOTION_SCAN_MS * 3)
            .expect("recovery must surface");
        let InputEvent::Motion(batch) = recovered else {
            panic!("recovery travels as a batch")
        };
        assert!(batch.sample.valid);
    }

    #[test]
    fn a_hardware_posture_and_a_contradicting_software_one_collapse() {
        let mut hardware = MotionEvents::new();
        assert!(hardware.push(MotionEvent::Portrait));
        let mut input = MotionInput::new(scripted(vec![Ok(Some(MotionReading {
            sample: telemetry([0, 900, 100], [0; 3], [0; 3]),
            hardware,
        }))]));

        let InputEvent::Motion(batch) = input.poll(0).expect("the frame publishes") else {
            panic!("expected a batch");
        };
        let published = batch.events.iter().collect::<Vec<_>>();
        assert_eq!(
            published,
            [MotionEvent::Portrait],
            "the engine's posture and the classifier's opposite one are one description"
        );
    }

    #[test]
    fn distinct_families_from_one_act_both_survive() {
        let mut hardware = MotionEvents::new();
        assert!(hardware.push(MotionEvent::Moving));
        let mut input = MotionInput::new(scripted(vec![Ok(Some(MotionReading {
            sample: telemetry([0, 900, 100], [0; 3], [TILT_ENTER_DEG_X10, 0, 3]),
            hardware,
        }))]));

        let InputEvent::Motion(batch) = input.poll(0).expect("the frame publishes") else {
            panic!("expected a batch");
        };
        let published = batch.events.iter().collect::<Vec<_>>();
        assert_eq!(
            published,
            [MotionEvent::Moving, MotionEvent::TiltEnter(TiltDir::Right)],
            "'in motion' and 'tilted' are separate claims about one act, not rivals"
        );
    }

    #[test]
    fn a_held_impulse_is_two_one_shot_transitions_not_a_burst() {
        let mut input = MotionInput::new(scripted(vec![Ok(Some(reading(MotionSample {
            accel_mg: [4_000, 0, 6_000],
            gyro_dps_x10: [0; 3],
            valid: true,
            ..MotionSample::error()
        })))]));

        let mut published = 0;
        let mut lifts = 0;
        for poll in 0..40 {
            if let Some(InputEvent::Motion(batch)) = input.poll(poll * MOTION_SCAN_MS) {
                published += usize::from(!batch.events.is_empty());
                lifts += usize::from(batch.events.iter().any(|event| event == MotionEvent::Lift));
            }
        }
        assert_eq!(
            published, 2,
            "a held excursion settles into a pose and a lift, one of each, not a burst"
        );
        assert_eq!(
            lifts, 1,
            "the transition into motion fires once, not per poll"
        );
    }
}
