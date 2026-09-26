use super::event::{MotionEvent, MotionEvents, TiltDir};
use super::{MOTION_SCAN_MS, MotionSample};

/// Tilt hysteresis: crossing the outer angle declares a tilt, and only falling
/// back inside the inner one retracts it. A single threshold would chatter at
/// the boundary and emit an enter/exit pair every poll.
pub const TILT_ENTER_DEG_X10: i16 = 150;
pub const TILT_EXIT_DEG_X10: i16 = 100;

/// W3C's reference shake threshold is 2.5 g on a 60 Hz single-axis magnitude;
/// this detector compares the vector residual instead and counts crossings. Two
/// things move the number: the recognizer only sees a sample every 20 ms, so a
/// crest-level threshold is missed between polls, and a 3 g shake leaves only
/// ~2.5 g of residual at 5 Hz. 2.0 g keeps two or three samples above the line
/// per reversal. Unverified on hardware; set from `SR`.
pub const SHAKE_ON_MG: i32 = 2_000;

/// One impulse is exactly what a desk knock looks like, so repetition is the
/// discriminator: a knock crosses the line once, a shake once per reversal.
/// Counted in crossings rather than time above the line — a leaky accumulator
/// cancels itself out, since a reversal spends most of its cycle below the
/// threshold.
pub const SHAKE_PEAKS: u8 = 3;

/// How far apart those crossings may be and still read as one gesture. Three
/// crossings have to fit inside it, which admits about 2.4 Hz and up while
/// still treating a slow deliberate turn as one excursion.
pub const SHAKE_WINDOW_MS: u16 = 800;

/// One gesture is one event, so a shake that is still going must not re-trigger
/// every confirm window. Matches the arbiter's cooldown.
pub const SHAKE_REFRACTORY_MS: u16 = 300;

/// Corner of the shake estimate, about 2 Hz at the same poll rate, kept short
/// enough to stay independent of the still test's band.
pub const SHAKE_GRAVITY_TAU_MS: i32 = 70;

/// One g in mG, the shell the still test measures against.
pub const GRAVITY_MG: i32 = 1_000;

/// Still test for the lift/place pair, on the departure of the measured
/// magnitude from one g. Subtracting a gravity estimate carried its error for
/// the whole run, hiding the desk/hand transition; raw magnitude drifts only at
/// the sensor's noise floor, so a band settled from the bench (8-22 mG rest
/// swing) keeps the 150 mG intent ST's LSM6DSO FSM escapes on. A smooth carry
/// stays at exactly one g and reads as still — the same blind spot ST shares.
pub const STILL_GRAVITY_DEV_MG: i32 = 150;

/// Dwell before a stillness transition is believed. ST waits 3 s, which reads
/// as unresponsive on a desk device; 150 ms keeps the debounce without the lag.
pub const DWELL_MS: u16 = 150;

/// Tap peak bar, in mG² on the squared gravity-removed magnitude, set from an
/// on-device probe: a deliberate knock peaks at 364-4866 mG against 9-38 mG of
/// resting noise, and 250 mG clears the noise ~7x while reaching everyday
/// tapping. A pick-up rises slowly and holds, so a knock must step out of a
/// quiet sample to count. `TR` publishes the root.
pub const TAP_PEAK_MAG_MG2: i64 = 62_500;

/// The level a peak must fall under and a tap's tail must stay under while the
/// gesture resolves, from the same probe: resting noise is 9-38 mG and a
/// knock's ringing tail reaches ~110-140 mG. At 150 the bar clears the noise 9x
/// while a pick-up's sustained hold crosses any 80 ms conformance window. Set
/// from the bench, like the shake's `SR`.
pub const TAP_QUIET_MG2: i64 = 22_500;

/// A knock is a transient, an excursion that lasts longer is a press or a turn,
/// so the peak has to decay under the quiet bar before this budget runs out.
pub const TAP_PEAK_WINDOW_MS: u16 = 80;

/// A resolved peak has to stay below the quiet bar this long before it counts
/// as an individual knock, so the ringing tail of one knock is not two.
pub const TAP_QUIET_WINDOW_MS: u16 = 80;

/// How far apart two knocks may land and still roll into one gesture; a single
/// tap also waits out this window before it reports. People double-tap
/// 150-300 ms apart, comfortably inside 500 ms.
pub const TAP_DOUBLE_WINDOW_MS: u16 = 500;

/// How much of the double-tap window a gesture may spend above the quiet bar
/// before it reads as an ongoing turn: a knock settles under the bar, a 2 Hz
/// turn rings over half the window.
pub const TAP_SETTLED_MOTION_MS: u16 = 200;

/// Corner of the tap baseline, tracking gravity on a slow enough constant that
/// a knock's 20-60 ms transient stays in the residual instead of being absorbed
/// as a change of orientation.
pub const TAP_BASELINE_TAU_MS: i32 = 300;

/// Below this on both in-plane axes the device is flat and has no meaningful
/// portrait/landscape reading, so neither is reported.
pub const POSTURE_FLAT_MG: i32 = 300;

/// Fixed-point denominator for the gravity blend, so the estimate can move by a
/// fraction of a milli-gravity without floating point on the target.
const GRAVITY_SCALE: i32 = 1_024;

