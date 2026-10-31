+++
title = "Motion Semantic Framework"
weight = 60
sort_by = "weight"
[extra]
source_file_hash = "76fb204df7e8eb7dade314b647859605d6ee389c"
translated_at = "2026-10-31T00:00:00Z"
+++

# Motion Semantic Framework

QMI8658A is only one source. What the framework exposes is **meaning** — "knocked once", "picked up" — not a particular bit of a particular register. This page covers the semantics, the arbitration rules, the QMI8658A binding details, and how to add a new source.

## Two paths

One motion input carries both paths at once, and both are first-class:

| Path |
| --- | --- | --- |
| Data path |
| Semantic path |

There is no "output semantics / output raw data" either-or.

Android's `SensorEvent` and Zephyr's `sensor_trigger` both keep two streams in parallel. The upper layers want the raw data for logging, calibration and filtering, while the semantics spare every product from rewriting the same gesture thresholds.

`MotionBatch` packs both into the same delivery, and a consumer takes whichever it needs.

### Where semantics come from

- **Hardware engine**: the part's own detectors, optimal in power and latency, with their parameters in registers. The QMI8658A now enables only No-Motion, so `Still` is the one hardware semantic out of the box.
- **The core recognizer**: `MotionRecognizer`, which only looks at the data path; every threshold lives in core, so swapping the sensor does not change what the business layer means. Tap, tilt, shake, lift/place and posture all live here — including `Tap`, which was once tried on hardware (see the post-mortem below).

The two meet in `MotionInput::poll` and are arbitrated together before dispatch.

Arbitration runs within a path: a software Tap and a software Shake share the `Action` family, so they collapse into one.

`MotionEvent::Tap` and `GestureEvent::Tap` are instead different events on two different paths, and are not merged.

The first is a mechanical knock measured by the accelerometer (it may come from the case, from a struck desk, or even from the device sitting on a vibrating surface). The second is a finger landing on glass, carrying a position and a press duration.

One physical action can produce both, and both are kept.

## Semantics

```
MotionSource::sample(now_ms) -> Result<Option<MotionReading>, MotionError>
```

`MotionReading { sample, hardware }` is the result of one successful poll.

`Ok(None)` means there was no new frame on that tick. Ordinary at a 20 ms cadence, and it must not be reported as a fault.

The recognizer's semantics are layered on inside `MotionInput`; a `MotionSource` implementation does not need to know that the recognizer exists.

`MotionEvent` mutual exclusion is defined over three families:

| Family | Members | Highest priority |
| --- | --- | --- |
| `State` | `Still` `Moving` `Activity` `Step` | `Step` |
| `Pose` | `TiltEnter` `TiltExit` `Portrait` `Landscape` | `TiltEnter` / `TiltExit` |
| `Action` | `Tap` `Shake` `Lift` `Place` | `Shake` |

Two candidates in the same family describe the same physical action, and arbitration keeps only one. Events across families are allowed to coexist ("moving" and "already tilted" are two facts that hold at the same time).

Priority lets the more specific description win: a shake includes the tilt chatter it causes itself, a lift includes the posture change that comes with it.

The per-family cooldown is 300 ms, suppressing repeated chatter from the same family.

`Tap` (40) ranks below `Shake` (100) and `Lift` (80), so an action that is both a knock and a lift reports only the latter.

`MotionEvents` has a capacity of 8 and must hold the pre-arbitration union — 1 hardware engine plus 5 core classifiers (tap / tilt / posture / shake / lift-place) makes 6 in all; only after arbitration does it collapse to at most one per family.

### Capability declaration

`MotionCapabilities` is a bitset that is **declared, not switched**. The data path ships unconditionally, so a source that lacks a semantic simply never raises it, rather than switching off the whole stream.

Capabilities come in two layers, to avoid crediting what software computed to hardware:

- `QMI8658_CAPABILITIES` — what the driver computes itself: `TELEMETRY` `STILL`.
- `RECOGNIZER_CAPABILITIES` — what core derives from any data path: `TAP` `TILT` `SHAKE` `LIFT_PLACE` `POSTURE`.
- `MOTION_CAPABILITIES` — the union of the two, i.e. what that board advertises externally.

The board layer declares its own union through `HasMotion::motion_capabilities()`; the app reads it once at startup and logs it (`[MOTION] declared capabilities 0x…`), so a field investigation can tell straight away what that firmware build claims to report. A board with no accelerometer returns `MotionCapabilities::EMPTY`.

