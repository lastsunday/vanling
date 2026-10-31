+++
title = "Echo Cancellation Implementation Record"
weight = 70
sort_by = "weight"
[extra]
source_file_hash = "6f0ce82128f902f4425edf567be0cbfa01cb56b8"
translated_at = "2026-10-31T00:00:00Z"
+++

<!-- doc-audience: ai -->

# Echo Cancellation Implementation Record

Judgements and rejected approaches from `apps/iot/crates/core/src/drivers/aec/`. **AI-produced, not reviewed by a human** — see the [parent note](@/records/_index.en.md).

Each judgement is marked with its basis: **`[test]`** a unit test backs it, **`[measured]`** a measurement with a way to reproduce, **`[reported]`** not reproduced, kept only for the record.

Linear AEC does one thing: model the acoustic path from the samples sent to the speaker, and subtract it from the microphone. Nonlinear suppression is not part of it — see the module documentation in `aec/mod.rs`.

## The first board numbers: the pairing holds, the cancellation does not

`[measured]` Reproduce: `moon run iot:probe-echo-s3`, flash to the LCKFB S3, `moon run iot:flash-probe-echo-s3` (**with two boards plugged in that task carries no `--port` and will pick one for itself**), then capture 30 s of serial.

| Figure | Measured | Verdict |
|---|---|---|
| paired / unpaired | 160–200 / **0** | ✅ the two streams are in step; log 160 of 2944, well inside its runway |
| worst single poll | 15.3–16.6 ms of a 30 ms bound | ✅ it fits, but at half the budget |
| feed | 200/s, 0 late, worst work 142–428 µs | ✅ steady |
| **ERLE** | **0 dB for all 60 s, not once moved** | ❌ |
| delay | None → 16 → 50 → 6, wandering the whole 64-sample window | ❌ |

**The delay search wandering is the finding here, not the zero ERLE.** A real acoustic path has a *fixed* delay: it converges and stays there. A delay that changes every second says there is no stable relationship between the reference and the microphone — so the filter never converges on anything, and it converges **quietly**.

### A/B: the microphone does hear the speaker, and "it does not" was an artifact I measured

`[measured]` An ERLE of 0 means two completely different things: the reference is mispaired, or **there is no echo to cancel at all**. The probe separates them by **flipping the amplifier's mute every 6 s** (`bench/probe/echo_room.rs`).

The first A/B said: with the amplifier on the microphone sat at −76…−90 dBFS against −43 dBFS off — **a 45 dB difference** — and concluded there was no measurable acoustic path on this board.

**That conclusion was wrong, and it was an artifact of how the measurement was built.** The envelope level is a rolling window of 200 columns × 20 ms = **four seconds**, while the first version flipped every 1 s — so every window straddled two complete cycles, both states got averaged, and what came out was a beat. After moving to a 6 s flip and withholding the reading until the window lies wholly inside one state, the same figure did not reproduce.

Printing the **per-poll peak** (20 ms, immune to the window) instead gives a continuous trace, and it **moves with the reference peak**:

| Reference peak LSB | Microphone peak LSB |
|---|---|
| 22971 | 2180 / 2375 |
| 17100 | 2220 |
| 9178 | 1413 / 1454 |
| 8884 | 1513 |

**The microphone follows the speaker — the acoustic path is real**, roughly 15 dB below the reference.

The conclusion therefore narrows: **an ERLE of 0 with a wandering delay is the canceller's own problem, not a missing acoustic path.** The filter is holding a reference that genuinely exists and genuinely correlates, and still converging on nothing — which is the same silent-failure road, just one layer deeper than before.

### The capture only hands over half the audio — which is why the delay search cannot lock

`[measured]` The probe now reports capture throughput (`capture rate`), which gives a number that needs no inference:

```
41 polls in 1007 ms, 8424 paired = 8365 samples/s of the 16000 the canceller runs, 205 per poll
```

**A poll period of 24.6 ms, pairing 205 decimated samples.** At 16 kHz, 24.6 ms should be 394 — it is 52% of that, **almost exactly half**.

That number is what breaks the search. The delay estimator correlates over 256 ms of history (`REFRESH_SAMPLES = 4096`), and the microphone now hands over half of real time on every poll: **the two streams cover spans that are not proportional, so the "same moment" the estimator pairs is not the same moment.** The correlation peak therefore lands on no grid point, `best_lag` picks noise out of the window every time, and the delay wanders `None → 16 → 50 → 6`.

**Two candidates, neither excluded:**

1. **The RX is configured at half the rate.** 24.6 ms × 24000 = 590 frames → 197 decimated samples, which matches the measured 205 closely.
2. **The drain stops early.** `drain` takes about 2460 bytes per poll where the period says the ring should hold about 4723.

> The next section settles both of these and gives the real cause.

### Root cause: the RX holds one sample per frame; the driver counted two

`[measured]` How it was decided: count, in the drain, how often the two halves of a four-byte group carry the same sample.

```
slots: 2/480 = 4 per mille
slots: 9/543 = 16 per mille
```

**Effectively zero** — the two halves of a four-byte group are not the same sample. The receive buffer holds **one sample per two bytes**, while the driver used `chunks_exact(BYTES_PER_FRAME = 4)` and took `frame[0..2]`, so it **kept one of every two consecutive samples and threw the other half of the microphone away**.

The source is esp-hal 1.2.2's `Channels` documentation, on the same page as the `Channels::MONO` definition:

> `mask` ... **Inactive channels do not consume or write data in the DMA buffer.**
> `fill` ... **This field is ignored in the receiver unit.**

`MONO = new_impl(2, 0b01, None)` — slot 0 is active and slot 1 **does not occupy the DMA buffer in the receiver**. So **the two directions have different frame widths to begin with**:

| Direction | Per frame | Mechanism |
|---|---|---|
| TX | 2 samples = 4 B | `fill=None`, so an inactive channel **repeats** the last active one |
| RX | **1 sample = 2 B** | the inactive channel **consumes no** DMA data |

The arithmetic closes: 48000 ÷ 2 ÷ 3 = **8000**, matching the measured 8000 samples/s exactly.

**Where the wrong model lived** is the `SLOTS_PER_FRAME` doc comment in core, which asserts that `Channels::MONO` is only a slot selection, that both slots reach the DMA, and so that the capture moves twice `SAMPLE_RATE_HZ` samples per second. **That comment is wrong, in core, and the driver only copied it** — so what got fixed is the model, not one multiplication.

### After the fix

`[measured]` Same probe, same flash:

| | Before | After |
|---|---|---|
| capture rate | 8000 samples/s | **15 561 samples/s** (target 16000) |
| paired / poll | 205 | **442** |
| unpaired | 0 | 0 |
| delay | `None→16→50→6`, wandering | `Some(16)…Some(64)`, `None` no longer appears |
| **ERLE** | 0 dB | **still 0 dB** |

**The capture rate is fixed; the pairing is not.** The delay search now answers every time (wandering over 16–64) instead of picking noise out of the window — but the ERLE is still zero, so **alignment is a separate problem that remains**.

### A second failure met while fixing it

The probe panicked immediately after the frame width was corrected (`LoadProhibited`, in `Es8311Tx::new`). A bisect showed it was **not the frame width**: the real cause was `BYTES_PER_POLL`, which also used `SLOTS_PER_FRAME` to size **capture** bytes, so every capture-side buffer derived from it was twice as large, the pairing scratch grew from 1601 to 3201 `i16`, and `Board::new` returns that struct by value on the stack — which overflowed the main task's stack.

**`BYTES_PER_POLL` is the root constant**: the capture ring's depth, a poll's drain budget, the lag conversion and the scratch all derive from it. Changing it from `SAMPLE_RATE_HZ × SLOTS_PER_FRAME × BYTES_PER_SAMPLE × CAPTURE_MS` to `SAMPLE_RATE_HZ × CAPTURE_BYTES_PER_FRAME × CAPTURE_MS` (3840 → **1920**) put the scratch back to size and the panic went away.

`[test]` Two tests written against the old constants then failed (`7708/7934/8188`, `9600/7680`). What they assert is a depth **in polls** — their own comment says "2.13 polls" — so the fix was not to copy the new observed numbers but to **rewrite them in polls** (`BYTES_PER_POLL * 2 + offset`), so the next frame-width correction cannot silently invalidate them again.

### Also found: the envelope's time axis has been running at half speed

`[measured]` `SAMPLES_PER_COLUMN` used `SLOTS_PER_FRAME` too, while `SampleStream::push_bytes` decodes one sample per two bytes — **so every column held half the samples its own stated duration claims**. The Audio page's window has been drawn at **half speed in every build so far**. Unrelated to the AEC: the same mistake's other victim.

### The delay search is not "measuring a large delay"; it has no peak at all

`[measured]` Widening `MAX_DELAY` from 64 to 256 (diagnostic, since reverted — 8x the search cost would not fit the poll budget) sent the reported delay straight to **223–255**.

**It pegs at whatever edge it is given**: window 64 reports 64, window 256 reports 255.

That is not "the true offset exceeds the window" — it is **the search returning the window boundary itself**. `best_lag` takes the highest-scoring grid point, and the score climbs monotonically toward the edge, which means **there is no correlation peak, only a ramp**.

