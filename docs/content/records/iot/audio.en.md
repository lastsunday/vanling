+++
title = "Audio Implementation Record"
weight = 40
sort_by = "weight"
[extra]
source_file_hash = "109db55f3ae6a05ae291e917e9d75bf7e71f5332"
translated_at = "2026-10-06T05:35:23Z"
+++

<!-- doc-audience: ai -->

# Audio Implementation Record

Judgements and rejected approaches from the capture path in `apps/iot`. **AI-produced, not reviewed by a human** — see the [section above](@/records/_index.en.md).

Each judgement is labelled by its evidence: **`[test]`** a unit test covers it, **`[measured]`** there is a hardware measurement with a reproduction, **`[reported]`** not reproduced, recorded only as a lead.

---

## Silence must read as "unchanged", or the whole diff chain spins

`[test]` `a_silent_capture_stops_reading_as_changed_once_the_ring_has_wrapped` (`core/src/drivers/audio.rs`).

Once the window is full, `cursor` advances on every poll, and a derived `PartialEq` counted each advance as a change. Measured: **0 of 600** successive silent polls compared equal.

The consequence is not visual jitter but the entire render diff pipeline re-running at the render cadence (20 ms) on a quiet room, each pass repainting a whole frame — and the `Diagnostics` it compares embeds a 1.6 KB audio envelope, so every comparison is a by-value copy of that.

So `AudioEnvelope` hand-writes `PartialEq` over what the panel can actually draw: the committed column count and the drawn column values, never the write pointer. The test is "did anything the render layer can see change" — a pointer that moves while the picture does not is not a change.

---

## The three backlog depths come from measurement

`[test]` `the_depth_a_healthy_capture_settles_at_is_not_called_a_backlog` (`core/src/drivers/audio.rs`).

A poll that arrives on time finds about one poll period of audio waiting; a loop running a fixed beat behind the wire settles **one whole period** deeper than that steady state, for ever. So putting the backlog line at two periods makes a ring that was never filling report every single poll.

The depths the panel reported while the ring stayed healthy were `7_708 / 7_934 / 8_188` bytes, on a `24_576`-byte ring. `CaptureBacklog`'s warn and clear lines sit there, one poll period apart.

`[reported]` The panel was seen declaring the whole ring drained and rebuilding the DMA, dropping audio that was mid-play. The trigger was never isolated; kept as a trace only.

---

## One cascade, two column sets

`[test]` `a_low_tone_drives_the_raw_columns_but_not_the_weighted_twin`, `a_mid_band_tone_reads_the_same_through_both_columns`, `spl_is_dbfs_with_the_microphones_own_reference_back` (`core/src/drivers/audio.rs`).

A-weighting is 0 dB at 1 kHz and about −26 dB at 62.5 Hz. So two sets of columns exist: the panel's scope and PK/RMS rows read the **unweighted** signal (that is the shape the codec handed over), and the corner readout reads A-weighted (that is a number comparable with a dB(A) from elsewhere).

`AWeight` runs its cascade once, and the envelope's peak columns and weighted columns absorb the **same batch** of samples. A second cascade over identical coefficients and zero state could only ever agree on the answer, and running both doubled the per-sample cost of the capture path.

---

## Quantisation noise fed a 65 dBA floor

`[test]` `a_quiet_floor_reads_quiet_instead_of_feeding_a_limit_cycle` (`core/src/drivers/audio.rs`).

A-weighting is nearly flat at low frequencies while the filter poles sit close to the unit circle. A fixed-point state wide enough recycles quantisation noise through those poles, so a board hearing nothing but its own codec floor settled at a steady ~65 dBA.

`STATE_FRACTION`'s width is derived from that ceiling in reverse: a quiet floor of a few dozen LSB must read as a few dozen LSB, not as a self-sustained four-hundred.

---

## 0 dBFS = 102 dB SPL is this microphone's calibration

`[measured]` The derivation and how to re-anchor it are in the [hardware constraint record](@/records/iot/hardware-constraints.en.md).

Core measures dBFS, and the offset belongs to the board (`SPL_OFFSET_DECIBELS` in `bsp-esp`). `iot-core` cannot depend on the board that carries the part, so `spl()` takes the offset as a parameter and the board passes its own value in.

`[test]` `spl_is_dbfs_with_the_microphones_own_reference_back` also pins that the difference between the two readings must be **entirely** that offset: anything in between means the two columns have stopped being the same measurement.