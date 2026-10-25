+++
title = "Motion Implementation Record"
weight = 50
sort_by = "weight"
[extra]
source_file_hash = "8cda2a0a5a0e6bac0e56b332c69f9f74b92bde63"
translated_at = "2026-10-06T05:35:23Z"
+++

<!-- doc-audience: ai -->

# Motion Implementation Record

Judgements and rejected approaches from the motion-detection path in `apps/iot`. **AI-produced, not reviewed by a human** — see the [section above](@/records/_index.en.md).

Each judgement is labelled by its evidence: **`[test]`** a unit test covers it, **`[measured]`** there is a hardware measurement with a reproduction, **`[reported]`** not reproduced, recorded only as a lead.

---

## The QMI8658A tap engine is unusable, so knock detection moved to core

`[measured]` The part's tap engine latches a tap bit that never releases **at its enable transient**, and freezes `TAP_NUM`: from about 0.4 s after power-on `STATUS1.TAP` is permanently `0x02`/`0x42`, `TAP_STATUS` is frozen at `0xB0` (`TAP_NUM = 0`, polarity/axis bits flipping over and over), and a real knock can no longer get into the register. The driver publishes a Tap event only on a 0→1 rising edge, so a latched bit never falls and the publish path never fires.

Investigation order and the hypotheses that were overturned:

1. **Is the ODR too slow?** §10.4 recommends a tap-detection ODR > 200 Hz; the old firmware sat at 224.2 Hz, so one knock's impulse landed on just 2–5 samples. Three window lengths (5/9/18) × two `UDMThr` tiers (`0x0190`/`0x0300`) behaved identically at 224.2 Hz.
2. **Raise it to 896.8 Hz** (the first 6DOF tier above 500 Hz), parameters back to the manual's example `UDMThr = 0x0190` with an 80 ms window, matching the only working public reference, WaveShare's TapDetectionExample. The impulse now lands on a wide enough surface; behaviour still does not move.
3. **Do the signal and thresholds even hold?** A mirrored reference driver printed the engine's per-axis Alpha mean and Gamma energy as `sq` (squared residual) and `pk` (energy) to the bench: a real knock lands at 2.4–4.9 g², runs past the 0.8 g² `PeakMagThr`, and the energy falls back below 0.4 g². Signal, threshold and timing all hold.

The blocker sits outside every parameter gate, in the state machine itself. The same math does hold on core's 20 ms stream, so the knock contract moved to `core/src/drivers/motion/recognizer.rs`; `QMI8658_CAPABILITIES` therefore does not claim `TAP` — attributing a core-side computation to hardware is the one way to break what the capability set is for (letting a panel tell "this semantic cannot be reported" apart from "its threshold never fired").

`ODR` stays at 896.8 Hz no longer for taps, but for the No-Motion engine's sample-counted windows and the gyroscope yaw diagnostic.

Bench checklist (the first three target the silicon engine itself, which the driver no longer uses — kept only for re-verifying chip behaviour; the last three cover paths still in use):

- Knock once and watch `TAP` count 1 with `2T`/`3T` unmoved, while `TR` on that poll should jump to about 250–4800 mG (the peak-threshold square root of 250 as the floor, thousands to the ~4850 clip for a deliberate knock)
- Knock twice quickly and `2T` counts 1; knock three times and `3T` counts 1
- A pick-up-hold-place must not report `TAP`
- After settling `STL` counts only 1 (no longer incrementing at frame rate)
- An ordinary lift/place (no need to swing it) counts `LFT` and `PLC` once each
- At rest `LR` should sit at tens of mG

---

## Knock detection reads raw magnitude, not magnitude minus gravity

`[test]` `recognizer.rs` covers knock, turn and the knock count.

The first approach subtracted a sliding gravity estimate before comparing against the bars. **Rejected**: the estimate carries its own error for the whole run, and the desk/hand transition is exactly the signal being measured. Raw magnitude drifts only at the sensor's noise floor — 8-22 mG of rest swing, measured — so a 150 mG band avoids both the estimate error and the real transition.

For the same reason the SDK-style smoothed baseline is frozen during a knock. A knock is a gravity-independent transient, so letting the baseline absorb it leaves an overshoot that sits above the quiet bar and starves the next knock's peak window.

---

## The peak window's semantics are taken from datasheet 10.1

`[test]` The `TapPhase` state machine in `recognizer.rs`.

The windows are not invented: they follow the walk-through decay semantics of datasheet 10.1, where a peak that **does not fall back below the quiet bar inside its window** is a press or a turn rather than a blow. That is the only line separating "one knock" from "holding a button down" within a single sustained motion.

`SHAKE_ON_MG`'s 2.0 g works the same way: what is compared is the vector residual and the count of **crossings**, not the time spent above the line — one knock (a single impulse) and one shake (an impulse per reversal) differ in count, not amplitude. The W3C reference figure of 2.5 g on a 60 Hz single-axis magnitude does not transfer: the recognizer only sees a sample every 20 ms and misses crests between polls, and a 3 g shake leaves only about 2.5 g of residual at 5 Hz.

> `SHAKE_ON_MG` and the stillness band are `[reported]` — not re-verified on hardware, values taken from `SR`.

---

## The accelerometer low-pass was measured to be irrelevant, so it is off

`[measured]` Two passes over the same gestures, aLPF off and on.

Resting noise and knock readings were **identical** either way: the attenuation comes from the structure and coupling, not from this filter.

So `CTRL5` leaves `aLPF_EN` clear and keeps the full 896.8 Hz bandwidth — a knock is a broadband transient and the filter can only shave the peak before it arrives. The recognizer's bars (peak 250 mG / quiet 150 mG) were calibrated on this unfiltered stream, so **turning the filter back on requires re-calibrating them together**. The gyro filter is off for the same reason: no semantic reads the gyro path's peaks.

---

## No-Motion is a level, not an event

`[test]` `a_still_bit_held_across_polls_reports_the_transition_once` (`bsp-esp/src/components/qmi8658.rs`).

The engine asserts `STATUS1.bit6` on **every** poll while the device is still. Publishing that bit as an event reports `Still` at the frame rate for as long as the device rests.

So only the 0→1 rising edge is published: a bit held high across polls stays out, and a later rise is a new one.

The raw `STATUS1` still rides along with the frame, so the panel's `ST` row can tell a latched event bit from one the engine is genuinely re-raising — dropping the register once the events are decoded would lose exactly that distinction.