The likeliest mechanism (**unverified**): if both signals carry a DC offset, `cross·1000/(√ref_pow·√mic_pow)` comes out near ±1000 at **every** lag — far above the 0.3 confidence gate. The search then takes the first grid point strictly greater than the running maximum over a flat line, so the answer is decided by traversal order and noise, and it appears as "always at the window edge".

**That accounts for every observation so far**: ERLE pinned at 0, the delay wandering, and the post-frame-width change from "`None`, wandering" to "stably 64" — all one symptom seen from different sides. **So the next step is not more lag tuning; it is to see what the correlation actually looks like.**

The smallest criterion: report `best_lag`'s winning score next to its lag. A score near 1000 says DC offset; a low score that still lands on the edge says the traversal or indexing is wrong.

### The criterion: the correlation peak reaches only 78-271 per mille, and the gate is 300

`[measured]` Reporting `best_lag`'s winning score (`delay_peak`) settles the whole chain of symptoms at once:

```
delay None     peak 215 per mille
delay Some(53) peak  89
delay Some(64) peak 271
delay Some(60) peak  78
delay Some(55) peak 193
```

**The gate `MIN_CORRELATION = 300` is never crossed.** So `estimate` is never updated, and **every reported `delay` is stale** — the "stably 64" seen after the frame-width fix was not convergence, it was a leftover.

**The DC-offset hypothesis is refuted**: that would score near 1000 at every lag, and the highest reading is 271.

**What is actually true: the microphone and the reference are barely correlated within ±64 samples**, and the best point the search finds is 0.08-0.27. The delay search is not broken — it is correctly answering "no measurable correlation here".

`MIN_CORRELATION`'s own comment said **`[reported]` 0.3 is a guess with headroom, and wants a bench number**. That number now exists: **this board's acoustic path reaches only 78-271 per mille.** The gate should be re-set from the measurement rather than the lag tuned further — which is precisely what that comment said not to do.

---

**This record is ordered algorithm first, hardware cost second.** I got that causality backwards once — measured the CPU, then used a synthetic room's number to argue the algorithm was fine. The order only works one way, because "is the algorithm worth doing" decides "is it worth spending CPU on", and not the reverse.

---

## There is no usable public recording: three measured unusable, a fourth unverifiable

`[measured]` The plan called for "measuring how many dB a fixed-point linear AEC takes off, using the official test vectors". After searching: **no clean (reference, microphone) recording of a real linear acoustic echo path exists publicly.**

**Espressif's `docs/_static/aec_in_far.wav` / `aec_in_near.wav`.** Labelled "far-end signal (playback reference)" and "near-end signal (microphone containing echo)", and the pair a reader reaches for first. **Not synchronous**: over the whole 53 s at every lag the best normalised cross-correlation is **0.137**, a single-tap fit accounts for **0.08 dB**, envelope correlation is 0.35, and the spectral centroids differ (1301 Hz against 2620 Hz). This canceller reports **−14.4 dB** on them — exactly what subtracting an unrelated signal does.

**Espressif's `audio_mic_file` / `audio_ref_file`.** What Espressif's own `test_afe.cpp` feeds to `aec_process`. Synchronous (correlation 0.743, single-tap gain 0.9956) — **but with no echo path in it**. Offline least squares, which bounds any stationary linear canceller:

| Taps | ERLE, whole file | Kernel peak |
| --- | --- | --- |
| 32 | −1.04 dB | 1.02, on the **last** tap |
| 64 | +0.04 dB | 1.03, on the **last** tap |
| 128 | +0.42 dB | 1.02, on the **last** tap |
| 256 | −4.97 dB | 1.02, on the **last** tap |

A kernel whose energy sits on the final tap, with no decaying structure and no bulk delay, is not an acoustic path. Fitting on the first half and testing on the second gives +5.08 dB against +6.15 dB in-sample — the relationship is persistent, it just is not an echo.

**Microsoft's ICASSP AEC-Challenge `real_doubled`.** The field's actual reference dataset, 10,000 real environments. **Forty GUIDs scanned and not one has a linear echo relationship**: the best offline Wiener bound over 128 taps is **−0.42 dB**, and the best correlation anywhere is 0.137. **This is documented behaviour.** Cutler et al., *ICASSP 2023 AEC Challenge* (arXiv:2309.12553), §4.1:

> Even though our application uses the WASAPI raw audio mode to bypass built-in audio effects, the PC can still include Audio DSP on the receive signal [...] it can also include **Audio DSP on the send signal, such as AEC and noise suppression**.

A consumer stack that already runs its own canceller between the loopback tap and the file leaves a *linear* model nothing to predict. The same section says ERLE is not this dataset's metric either (**PCC 0.31** with subjective quality).

**ITU-T G.168 (04/2015)** was the next answer and pointed exactly the right way: Annex D gives eight echo-path models `m_i(k)` as **explicit impulse response tables** with scaling factors `K_i`, so the reference material genuinely needs no audio files at all. The PDF is publicly downloadable (154 pages). **It is not used**, because the transcription could not be verified:

- On some pages text extraction loses the first column of each row (m1's first value sits 1–2 pt above the rest of its row)
- The reading order is ambiguous ("to be read in columns"); column-major and row-major differ by a factor of two
- **Both readings violate the standard's own invariant** — that the scaled path's magnitude response must not exceed 0 dB — by 1 to 11 dB

A table that cannot be checked is not something a test should be built on. Same reason the software clock calibration was deleted rather than shipped: **a number you cannot stand behind is worse than no number.**

G.168 does settle one thing, in its Test 12 note:

> This test may be enhanced by the future addition of test vectors that represent real acoustic echo signals. **This enhancement is for further study.**

The standards body says the real acoustic test vectors do not exist either.

### So the reference is an offline optimum on the same path

Not a borrowed path: for each path the test **solves the least-squares problem there and then** and reports what the best filter of the same length could reach. That number is computed rather than transcribed, so it cannot be wrong, and it answers the question a linear adaptive filter exists to answer: **did it find the optimal linear solution, and how close is it?**

To reproduce (no external assets, no network):

```sh
cd apps/iot && cargo test -p iot-core --test aec_erle -- --nocapture
```

| Path | Taps | Offline optimum | This implementation | Gap |
| --- | ---: | ---: | ---: | ---: |
| short, fast decay | 16 | 62.9 | 61.0 | 1.9 |
| medium, moderate decay | 48 | 58.8 | 57.2 | 1.6 |
| long tail, slow decay | 192 | 38.9 | 38.8 | **0.1** |
| **tail longer than the filter** | 256 | **14.4** | 13.2 | 1.2 |

### The 15 dB bar turns out to be about the path, not the algorithm

**When the tail is longer than the filter, the offline optimum itself is 14.4 dB.** No 128-tap linear filter can reach 15, so asking the algorithm to is asking it to be wrong.

The gate is therefore stated as **a gap to the offline optimum (≤ 3 dB)**, with the 15 dB bar applied only to paths the filter can hold in full. That distinction is something only a self-verifying reference can give: a borrowed path cannot tell you who set the ceiling.

Two caveats stay attached. Clearing 15 dB is **necessary, not sufficient** (PCC 0.31). And the paths are generated, so they test the model rather than a room — this board's loop gain, delay and thresholds still need this board.

---

## A generated path needs a direct reflection, or the delay search has nothing to lock onto

`[measured]` The first version made **every tap the same order of random noise**, so the path had no onset: every lag correlated about equally, the delay search had nothing to hold and reported wherever the noise happened to peak. A 12-sample path reported 15 and a 4-sample one reported 22 (cross-checked offline, where the peak genuinely was), and the filter reached **0.4 dB** because it was aiming at the wrong place.

A real room impulse response has **one loud direct arrival** above every reflection after it. With that added (direct 30,000, reflections ±8,000) the correlation peak lands **exactly on the true delay** for all four paths (8/12/6/4), and the ERLE goes from 0.4 dB to 61 dB.

The same mistake appeared once in an assertion: the original check that "the talker was not learned as an echo" measured the **share** of filter energy past the end of the path — but a 128-tap filter on a 48-tap path already carries 46% of its energy there, as many small coefficients. **The quantity is growth, not share.**

---

## The freeze stops the damage but not the first half second of it

`[measured]` A new test found this. The coherence estimate has a time constant of about 512 samples, so after a talker begins the filter still adapts at **full speed** for roughly 543 samples, and then the freeze locks in whatever state that produced.

| | Value |
| --- | --- |
| Coverage of the freeze | 31,457 / 32,000 samples (98%) |
| Energy past the path, quiet | 3.203e10 |
| When the freeze took hold | **1.176e17 (3.7e6×)** |
| At the end | 1.176e17 (**no further growth once frozen**) |

So the design is **partly** effective: the freeze does stop the damage growing, but it does not stop the beginning of it. In Q30 that is about 3.6% of gain learned into taps that should be zero — not nothing, and not everything.

**The cause is the meter's lag, not the threshold.** Coherence takes hundreds of samples to read "incoherent", and the filter has already run through them. Shortening the hold does not fix it, because coherence is no faster. The real fix needs a **shorter meter dedicated to the freeze decision**, which is a design change and was not made during a validation pass.

`[reported]` How bad that half second sounds has not been measured on this board.

---

## The step must not be scaled by the residual

`[measured]` Valin's variable-step MDF/NLMS (TASLP 2007) does scale the step by the residual, and that is the obvious reading of "derive the step from the residual". **But before the filter has learned anything, the residual is the microphone itself** — it is not a quantity with any relation to the filter. It is the loudest thing in the room.

On a room at 12 000 LSB that put the opening step at 1/48 of full speed, and the filter reached **−0.5 dB**; the same loop at a constant full-speed step reached 20 dB. A slow start does not cost a little accuracy — it costs the entire convergence.

So the step in `gate.rs` is a **constant**: full speed, or zero while frozen. The residual is used only for the one decision it can answer unambiguously.

"Full speed" is a choice, not a default: NLMS is stable anywhere in `(0, 2)`, and `1 << 29` (0.5 in Q30) is the conventional pick with room left below it.

---

## Freezing cannot look at the residual level alone: a converging filter and a talking person both leave a large error

`[test]` This is the same failure above, arriving from the other side. An absolute threshold on the residual freezes the filter during its own startup.

What separates the two is **coherence**:

- While the filter is still converging, the error is mostly **echo it has not yet accounted for**, and is therefore highly correlated with the reference (coherence ≈ 1);
- While someone is talking, the error is speech, and is not (coherence ≈ 0).

That is Gänsler's coherence-based double-talk detector (JASIP 2015 calls out its two drawbacks: a threshold that depends on background noise and on the talker's level). It lives beside the filter rather than as a second detector beside that, because the filter already computes everything it needs: three running means, one extra multiply per sample.

