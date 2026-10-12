+++
title = "Echo Cancellation Implementation Record"
weight = 70
sort_by = "weight"
[extra]
source_file_hash = "df5c8f52436d3e1d8333cca182e1b7a07ad25807"
translated_at = "2026-10-12T03:47:23Z"
+++

<!-- doc-audience: ai -->

# Echo Cancellation Implementation Record

Judgements and rejected approaches from `apps/iot/crates/core/src/drivers/aec/`. **AI-produced, not reviewed by a human** — see the [parent note](@/records/_index.en.md).

Each judgement is marked with its basis: **`[test]`** a unit test backs it, **`[measured]`** a measurement with a way to reproduce, **`[reported]`** not reproduced, kept only for the record.

Linear AEC does one thing: model the acoustic path from the samples sent to the speaker, and subtract it from the microphone. Nonlinear suppression is not part of it — see the module documentation in `aec/mod.rs`.

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

Two caveats stay attached. Clearing 15 dB is **necessary, not sufficient** (PCC 0.31). And the paths are generated, so they test the contract rather than a room — this board's loop gain, delay and thresholds still need this board.

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

So the step in `learning.rs` is a **constant**: full speed, or zero while frozen. The residual is used only for the one decision it can answer unambiguously.

"Full speed" is a choice, not a default: NLMS is stable anywhere in `(0, 2)`, and `1 << 29` (0.5 in Q30) is the conventional pick with room left below it.

---

## Freezing cannot look at the residual level alone: a converging filter and a talking person both leave a large error

`[test]` This is the same failure above, arriving from the other side. An absolute threshold on the residual freezes the filter during its own startup.

What separates the two is **coherence**:

- While the filter is still converging, the error is mostly **echo it has not yet accounted for**, and is therefore highly correlated with the reference (coherence ≈ 1);
- While someone is talking, the error is speech, and is not (coherence ≈ 0).

That is Gänsler's coherence-based double-talk detector (JASIP 2015 calls out its two drawbacks: a threshold that depends on background noise and on the talker's level). It lives beside the filter rather than as a second detector beside that, because the filter already computes everything it needs: three running means, one extra multiply per sample.

Both thresholds in `learning.rs` are marked `[reported]`: `LEAKAGE_FREEZE = 1<<24` (~4096 LSB RMS, taken from the 30 cm speech peaking to 80 dB SPL calibration in the [audio record](@/records/iot/audio.en.md)) and `MAX_COHERENCE = 0.6`. Both want re-measuring on a bench with a person in the room. The freeze is confirmed working on a synthetic talker — 98% coverage — but see the onset burst above.

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

### The CPU is at 80 MHz

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

`CpuClock`'s default arm is `_80MHz`, so the whole board has been running at a third of its rated clock. esp-hal notes that its 160/240 presets move the PLL off the 480 MHz the chip boots on and that this "breaks USB Serial/JTAG" — which is this repository's console. **`[reported]`, never tested.**

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

## Open, and the edges of this

- **CPU passes.** Block adaptation (`UPDATE_STRIDE = 4`) puts **L=16 at 9,680 µs against a ≤10 ms bar**, L=32 at 13.5 ms. The cost is slower convergence (whole-file 20.9 → 16.2 dB) with steady-state ERLE bit-identical. **The road is open again**, but only as far as L=16/L=32. `aec/mod.rs` with its 44 unit tests and `tests/aec_erle.rs` all stay in the tree, as the evidence that the road does not go and as proof the algorithm itself is correct; `aec-probe` stays in `check-s3`'s bin list, so a build failure means the cost moved again.
- **The clock is an open board-level question worth 3× more than the AEC.** The whole board runs at 80 MHz; `CpuClock` also offers 160 and 240. esp-hal notes that presets off the 480 MHz PLL affect USB Serial/JTAG, which is this repository's console. `[reported]` Whether 160 MHz is safe is untested.
- **The update's 119 cycles/MAC is still unexplained.** Decimation routes around it; the cause is untouched. Fetch is ruled out (IRAM is 6% slower) and a second core is ruled out (both cores report identical digits), which leaves the memory system itself. `[reported]` Flash controller, absent data cache, or un-pipelined multiplication is untested.
- **The onset burst is a known, unfixed defect.** The freeze stops the damage growing but not its first half second, because the coherence meter needs ~512 samples to read "incoherent" and the filter has already run through them. A shorter meter dedicated to the freeze decision is the fix, and it is a design change that was not made here.
- **`[reported]` `LEAKAGE_FREEZE` and `MAX_COHERENCE` are not calibrated on this board**; every threshold still comes from the microphone calibration in `audio.md`.
- **RAM measures 3,040 B on the board** (`state_bytes::<128>()`; 2,592 B at L=16) against a 4 KB budget. 3,064 B on host. That is the main advantage over ESP-SR's 30.9 KB internal + 90 KB PSRAM.
- **`state_bytes()` under-reported by 3.4–5.8×** until it was changed to measure the whole canceller. See *The memory accounting was a lie* below.
- **The reference is not wired up.** The digital tap takes every TX sample written by `audio_out::render_slice`; `Speaker` has no `played_frames()`, and one needs adding.
- **PSRAM is still off**, per the decision to measure first. 3 KB does not need it.
- **No nonlinear/residual suppression.** The ERLE measured here is the value before it.
- **A real echo path remains the one unanswered part.** No public recording has one, and G.168 says the real acoustic vectors do not exist yet either. The only way to close it is to record this board: play a known signal, capture the microphone. That is a capture session, and the capture session is exactly what the CPU measurement has already ruled out.