/// Derives core-side semantics from the data plane. Every threshold lives here
/// rather than in a driver, so swapping the sensor cannot change what the
/// business layer means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionRecognizer {
    tilt: Option<TiltDir>,
    posture: Option<MotionEvent>,
    /// Gravity estimate on a short time constant tuned so the shake band
    /// survives the high-pass instead of being absorbed as gravity.
    shake_gravity_mg: [i32; 3],
    /// Until the first sample arrives the estimate is unknown, and treating zero
    /// as gravity would read a device that boots mid-shake as a full g of motion.
    gravity_ready: bool,
    last_ms: Option<u64>,
    shake_peaks: u8,
    shake_window_ms: u16,
    /// Whether the last poll was already over the line, so a crossing is counted
    /// once per excursion instead of once per poll spent above it.
    shake_above: bool,
    shake_refractory_ms: u16,
    still_ms: u16,
    motion_ms: u16,
    lifted: bool,
    /// Tap baseline on a slow corner, so the knock transient survives the
    /// subtraction while a deliberate turn is absorbed as orientation.
    tap_baseline_mg: [i32; 3],
    /// Whether the last poll sat above the peak bar, so one excursion is one
    /// crossing no matter how long it is held up there.
    tap_above: bool,
    /// Whether the last poll sat under the quiet bar. A rising edge only counts
    /// as a blow when it arrives from an actually-quiet sample: a knock is a
    /// step from rest, while a turn's residual climbs slowly across the bars,
    /// so the one-poll jump is what tells the two apart.
    tap_prev_quiet: bool,
    tap_phase: TapPhase,
    /// How many knocks have counted into the gesture in progress.
    tap_knocks: u8,
    /// When the current phase was entered, for the peak/quiet budgets.
    tap_at_ms: Option<u64>,
    /// When the first knock of a gesture counted, the anchor of the double-tap
    /// window. `None` until a tap is actually in progress.
    tap_from_ms: Option<u64>,
    /// How long the gesture has spent above the quiet bar since `tap_from_ms`,
    /// for the settled-motion gate a turn's ringing accumulates past.
    tap_motion_ms: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TapPhase {
    /// No peak in flight; waiting for the squared residual to cross the bar.
    Idle,
    /// A peak is being followed, under the decay budget: if the residual falls
    /// under the quiet bar the peak is a blow, if the budget runs out first it
    /// was a press or a turn and the excursion is dropped.
    Peak,
    /// A valid peak, waiting out the quiet window so the ringing tail of one
    /// knock is not read as a second one.
    Quiet,
    /// At least one knock has counted; another landing inside the double-tap
    /// window rolls into the same gesture, and the window's end is the report.
    Between,
}

impl MotionRecognizer {
    pub const fn new() -> Self {
        Self {
            tilt: None,
            posture: None,
            shake_gravity_mg: [0; 3],
            gravity_ready: false,
            last_ms: None,
            shake_peaks: 0,
            shake_window_ms: 0,
            shake_above: false,
            shake_refractory_ms: 0,
            still_ms: 0,
            motion_ms: 0,
            lifted: false,
            tap_baseline_mg: [0; 3],
            tap_above: false,
            tap_prev_quiet: false,
            tap_phase: TapPhase::Idle,
            tap_knocks: 0,
            tap_at_ms: None,
            tap_from_ms: None,
            tap_motion_ms: 0,
        }
    }

    /// Consumes every poll, including ones that raise nothing, because the
    /// stillness and confirm accumulators are time-based and would drift if a
    /// quiet device skipped them.
    pub fn recognize(&mut self, sample: &mut MotionSample, now_ms: u64) -> MotionEvents {
        let mut events = MotionEvents::new();
        if !sample.valid {
            return events;
        }
        let elapsed_ms = self.elapsed_ms(now_ms);
        if self.gravity_ready {
            blend_gravity(
                &mut self.shake_gravity_mg,
                sample.accel_mg,
                elapsed_ms,
                SHAKE_GRAVITY_TAU_MS,
            );
        } else {
            // The first valid sample defines orientation instead of being
            // measured against it, so a boot mid-handling reads as no motion.
            self.shake_gravity_mg = sample.accel_mg;
            self.tap_baseline_mg = sample.accel_mg;
            self.gravity_ready = true;
        }
        let shake_mg = vector_residual_mg(sample.accel_mg, self.shake_gravity_mg);
        sample.gravity_deviation_mg = gravity_deviation_mg(sample.accel_mg);
        sample.shake_residual_mg = shake_mg;

        self.update_tilt(&sample.tilt_deg_x10, &mut events);
        self.update_posture(sample.accel_mg, &mut events);
        self.update_shake(shake_mg, elapsed_ms, &mut events);
        self.update_lift_place(sample.accel_mg, elapsed_ms, &mut events);
        self.update_tap(sample, elapsed_ms, now_ms, &mut events);

        events
    }

    fn elapsed_ms(&mut self, now_ms: u64) -> u16 {
        let elapsed = self
            .last_ms
            .map_or(MOTION_SCAN_MS, |previous| now_ms.saturating_sub(previous))
            .min(u64::from(u16::MAX));
        self.last_ms = Some(now_ms);
        elapsed as u16
    }

    fn update_tilt(&mut self, tilt_deg_x10: &[i16; 3], events: &mut MotionEvents) {
        let roll = i32::from(tilt_deg_x10[0]);
        let pitch = i32::from(tilt_deg_x10[1]);
        if let Some(current) = self.tilt
            && !tilt_holds(current, roll, pitch)
        {
            self.tilt = None;
            events.push(MotionEvent::TiltExit(current));
        }
        if self.tilt.is_none()
            && let Some(next) = dominant_tilt(roll, pitch)
        {
            self.tilt = Some(next);
            events.push(MotionEvent::TiltEnter(next));
        }
    }

    fn update_posture(&mut self, accel_mg: [i32; 3], events: &mut MotionEvents) {
        let horizontal = accel_mg[0].abs();
        let vertical = accel_mg[1].abs();
        let next = if horizontal.max(vertical) < POSTURE_FLAT_MG {
            None
        } else if horizontal > vertical {
            Some(MotionEvent::Landscape)
        } else {
            Some(MotionEvent::Portrait)
        };
        if next != self.posture {
            if let Some(current) = next {
                events.push(current);
            }
            self.posture = next;
        }
    }