Both thresholds in `gate.rs` are marked `[reported]`: `LEAKAGE_FREEZE = 1<<24` (~4096 LSB RMS, taken from the 30 cm speech peaking to 80 dB SPL calibration in the [audio record](@/records/iot/audio.en.md)) and `MAX_COHERENCE = 0.6`. Both want re-measuring on a bench with a person in the room. The freeze is confirmed working on a synthetic talker — 98% coverage — but see the onset burst above.

**The `has_path` layer was removed.** It used "is the echo estimate large enough" in place of coherence, and a filter that predicts the entire microphone also has a large echo estimate — it has learned nothing. Coherence alone is enough.

---

## The numerator of coherence is a signed mean, not a squared one

`[test]` `coherence = |E[e·x]| / (rms_e · rms_x)`. The first version squared `e·x` before averaging, giving **magnitude-squared coherence** — which cannot tell an inverted path from an absent one, and those have to be told apart here.

It costs the same one multiply. So `power.rs` gained a `SignedMeter`, at the same shift and for the same reason as `PowerMeter`: the ±1 floor when the shift truncates is the only thing that lets it actually reach its target rather than stopping half a step short, and on a signed value that matters twice over — a mean that cannot cross zero is not a mean.

---

## `Noise::new(seed | 1)` folds neighbouring seeds onto the same sequence

`[test]` An xorshift is stuck at zero forever if it starts there, and zero is the one seed a caller is likely to write — so it needed substituting. **But it needs nonzero, not odd.**

The original `seed | 1` folded 2 onto 3 and 3 onto 3. Two "independent" noise sources were then the same signal, coherence read 1.0, and the correct answer is 0. The danger is that it is entirely silent: the tests still run, they just assert a false number.

`signal.rs` now has a test that pins this specifically.

Found in the same rewrite: an `i32` LCG with `n >> 16` used to generate uniform noise is wrong. The arithmetic shift makes half the values negative, `% 16000` keeps the sign, subtracting 8000 then leaves the sample range at [−24000, −1] — mean square 6.8× too large, which made the freeze threshold meaningless. Everything now uses `signal::Noise`.

---

## The delay search needs a confidence gate

`[test]` A path further away than the window correlates at nothing in every lag inside it, and a search returns the **largest** of those — a maximum over noise. A power gate cannot catch this case: two rooms that are not silent still produce a peak.

Without a gate, a path beyond the search range was answered with a random lag inside it (measured: 2, true 84) and the caller had no way to tell. `MIN_CORRELATION = 300` (per thousand) makes the same input answer "not found", with `settled()` staying false.

Returning the window edge is equally wrong — that is a confident wrong number. It now returns `None`.

---

## The delay search must be rate-limited: it costs more than the filter that uses it

`[measured]` One search is `(2·SEARCH + 1)·WINDOW = 65 × 256 = 16,640` products, about two thirds of what the adaptive filter does on the same sample. **Running it per sample makes finding the delay cost more than cancelling with it.**

The delay is a property of where the board is sitting, not of the audio, so it runs every 40 ms (`REFRESH_SAMPLES = 16 × WINDOW`). The first search fires the moment the history is full rather than one interval later — the delay is the thing worth having early.

Before the throttle `cargo test -p iot-core aec` took 45 s (quadratic over 851,940 samples); after, 0.14 s. `DelayEstimator::searches()` reports the real count, because whoever raises that rate is entitled to know what they bought.

---

## Steady-state ERLE and whole-file ERLE are different quantities

`[measured]` Same audio, same filter, two numbers that are both correct and answer different questions. **The AEC Challenge rule agrees** (Cutler et al., §7):

> For the far end single talk scenario, we evaluate the **second half** of each clip, to avoid initial degradations from initialization, convergence periods, and initial delay estimation.

So **reporting the second half only is the standard practice**, and the whole-file figure is demoted to an aside.

The first version compared the whole-file sum against the end-of-file instantaneous reading, saw 51 dB of disagreement, and went looking for a broken meter. The actual lesson: **convergence and steady state have to be reported separately**, or one of them will always be mistaken for the other. The per-second curve (16 → 19 → 21 dB) is the evidence for how fast it converges — a single end-point number cannot distinguish "converged fast" from "converged after twenty seconds", and that difference decides whether the first half of a call is usable.

The meter's own reading (63.0 dB) and the summed samples (57.2 dB) differ by 6 dB, within tolerance; the windows differ, one being a ~30 ms exponential average and the other the whole record.

---

## Arithmetic places this implementation got wrong first

`[test]` Each was wrong, then refuted by a measurement, then changed:

**A `PowerMeter` step floor cannot use `max`.** `max` takes the **larger** value, so a falling level moves 1 LSB per sample instead of 2⁹ and the meter holds the level for about 1000× too long. A residual settled at 389 against a true 900. The floor applies only where the shift produced nothing at all.

**The ring must advance before it writes.** Otherwise `head` points one slot past the newest sample, `tap(0)` reads the wrong slot, and every peak lands a tap late.

**Integer `log2` tail extraction.** When squaring the mantissa, the carry bit has to be separated from the scaled value: the carry is `squared >> 1`, not `squared >> (LOG_FRACTION + 1)`.

**`ErleMeter`'s ratio should not be Q-scaled.** Scaling the numerator by 2¹⁶ comes back out of the logarithm as sixteen extra octaves. Integer division truncates under half a decibel.

**The hold counter must be capped.** Left free-running, `above` saturates to `u32::MAX` during a long stretch of speech, and since coming back costs one decrement per sample from wherever it stopped, the filter can never be released again — a permanent freeze.

---

## CPU: measure the clock before talking about anything else. 80 MHz, not 240

`[measured]` The first version of this section carried a table I could not stand behind. It said L=128 costs 74.7 ms/tick and "219 cycles per MAC", converted using a clock off the datasheet. **Both were wrong, and for the same reason: I had never read what this part is actually configured to.**

### A reference loop caught it

The probe gained a deliberately plain loop: a 256-coefficient, 1024-sample **integer** FIR, no bounds checks, no `&mut self` — **the verbatim case of Espressif's published `dsps_fir_f32` benchmark**, so the number is directly comparable.

I expected it to land near 1.7 cycles per MAC. It came out at **12**. A loop simpler than the canceller running 193× slower than it means the problem is not in the canceller's code.

### The CPU was at 80 MHz

`esp_hal::clock::cpu_clock()` reads back **80,000,000 Hz**. The reason is in esp-hal 1.2's own source:

```rust
pub enum CpuClock {
    /// 80 MHz CPU clock
    #[default]
    _80MHz  = 80,
```

`Config` derives `Default`, and `CpuClock`'s default arm is `_80MHz`. **`esp_hal::init(Config::default())` gives 80 MHz.** The claim I quoted earlier — that `Config::default()` sets the maximum frequency — came from a blog about Arduino-ESP32, not from esp-hal.

At 80 MHz the reference loop is 12 cycles/MAC, while Espressif's **float** assembly at 240 MHz is 1.69 — 36 when folded to 80 MHz. **My integer loop does better than that. The platform, clock, timer and build are all sound.**

### The corrected numbers

| | µs / tick (L=128) | cycles/MAC @80 MHz |
| --- | ---: | ---: |
| **Reference FIR** (trivial integer loop) | — | **12** |
| AEC convolution | 16,206 | 31.6 |
| **AEC coefficient update** | 48,941 | **119** |

**Not 219.** But so is the attribution I gave for the remaining gap — that it came from bounds checks and the per-iteration stack reload `&mut self` forces. **That is wrong too**, and the next section is why.

### Block adaptation: it passes

The coefficient update is the largest single item — 7.7 ms at L=16, 49 ms at L=128 — and it does not have to happen on every sample. Once every four (`UPDATE_STRIDE`, counted off `head` so it carries no state of its own):