## Parameter lookup

Bench calibration is run against the two tables below. The parameters sit in two layers: the core recognizer (pure software thresholds) and the QMI8658A engine (hardware thresholds in registers), with the details in the two sections below.

The landscape panel lays this page out as two text blocks and a dial: the first block is the axis readings, the second is the estimator outputs and the semantic counts, and the third is the attitude dial.

A block holds eighteen rows and the axis readings alone take fifteen of them, so the four estimator rows went to the second block rather than into a fourth column.

### `ATTITUDE` first block: the axis readings

| Label | Field | Unit | Meaning | Corresponding parameter |
| --- | --- | --- | --- | --- |
| `A0`–`A2` | `raw_accel` | LSB | raw acceleration counts (±4 g, 8192 LSB/g) | — |
| `M0`–`M2` | `accel_mg` | mG | acceleration, before gravity removal | — |
| `G0`–`G2` | `raw_gyro` | LSB | raw gyroscope counts (±1024 °/s, 64 LSB/°/s) | — |
| `D0`–`D2` | `gyro_dps_x10` | 0.1 °/s | angular rate | — |
| `R0`–`R2` | `tilt_deg_x10` | 0.1° | roll / pitch / yaw (yaw drifts without a magnetometer) | `TILT_ENTER_DEG_X10` / `TILT_EXIT_DEG_X10` |

### `ATTITUDE` second block

The first four rows are estimator outputs rather than raw axis values: they are what the recognizer **decided**, not what the sensor **measured**.

| Label | Field | Unit | Meaning | Corresponding parameter |
| --- | --- | --- | --- | --- |
| `ST` | `status` | bitmap | the raw value of `STATUS1`(0x2F) | — |
| `LR` | `gravity_deviation_mg` | mG | \|measured magnitude − 1 g\| (the quantity the still criterion decides on) | `STILL_GRAVITY_DEV_MG` |
| `SR` | `shake_residual_mg` | mG | shake-estimate residual (vector length) | `SHAKE_ON_MG` |
| `TR` | `tap_residual_mg` | mG | square root of the tap squared sum (the sum of squared linear acceleration across axes) | `TAP_PEAK_MAG_MG2` |

These four are calibration-only: in the raw axis values these quantities are 0 by definition (at rest the magnitude lands exactly on the 1 g shell), so the thresholds can only be read on the device. The fourteen rows below them are the cumulative count per semantic.

| Label | Semantic | Trigger condition | On this board |
| --- | --- | --- | --- |
| `TAP` | Single tap | the recognizer reports `Tap{count}`, count=1 | counted |
| `2T` | Double tap | same, count=2 | counted |
| `3T` | Triple tap | same, count=3 (capped) | counted |
| `STL` | Still | No-Motion engine (three axes, AND, rising edge) | counted |
| `MOV` | Moving | not advertised | `N/A` |
| `ACT` | Activity | not advertised | `N/A` |
| `STP` | Step | not advertised | `N/A` |
| `TIN` | Tilt enter | 15°, signed | counted |
| `TOX` | Tilt exit | 10° hysteresis | counted |
| `SHK` | Shake | residual > 2000 mG and 3 up-crossings within 800 ms | counted |
| `LFT` | Lift | the magnitude leaves the `[850, 1150]` mG band (> 150 mG off 1 g) for 150 ms | counted |
| `PLC` | Place | the magnitude returns to the `[850, 1150]` mG band for 150 ms | counted |
| `PRT` | Portrait | the larger of the X/Y horizontal components ≥ 300 mG, and Y is dominant | counted |
| `LND` | Landscape | same but X dominant | counted |

The difference between `N/A` and `0` is deliberate: `0` means the threshold did not fire (needs tuning), `N/A` means this board does not report it at all (nothing to tune). Counts are `u16` saturating accumulators, reset on every boot, with no clear entry point.

## Recognizer thresholds

All of it lives in `recognizer.rs`, none of it in the driver.

| Semantic | Criterion | Parameters |
| --- | --- | --- |
| `TiltEnter` / `TiltExit` | 15° to enter / 10° to exit, signed hysteresis | — |
| `Tap` | residual crosses the peak threshold with the previous poll below the quiet one, and the peak must fall back and settle | below |
| `Shake` | residual vector length > 2000 mG, 3 up-crossings within 800 ms | below |
| `Lift` / `Place` | measured magnitude deviating 1 g by more than 150 mG for 150 ms | below |
| `Portrait` / `Landscape` | with both horizontal axes under 300 mG there is no meaningful pose, so neither is reported | — |