    /// Confirms the gesture by counting reversals, each crossing carrying one
    /// count against the window running on the clock rather than on polls.
    fn update_shake(&mut self, shake_mg: i32, elapsed_ms: u16, events: &mut MotionEvents) {
        if self.shake_refractory_ms > 0 {
            self.shake_refractory_ms = self.shake_refractory_ms.saturating_sub(elapsed_ms);
            return;
        }
        if self.shake_window_ms < elapsed_ms {
            self.shake_window_ms = 0;
            self.shake_peaks = 0;
        } else {
            self.shake_window_ms -= elapsed_ms;
        }
        if shake_mg <= SHAKE_ON_MG {
            self.shake_above = false;
            return;
        }
        if self.shake_above {
            return;
        }
        self.shake_above = true;
        if self.shake_window_ms == 0 {
            self.shake_window_ms = SHAKE_WINDOW_MS;
        }
        self.shake_peaks += 1;
        if self.shake_peaks >= SHAKE_PEAKS {
            self.shake_peaks = 0;
            self.shake_window_ms = 0;
            self.shake_refractory_ms = SHAKE_REFRACTORY_MS;
            events.push(MotionEvent::Shake);
        }
    }

    fn update_lift_place(
        &mut self,
        accel_mg: [i32; 3],
        elapsed_ms: u16,
        events: &mut MotionEvents,
    ) {
        if is_still(accel_mg) {
            self.still_ms = self.still_ms.saturating_add(elapsed_ms);
            self.motion_ms = 0;
        } else {
            self.motion_ms = self.motion_ms.saturating_add(elapsed_ms);
            self.still_ms = 0;
        }
        if !self.lifted && self.motion_ms >= DWELL_MS {
            self.lifted = true;
            events.push(MotionEvent::Lift);
        } else if self.lifted && self.still_ms >= DWELL_MS {
            self.lifted = false;
            events.push(MotionEvent::Place);
        }
    }

    /// The knock detector the QMI8658A tap engine could not deliver (it latched a
    /// stuck tap bit at its enable transient and never resolved a blow). A
    /// per-axis moving average is subtracted and the squared sum compared
    /// straight against both bars, the windows below being the datasheet's 10.1
    /// walk-through decay semantics; a peak that does not decay under the quiet
    /// bar inside its window is a press or a turn, and first-knock departures
    /// anchor the double-tap window so a double reports once with its count.
    fn update_tap(
        &mut self,
        sample: &mut MotionSample,
        elapsed_ms: u16,
        now_ms: u64,
        events: &mut MotionEvents,
    ) {
        // The gravity corner only runs at rest. A blow is a gravity-independent
        // transient, so absorbing it leaves an overshoot that lingers over the
        // quiet bar and starves a later blow's peak window; the blend is frozen
        // for the whole gesture and resumes after.
        if matches!(self.tap_phase, TapPhase::Idle) {
            blend_gravity(
                &mut self.tap_baseline_mg,
                sample.accel_mg,
                elapsed_ms,
                TAP_BASELINE_TAU_MS,
            );
        }
        let mut square_sum: i64 = 0;
        for axis in 0..3 {
            let linear = i64::from(sample.accel_mg[axis]) - i64::from(self.tap_baseline_mg[axis]);
            square_sum += linear * linear;
        }
        let residual = if square_sum >= i64::from(i32::MAX) {
            i32::MAX
        } else {
            isqrt(square_sum as i32)
        };
        sample.tap_residual_mg = residual;

        let rested = self.tap_prev_quiet;
        let above = square_sum > TAP_PEAK_MAG_MG2;
        // A knock is a step out of rest, so only a rise from an actually-quiet
        // sample counts; a turn climbs across the bars over a few polls and
        // never assembles that one jump.
        let rising = !self.tap_above && above && rested;
        self.tap_above = above;

        match self.tap_phase {
            TapPhase::Idle => {
                if rising {
                    self.tap_phase = TapPhase::Peak;
                    self.tap_at_ms = Some(now_ms);
                }
            }
            TapPhase::Peak => {
                if square_sum <= TAP_QUIET_MG2 && !self.expired(now_ms, TAP_PEAK_WINDOW_MS) {
                    self.tap_phase = TapPhase::Quiet;
                    self.tap_at_ms = Some(now_ms);
                } else if self.expired(now_ms, TAP_PEAK_WINDOW_MS) {
                    // The budget ran out above the quiet bar: an excursion that
                    // stays high is a press or a turn, not a blow.
                    self.tap_phase = TapPhase::Idle;
                    self.tap_at_ms = None;
                }
            }
            TapPhase::Quiet => {
                if rising {
                    // A second blow inside the quiet window is the ringing tail
                    // of the first, not a separate knock.
                    self.tap_phase = TapPhase::Peak;
                    self.tap_at_ms = Some(now_ms);
                } else if self.expired(now_ms, TAP_QUIET_WINDOW_MS) {
                    if self.tap_knocks == 0 {
                        self.tap_from_ms = Some(now_ms);
                    }
                    if self.tap_knocks < 3 {
                        self.tap_knocks += 1;
                    }
                    self.tap_phase = TapPhase::Between;
                    self.tap_at_ms = None;
                }
            }
            TapPhase::Between => {
                if rising {
                    self.tap_phase = TapPhase::Peak;
                    self.tap_at_ms = Some(now_ms);
                } else if self.gesture_ended(now_ms) {
                    // A settling knock spends the window near rest; one that has rung it
                    // over the quiet bar most of the way was never a blow.
                    if self.tap_motion_ms < TAP_SETTLED_MOTION_MS {
                        events.push(MotionEvent::Tap {
                            count: self.tap_knocks,
                        });
                    }
                    self.tap_phase = TapPhase::Idle;
                    self.tap_knocks = 0;
                    self.tap_at_ms = None;
                    self.tap_from_ms = None;
                    self.tap_motion_ms = 0;
                }
            }
        }
        if self.tap_from_ms.is_some() && square_sum > TAP_QUIET_MG2 {
            self.tap_motion_ms = self.tap_motion_ms.saturating_add(elapsed_ms);
        }
        self.tap_prev_quiet = square_sum <= TAP_QUIET_MG2;
    }

    fn expired(&self, now_ms: u64, window_ms: u16) -> bool {
        self.tap_at_ms
            .is_some_and(|at| now_ms.saturating_sub(at) > u64::from(window_ms))
    }