`[measured]` **L=16 falls from 15,531 to 9,680 µs, under the ≤10 ms bar.**

| | per sample | stride 4 |
| --- | ---: | ---: |
| L=16 coefficient update | 7,699 µs | **1,880 µs** (−76%) |
| L=16 whole filter | 9,803 | **3,945** (−60%) |
| **L=16 whole canceller** | 15,531 | **9,680 µs** |
| L=32 | 22,892 | 13,528 |
| L=64 | 36,772 | 20,448 |
| L=128 | 70,945 | **34,911 µs** (−51%) |

What it costs shows up in ERLE: **steady state is bit-identical** (the gap to the offline optimum is still 1.9 / 1.6 / 0.1 / 1.2 across four paths, and a path that moves is still tracked across a six-sample shift), **but the whole-file reading falls 20.9 → 16.2 dB.** Convergence is slower; what comes after convergence is not.

The convolution is now half the filter (2,065 against 1,880) and the update is no longer the dominant cost.

### The frequency-domain route: the FFT is written, and the answer is no

Best practice points at MDF / partitioned block frequency-domain NLMS. `arduino-audio-tools` happens to carry both side by side (time-domain NLMS alongside a port of speexdsp's MDF), and its verdict on the time-domain one is:

> O(filter_len) work per sample, no FFT … **a reasonable fit for small microcontrollers with a short, simple echo path**
> cost grows linearly with it — **this is not the right tool for a long, reverberant echo tail**

So the plan held: write the FFT and measure it before discussing any architecture. A 512-point radix-2 fixed-point transform whose twiddles come from binary exponentiation (filling the table by repeated multiplication drifts; this way each entry is at most 9 roundings deep), with seven tests, one of which checks every bin against an independent f64 direct DFT.

`[removed]` **The FFT was deleted after it was measured.** The finding stays; the code does not — it had no caller on the product path, and a record carrying "the transform costs 182% of the budget" is worth more than a transform nobody uses. **The numbers below are measured, not inferred.**

`[measured]`:

| | |
| --- | ---: |
| 512-point forward transform | **3,046 µs** |
| cycles/butterfly | **105.8** |
| transforms one 320-sample tick needs | 2 blocks × 3 = 6 |
| **Cost of the transform alone** | **18,276 µs = 182% of the AEC's entire budget** |

**The transform alone is 1.8× over budget, before a single line of partitioned convolution, filter update or delay search exists to be counted.**

Smaller frames do not rescue it: the per-sample cost goes as 3·5·2M·log(2M)/M = **15·log(2M)** — 135 at M=256, 120 at 128, 90 at 32 — which **asymptotes rather than falling.** Packing a real input into a half-length complex transform halves it to 9.1 ms (91%), leaving 9% for the filter, which is not an arrangement. RAM goes the same way: 2 KB of twiddles plus the caller's 4 KB of working buffer, against **3,064 B** for the whole time-domain canceller — so frequency domain loses on CPU by 1.8× and on RAM it is merely a wash, which is not a reason to buy it.

### The second core: identical to the digit

`FROM_CPU_INTR1` is a second timer interrupt source on the *same* core (`InterruptExecutor<1>`), not a second core — **the product runs entirely on core 0 and core 1 is idle.** The probe now runs the same measurement on both, one after the other (never overlapping, which would compete for the very memory the question is about):

| | cpu 0 | cpu 1 |
| --- | ---: | ---: |
| FFT forward transform | 3,046 µs | **3,046 µs** |
| cycles/butterfly | 105 | **105** |
| Reference FIR | 34,628 µs | **34,628 µs** |
| L=128 whole canceller | 70,945 µs | **70,945 µs** |
| Coefficient update | 48,939 µs | **48,939 µs** |

Twenty-odd independent measurements, across four tap counts, means and worst cases: **all of them identical to the digit.**

**A second core is worth nothing here**, and that closes the diagnostic question left open earlier. Two independent LX7 cores run into the same shared resource; the per-access latency is the same and the operation count is the same, so the total must be. **This is not "core 0 is busy" — it is a bottleneck the two cores share.**

### IRAM: not that either

The butterfly was pulled out on its own into `.rwtext`, which the linker maps to 0x4037a4c4 in IRAM — confirmed not to be flash's 0x42010020:

| | flash | IRAM |
| --- | ---: | ---: |
| FFT forward transform | 3,046 µs | **3,235 µs** |
| cycles/butterfly | 105 | **112** |

**Putting it there made it 6% slower. Instruction fetch is not the bottleneck.**

### Four loops, all at an IPC of 0.1–0.33

| Loop | cycles | instructions (approx) | IPC |
| --- | ---: | ---: | ---: |
| Reference FIR | 12 /MAC | 4 | 0.33 |
| Convolution | 31.6 /MAC | 5 | 0.16 |
| Coefficient update (per sample) | 119 /MAC | 12 | 0.10 |
| **FFT butterfly** | **105.8** | **10** | **0.09** |

The butterfly is the **cleanest code of the four** — no clamps, no `&mut self`, no 64-bit accumulator chain — and it has the lowest IPC. Fetch is ruled out and a second core is ruled out, so what is left is the memory system itself. `[reported]` Whether it is the flash controller, an absent data cache, or un-pipelined multiplication is untested.

### The delay search: the grid cannot be thinned, and it is the last fixed cost

`[measured]` One search costs about **32 ms** and runs once every 256 ms (`REFRESH_SAMPLES = 4096`), which is **3.0 ms per tick** amortised — 31% of L=16's 9,680 µs. **And on its own it can exceed the product's 30 ms `MAX_SINGLE_HOLD_MS` bound.**

The obvious move is to thin the lag grid to every fourth sample, quartering the search. **Tried; it does not work.** On this module's own reference signal:

| True delay | Highest score on the grid |
| --- | ---: |
| 0 | lag 0, **1000** |
| 1 | lag −48, **146** |
| 7 | lag 56, **101** |

With a one-sample delay, **not even the adjacent lags 0 and 4 correlate.** Against a broadband reference **the correlation peak is one sample wide**, so not one grid point may be dropped. "It only costs a few taps of slack" was wrong.

What is left is `SEARCH` (the range of delays findable) and `WINDOW` (the pitch structure the correlation locks onto) — **both trade capability for time rather than being free.** `delay.rs` keeps `LAG_STEP = 1` and a `const _: () = assert!` so that anyone reaching for a stride has to delete a statement that says why not.

### One more: the FPU is off on this target

`[measured]` A same-shaped f32 reference loop in the probe ran **over 120 seconds** while its i32 twin ran in 140 ms, and the binary carries libgcc's whole soft-float complement (`__mulsf3`, `__addsf3`, …). **Float runs in software emulation on `xtensa-esp32s3-none-elf`.**

So Espressif's 1.69 cycles/MAC is out of reach here, and "switch to float" is not a shortcut. The f32 twin was removed from the probe — otherwise every run costs two extra minutes — and the result is recorded here instead.

### The clock: 80 → 240 MHz, and the console survived

`CpuClock`'s default arm is `_80MHz`, so at the time the whole board ran at a third of its rated clock. esp-hal notes that its 160/240 presets move the PLL off the 480 MHz the chip boots on and that this "breaks USB Serial/JTAG" — which is this repository's console. **`[reported]`, never tested.**

`[measured]` **Tested: 240 MHz runs, and the USB Serial/JTAG console is fine throughout.** (Measured once; that is the whole of its evidence.)

| | @80 MHz | @240 MHz |
| --- | ---: | ---: |
| L=16 whole canceller | 9,735 µs | **3,248 µs** |
| **L=16 worst tick** | **39,594 µs (over the 30 ms bound)** | **13,287 µs** |
| **Delay search, worst** | **32,870 µs (over the bound on its own)** | **10,957 µs** |
| L=32 / L=64 / L=128 whole | 13,528 / 20,448 / 34,911 | 4,511 / 6,818 / **11,639** |
| L=128 worst tick | — | **21,614 µs** (inside the 30 ms bound) |
| FFT / reference FIR | 3,235 / 34,629 µs | 1,079 / 11,549 µs |
| **cycles/butterfly** | 112 | **112 (unchanged)** |
| **Reference FIR cycles/MAC** | 12 | **12 (unchanged)** |

Two things are true at once: **the worst-tick problem removes itself** (the delay search goes from 32.9 ms to 11.0 ms, inside the bound on its own), and **not one cycle count moves** — the clock is only a divisor.

`chip_init_at(CpuClock)` is now in `iot-chip-esp`, and the probe switches between the two with a single constant, so **both tables come from the same source tree and differ only there.**

### The data cache: not reachable, and not because it is simply "off"

The sdkconfig ESPHome uses for AEC on the ESP32-S3 is:

```
CONFIG_ESP32S3_DEFAULT_CPU_FREQ_240: "y"
CONFIG_ESP32S3_DATA_CACHE_64KB: "y"
CONFIG_ESP32S3_DATA_CACHE_LINE_64B: "y"
```

The last two match the footnote on ESP-SR's AEC performance table **verbatim**, and without them ESP-SR **refuses to start** (`E (4) SR_SYS: Data cache should be 64KB`). Datasheet 3.1.5: the data cache is 32 KB in one bank or **64 KB in two**.

**But that road is closed here**: esp-hal 1.2 exposes no S3 cache configuration, and esp-metadata exposes no eFuse field for it. **ESP-SR refusing rather than configuring is itself the tell that the size is a property the chip has at boot, not one software sets.**

And the IPC puzzle is not explained by it either. Ordered by whether they write rather than by working-set size:

| Loop | working set | access | cycles |
| --- | ---: | --- | ---: |
| Reference FIR | 5.2 KB | **read-only** | 12 |
| Convolution | 450 B | **read-only** | 31.6 |
| FFT butterfly | 6 KB | **read+write** | 105.8 |
| Coefficient update | 450 B | **read+write** | 119 |

**The smallest working set is not the fastest, and the read-only loops beat the read-write ones.** The S3's data cache is write-back, and an inner loop that writes back the same location every sample per tap is the worst case for one — which is also why stride 4 bought 76% outright: the writes divided by four.

`[reported]` Whether this board has a 64 KB data cache was not read out. **But after the clock change it no longer decides whether the AEC fits** — the cycles are unchanged, only the wall clock divides by three.

## The timer: three folded calibrations, and a replacement that is stronger

`[test]` The probe first reported cycles per MAC and needed a clock calibration. **Three versions were rejected:**

1. An `acc += 1` chain strength-reduces to `acc += 64`, and read **85 GHz**.
2. An `acc = acc * M + C` chain (an LCG) is exactly what InstCombine's **pop idiom** recognises; equally folded, reading **251 MHz**.
3. `black_box` inside the chain stops the folding, but each `black_box` itself costs ~100 cycles, reading **2.34 MHz**.

**The software calibration was removed.** esp-hal exposes no cycle counter on this target, and every calibration I could write was either folded or dominated by its own barrier cost.

**Its replacement is the reference loop**, and it is stronger than a calibration: it does not guess the clock, it **runs a known amount of work in the same image, on the same board, under the same build** and gives the ratio back. The clock itself is read directly with `esp_hal::clock::cpu_clock()`.

Microseconds are trustworthy: the systimer runs off the crystal (40 MHz × 0.4 = 16 MHz, `ticks >> 4`), independent of the CPU clock, and the log's wall clock agrees with it.

---

## Literature alignment: three routes killed by evidence, and one planned change that should not have happened

This section records the things that were **decided against** after reading outside sources, and why.

### Proportionate NLMS (PNLMS / IPNLMS): rejected

The obvious refactor — step size allocated per tap by magnitude. Duttweiler 2000 and the secondary literature's statement of where it pays:

> The true benefits of using the PNLMS algorithm (faster convergence) are achieved when echo paths are sparse. Therefore, predominant applications of PNLMS algorithms are in Line/Network echo cancellers (LEC/NEC), as the typical hybrid-related echo paths are sparse. **Otherwise, for dispersive echo paths (typically observed in acoustical environment) there are hardly [any benefits].**

This board is an acoustic environment. Copying the mechanism in buys nothing.

### Round-robin subset partial update: rejected, and it is a downgrade

The plan was to replace "update all taps every 4th sample" with "update a quarter of the taps every sample, round robin" — identical multiply-accumulate count, apparently strictly better. The Partial Update LMS paper compares exactly these two:

> The updates after every Pth iteration result in exactly the same filter as obtained from LMS with P updates done every iteration. **Therefore, the algorithm suffers no degradation with respect to convergence.**

The subset variant *does* degrade, and pays for it by dropping the step from 0.004 to 0.001. **The implementation is already the better of the two.** Nothing to change.

### Hybrid open/closed-loop double-talk detection: implemented, then withdrawn

The literature (IRISA; Benesty et al. 2000) splits double-talk detection into open loop (microphone × excitation, independent of the filter, usable before convergence) and closed loop (microphone × the filter's estimate, sharpest after convergence and meaningless before it), and recommends **open loop until sufficient convergence, then closed loop**. The current implementation is neither: it correlates `residual × reference`, and the residual is `mic − echo`, so filter state is already in it.

All three statistics were implemented and compared sample by sample on one room path:

| Talker peak | False freezes | Onset | Missed | Residual mean square | Passes `LEAKAGE_FREEZE` |
|---|---:|---:|---:|---:|---|
| 8000 | 0 | 752 | 751 | 26 038 827 | yes |
| 4000 | 0 | 0 | 32 000 | 7 366 231 | no, 43% under |
| 2000 | 0 | 0 | 32 000 | 1 841 437 | no, 89% under |
| 1000 | 0 | 0 | 32 000 | 460 200 | no |
| 500 | 0 | 0 | 32 000 | 114 877 | no |

**All three agree to the sample at every level.** The binding constraint is the `loud` level gate, not the correlation: `LEAKAGE_FREEZE = 1<<24` is a mean square (RMS 4096 LSB), and at a 4000 LSB talker peak the residual measures 7 366 231, **43% under the threshold**. The correlation is never consulted.

So at this threshold setting the hybrid buys **nothing**. It was withdrawn and `update` keeps two arguments. What remains is not a mechanism but a **pinned blind spot**: `a_talker_below_the_level_gate_is_not_detected` in `gate.rs` asserts that a 2000 LSB talker measures under the gate and the gate does not move, so moving the threshold is a deliberate act rather than a side effect.

It also says the thing that actually needs fixing is the threshold — and the threshold is marked `[据称]`, reasoned from the calibration in `audio.md` rather than measured on a bench.

### Bi-LMSM: deferred

Gänsler 2000 notes it is, unlike PNLMS, not dependent on the sparse-path assumption, which is what an acoustic scene wants. But it needs two coefficient sets plus a per-sample choice, doubling the multiply-accumulates. This board's steady-state ERLE is 61.0 dB against G.168's 15 dB bar, so **there is no problem for it to solve.**

### Frequency-domain / multidelay filter: rejection stands

The 512-point fixed-point FFT was written, measured at 3,046 µs per tick — 182% of the 10 ms budget — and deleted. Nothing in the literature overturns that.

## The memory accounting was a lie

`state_bytes()` is documented as what a board reads to know the cost without asking. It counted `ring + taps` and **covered only the filter**:

| | old function reported | measured on the board | measured on host | under-reported |
|---|---:|---:|---:|---:|
| L=16 | 450 B | **2 592 B** | 2 616 B | 5.8× |
| L=128 | 898 B | **3 040 B** | 3 064 B | 3.4× |

The board reads 24 B under the host because `size_of` follows the target's word size — `usize` is 4 bytes on Xtensa. **Quoting this figure has to say which target it was measured on**: the host number is not this board's cost. The probe prints the true figure for the target it is built for.

What it missed is `DelayEstimator`'s **2 096 B** — two `[i16; 512]` history buffers. And `assert!(state_bytes(MAX_TAPS) < 1024, "a kilobyte was budgeted")` in `nlms.rs` **was built on that number and passed.** The probe has been logging `[ECHO] --- L=16, 450 B of state ---` throughout.

Two tests let it survive, because they assert the wrong function's definition:

```rust
assert_eq!(bytes, ring + taps, "the estimate counts something twice");  // vacuous
assert!(bytes < 1024, ...);                                             // passes on the lie
```

`state_bytes<L>()` is now `size_of::<Canceller<L>>()`, with `L` a const generic rather than a runtime argument — **a runtime argument let callers pass a number the compiler could not check, which is exactly how it drifted.** The new `the_reported_size_covers_the_whole_canceller_not_just_the_filter` asserts the reported figure is at least large enough to hold the filter and the delay estimator together.

## The reference is wired in, and "take it from `render_slice`" was the wrong answer

`[measured]` The line in the list below — "the digital tap comes from `audio_out::render_slice`" — turned out not to work, and the reason is the same class of error this file keeps running into: the number was right and the place was wrong.

`render_slice` is the **write** side. The transmit ring is 24 descriptors × 960 B = 23 040 B = 5 760 frames, which at 48 kHz is **120 ms of audio still in flight**. A tap taken there is a reference up to 120 ms *ahead* of the echo it has to cancel, while the delay search covers `MAX_DELAY = 64` samples — 4 ms at 16 kHz. That is **thirty times outside the window**: the search answers "nothing found", and the filter converges on whichever noise the correlation grid favours, looking entirely normal throughout.

`[test]` The difference is pinned on the host, and it pins the **threshold rather than a slogan** (`aec/stage.rs::a_small_misalignment_is_absorbed_and_a_large_one_is_not`):

| Reference's offset from the microphone | Result |
|---|---|
| 0 frames | cancelled |
| **1 frame** | **still cancelled** |
| `MAX_DELAY + 1` = 65 frames | the echo survives essentially intact |

A one-frame mismatch is **recoverable**: the delay search's ±64-sample window plus the filter's own sliding window learn the path one tap over. So "one frame off ruins it" was never true, and the test written to assert it said the correct thing instead — the boundary is **outside the search window, not at one frame**.

So the wiring is shaped by that:

- The **write** side taps as before (one sample per **frame** in `render_slice` — both slots carry the same stream, so a tap per slot halves the rate), and the **read** side goes through a play cursor: `Es8311Tx::advance_play_cursor` derives how far the wire has travelled from the DMA's own free space and feeds the delta to `ReferenceLog::played`. That is the answer to "`Speaker` has no `played_frames()`" — **no method is added to `Speaker`**. What is added is a shared cell the speaker publishes into, one direction only.
- The cell **crosses two executors**: the feed is on a Priority2 interrupt, the folding is in the cooperative `input_task`, so it is a `CriticalSectionRawMutex` cell, taken **once per feed and once per poll, never once per sample** — a lock per sample at 48 kHz would cost more than the canceller it is feeding.
- **Where it sits**: the decimator in core (`drivers/audio/decimator.rs`, indexed by frame number rather than a free-running counter, since a phase mismatch between the two streams is a silent failure), the reference shadow log in core (`drivers/aec/reference.rs`), the tap in the bsp transmit side, the pairing in the bsp capture side, the wire between them in the board module's `Board::new`. **`Speaker` gained nothing** — a method there would pull a data-path coupling back across the interrupt boundary the interface exists to prevent.
- **Sized from geometry**: `REFERENCE_SAMPLES = ⌈(5 760 + 2 880 + 192) / 3⌉ = 2 944`, with a `const _: () = assert!` holding that it spans the whole runway. The capture side asserts separately that its ring really is three poll periods deep — **two modules each spelling out one number is free to drift**, and a stale copy makes the log quietly lap.

`[test]` The most important of `ReferenceLog`'s 14 tests is the **index unit** in `at_play_cursor`: the ring is addressed in *kept samples* while the counters count *frames*. The first version used `written % N` with `written` incrementing per frame, so only one slot in three was ever written — **every test green, every sample zero**. It now asserts **specific sample values** (`frames(n) = n × 97`) rather than indices, because a wrong index looks like a right one.

### Free space is an *occupancy*, not a position — off by a whole ring

`[measured]` Found after the first `advance_play_cursor` was written, and it is the most dangerous bug in this whole section: **the output still looked entirely normal.**

The DMA frees ring bytes and the CPU refills them at once, so "free" measures how much audio is **queued** (`ring − (pushed − played)`), not how far along the stream the wire is. Read as a position, it puts the wire a whole ring ahead at boot — the ring holds a full ring of prefilled silence that the DMA has not sent a byte of — so `played` starts at 5 760 and `at_play_cursor` returns either `None` or the oldest thing in the ring. **Every pair is garbage, and every log line looks like the system is working.**

The fix moves the arithmetic into core as `wire_position` and counts the **prefill** with everything else in `pushed_frames`:

```
wire_position = pushed_frames − (ring_frames − free_bytes / frame_bytes)
```

`[test]` Two tests hold it: at boot (a whole ring pushed, nothing freed) it must read **0**, not 5 760; and a free-space reading larger than anything pushed must never report the wire ahead of the pushed count, which is the guarantee the driver's own saturating delta then builds on. Writing those two tests I got it wrong once as well — mixing the free space from *before* a push with the pushed count from *after* it, which of course does not agree. **The two numbers have to come from the same instant**, which is now in the comment too.

### The two streams run in opposite directions, and zipping them pairs the whole run wrong

`[measured]` The second bug of the same class, and equally silent. The microphone is walked **forwards** (`scratch_mic[0]` is the oldest frame the drain saw) while the log can only be read **backwards** (`out[0]` is the frame on the wire *now*, which is what the newest microphone sample heard). Zipped as they stand, **the oldest microphone is paired with the newest reference** — every pair wrong by the length of the run — and the filter still converges on *something*, so the ERLE stays a plausible number rather than an obvious zero.

The fix reverses the reference back into time order in `pair` before it is fed (it is `cancel_pairing` that then hands it to the canceller). `[test]` `a_reference_run_read_newest_first_pairs_once_reversed` pins both halves: reversing a reversed run recovers the aligned pairing exactly, and not reversing it is clearly worse. `Reference::read_backwards`'s own doc now states the direction, so the next person to read it does not have to rediscover it.

Both are kept here because they are **the same kind** of thing: `RING_BYTES - free` looks like a position and is an occupancy, and `read_backwards` looks like something to zip and needs reversing first. **Both read entirely normally and produce nothing but garbage** — which is the only genuinely dangerous class of defect on this path, and the reason every pairing column is reported beside the ERLE rather than instead of it.

### The microphone ring's **latency** was not applied — a 60 ms skew

`[measured]` The worst of the three, and the only one where leaving it unfixed throws away the whole thing.

When a capture ring is drained, **how long ago was its newest frame captured?** Answer: however deep the ring is — three poll periods on this board, 60 ms. And `far[0]` is the frame the wire was carrying *at the moment the poll runs*. Pairing those two directly hands every sample a reference **up to 60 ms newer than it**:

```
worst skew   960 samples @16 kHz = 60 ms
search window 64 samples @16 kHz =  4 ms
```

**Fifteen times outside the window.** The delay search still answers "nothing found", the filter still converges on something, and the ERLE is still a plausible low number.

`[measured]` The fix passes the ring depth measured *before* the drain (`produced / BYTES_PER_FRAME` — the number `poll` was already computing) into the pairing as a **lag**, and reads the reference from `play_cursor − lag`. Converting to kept samples **rounds up** (`lag_in_samples`): rounding down reaches back a little too little and leaves the pair out by less than a sample; rounding up reaches back at least as far, overshoots by less than a sample, and is still inside the window.

The **sizing had already paid for this**. The log is sized from `5 760 + 2 880 + 192` frames into 2 944 samples, which covers the transmit runway, the capture latency and the search window together. `2 944 − 960 = 1 984` samples of headroom after reaching back. So this is **not extra memory — it is using a term the sizing formula already contained and nobody read**. That is the argument for deriving sizes from geometry rather than quoting them: the number that was silently unused was the one that would have flagged the bug.

`[test]` `a_capture_ring_s_lag_rounds_up_into_whole_samples` pins the rounding direction and checks across a range that the samples converted back out never fall short of the frames they came from.

**L is fixed at 16**, because at 240 MHz an L=128 tick is 21.6 ms in the worst case, the input tick already takes 17–25 ms, and `MAX_SINGLE_HOLD_MS` is 30 — **128 taps do not fit in this poll**. Going larger is either a slower tap count in the filter or a tick that stops folding everything else: a design change, not a constant.

### The product firmware leaves it **off**

`[measured]` A problem this work introduced in itself, and the easiest one to make while wiring a path like this: **the envelope still folds the raw microphone, so the canceller's output was being discarded** — while the canceller ran on every poll. That is precisely the quadrant `architecture.md` calls unexpressible: low fidelity at high cost.

**The cause is not the arithmetic, it is the rates**: the canceller works at 16 kHz while the envelope is specified on 48 kHz with two slots a frame. Folding the residual in means resampling back, and **resampling back to feed a level meter is a measurement decision nobody has made** — the 6th-order A-weighting cascade would be looking at a staircase. Until something reads it, running it is 3 248 µs a tick for an output nobody looks at.

So it gets a **compile-time switch, default false**, in the same shape as `HPF_CORNER_WALK` and `CAL_SPL_LOG` already in this file — not a Cargo feature, because this is a measurement being taken rather than a product variant (`features.md` criterion 1). The wiring stays in the tree either way, `echo-probe` turns it on explicitly and reads the numbers, and the state stays resident at 2.6 KB (against a board already carrying a 150 KB frame arena), which buys back the *time* without a feature or a second code path.

There is **no ERLE row on the panel** either. Same reason: the cell would read zero today, and adding a row to a layout `audio.md` records as "a column computed wrong looks like a considered design" for a number that cannot be validated yet is spending the most expensive thing available on the least verifiable one.

### 12.7 KB of new resident RAM

`[measured]` Derived from this board's geometry rather than estimated:

| Part | Bytes | Note |
|---|---:|---|
| Reference log | 5,888 | 2,944 `i16`, a `&'static` cell |
| Canceller at L=16 | 2,592 | measured on the board, above |
| Microphone scratch | 1,282 | a poll's worst drain over the decimation |
| Reference scratch | 3,202 | **960 of ring latency + 641 of run**, see above |
| **Total** | **12,964 (12.7 KB)** | |

For scale, the board already carries a 150 KB frame arena (the panel's DMA and the camera share it) plus a 64 KB heap. **The 12.7 KB is resident and off the heap** — `feed` may not allocate, which is exactly why the log is fixed-length and its size is written down rather than grown. PSRAM is still off (measure first, per the earlier decision) and these 12.7 KB do not need it.

`[measured]` Both images link with this in place: the product `vanling` (`.bss` 303,688 B / `.data` 9,316 B) and the `echo-probe` (`.bss` 259,872 B / `.data` 8,644 B, smaller because it paints no panel and so carries no frame arena).

## Open, and the edges of this

- **CPU passes.** Block adaptation (`UPDATE_STRIDE = 4`) puts **L=16 at 9,680 µs against a ≤10 ms bar**, L=32 at 13.5 ms. The cost is slower convergence (whole-file 20.9 → 16.2 dB) with steady-state ERLE bit-identical. **The road is open again**, but only as far as L=16/L=32. `aec/mod.rs` with its 44 unit tests and `tests/aec_erle.rs` all stay in the tree, as the evidence that the road does not go and as proof the algorithm itself is correct; `aec-probe` stays in `check-s3`'s bin list, so a build failure means the cost moved again.
- **The clock has moved to 240 MHz, product firmware included** (`app/src/main.rs` calls `chip_init_at(_240MHz)`). An earlier version of this list said it was probe-only — that is out of date, and so is the 17–25 ms input-tick figure it was read against, which was measured at 80 MHz and needs the 3× headroom. What 240 MHz costs is power, the PLL, and the long-term stability of USB Serial/JTAG (measured once, which is not a long-term result).
- **The update's 119 cycles/MAC is still unexplained.** Decimation routes around it; the cause is untouched. Fetch is ruled out (IRAM is 6% slower) and a second core is ruled out (both cores report identical digits), which leaves the memory system itself. `[reported]` Flash controller, absent data cache, or un-pipelined multiplication is untested.
- **The onset burst is a known, unfixed defect.** The freeze stops the damage growing but not its first half second, because the coherence meter needs ~512 samples to read "incoherent" and the filter has already run through them. A shorter meter dedicated to the freeze decision is the fix, and it is a design change that was not made here.
- **`[reported]` `LEAKAGE_FREEZE` and `MAX_COHERENCE` are not calibrated on this board**; every threshold still comes from the microphone calibration in `audio.md`.
- **RAM measures 3,040 B on the board** (`state_bytes::<128>()`; 2,592 B at L=16) against a 4 KB budget. 3,064 B on host. That is the main advantage over ESP-SR's 30.9 KB internal + 90 KB PSRAM.
- **`state_bytes()` under-reported by 3.4–5.8×** until it was changed to measure the whole canceller. See *The memory accounting was a lie* below.
- **The reference is wired up**, through a play cursor rather than the write pointer; see *The reference is wired in* above. **It has not yet run against a real room** — `echo-probe` is the build for that, below.
- **PSRAM is still off**, per the decision to measure first. 3 KB does not need it.
- **No nonlinear/residual suppression.** The ERLE measured here is the value before it.
- **The delay locks; the amount cancelled still has no number.** See *Once the delay is locked: it locks, ERLE still has no number* above: across two measurements delay moved exactly once (26; elsewhere 53 → 42), while the firmware's ERLE meter has **never returned a non-zero reading on hardware**. The 12.3 dB computed midway came from an offline dump that has since been deleted and cannot be reproduced, so it does not count. **L=16 is still from a synthetic host room and an offline optimum** — nothing measured on this board has changed the tap count.
- **`echo-probe` reports the case where the ERLE looks fine and the pairing is broken**, so the columns cannot be read apart: a zero pair count pins ERLE at zero (a meter that has not moved has measured nothing), while a lapped log can leave ERLE looking normal. `EchoReport` bundles them so no caller can read them separately. The probe also confirms **the canceller is actually running** on every line — a run of zeros is the same output from a broken pairing and from a canceller that never ran.
- **The product firmware leaves the canceller off** (a compile-time switch, above), so every one of those figures reads zero in the product image. **Only `echo-probe` produces real numbers.**
- **The decimation phase is 0 on both sides today** (the transmit unit is armed before the capture, so frame 0 pairs to frame 0). A board that armed the microphone first has to state its offset to *both* the reference log's phase and the capture's decimator, and the delay search covers the remainder — **this is not automatic**, it is a constant that fails silently by one tap.

## MIC3 as a hardware reference: the registers are right and the link is still silent

The Lichuang Shizhianpai board wires the ES8311's output to MIC3, and the vendor's own example asks for `tdm_enable = true`. This round took that route to the end and three of the results **overturn what was believed before**.

### The first repair was a misreading of the datasheet, sitting in the code

`es7210.rs` carried the comment "bit1 of `REG_SDP_INTERFACE2` selects TDM", and the name `SDP2_NO_TDM` fixed the mistake in place. What the datasheet says:

- **Bit 1 of register 0x12 is `TDMIN_PDN_OFF` — the TDMIN pin's internal pull-down, not a serial format select.** Writing `0x02` turns the pull-down off, the part stops driving the data line, and the capture reads zeros.
- The format is **`SP_PROTOCAL`, bits 1:0 of register 0x11**, where `11` is DSP mode.
- The channel count is **`LRCK_RATE_MODE`, bits 7:4 of register 0x08**, where `2` means four channels — **and it only means anything once `SP_PROTOCAL` is `11`**.

The old code wrote `0x00`, which kept the pull-down: **right result, wrong reason.** That accounts for the earlier readings of zero and zero — the hardware was never the broken part.

### All three registers land correctly and the part is still silent

`init` now reads the serial registers back and prints them, because **a wrong serial format has no other symptom**: the part keeps driving, the receiver sees silence, and each half of that is individually plausible.

`[measured]` With TDM on, the part holds `mode 0x20` (`LRCK_RATE_MODE = 2`, four channels), `serial 0x63` (`SP_PROTOCAL = 11`) and `interface 0x00` (pull-down kept) — **exactly what the datasheet specifies.** The capture still reads zero on **all four slots**, over the same clock table the two-channel path uses.

The read-back also confirms the field decoding itself: with TDM off the part holds `mode 0x10` (`LRCK_RATE_MODE = 1`, which the datasheet lists under "0/1 – 2 channels").

**The suspect is therefore on the receiver's half of the link** — whether esp-hal's slot count and MCLK divider stop the ES7210's sample-rate detector from locking — rather than in the registers. **The route is closed for now and the MIC3 reference stays off**, with the reasoning on `USE_MIC3_REFERENCE`.

### What the diagnostic is worth: a peak cannot tell a copy from a channel

Under the two-slot configuration all four slots read almost bit-for-bit equal at every frame width tried (2557/2558, 946/949) — which is equally what two different signals look like and what one microphone's sample duplicated across left and right looks like. So the drain now also reports the **peak of the difference between the two slots**: if one is a copy, that figure is zero. Peaks cannot separate the two cases; the difference can.

## The delay is measured for the first time, and measuring it needs a switch that used to be off by default

`A_B_RUN` defaulted to off, on the grounds that only a muted run measures the microphone's floor with no echo present. That is true, and it hid something: **with the amplifier silent for the whole run the canceller is handed a silent reference, there is nothing to correlate against, and the delay search cannot return an estimate however good the room is.** `[measured]` Every run of a muted-speaker probe sat at `delay None` — which reads exactly like a board with no echo path at all.

With the alternation on:

```
[amplifier on — reference peaked 22876 LSB, microphone peaked 936 LSB]
321 paired this poll | ERLE 0 dB, delay Some(17), tap 13 ..., adapting
```

**The delay settles at 17 samples — 1.06 ms at 16 kHz**, which is a desk-to-speaker distance plus a codec round trip, and the correlation clears the gate. **The path exists and the pairing works**; both are established now. The earlier "no echo path" conclusion came from the probe's configuration, not from the hardware.

So the alternation is now the default: the muted floor is still reported every window, but the delay is no longer blocked by a silent reference.

## A wide scan, to separate "no path" from "path beyond the filter"

`SEARCH` is only `MAX_DELAY = 64` samples, which is **4 ms** at 16 kHz. Nothing could tell "this room has no echo" from "the echo is further away than that" — both are no peak inside the search.

`DelayEstimator::scan_wide(reach)` closes it, scanning out to `HISTORY - WINDOW` (256 samples, 16 ms) **on a grid of every single sample**. That is not a preference but this module's own documented finding: against a broadband reference the correlation peak is one sample wide, so a coarse grid can only return the largest of a few noise samples. The new tests pin that, and also pin that noise is not read as a path and that a path past the scan's reach is not found by it.

## The new blocker: the capture stream has holes

ERLE is still 0 and the **loudest tap keeps moving** (13/14/1/13/14/9/6/13/12/15) — coefficients dominated by noise, not a filter converging. The same log says why:

```
ring 1920 B arrived, 2400 B drained
capture rate: 12611 paired = 12400 samples/s of the 16000 the canceller runs
```

**The real rate is 77% of target.** An adaptive filter cannot converge on gapped data — which sits alongside a correct delay without contradiction: the delay was measured across a continuous stretch, but convergence has to happen over the whole stream.

**The cause is not the link. It is one constant divided by the wrong frame width.** `DECIMATED_PER_POLL` and two neighbours are derived from the *transmit* `BYTES_PER_FRAME` (4 bytes) while the capture delivers `CAPTURE_BYTES_PER_FRAME` (2 bytes):

| constant | was | is | consequence |
|---|---|---|---|
| `DECIMATED_PER_POLL` | 321 | **641** | a poll's budget produces 400 decimated samples into 321 |
| `DECIMATED_RING_LATENCY` | 480 | **960** | the microphone ring's latency counted at half |
| `lag_frames` | `produced / 4` | `produced / 2` | the reference reached back half as far as it should |

`if *frames_this_pass < DECIMATED_PER_POLL` was **silently dropping the last 79 samples of every poll — 4.9 ms contiguous**, not scattered: one hole every 25 ms. 321/400 = 80%, and the measured rate was `12400/16000` = 77.5%.

**The doc on `BYTES_PER_POLL` already says "this figure is the root every capture-side size is derived from"** — and all three derivations used the wrong width anyway.

`[measured]` After the fix: `paired this poll` 321 → **400/413**, the rate → **15 130–15 515 / 16 000**, and the delay 17 (jumping) → **41 (steady)**. All three match the arithmetic predicted beforehand.

The silent drop went with it, replaced by a compile-time assertion: a wrong size must fail the build rather than quietly lose data.

## The algorithm is right — that rules it out

`core/tests/aec_erle.rs` runs the canceller against the **offline least-squares optimum** on every path, on the host. `[measured]`:

| path | taps | offline | canceller | gap |
|---|---|---|---|---|
| short, fast decay | 16 | 62.9 dB | 61.0 dB | 1.9 |
| medium, moderate decay | 48 | 58.8 dB | 57.2 dB | 1.6 |
| long tail, slow decay | 192 | 38.9 dB | 38.8 dB | 0.0 |
| tail longer than the filter | 256 | 14.4 dB | 13.2 dB | 1.2 |

All four land within 0–1.9 dB of the optimum, and none beats it (`the_canceller_does_not_beat_the_offline_optimum` pins that — exceeding the optimum can only be overfitting or an arithmetic error). The delay is found correctly on every path.

**So ERLE = 0 on the hardware is not an algorithm problem.** It is still 0 after the sample loss is fixed, and the variable still missing is **how long this room's path is** — L=16 was chosen against a synthetic room, and nobody has measured how long the tail is in the room this board is actually in.

## Measuring the impulse response was itself the wrong road, twice

The missing variable above was the room's impulse response, so it went to being measured — by dumping the microphone out and deconvolving it on the host. It broke in the same place twice, and neither time in the code.

### First: clipping, and the conclusion drawn was that the sweep's clock was unreliable

`[measured]` the sweep first ran at half rail (`SWEEP_PEAK_LSB` was 11 500) and the microphone recorded **32 767** — the ADC was clipping. Four analyses of the same recording gave four answers that contradicted each other.

The reason given at the time was that a sweep decorrelates the moment playback and capture slip by one sample — 45° of phase at 6 kHz — so it needs two independent crystals aligned, and every alignment gave a different answer. So the **excitation was replaced with a 1 kHz tone**.

**That reason does not hold on this board, and the code that refutes it is in the same repository:**

- `components/audio_clock.rs:3`: "Both codecs are slaves of one host MCLK", with a test pinning it;
- `es8311.rs:55` / `es7210.rs:50,56`: both parts are slaves, MCLK comes from a pin;
- `lckfb_szpi_esp32s3/mod.rs:481-493`: **one `i2s_master::I2s::new(I2S0, DMA_CH1, shared_tdm_config()).with_mclk(GPIO38)`, with `tx` and `rx` built from the same handle.**

One `I2S0` peripheral and one MCLK pin means the sample handed to the DAC and the sample the ADC returns are the same sample index in the same clock. The literature's failure mode — sweep and MLS deconvolution breaking under unsynchronised clocks — is real, but it requires two clocks that are *not* synchronised. This board has one.

And a tone is an **excitation that cannot measure a path**: its energy sits at one frequency, so it gives that frequency's transfer function and nothing else — no path length, no tail, and no answer to "how many taps does this room need".

Lowering `SWEEP_PEAK_LSB` from 11 500 to **2 500** was the actual fix. Clipping's harmonics **cannot be undone by deconvolution**, because the harmonics are in the recording. The sweep went back.

### Second: this console cannot carry 16 kHz

The dump had a larger premise underneath it that nobody ever checked: **whether this console can carry paired samples out at 16 kHz continuously.**

The capture script was `B115200`, no flow control, and slept 50 ms whenever a read came back empty. On the board, one line per sixteen samples, sixty-four hex characters per half. `log::info!` waits on the USB FIFO, so a host that reads slowly blocks the firmware — on the very executor that still has to poll the capture. The capture ring is 60 ms deep, and one overrunning drain loses it.

The first version of this route **did not even report the loss**: the probe printed a pair count and nothing else, so a hole in the recording was invisible from the recording itself — and a signal with a hole in it deconvolves into *the room's shape smeared by the length of the hole*, which reads as an ordinary, slightly wrong answer.

`[claimed]` How much is actually lost was never measured. The direction is enough to reject the route: its only deliverable is a number that **has to prove it has no holes before it can be believed**, and the proof would have to come over the same console that is already struggling.

### So the measurement moved back onto the board: the path profile

`DelayEstimator::scan_wide` **already** computes a normalised correlation at every lag, then keeps the maximum and throws the rest away. Accumulating them **by which lag they landed on** costs almost nothing — one multiply-add per lag, in an inner loop that is already dividing.

`PROFILE_BANDS = 8`, `PROFILE_BAND = 32` — 2 ms per band at 16 kHz, covering the scan's whole 256-sample reach. `DelayEstimator::unreachable_by(taps)` reports **the share of the correlated path a filter of `taps` cannot reach**, in hundredths — the input to the tap budget. `echo-probe` prints one line once the wide scan has located a path:

```
[PATH] out of reach of a filter of — 16 taps N%, 64 taps N%, 128 taps N%, 256 taps N%.
```

**No dump, so no hole; no deconvolution, so nothing reconstructed; beside the correlation that produced it, so it has 79 unit tests.**

### What this measure can and cannot say

Written down because it is coarser than an impulse response, and is one step away from being read as one:

- **It is not an impulse response.** It is the scan's own correlations accumulated by lag, smeared by `WINDOW` (the 256-sample correlation window) and quantised to a band. Its job is *how much of the path lies past what a filter of a given length can hold*, and a band is the right resolution for that.
- **It must not be accumulated behind a per-lag gate**, and that is measured. A gate at 300‰ admits only the main lobe: the correlation at lag *L* is the path's own autocorrelation at *L*, so **holding a lag to 300‰ holds the path's energy at *L* to nearly the direct sound's** — which no room is. Every room came back with a tail of nothing. So it is accumulated whole, and the noise floor is removed when the numbers are read: the quietest band is taken as the floor and taken off every band.
- **The bottom resolves only to a few percent.** A strong band smears some of itself into the bands after it. So "near zero" means "below what this can resolve", not "none". **What it resolves is the difference between rooms**, and that difference is what a budget turns on.
- **It counts the correlated share.** Energy that does not correlate with the reference is not something a linear filter can hold either, so leaving it out costs the measurement nothing; anything a filter would have reached is inside the number.
- **Resolution is one band** (2 ms). Two tap counts less than a band apart cannot be told apart by it.

`[test]` What is pinned is the guarantee it can actually carry: more taps never reach less; a damped room leaves far less over than a live one (`quiet[2] = 3%` against `loud[2] = 12%`); no path and no scan both return −1 rather than 0 — because 0 would be read as "everything reached"; and the loudest band is the one the direct reflection falls in.

**Those four numbers have not been produced on the board yet.** `PATH_RUN` is a compile-time switch, off by default; turned on it plays the sweep, leaves the canceller off, and prints that line once the wide scan has located a path. **It answers "how many taps does this room need", not "what the impulse response looks like"** — the latter still does not exist, and the former is already enough to decide L.

## Once the delay is locked: it locks, ERLE still has no number

The **delay** half of the "pairing holds, cancellation is zero" section above ends here. Locking the delay (first search takes the lock; after that it may only be pulled by `DRIFT_SAMPLES`, and only yields after `MISSES_BEFORE_MOVING` searches in a row disagree) changes the same measurement to:

| Reading | Before | After |
|---|---|---|
| delay | None → 16 → 50 → 6, jumping across the 64-sample window | **26, not once**; measured again elsewhere on the desk, 53 → 42, still one move |

`[measured]` Reproduce: `moon run iot:probe-echo-s3` → `IOT_PORT=/dev/cu.usbmodem1422301 moon run iot:flash-probe-echo-s3` → attach the serial port after the reset, read for 30 s or more, and count the distinct values delay takes. Across both runs, 28 polls, two distinct values, and the single transition came exactly after four consecutive searches agreed on the new one.

**So "the delay search jumps around" is closed. The ERLE half is not.**

### A 12.3 dB figure that does not count

One intermediate firmware dumped three channels of 512 samples each (reference, microphone, residual) once the delay was locked. Over that data `mic_rms = 20.7` and `res_rms = 5.0`, i.e. 12.3 dB.

**But the probe's own ERLE column read `--` on that run, not 12.** The 12.3 came from computing it on the PC side from the CSV, not from the firmware. And:

- the dump code has since been removed and that log is gone, so **the number cannot be reproduced**;
- the firmware's ERLE meter has **never produced a non-zero reading on hardware** — the recheck run reported `ERLE 0 dB` on all 36 polls, including the phases where the amplifier was on.

The residual-waveform approach is still the right one (ERLE is a ratio: it says how much went, not whether what went was echo), but **it needs a reproducible measurement, and that run was not one.** Making it one means the dump becomes a standing output of the probe rather than throwaway debug code.

### What this does and does not establish

- **It does**: on this board the delay locks. Two measurements, two placements, one move each.
- **It still does not**: how much the linear AEC cancels on the board. The firmware's meter has never produced a number, and the single 12.3 dB came from an offline calculation that can no longer be repeated.
- **Whether 12 dB is enough** is a question about how it sounds, and nothing here measured that. This device is quiet, and in a quiet room that difference is the most audible one there is.
- **Still absent**: nonlinear suppression, the other half (`aec_nlp_process`). A class-D amplifier near clipping produces harmonics no linear model can predict, so they remain in the residual even once the linear part has converged.
- **The next thing to measure is the ERLE meter itself**, not the filter: it has never returned a number on hardware, so find out why it does not accumulate.