Signed hysteresis is required: a single threshold chatters an enter/exit pair out on every poll at the boundary.

### `Tap`

Gravity removal is a per-axis 300 ms moving average (`TAP_BASELINE_TAU_MS`), but the baseline only blends at rest.

It is frozen for the whole gesture (Peak/Quiet/Between). Otherwise a few quick strikes get absorbed into the baseline and the overshoot lifts the "quiet reading" high enough to starve a later strike's 80 ms peak budget: the probe saw three straight knocks push the residual floor to about 155 mG, which is right at the quiet bar.

The criterion compares the sum of squared linear acceleration over the three axes, Σlin², raw, with no energy smoothing — the quantity to measure is the residual's own speed falling under the quiet bar within the peak budget.

A strike rising edge needs the instantaneous square to cross the 250 mG peak threshold (`TAP_PEAK_MAG_MG2` = 62_500 mG²) while the previous poll was below the 150 mG quiet threshold (`TAP_QUIET_MG2` = 22_500 mG²).

That condition rejects two classes of false positive.

A knock steps from quiet straight across the peak threshold; the residual of a rotation climbs across both thresholds poll by poll and can never assemble that one jump.

A slow rise whose previous poll was not quiet also fails — a pick-up easing up to 320 mG, a rotation crossing line after line.

After the peak stage begins there is a decay budget: the residual has to fall back below the quiet threshold within 80 ms (`TAP_PEAK_WINDOW_MS`) for the peak to stand.

If the budget runs out with the signal still high, that is a press, a pick-up (which holds high for about 1 s, so no 80 ms window anywhere can satisfy it) or a rotation, and the whole segment is discarded.

A peak that does stand must also stay fully below the quiet threshold for a complete 80 ms (`TAP_QUIET_WINDOW_MS`) before it counts as one strike — the residual ring of a single knock is not a second one.

The instant the first one takes effect an anchor is recorded, and every strike arriving within the next 500 ms (`TAP_DOUBLE_WINDOW_MS`) rolls into the same tap (running peak → quiet → effective again), with the count capped at 1/2/3.

When the window closes a single `Tap` is reported according to `count`, so a single tap is naturally 500 ms late before it is settled.

The final closing criterion is a settling budget (`TAP_SETTLED_MOTION_MS` = 200 ms): counted from the first strike, if the signal accumulates more than 200 ms above the quiet threshold, the gesture did not come to rest.

The residual ring after a knock settles quietly below the threshold, while a rotation keeps crossing and returning, so the segment is discarded on the spot and not reported.

The start-up transient of a rotation is locally the same shape as a knock — the hardware engine is in fact latched onto exactly that kind of transient — and only "whether the whole gesture window comes to rest below the quiet threshold" can separate the two.

The thresholds live in the mG² domain, and `TR` publishes their square root (the peak threshold is about 250 mG).

Calibrated on hardware (a probe printed the per-poll residual stream at 50 Hz): resting noise is 9–38 mG, against which the 250 mG bar holds about seven-fold margin.

A deliberate knock is a one-to-two-sample 364–4866 mG transient (clipped at the ±4 g range), and the bar also reaches down to ordinary everyday tapping.

A pick-up-hold-place slow rise sits at 50–350 mG for about a second, with the lifting step topping out near 320 mG. It can cross the peak threshold, but the sustained high reading cannot satisfy any 80 ms decay budget.

Occasional desk-coupling one-sample spurs of 300–988 mG are indistinguishable from a real knock and count as one (acceptable).

`event.rs` files `Tap` under the `Action` family (priority 40), where a shake (100) or lift (80) in the same family can override it.

### `Shake`

The criterion is repetition count rather than duration.

What separates knocking on a desk from shaking is not the size of any single impulse but whether there is a second one: a single impulse crossing the line gives one up-crossing, a single shake gives one per reversal.

A leaky integrator of "accumulate the time above the threshold, decay by the same amount once it drops below" cannot be used.