    fn gesture_ended(&self, now_ms: u64) -> bool {
        self.tap_from_ms
            .is_some_and(|from| now_ms.saturating_sub(from) > u64::from(TAP_DOUBLE_WINDOW_MS))
    }
}

impl Default for MotionRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Moves a gravity estimate a fraction of the way toward the current reading,
/// the fraction falling out of the elapsed time over the time constant.
fn blend_gravity(estimate: &mut [i32; 3], accel_mg: [i32; 3], elapsed_ms: u16, tau_ms: i32) {
    let elapsed = i32::from(elapsed_ms);
    let blend = elapsed * GRAVITY_SCALE / (tau_ms + elapsed);
    for (axis, &measured) in accel_mg.iter().enumerate() {
        estimate[axis] += (measured - estimate[axis]) * blend / GRAVITY_SCALE;
    }
}

fn magnitude_squared(accel_mg: [i32; 3]) -> i64 {
    let mut mag_sq = 0i64;
    for axis in accel_mg {
        let value = i64::from(axis);
        mag_sq += value * value;
    }
    mag_sq
}

/// Signed deviation of the measured magnitude from one g, in mG. Only the `LR`
/// readout takes the root; every decision measures the band in the squared
/// domain, where the bounds stay integer.
fn gravity_deviation_mg(accel_mg: [i32; 3]) -> i32 {
    (isqrt(magnitude_squared(accel_mg) as i32) - GRAVITY_MG).abs()
}

/// The stillness test as a band on the squared magnitude: an interval on the
/// non-negative magnitude is an interval on its square, keeping the check in
/// the pure-integer domain.
fn is_still(accel_mg: [i32; 3]) -> bool {
    let mag_sq = magnitude_squared(accel_mg);
    let lo = (GRAVITY_MG - STILL_GRAVITY_DEV_MG) as i64;
    let hi = (GRAVITY_MG + STILL_GRAVITY_DEV_MG) as i64;
    mag_sq > lo * lo && mag_sq < hi * hi
}

/// The residual taken as a vector length, which is what the shake test wants.
fn vector_residual_mg(accel_mg: [i32; 3], estimate: [i32; 3]) -> i32 {
    let mut squared = 0i32;
    for (axis, &measured) in accel_mg.iter().enumerate() {
        let delta = measured - estimate[axis];
        squared += delta * delta;
    }
    isqrt(squared)
}

/// Integer square root by Newton iteration, keeping the shake magnitude on the
/// same fixed-point footing as the rest of the recognizer.
fn isqrt(value: i32) -> i32 {
    if value <= 0 {
        return 0;
    }
    let mut root = value;
    let mut next = (root + 1) / 2;
    while next < root {
        root = next;
        next = (root + value / root) / 2;
    }
    root
}

/// Hysteresis is signed, not a magnitude test: rolling from +20° to -20° keeps
/// the same absolute angle, so a magnitude test would hold the old direction
/// and the flip would never register as an exit.
fn tilt_holds(dir: TiltDir, roll: i32, pitch: i32) -> bool {
    let exit = i32::from(TILT_EXIT_DEG_X10);
    match dir {
        TiltDir::Right => roll >= exit,
        TiltDir::Left => roll <= -exit,
        TiltDir::Up => pitch >= exit,
        TiltDir::Down => pitch <= -exit,
    }
}