A sine spends at most half of a period above a threshold near its own peak (measured at about 10% for 5 Hz under this configuration), so after equal-amount decay the net gain per cycle is always negative and no oscillation ever banks enough to fill the confirmation window.

  The threshold comes from the reference implementation in W3C's *Accelerometer* (`shakeThreshold = 25` m/s² ≈ 2.5 g). That specification is the accelerometer one, not *Device Orientation and Motion* from the same family.

  It is brought down in two places. The recognizer only looks at the signal every 20 ms, so a threshold sitting on the peak misses it for a whole poll; and a 3 g shake at 5 Hz leaves only about 2.5 g of residual, with 2.5 g sitting exactly at the top of the signal.

  2.0 g is the value that leaves 2–3 above-threshold samples per reversal.

  W3C's example is 60 Hz, watches only a single axis `sensor.x`, and uses a single-poll threshold. The vector length, the requirement of 3 crossings and the 800 ms bound are this product's own design rather than the specification's requirement. Not yet calibrated on hardware.

### `Lift` / `Place`

A measured magnitude deviating more than 150 mG from 1 g counts as stillness being broken, and is believed after 150 ms. The comparison is done entirely in the squared domain (still ⇔ `850² < x²+y²+z² < 1150²`), pure integers with no square root.

The magnitude is direction independent and carries no estimate error. The old criterion subtracted its own gravity estimate, so the criterion drifted exactly as much as the estimate did, and a desk could sit 100 mG away from its own estimate; the raw magnitude is affected only by the accelerometer's noise floor.

The 150 mG comes from ST's official FSM pick-up example (LSM6DSO: on-table decides at 0.9 g, pick-up escapes at 1.1 g — that is, a pick-up needs only a 100 mG deviation from gravity).

The resting magnitude on the bench wobbles only 8–22 mG, so 150 mG leaves about seven times the margin.

The wider 350 mG band is only crossed on the bench when the device is swung around (large acceleration), so ordinary lifts get missed.

Pure rotation (a flip, a lift without adding force) does not change the magnitude and still reads as still. That is a blind spot shared by ST's magnitude-escape method, not one unique to this implementation.

What is detected is the transition into and out of stillness, and it does not claim to tell a desk from a hand.

### `Portrait` / `Landscape`

Both horizontal components of the two axes below 300 mG counts as lying flat; there is no meaningful posture at that point, so neither is reported.

## Gravity removal estimates

Gravity removal uses a first-order low pass, written as a time constant rather than a per-poll coefficient, so the corner does not move when the polling frequency changes. `Shake` and `Tap` each have one estimate, with different time constants, and they do not interfere with each other.

| Estimate | Time constant | Corner under 20 ms polling | Who uses it |
| --- | --- | --- | --- |
| Shake estimate | 70 ms | ≈ 2.0 Hz | `Shake` |
| Tap baseline | 300 ms | ≈ 0.5 Hz | `Tap` |

The first valid sample defines the current orientation rather than being taken and subtracted from the orientation — otherwise a device that is being picked up the instant it boots would gain a phantom impulse out of nowhere.

`Lift`/`Place` does no gravity removal; its criterion is the raw magnitude (see the table above), and it does not depend on any estimate.

`Shake` uses the vector length of the residual, not the sum of the three axes.

Summing the three axes amounts to rectification, and a rectified multi-axis oscillation does not return to zero between reversals — it rides above the threshold, so there are no up-crossings left to count at all. The vector length goes to zero at a reversal, which is what gives a clean edge.

When tuning, `SHAKE_ON_MG` and the 150 mG bandwidth of `LFT`/`PLC` are entirely independent of each other and can be calibrated separately.

The sensor ODR is 896.8 Hz (in 6DOF mode the rate is derived from the gyroscope, see below), and the core polls every 20 ms (50 Hz), which means taking only 1 frame out of every 18.

A 5–10 Hz shake still yields 5–10 samples per oscillation at 50 Hz, so reversals can be counted.

But the width of a reversal is measured in phase. For an up-crossing to be sampled, the threshold has to be low enough that the phase span of that segment exceeds one poll, which is one of the reasons `SHAKE_ON_MG` was brought down from 2.5 g to 2.0 g.

The cost is that 40 ms polling and 20 ms polling report the same shake at different moments (coarse polling can miss a crest entirely and wait an extra cycle to accumulate enough crossings). That is decided by sampling, not by the time base.

The tap criterion likewise runs on this 20 ms path and is independent of the ODR. The high 896.8 Hz ODR serves only the No-Motion engine's sample-counted windows and the gyroscope yaw diagnostic (see below).

`yaw` is an integral quantity, and with no magnetometer it drifts without bound; it is for diagnostic display only, and no semantic or business decision may read it.