fn dominant_tilt(roll: i32, pitch: i32) -> Option<TiltDir> {
    let enter = i32::from(TILT_ENTER_DEG_X10);
    if roll.abs() >= pitch.abs() {
        match roll {
            value if value >= enter => Some(TiltDir::Right),
            value if value <= -enter => Some(TiltDir::Left),
            _ => None,
        }
    } else {
        match pitch {
            value if value >= enter => Some(TiltDir::Up),
            value if value <= -enter => Some(TiltDir::Down),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(accel_mg: [i32; 3], tilt_deg_x10: [i16; 3]) -> MotionSample {
        MotionSample {
            accel_mg,
            tilt_deg_x10,
            valid: true,
            ..MotionSample::error()
        }
    }

    /// Where this recognizer's own time base currently stands, so a fixture
    /// resumed after an earlier feed continues instead of restarting at zero.
    fn cursor(recognizer: &MotionRecognizer) -> u64 {
        recognizer.last_ms.unwrap_or_default() / MOTION_SCAN_MS + 1
    }

    fn feed(
        recognizer: &mut MotionRecognizer,
        sample: &MotionSample,
        polls: u64,
    ) -> Vec<MotionEvent> {
        let start = cursor(recognizer);
        feed_from(recognizer, sample, start, polls)
    }

    fn feed_from(
        recognizer: &mut MotionRecognizer,
        sample: &MotionSample,
        start: u64,
        polls: u64,
    ) -> Vec<MotionEvent> {
        let mut frame = *sample;
        let mut seen = Vec::new();
        for offset in 0..polls {
            seen.extend(
                recognizer
                    .recognize(&mut frame, (start + offset) * MOTION_SCAN_MS)
                    .iter(),
            );
        }
        seen
    }

    /// Alternates the per-axis offset every poll, so it reads as motion
    /// rather than as a new orientation the estimate would absorb.
    fn swinging(offsets: [i32; 3]) -> impl FnMut(usize) -> MotionSample {
        let mut polls = 0;
        move |_| {
            let sign = if polls % 2 == 0 { 1 } else { -1 };
            polls += 1;
            sample(
                [
                    offsets[0] * sign,
                    offsets[1] * sign,
                    1_000 + offsets[2] * sign,
                ],
                [0; 3],
            )
        }
    }

    /// A sine on one axis at a fixed frequency, read off the recognizer's clock
    /// rather than a poll counter so the same gesture is rate-independent.
    fn tone(hz: f64, amplitude_mg: i32) -> impl FnMut(u64) -> MotionSample {
        move |now_ms| {
            let phase = 2.0 * core::f64::consts::PI * hz * now_ms as f64 / 1_000.0;
            sample(
                [(f64::from(amplitude_mg) * phase.sin()) as i32, 0, 1_000],
                [0; 3],
            )
        }
    }

    /// Gravity itself swinging, which is what deliberately turning the device
    /// looks like: the reading stays near one g and only its direction changes.
    fn turning(hz: f64, amplitude_mg: i32) -> impl FnMut(u64) -> MotionSample {
        move |now_ms| {
            let phase = 2.0 * core::f64::consts::PI * hz * now_ms as f64 / 1_000.0;
            let swing = f64::from(amplitude_mg) * phase.sin();
            let upright = (1_000.0 * 1_000.0 - swing * swing).max(0.0).sqrt();
            sample([swing as i32, 0, upright as i32], [0; 3])
        }
    }

    /// Settles a fresh recognizer on a device lying still, so a gesture starts
    /// from a converged estimate rather than from the priming sample.
    fn settled() -> (MotionRecognizer, u64) {
        let mut recognizer = MotionRecognizer::new();
        let still = sample([0, 0, 1_000], [0; 3]);
        feed(&mut recognizer, &still, 20);
        let start = recognizer.last_ms.unwrap();
        (recognizer, start)
    }

    fn shake_in(events: MotionEvents) -> bool {
        events.iter().any(|event| event == MotionEvent::Shake)
    }

    /// The same question over a run of events gathered across many polls.
    fn shook(events: &[MotionEvent]) -> bool {
        events.contains(&MotionEvent::Shake)
    }

    #[test]
    fn a_flat_still_device_reports_nothing() {
        let mut recognizer = MotionRecognizer::new();
        let flat = sample([0, 0, 1_000], [0; 3]);
        assert!(
            feed(&mut recognizer, &flat, 100).is_empty(),
            "a resting device must stay silent"
        );
    }

    #[test]
    fn tilt_enters_once_and_exits_only_past_the_inner_angle() {
        let mut recognizer = MotionRecognizer::new();
        let upright = sample([0, 0, 1_000], [0; 3]);
        feed(&mut recognizer, &upright, 10);

        let held = sample([0, 0, 1_000], [TILT_ENTER_DEG_X10, 0, 0]);
        assert_eq!(
            feed(&mut recognizer, &held, 5),
            [MotionEvent::TiltEnter(TiltDir::Right)]
        );

        let between = sample([0, 0, 1_000], [TILT_EXIT_DEG_X10 + 5, 0, 0]);
        assert!(
            feed(&mut recognizer, &between, 5).is_empty(),
            "hysteresis must not chatter"
        );

        let flat_again = sample([0, 0, 1_000], [0; 3]);
        assert_eq!(
            feed(&mut recognizer, &flat_again, 2),
            [MotionEvent::TiltExit(TiltDir::Right)]
        );
    }

    #[test]
    fn a_flipped_axis_raises_tilt_exit_and_enter_in_one_poll() {
        let mut recognizer = MotionRecognizer::new();
        let right = sample([0, 0, 1_000], [200, 0, 0]);
        feed(&mut recognizer, &right, 2);
        let left = sample([0, 0, 1_000], [-200, 0, 0]);
        assert_eq!(
            feed(&mut recognizer, &left, 1),
            [
                MotionEvent::TiltExit(TiltDir::Right),
                MotionEvent::TiltEnter(TiltDir::Left)
            ]
        );
    }

    #[test]
    fn the_first_sample_defines_the_orientation_instead_of_measuring_against_it() {
        let mut recognizer = MotionRecognizer::new();
        // A device picked up and shaken the instant it boots: with no estimate to
        // subtract, the very first reading is the orientation, not an impulse.
        let mut gesture = tone(5.0, 3_000);
        let mut first = gesture(0);
        let events = recognizer.recognize(&mut first, 0);
        assert!(
            !shake_in(events),
            "a boot-time gesture must not open with a phantom shake, got {:?}",
            events.iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_hand_shake_is_reported_across_the_band_a_hand_produces() {
        // One gravity estimate cannot serve the rest and the shake test at once:
        // at the rest corner a 5 Hz 3 g shake arrives at a third of its amplitude.
        for hz in [3.0, 5.0, 8.0, 10.0] {
            let (mut recognizer, start) = settled();
            let mut gesture = tone(hz, 3_000);
            let polls = 250;
            let mut fired = 0;
            for index in 0..polls {
                let now = start + (index + 1) * MOTION_SCAN_MS;
                fired += usize::from(shake_in(recognizer.recognize(&mut gesture(now), now)));
            }
            assert!(
                fired > 0,
                "a {hz} Hz shake of 3 g must report, fired {fired} times"
            );
        }
    }

    #[test]
    fn a_deliberate_turn_is_not_a_shake() {
        // Turning moves gravity without adding energy, so the gesture sits where
        // the shake estimate absorbs it as orientation.
        for hz in [0.5, 1.0, 2.0, 3.0] {
            let (mut recognizer, start) = settled();
            let mut gesture = turning(hz, 1_000);
            let mut seen = Vec::new();
            for index in 0..250 {
                let now = start + (index + 1) * MOTION_SCAN_MS;
                seen.extend(recognizer.recognize(&mut gesture(now), now).iter());
            }
            assert!(
                !shook(&seen),
                "turning the device at {hz} Hz is not a shake, got {seen:?}"
            );
        }
    }

    #[test]
    fn a_knock_is_one_crossing_and_does_not_reach_the_peak_count() {
        let (mut recognizer, start) = settled();
        let mut seen = Vec::new();
        for index in 0..60 {
            let now = start + (index + 1) * MOTION_SCAN_MS;
            let struck = index == 2 || index == 3;
            let mut impulse = sample([if struck { 3_400 } else { 0 }, 0, 1_000], [0; 3]);
            seen.extend(recognizer.recognize(&mut impulse, now).iter());
        }
        assert!(
            !shook(&seen),
            "one knock crosses the line once and must stay a knock, got {seen:?}"
        );
    }

    #[test]
    fn one_long_excursion_is_not_a_shake() {
        // Held and turned hard without reversing: one crossing, no gesture.
        let (mut recognizer, start) = settled();
        let mut seen = Vec::new();
        for index in 0..120 {
            let now = start + (index + 1) * MOTION_SCAN_MS;
            let mut held = sample([if index > 1 { 2_600 } else { 0 }, 0, 1_000], [0; 3]);
            seen.extend(recognizer.recognize(&mut held, now).iter());
        }
        assert!(
            !shook(&seen),
            "a sustained excursion has no reversals to count, got {seen:?}"
        );
    }

    #[test]
    fn a_gentle_oscillation_stays_under_the_threshold() {
        let (mut recognizer, start) = settled();
        let mut gesture = tone(5.0, 1_000);
        let mut seen = Vec::new();
        for index in 0..250 {
            let now = start + (index + 1) * MOTION_SCAN_MS;
            seen.extend(recognizer.recognize(&mut gesture(now), now).iter());
        }
        assert!(
            !shook(&seen),
            "1 g of oscillation is not a shake, got {seen:?}"
        );
    }

    #[test]
    fn a_magnitude_deviation_is_what_the_dwell_sees() {
        let mut recognizer = MotionRecognizer::new();
        let flat = sample([0, 0, 1_000], [0; 3]);
        feed(&mut recognizer, &flat, 5);

        // Picked up straight: gravity leaves one axis while the hand accelerates
        // it, pulling the magnitude off the one-g shell past the 150 mG band.
        let lifted = sample([0, 0, 500], [0; 3]);
        let moved = feed(&mut recognizer, &lifted, 10);
        assert!(moved.contains(&MotionEvent::Lift));
        assert!(!moved.contains(&MotionEvent::Place));

        let placed = feed(&mut recognizer, &flat, 10);
        assert!(placed.contains(&MotionEvent::Place));
    }

    #[test]
    fn the_still_band_sits_exactly_on_still_gravity_deviation_mg() {
        // The band comes from the constant: just below is still, just above is lift.
        let mut recognizer = MotionRecognizer::new();
        let flat = sample([0, 0, 1_000], [0; 3]);
        feed(&mut recognizer, &flat, 5);

        let inside = sample([0, 0, 1_000 - STILL_GRAVITY_DEV_MG + 30], [0; 3]);
        let seen = feed(&mut recognizer, &inside, 10);
        assert!(
            !seen.contains(&MotionEvent::Lift),
            "{STILL_GRAVITY_DEV_MG} mG band must still hold {}, got {seen:?}",
            1_000 - STILL_GRAVITY_DEV_MG + 30,
        );

        let outside = sample([0, 0, 1_000 - STILL_GRAVITY_DEV_MG - 30], [0; 3]);
        let seen = feed(&mut recognizer, &outside, 10);
        assert!(
            seen.contains(&MotionEvent::Lift),
            "{STILL_GRAVITY_DEV_MG} mG band must have been broken at {}, got {seen:?}",
            1_000 - STILL_GRAVITY_DEV_MG - 30,
        );
    }

    #[test]
    fn a_spin_keeps_the_magnitude_on_the_shell_and_reports_no_lift() {
        // Picked up without moving against gravity: magnitude stays on the shell.
        let mut recognizer = MotionRecognizer::new();
        let upright = sample([0, 0, 1_000], [0; 3]);
        feed(&mut recognizer, &upright, 10);
        let spun = sample([1_000, 0, 0], [0; 3]);
        let seen = feed(&mut recognizer, &spun, 10);
        assert!(
            !seen.contains(&MotionEvent::Lift) && !seen.contains(&MotionEvent::Place),
            "reorienting without leaving the shell is not a lift, got {seen:?}"
        );
    }

    #[test]
    fn the_published_residuals_are_the_numbers_the_thresholds_decide_on() {
        // Both readouts must carry the quantity the thresholds actually compare.
        let (mut recognizer, start) = settled();
        let still = sample([0, 0, 1_000], [0; 3]);
        let mut reading = still;
        recognizer.recognize(&mut reading, start + MOTION_SCAN_MS);
        assert!(
            reading.gravity_deviation_mg < STILL_GRAVITY_DEV_MG,
            "a resting device must sit under the still band, got {}",
            reading.gravity_deviation_mg
        );
        assert_eq!(reading.gravity_deviation_mg, 0, "nothing off-shell at rest");

        let mut gesture = tone(5.0, 3_000);
        let mut peak = 0;
        for index in 0..100 {
            let now = start + (index + 1) * MOTION_SCAN_MS;
            let mut frame = gesture(now);
            recognizer.recognize(&mut frame, now);
            peak = peak.max(frame.shake_residual_mg);
        }
        assert!(
            peak > SHAKE_ON_MG,
            "a 3 g shake has to publish a residual above the threshold, peaked at {peak}"
        );
    }

    #[test]
    fn one_gesture_reports_once_then_holds_the_refractory() {
        let (mut recognizer, start) = settled();
        let mut gesture = tone(5.0, 3_000);
        let mut seen = Vec::new();
        for index in 0..250 {
            let now = start + (index + 1) * MOTION_SCAN_MS;
            seen.extend(recognizer.recognize(&mut gesture(now), now).iter());
        }
        let fires = u64::try_from(seen.iter().filter(|e| **e == MotionEvent::Shake).count())
            .unwrap_or_default();
        // Five seconds of shaking, each event held for the refractory period, so
        // the count lands near one per refractory window, not one per poll.
        let span_polls = 250u64;
        let least = span_polls * MOTION_SCAN_MS / u64::from(SHAKE_REFRACTORY_MS + SHAKE_WINDOW_MS);
        let most = span_polls * MOTION_SCAN_MS / u64::from(SHAKE_REFRACTORY_MS);
        assert!(
            (least..=most).contains(&fires),
            "5 s of shaking gave {fires} events, expected {least}..={most}"
        );
    }

    #[test]
    fn the_same_gesture_reports_at_any_poll_rate() {
        // The peak window runs on the clock, so a slow loop sees the same gesture
        // despite collecting fewer samples of it.
        let trigger = |step_ms: u64| {
            let mut recognizer = MotionRecognizer::new();
            let still = sample([0, 0, 1_000], [0; 3]);
            feed(&mut recognizer, &still, 20);
            let start = recognizer.last_ms.unwrap();
            let mut gesture = tone(5.0, 3_000);
            for index in 0..60u64 {
                let now = start + (index + 1) * step_ms;
                if shake_in(recognizer.recognize(&mut gesture(now), now)) {
                    return Some(now - start);
                }
            }
            None
        };
        let fast = trigger(MOTION_SCAN_MS);
        let slow = trigger(MOTION_SCAN_MS * 2);
        assert!(fast.is_some(), "20 ms polling must report the gesture");
        assert!(slow.is_some(), "40 ms polling must report the gesture");
        // Not the same instant at both rates — a reversal narrower than the poll
        // interval falls between samples — just within the window.
        assert!(
            fast.unwrap().abs_diff(slow.unwrap()) <= u64::from(SHAKE_WINDOW_MS),
            "20 ms reported at {:?} but 40 ms at {:?}, further apart than the peak window",
            fast,
            slow
        );
    }

    #[test]
    fn a_reorientation_is_absorbed_and_does_not_read_as_a_shake() {
        let mut recognizer = MotionRecognizer::new();
        let still = sample([0, 0, 1_000], [0; 3]);
        feed(&mut recognizer, &still, 20);

        // Tipped onto its side: a one-off orientation change, not a gesture.
        let on_its_side = sample([1_000, 0, 0], [0; 3]);
        let seen = feed(&mut recognizer, &on_its_side, 25);
        assert!(
            !shook(&seen),
            "gravity moving into a new axis is not a shake, got {seen:?}"
        );
    }

    #[test]
    fn resting_after_motion_reads_as_lift_then_place() {
        let mut recognizer = MotionRecognizer::new();
        let still = sample([0, 0, 1_000], [0; 3]);
        feed(&mut recognizer, &still, 20);

        // Carried, not set down: the reading keeps changing, which is the only
        // thing that separates "in use" from "left on the desk".
        let mut carried = swinging([0, 0, 400]);
        let start = recognizer.last_ms.unwrap();
        let dwell = u64::from(DWELL_MS) / MOTION_SCAN_MS + 2;
        let mut seen = Vec::new();
        for index in 0..dwell {
            seen.extend(
                recognizer
                    .recognize(
                        &mut carried(index as usize),
                        start + (index + 1) * MOTION_SCAN_MS,
                    )
                    .iter(),
            );
        }
        assert!(
            seen.contains(&MotionEvent::Lift),
            "sustained motion reads as a lift, got {seen:?}"
        );
        assert!(
            !seen.contains(&MotionEvent::Shake),
            "carrying is not shaking, got {seen:?}"
        );

        let seen = feed(&mut recognizer, &still, dwell);
        assert!(
            seen.contains(&MotionEvent::Place),
            "settling back down reads as a place, got {seen:?}"
        );
    }

    #[test]
    fn posture_needs_an_in_plane_component() {
        let mut recognizer = MotionRecognizer::new();
        let flat = sample([0, 0, 1_000], [0; 3]);
        assert!(
            feed(&mut recognizer, &flat, 5).is_empty(),
            "flat has no posture"
        );

        let upright = sample([0, 900, 100], [0; 3]);
        assert_eq!(feed(&mut recognizer, &upright, 1), [MotionEvent::Portrait]);

        let sideways = sample([900, 0, 100], [0; 3]);
        assert_eq!(
            feed(&mut recognizer, &sideways, 1),
            [MotionEvent::Landscape]
        );
    }

    #[test]
    fn an_invalid_sample_advances_nothing() {
        let mut recognizer = MotionRecognizer::new();
        let mut fault = MotionSample::error();
        assert!(recognizer.recognize(&mut fault, 0).is_empty());
    }

    /// Feeds a mid-band knock on X at the given poll indices, still otherwise;
    /// the amplitude pins the strength under test (900 mG firm, 300 mG light).
    fn blips(poll_indices: &[u64], amplitude_mg: i32) -> impl FnMut(u64, u64) -> MotionSample + '_ {
        move |index, _now| {
            let struck = poll_indices.contains(&index);
            sample([if struck { amplitude_mg } else { 0 }, 0, 1_000], [0; 3])
        }
    }

    fn taps_of(seen: &[MotionEvent]) -> Vec<u8> {
        seen.iter()
            .filter_map(|event| match event {
                MotionEvent::Tap { count } => Some(*count),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_single_knock_reports_a_one_count_once_the_window_passes() {
        let (mut recognizer, start) = settled();
        let mut seen = Vec::new();
        let mut gesture = blips(&[0, 1], 900);
        for index in 0..60 {
            let now = start + (index + 1) * MOTION_SCAN_MS;
            seen.extend(recognizer.recognize(&mut gesture(index, now), now).iter());
        }
        assert_eq!(
            taps_of(&seen),
            vec![1],
            "one transient is one knock, reported once: got {seen:?}"
        );
    }

    #[test]
    fn a_light_knock_just_above_the_peak_bar_counts() {
        // The bar was eased from 300 to 250 mG so an ordinary tap crosses it.
        let (mut recognizer, start) = settled();
        let mut seen = Vec::new();
        let mut gesture = blips(&[0, 1], 300);
        for index in 0..60 {
            let now = start + (index + 1) * MOTION_SCAN_MS;
            seen.extend(recognizer.recognize(&mut gesture(index, now), now).iter());
        }
        assert_eq!(
            taps_of(&seen),
            vec![1],
            "a light tap above the peak bar counts as one knock: got {seen:?}"
        );
    }

    #[test]
    fn a_double_knock_within_the_window_reports_a_two_count_once() {
        let (mut recognizer, start) = settled();
        let mut seen = Vec::new();
        // The second blow must land outside the 80 ms quiet window (ringing
        // tail) yet inside the 500 ms double window: 10 polls is 200 ms. A
        // double (two crossings) must not trip the shake's three-crossing count.
        let mut gesture = blips(&[0, 1, 10, 11], 900);
        for index in 0..70 {
            let now = start + (index + 1) * MOTION_SCAN_MS;
            seen.extend(recognizer.recognize(&mut gesture(index, now), now).iter());
        }
        assert_eq!(
            taps_of(&seen),
            vec![2],
            "two transients 200 ms apart are one double knock: got {seen:?}"
        );
    }

    #[test]
    fn a_third_knock_rolls_into_a_triple_count() {
        let (mut recognizer, start) = settled();
        let mut seen = Vec::new();
        let mut gesture = blips(&[0, 1, 10, 11, 20, 21], 900);
        for index in 0..70 {
            let now = start + (index + 1) * MOTION_SCAN_MS;
            seen.extend(recognizer.recognize(&mut gesture(index, now), now).iter());
        }
        assert_eq!(
            taps_of(&seen),
            vec![3],
            "three transients inside the window resolve a triple: got {seen:?}"
        );
    }

    #[test]
    fn a_sustained_excursion_is_not_a_tap() {
        // Held past the peak bar without decaying: the budget burns out before
        // the residual falls under the quiet bar — a press, not a blow.
        let (mut recognizer, start) = settled();
        let mut seen = Vec::new();
        for index in 0..120 {
            let now = start + (index + 1) * MOTION_SCAN_MS;
            let mut held = sample([if index > 1 { 2_600 } else { 0 }, 0, 1_000], [0; 3]);
            seen.extend(recognizer.recognize(&mut held, now).iter());
        }
        assert!(
            taps_of(&seen).is_empty(),
            "a sustained excursion has no decaying peak to count, got {seen:?}"
        );
    }

    #[test]
    fn a_pickup_is_not_a_tap() {
        // A pick-up opens with a sharp step over the peak bar, then holds the
        // residual up longer than the 80 ms budget: the budget times out.
        let (mut recognizer, start) = settled();
        let mut seen = Vec::new();
        for index in 0..80 {
            let now = start + (index + 1) * MOTION_SCAN_MS;
            let held = if index < 60 { 320 } else { 0 };
            let mut frame = sample([held, 0, 1_000], [0; 3]);
            seen.extend(recognizer.recognize(&mut frame, now).iter());
        }
        assert!(
            taps_of(&seen).is_empty(),
            "a pick-up holds the residual over the quiet bar, got {seen:?}"
        );
    }

    #[test]
    fn resting_noise_stays_under_the_quiet_bar() {
        // Resting noise (9-38 mG on the probe) against the 150 mG quiet bar must
        // never light the peak entry.
        let (mut recognizer, start) = settled();
        let mut seen = Vec::new();
        for index in 0..120 {
            let now = start + (index + 1) * MOTION_SCAN_MS;
            let jitter = [40, -20, 30][index as usize % 3];
            let mut frame = sample([jitter, 0, 1_000], [0; 3]);
            seen.extend(recognizer.recognize(&mut frame, now).iter());
        }
        assert!(
            taps_of(&seen).is_empty(),
            "resting jitter must never cross the peak bar, got {seen:?}"
        );
    }

    #[test]
    fn a_deliberate_turn_is_not_a_tap() {
        // A turn's onset transient is knock-shaped (the silicon latched on it), so
        // it is caught over the whole gesture: a knock settles under the bar.
        for hz in [0.5, 1.0, 2.0, 3.0] {
            let (mut recognizer, start) = settled();
            let mut gesture = turning(hz, 1_000);
            let mut seen = Vec::new();
            for index in 0..300 {
                let now = start + (index + 1) * MOTION_SCAN_MS;
                seen.extend(recognizer.recognize(&mut gesture(now), now).iter());
            }
            assert!(
                taps_of(&seen).is_empty(),
                "turning the device at {hz} Hz is not a tap, got {seen:?}"
            );
        }
    }

    #[test]
    fn a_shake_is_not_a_tap() {
        // A shake never spends a full quiet window below the bar, so none of its
        // crossings can confirm; the Shake event itself beats a tap.
        let (mut recognizer, start) = settled();
        let mut gesture = tone(5.0, 3_000);
        let mut seen = Vec::new();
        for index in 0..250 {
            let now = start + (index + 1) * MOTION_SCAN_MS;
            seen.extend(recognizer.recognize(&mut gesture(now), now).iter());
        }
        assert!(shook(&seen), "the fixture must actually shake");
        assert!(
            taps_of(&seen).is_empty(),
            "a continuous shake cannot confirm a quiet window, got {seen:?}"
        );
    }

    #[test]
    fn a_reorientation_is_absorbed_and_does_not_read_as_a_tap() {
        let mut recognizer = MotionRecognizer::new();
        let still = sample([0, 0, 1_000], [0; 3]);
        feed(&mut recognizer, &still, 20);
        // Tipped onto its side and held: the tap analogue of the shake's
        // reorientation canary — a one-off orientation change, not a blow.
        let on_its_side = sample([1_000, 0, 0], [0; 3]);
        let seen = feed(&mut recognizer, &on_its_side, 25);
        assert!(
            taps_of(&seen).is_empty(),
            "gravity moving into a new axis is not a tap, got {seen:?}"
        );
    }

    #[test]
    fn tap_residual_reports_the_quantity_the_peak_bar_decides_on() {
        let (mut recognizer, start) = settled();
        let mut resting = sample([0, 0, 1_000], [0; 3]);
        recognizer.recognize(&mut resting, start + MOTION_SCAN_MS);
        assert!(
            resting.tap_residual_mg < isqrt(TAP_PEAK_MAG_MG2 as i32),
            "a resting device must sit under the peak bar, got {}",
            resting.tap_residual_mg
        );

        let mut struck = sample([3_400, 0, 1_000], [0; 3]);
        recognizer.recognize(&mut struck, start + 2 * MOTION_SCAN_MS);
        assert!(
            struck.tap_residual_mg > isqrt(TAP_PEAK_MAG_MG2 as i32),
            "a 3.4 g knock has to publish a residual above the bar, got {}",
            struck.tap_residual_mg
        );
    }
}