Note: after switching to magnitude and gravity-removed-residual criteria, no semantic comes from the gyroscope any more. It still stays in `MotionSample` for telemetry and the on-screen `G` readout, and the driver still reads it as usual, but `SHAKE`, `TAP` and `LIFT_PLACE` all look only at acceleration.

The gyroscope is still enabled half for its telemetry value and half because in 6DOF mode the ODR is derived from it: turning it off drops the part onto the accelerometer-only column (1000 Hz rather than 896.8 Hz) and shifts the No-Motion window conversion above.

## QMI8658A binding

The S3 board wires no INT1/INT2, so everything is polled. That produces three hard constraints:

1. **CTRL8 bit7 must be set to 1.** It reroutes the completion signal of CTRL9 commands from INT1 to `STATUSINT.bit7`; without it there is nothing but blind waiting. Once rerouted, `CTRL8 = 0x84` (No-Motion + handshake select).
2. **CTRL7 must be 0 while the parameters are being configured.** The datasheet requires `aEN = gEN = 0`, so the order is: write CTRL1/2/3/5 → `CTRL7 = 0` → two CAL writes along with the CTRL9 commands (the two halves of the Motion engine) → `CTRL8 = 0x84` → `CTRL7 = 0x03`. The parameters are installed first and the engine is armed afterwards, otherwise the engine runs on the reset defaults
3. The handshake must be bounded, but it is measured on the wall clock. `await_cmd_done` polls at most 1000 times, 1 ms apart, and returns `EngineConfig` on failure — it must not hang the input loop, since touch and button scanning are also hanging off that loop.

   Wall clock is used rather than a tight poll loop because `CmdDone` is a sticky flag: a tight poll loop gives up before the device has finished computing, while a single false timeout here permanently kills the whole motion device.

   The manual gives no worst-case time for `CmdDone`, so the budget is taken from the only independent implementation that publishes one (RIOT uses a 1000 ms command timeout). Loosening it here is free, because it only runs the one-time boot sequence inside `init()` and the normal path returns on the very first poll.

CTRL5 is explicitly written as `0x00`, so `aLPF_EN` and `gLPF_EN` are both off. In Table 22 of the manual bit4 is `gLPF_EN` and bit0 is `aLPF_EN`, and `aLPF_MODE`(2:1) of `00` is 2.66% of ODR.

The accelerometer low-pass is off, keeping the full 896.8 Hz bandwidth.

The 24 Hz aLPF was once suspected of being the main attenuator of the tap transient, but an on-device A/B pass (same gestures, aLPF off/on) showed both the resting noise and the knock readings were identical. The attenuation comes from the structure and the coupling, not from that low-pass.

Turning it off only removes a suspect and leaves the tap transient full bandwidth, with no measurable cost.

The gyroscope low-pass stays off (bit4=0): no semantic reads the gyro path looking for peaks.

ODR and window conversion: `CTRL2 = 0x13`, `CTRL3 = 0x53`, ODR index `0b0011`.

The accelerometer column of Table 22 in the manual reads 1000 Hz, but the 6DOF column is 896.8 Hz, and note 13 says that when both sensors are enabled it is the 6DOF column that actually applies.

The ODR is no longer serving the tap (the tap criterion runs on core's 20 ms path and is independent of the ODR), but 896.8 Hz still comfortably exceeds what the No-Motion engine needs.

It also keeps the gyroscope yaw diagnostic at full rate.

The No-Motion engine counts its windows in samples while the product thinks in milliseconds. The windows are expressed as milliseconds in code, converted to a sample count (`odr_samples`), and pinned to the register values — changing the ODR once silently rescales every window. Currently 80 ms → 72 samples.

Each engine's parameters are split into two halves, selected by the high nibble of CAL4_H (`0x01` for the first half, `0x02` for the second), and the Motion engine takes two CTRL9 commands.

A write is one 8-byte burst starting at `CAL1_L` (CAL1_L, 1_H, 2_L, 2_H, 3_L, 3_H, 4_L, 4_H). What the two halves hold is not contiguous, and the 16-bit parameters straddle a pair of registers, so the bytes are filled in one by one following Table 34 (Motion):

| Register | Motion first half | Motion second half |
| --- | --- | --- |
| CAL1_L | AnyMotionXThr | AnyMotionWindow |
| CAL1_H | AnyMotionYThr | NoMotionWindow |
| CAL2_L | AnyMotionZThr | SigMotionWait[7:0] |
| CAL2_H | NoMotionXThr | SigMotionWait[15:8] |
| CAL3_L | NoMotionYThr | SigMotionConfirm[7:0] |
| CAL3_H | NoMotionZThr | SigMotionConfirm[15:8] |
| CAL4_L | MOTION_MODE_CTRL | NA |
| CAL4_H | `0x01` | `0x02` |

This kind of interleaved filling makes it very easy to misfile a parameter without noticing, so `init_pushes_both_parameter_sets_of_the_no_motion_engine` pins all 16 bytes outright and checks them against the table row by row.

Status mapping is down to a single path: `STATUS1.bit6` → `Still`.

This is a level, not an event. As long as the No-Motion engine still considers the device still, it asserts the bit on every poll, and publishing it directly would turn one settling into a frame-rate flood of `Still` (the bench saw `STL` incrementing without bound).

The driver reports it only once on the 0→1 rising edge, stays silent while the bit holds, and treats a fall followed by a re-assert as one new report.

The Tap bit (bit1) is explicitly ignored and not decoded. It may still be set by the silicon engine's latched transient, but the driver does not read its result registers and treats it even less as an event — what `a_tap_bit_is_no_longer_decoded` pins is "the tap bit neither produces an event nor turns into an extra I²C read".

The handshake rides on `STATUSINT.bit7`, and a command must be followed by writing back `CTRL9 = 0x00` to ACK.

The reset needs a handshake, not a blind wait.

Section 7.4 says the reset takes at most 15 ms, while both 5.9 and 7.4 require reading `0x4D` immediately after the reset: `0x80` means the reset succeeded (both power-on and soft reset set it), and that bit gets overwritten by later operations, so it has to be read and then move on.

The driver polls `0x4D` instead (at most 100 times × 200 µs = 20 ms) rather than using a fixed delay, returns `ResetIncomplete` on failure and writes no configuration register at all.

The manual contradicts itself about the reset byte: Table 27 says write `0xB0` while the prose of 7.4 says write `0x0B`, which are bit reversals of each other.

The `0xB0` from Table 27 is taken, and the `0x4D` handshake turns exactly this ambiguity into a detectable outcome — with the wrong byte the flag is not set and `ResetIncomplete` is reported straight away instead of a silent configuration failure.

`STATUS1` bit-clearing semantics — the manual documents exactly one. The original sentence in §12.4 reads "Reading STATUS1 by the host will clear the WoM bit", which is about bit2, the wake flag, not No-Motion (bit6) or Tap (bit1) — and whether a bit self-clears is undefined behavior.

The driver does not guess: it only reports on the rising edge and carries the real `STATUS1` along with the frame, so the field can verify with `ST` against the left-column calibration rows how many reports one physical action produces.

The bench readings (bit6 continuously asserted while at rest) are consistent with "level, self-clearing", so the edge criterion restores the action count 1:1.

Do not go and read `STATUSINT` to judge data readiness: with `syncSmpl=0` its bit0 is the INT2 level and bit1 is the INT1 level, and only bit7's CmdDone is meaningful. Data readiness has to come from `STATUS0` (bit0=aDA, bit1=gDA).

After a configuration failure the driver does not self-heal: `init` is the only configuration entry point, and polling while uninitialized returns `MotionError::NotReady`.

The S3 board treats motion as an optional sensor, so a failed `init` only logs one line and boot continues: the device works as usual, just without a motion device. The rate-limited `NotReady` never floods the log.

This product arms only No-Motion; the Tap engine is deliberately not enabled (see below), the Any-Motion / Significant-Motion / Pedometer bits in CTRL8 stay at 0, and those semantics belong to the core recognizer.

`NoMotionX/Y/ZThr` steps by 0.03125 g (currently `0x08` = a 0.25 g slope), and `NoMotionWindow` is counted in samples (80 ms → 72).

In `MOTION_MODE_CTRL = 0xF7`, bit0-2 enable the three AnyMotion axes, bit4-6 enable the three NoMotion axes, and bit7 decides whether the engine ANDs or ORs the enabled axes.

Taking `0xF7` (all three axes on + AND) is the strict reading of "still": all three axes must stay below their own slope thresholds for the entire window.

The `0x07` OR logic reports still as soon as one axis settles, so a device that is mid-turn can falsely report still because another axis happens to be steady — which is exactly one of the sources of the over-counting `STL` used to show.

`AnyMotionX/Y/ZThr = 0x00` is a placeholder value rather than a chosen threshold. A real 0 would make the engine fire on any deviation at all, and that is exactly why it stays disabled and is pinned there by a test.

### Taps do not go through the silicon engine

The QMI8658A's tap engine is unreachable under this product's polling scheme; the tap criterion runs on core's 20 ms stream (see `Tap` under "Recognizer thresholds" above), the driver keeps only No-Motion and does not declare a `TAP` capability.

`ODR = 896.8 Hz` is not kept for taps; it serves the No-Motion engine's sample-counted windows and the gyroscope yaw diagnostic.

The investigation, register readings and the two hypotheses that were overturned are in [Motion implementation record](@/records/iot/motion.en.md).

## Cadence and I²C budget

`MOTION_SCAN_MS = 20`. Every source shares the app's 5 ms base tick, so that is once every 4 ticks: fast enough not to miss a knock or a lift, slow enough to leave the shared I²C room for touch and buttons.

A sequential poll costs about 0.4 ms per poll (`STATUS1` 1 byte + `STATUS0` 1 byte + 12 bytes of data), leaving ample headroom in 20 ms. Reads never busy-wait.

Dispatch does delta suppression: a frame goes out only when the acceleration changes by > 20 mG or the angular rate by > 5 dps. But the recognizer sees data every tick, so a still frame is not missed because of that.

## Diagnostic display

The right column is a **cumulative count table per semantic** (labels and criteria in [Parameter lookup](#ATTITUDE_second_block)).

`TAP`/`2T`/`3T` are triggered respectively by the count=1/2/3 of `Tap{count}`, and all of them are counted (the count caps at 3, so a gesture past the third strike is a triple tap). Once count=3 can be obtained too, `3T` is no longer permanently 0 as it was in the chip's era.

`TIN`/`TOX` merge the enter/exit of all four directions into one count.

A capability this board does not advertise shows `N/A` (the current S3 advertises `0x3CB`, which excludes `STP` `MOV` `ACT`) — neither `RECOGNIZER_CAPABILITIES` nor the driver capabilities enable them, so it is not a threshold question.

The counts are `u16` saturating accumulators, reset on every boot, with no clear entry point (the same semantics as the touch counters).

Choosing `u16` rather than the touch side's `u8` is because a shake runs at about 3/s under the 300 ms refractory period — 255 overflows in under a minute and a half, which is not enough for calibration.

One tick can carry several cross-family events at the same time after arbitration (a shake and the tilt it causes arrive in the same batch), and **all** of them are counted rather than keeping only the highest-priority one, otherwise the tilt would disappear from the statistics.

## Adding a source

1. Write a driver in `iot-bsp-esp` that depends only on `embedded_hal` and implements `MotionSource`.
2. Return a `MotionReading`, filling the hardware engine's verdict into `hardware`; do not run core thresholds inside the driver.
3. Declare the union of the capabilities that driver itself has; do not re-declare the core-derived part.
4. Hook `MotionInput::new(driver)` onto the board layer's `take_motion`.

Business mapping is a **separate layer**. `InputEvent::Motion` currently maps to `BusinessIntent::Invalid` — the framework is already in place, and wiring semantics onto concrete product intents is a business decision, not something done in the driver layer.

With multiple IMUs, sample, health-check and recognize each source independently, with one `MotionCapabilities` per source. A 50 Hz semantic bus does not suit scenarios like flight control / balancing vehicles that need a high-rate inertial stream — that is a different road.

## Testing

```bash
moon run iot:test      # core: semantics, recognizer, arbitration, state machine
moon run iot:test-bsp  # driver: register sequence, handshake, status mapping
```

`test-bsp` relies on the target isolation of `esp-hal` in `Cargo.toml` (`cfg(target_os = "none")`). The QMI8658A and PCA9557 sources depend only on `embedded_hal`, so keeping this ESP layer out of the bare-metal target lets the driver unit tests run directly on a workstation, without paying the build cost of the chip HAL and without creating a new crate.

Driver test coverage: the enable ordering (the engine must be armed only after all the CTRL9 commands), the contents of the two parameter-set writes, a handshake timeout that does not hang, the tap bit no longer being decoded, and an engine event still being reported when the data path has not been updated.

Also: identity mismatch, range conversion, yaw wrap-around and its step bound, and capability attribution.
