+++
title = "Audio Metering Semantics"
weight = 260
sort_by = "weight"
[extra]
source_file_hash = "adf819704dbbfe9bb84d5b845b9a7b9a7cb2d283"
translated_at = "2026-10-01T07:37:04Z"
+++

# Audio Metering Semantics

The ES7210 is only one source. What the page exposes is **level** — how many dBFS right now, and how much of it persisted over the last two seconds — not a count of LSBs and certainly not raw samples. This page covers the contract, the meaning of every readout, and why the numbers are drawn the way they are.

> This page was written by AI and has not been reviewed by a human. It describes what the system **should be**; how the implementation became so, and what was rejected along the way, is in the [development records](@/records/_index.en.md).

## The contract

The capture driver **never stops**. It is always folding the samples the I2S delivers into the envelope; the `ST` phase only decides which envelope the panel draws and whether the sweep follows the live one. Reading `ST` as "the recorder is running / stopped" gets two things wrong at once: recording makes the driver do no additional work, and stopping it makes the driver do no less.

A capture poll is pure data plane: it refreshes the envelope and raises **zero** semantics. Starting and stopping are decided only by a business-side click (`ToggleRecord` in `intent.rs`), and only on the Audio page — a click on any other page still cycles the color.

There are three phases: `IDLE` (nothing captured, the panel draws only `TAP TO REC` and no sweep), `REC` (follows the live envelope), and `STOP` (draws the latched copy). A click on either `IDLE` or `STOP` goes straight back to `REC`; there is no "resume" semantic, because what gets latched is **the window at the moment of stopping**, not a recording.

When the board finds no ES7210, `audio_enabled` is false and the `AUDIO` page never enters the page cycle (`DisplayPage::next_page` skips pages no capability backs); every other page is unaffected. Getting to the page is a triple-click of the button or a three-finger tap (`TogglePage`).

The side that makes sound is in [Playback Semantics](@/development/iot/playback.en.md). The two pages share one 12.288 MHz MCLK but their contracts are opposite: capture **never stops**, playback is over when the sound is — which is why playback has a phase and this page does not.

## Parameter lookup

Work the bench against these two tables. Every metering parameter lives in core (`drivers/audio.rs`, `overlay.rs`) and is independent of the hardware; the board only decides how many bytes to feed it, and how many decibels this microphone's full scale is worth.

### `AUDIO` readout rows

| Tag | Field | Unit | Meaning | Parameter |
| --- | --- | --- | --- | --- |
| `ST` | `audio.phase` | — | `IDLE` not capturing / `REC` follows the live envelope / `STOP` draws the latched envelope | — |
| `MS` | `audio.elapsed_ms` | ms | Wall-clock time elapsed, **not** a sample count | `CAPTURE_MS` |
| `PK` | `envelope.loudest()` | dBFS / dB SPL | Loudest peak in the window | `SCOPE_FLOOR_DECIBELS` |
| `RMS` | `envelope.loudest_rms()` | dBFS / dB SPL | Largest per-column RMS in the window | `SCOPE_FLOOR_DECIBELS` |
| `COL` | `envelope.committed()` | columns | Columns committed so far, capped at 200 | `ENVELOPE_COLUMNS` |
| `RST` | `audio.restarts` | times | Re-arms after a stalled DMA | `CaptureWatchdog` |

`PK` and `RMS` each carry two readings of the same instant: dBFS first, followed by `DBFS`, then dB SPL, followed by `SPL`. These two rows are the only readouts on this page too wide for one text block, so they run the full width and the sweep starts below them; `ST` / `MS` / `COL` / `RST` and the `CLIP` latch live in the narrow block to the right of the sweep, with the **live dB(A)** on the row heading that block, described below.

All four marks sit in fixed columns — `LEVEL_UNIT_X` / `LEVEL_SPL_X` / `LEVEL_SPL_UNIT_X` in `overlay.rs` — and do not move with the width of a reading. Right-aligning them would walk each unit sideways as its number gained a digit, leaving two rows disagreeing about where the unit is and the eye hunting between two positions.

The gap from `LEVEL_UNIT_X` to `LEVEL_SPL_X` is `LEVEL_UNIT_GLYPHS + 1` character cells, not a reading's worth: `DBFS` is four glyphs wide, so spacing on reading width runs it into the SPL value beside it. Every adjacent pair therefore clears a whole blank character cell (13 px). `every_level_column_keeps_a_whole_blank_cell_between_it_and_the_next` in `overlay.rs` pins that distance, computed from each column's *actual* end rather than a flat glyph count — a fixed count is exactly what let `DBFS` sit one pixel from the value next to it while every numeric test still passed.

`COL` climbing while `PK` does not is what a dead capture path looks like: in a quiet room `PK` climbs with it, and a reading parked at `-90` says no sample arrived for this whole window. Read `RST` together with `COL` — both moving while `PK` sits at `-90` means the driver is re-arming over and over; neither moving means the page is not running.

`MS` counts wall-clock time rather than samples because every other readout on the panel is stamped from the same clock, and a late poll must not make the capture look shorter than it was.

### Converting dBFS to dB SPL

The two differ by a constant. Core's `spl(dbfs, offset)` only adds, and the board supplies the constant:

```
dB SPL = dBFS + SPL_OFFSET_DECIBELS
```

`SPL_OFFSET_DECIBELS` lives in `bsp-esp/src/components/es7210.rs` and is currently **102**. The chain:

- ZTS6216 sensitivity of −38 dBV/Pa (0 dBV referenced to 1 Vrms) is 12.59 mV/Pa
- `GAIN_30DB` multiplies that by 31.6 to 397.8 mV/Pa
- The ES7210 datasheet gives the analog input full scale as **AVDD/3.3 Vrms**, which on `ANALOG_POWER_RUN`'s 3.3 V analog rail is **1.0 Vrms** — not the 2 Vrms that "a 3.3 V rail leaves half as headroom" suggests, a whole factor of two (6.02 dB) apart
- 1.0 Vrms of full scale is 2.51 Pa, which against the 20 µPa reference pressure is **101.98 dB**

So 0 dBFS is 102 dB SPL and the −90 window floor is 12 — the whole 16-bit range, chosen so a room a phone reads at ~25 dB SPL still has a dozen decibels of scale under it instead of sitting on the meter's own bottom.

One more constraint from the same datasheet: with `VDDA` below 2 V, a microphone application must set the PGA gain at or above 21 dB. This board runs VDDA at 3.3 V, so that does not apply and `GAIN_30DB` stays at 30 dB.

**Calibration record (2026-09-28): the value still stands at 102.** Anchored against **normal speech at 30 cm** as a voice reference: set the compile-time `CAL_SPL_LOG` in `virtual_components/audio.rs` to `true` and rebuild, speak continuously at normal conversational volume 30 cm from the capsule for about 30 s, and the serial logs `[AUDIO] CAL floor … dB SPL, rms … dB SPL` every 5 s. The speech block read a 66 dB SPL floor with peaks up to 80 — exactly where an unweighted meter puts conversational speech — so the datasheet derivation is within ±3 dB. `CAL_SPL_LOG` stays `false` in ordinary builds; re-anchor the same way the day the capsule or `GAIN_30DB` changes. Phone SPL apps are not a trustworthy reference here (one read 20 dB in the same sound field), so they are not used as one.

The conversion is integer addition rather than floating point: no `float` reaches the metering side of core (playback's synthesis is the one exception, for the opposite reason — see [Playback Semantics](@/development/iot/playback.en.md)), and once the offset is fixed the fraction it adds is finer than the panel can resolve.

### The live dB(A) readout

The top-right of the title row used to carry a static `+102dB`; it now shows a **live dB(A)** as `NN dBA`. It answers "how loud is this room right now", while `PK`/`RMS` remain the bench's unweighted meter — the two coexist and neither replaces the other.

- **Where the number comes from**: the I2S samples pass a real A-weighting filter (IEC 61672), slow-tracked by a single exponential average (τ ≈ 1.4 s), and `level_lsb()` is then folded into dB(A) through `dbfs()` and `spl(·, SPL_OFFSET_DECIBELS)`. **The offset is still 102**: A-weighting is exactly 0 dB at 1 kHz, so the full-scale conversion is untouched.
- **Why A**: the curve a person uses to judge "how loud" is A-weighting. A quiet room reads a notch below the unweighted column because A-weighting presses below 100 Hz by 19 dB or more, and the unweighted column's floor here is low-frequency codec noise the filter simply removes — the dBA cell falls toward the 12 dB SPL floor while `RMS` sits ~60 dB higher. The capture driver never stops, so this cell always tracks "now". **The sweep is weighted the same way**: each column runs through the same filter before it is drawn, so the solid band rises with what is audible and collapses when the room quiets, instead of staying a thick slab on the codec's unweighted floor.
- **Implementation** (core, fixed point the whole way): `drivers/audio/weighting.rs`. Three Q30 fixed-point bilinear biquads (prewarped, normalized at 1 kHz) cascade into the 6th-order A-weighting, ±0.7 dB in-band, with the output clamped back to i16. The signal rides the cascade at eight extra fractional bits (`STATE_FRACTION`): the highest-Q poles sit near the unit circle, and a state step wide enough to drop a signal's low bits would recycle that quantization error into a self-sustaining limit cycle — a quiet room read ~65 dBA no matter what it heard (reproduced on the host, `input RMS 19 LSB → filter out 489 LSB`), and scaling the *signal* up shrinks that amplifier by 2⁸ with no coefficient change. `DbaMeter` feeds the square of every sample into an exponential average (`METER_RELEASE_SHIFT = 16`, τ ≈ 1.37 s) and `level_lsb()` is its `isqrt`. The tests pin each frequency's gain to an exact integer copy of the module through the 384-point sine table rather than guessing against a floating-point expectation, and one asserts a quiet input reads quiet instead of feeding the poles.

### Scale and graphics

| Element | Meaning | Parameter |
| --- | --- | --- |
| Left `0` | 0 dBFS, the top of a bar | `SCOPE_FLOOR_DECIBELS` |
| Left `-24` / `-48` | Ruler ticks; a dotted grid line every 12 dB | `GRID_DECIBELS` |
| Left `-90` | The noise floor, which is the centre line (solid) | `SCOPE_FLOOR_DECIBELS` |
| Centre line | Silence. Bars grow upward and downward from it, **mirrored** | `WAVE_CY` |
| One column per 10 ms | Time width of a column | `COLUMN_MS` |
| 200 columns | The whole window, 2.0 s | `ENVELOPE_COLUMNS` |
| Solid core in a column | That column's **A-weighted** RMS, i.e. perceived loudness | `weighted_rms_columns()` |
| Dithered shoulder in a column | The A-weighted peak above the A-weighted RMS | `weighted_released_peaks()` |
| Block at the top | The column where clipping happened (latched on the raw peak) | `FULL_SCALE_LSB` |
| Short dash on the tallest bar | Peak hold: the window's tallest drawn bar and where it sits | `weighted_released_peaks()` |
| `2.0S` below the sweep | Window length, computed from the constants | `COLUMN_MS` × `ENVELOPE_COLUMNS` |
| `FPS` bottom-right | The panel's repaint rate; the only strip that belongs to no page | `FPS_WINDOW_MS` |
| `TAP TO REC` | The only mark while `IDLE`; no sweep is drawn then | `AudioPhase::Idle` |

## Metering semantics

The tables say what each cell is; they cannot say why it is drawn that way. The four points below are what the next person to touch the drawing is most likely to get wrong.

### The mirrored scale grows the wrong way

A bar grows upward and downward from the centre line at once, so 0 dBFS sits at both edges of the band and the floor sits at the centre. A bar's height measures "how far from silence", not "how far from the top". The ruler therefore labels only the upper half, and the lower half mirrors it; the edges need no rule, because they *are* full scale.

Labelling the scale as "top is 0, decreasing downward" would put every tick an octave away from where the bar actually ends, and it would be hard to spot: every row would still have something on it.

### One-ink overprinting has no second grey

The overlay is an inverted `stamp_pixel`; there is no second level of grey. So the RMS **cannot** be drawn as a thin stripe inside the peak: the stripe's pixels are the wide bar's pixels, and identical pixels are the same as none drawn at all.

A solid core with a dithered shoulder is the only workable drawing and also the cheaper one: the dither lights only half the rows of the shoulder, and drawing a full solid bar takes more `stamp_pixel` calls still. `CLIP` is the same case — it is not a level reading but one sentence about the whole window, so it is text and not a mark alone.

### The release rate is derived from the window, not borrowed from a table

A meter's usual release is 16–20 dB/s, and it is deliberately **not** used here. That ballistics curve is chosen for a *moving needle*: a needle has to settle fast enough to read as a single level. A column here is a two-second record of the past, and a release slow enough for a needle leaves a single transient thirty decibels above the floor for the entire window — the window never returns to silence, the floor stops meaning anything, and every later event has to be read against the tail of an earlier one.

`RELEASE_SHIFT = 4` is derived rather than guessed: each column falls by 1/16, which is 0.561 dB per column and 56 dB/s, so walking the full 90 dB of scale takes just over 160 columns, still inside a 200-column window. The step is at least one LSB, so a fall clears the ~15-LSB fixed point the shift's integer truncation would otherwise stall at and a long silence reads the bottom of the band — before the scale was lowered, that residue sat below the −60 floor and stayed invisible. The first column still has to hold a 3 ms transient, so the first column cannot be slower either. A core test asserts the "columns until back at the floor < window length" requirement directly, which ties the parameter to that demand.

### The log map's error is a chord error, not a quantization error

Reading the top bits of `log₂` is the usual trick, and the error it leaves behind is commonly called a quantization error. It is not: quantization is worth 1/16 of an octave (0.38 dB), while the **chord deviation** between a linear mantissa and true `log₂` approaches 0.5 dB half an octave up, which is **right at −20 dBFS** — the loudness of someone speaking. A bare chord reads −20 dBFS as −21.

The 16-byte correction table (`LOG2_16_CHORD_ERROR`, indexed by the recovered mantissa) pulls the error across the whole scale inside 0.02 dB. What matters more is that `scope_height()` and `dbfs()` share one `log2_16()`, so a bar's height and the number next to it **cannot** contradict each other; the tests pin that down too, including the invariant that release only ever pulls a bar down, so the tallest bar is the column set's own maximum. The sweep is weighted as a whole, so its tallest bar no longer answers to the unweighted `PK` row — those are two different filters, and the `PK`/`RMS` rows and the sweep each only need to be self-consistent.

`PK` and `RMS` matching at their window maximum is a steady state; a peak far above the RMS is a knock or a transient. The two are peers because a peak is a property of one sample while RMS is perceived loudness, which is why audio tools draw both tiers instead of choosing one.

### The DC offset is removed by the ES7210's own blocking filter

The microphone is biased at 2.87 V DC (`MIC_BIAS = 0x70`), so its output carries that DC operating point with it, which is why the ZTS6216 datasheet **explicitly requires** a DC-blocking capacitor at the output. The board has one — without it MIC1 would see 2.87 V of DC, and the reading would be pinned at the rail rather than sitting at some small value.

What removes the residual offset is the converter's own DC-blocking filter, in registers `0x22` / `0x23` (the ADC1/2 pair, which is where MIC1 lives). The two values `0x0A` and `0x2A` **differ only in bit 5** (`0x0A ^ 0x2A = 0x20`); the low three bits are identical (`& 0x07` is `0x02` either way) — so bit 5 does **not** control the cutoff frequency, and changing it neither steepens nor relaxes the filter.

**The meaning of bit 5 is not documented anywhere public.** The ES7210's datasheet is marked "Everest Semiconductor Confidential", 11 pages, and the body defers exactly this: *"For more information, please refer to the user guide"*. The two driver families even disagree on the register names:

| Source | `0x22` | `0x23` |
| --- | --- | --- |
| `espressif/esp-bsp` | `HPF2` | `HPF1` |
| `esp-audio-dev` | `HPF1` | `HPF2` |

No public source can settle which stage comes first, so the registers are **not named for a stage** but for the bit they carry: `REG_ADC12_FILTER_SET` (`0x22`, holding `0x2A`) and `REG_ADC12_FILTER_CLEAR` (`0x23`, holding `0x0A`). The write order stays the one `esp-bsp` issues its bytes in — the bit-5-clear register first, then the bit-5-set one — rather than being re-ordered into a stage order nothing documents. The clash between the constant names and the write order is therefore removed rather than papered over by a rename. The two assumptions the walk rests on (the values differ only in bit 5, and they share the low three bits as a corner) are pinned by `const _: () = assert!(...)`: what would change them is a datasheet, not a refactor.

**Swapping them was tried once, and it made things worse.** With the `0x22`/`0x23` values exchanged, `PK` in a quiet room rose from about `-40` to about `-30` — the floor went *up* by roughly 10 dB, the opposite of what a fix for residual offset would do. Since bit 5 is not the cutoff, the swap only changed a mode, and the bench says that mode is the worse one. The driver is back on the `esp-bsp` baseline.

### Walking the HPF corner once at boot

There is one way to tell what the floor is: **walk the HPF corner** (hold bit 5, move the low three bits from `0x00` to `0x07`) and watch whether the floor collapses as the corner rises. Collapsing means there is low-frequency energy to be filtered; unmoved means broadband room noise and the reading stands.

Eight reflashes is too many for a question worth one, so the walk is a one-shot boot self-test rather than a panel feature:

- `Es7210::set_hpf_corner` is a **read-modify-write** touching only `FILTER_CORNER_MASK = 0x07` and leaving every other bit as the part holds it. It does not guess what bit 5 means, nor what a future datasheet might add, so a walk can return to where it started without knowing the rest of the byte.
- The sequencing lives in `iot_core::drivers::audio::CornerSweep` — pure logic, in core because that is the half a host can test; eight corners take about 24 s. The bsp only advances one step per poll from `Es7210Rx::sample` and reads `loudest_rms()`.
- Each code gets `CORNER_SETTLE_MS = 3000` to settle. A DC-blocking filter's transient is well under a millisecond, but the reading that matters is a level the room keeps producing, so the wait is set by the envelope rather than by the filter: 150 polls put the whole window behind the reading after the write.
- The walk returns to `HPF_CORNER` when it ends, including when a write fails part way, and then drops itself — so it costs one boot's settling and nothing afterwards, and capture and display are untouched by it. A failed write abandons the walk rather than retrying: a codec that will not take a corner write will not take the next one either, and a walk that limps on reports levels from a state nobody can name.
- The switch is the compile-time `HPF_CORNER_WALK` in `virtual_components/audio.rs` (not a Cargo feature); production sets it to `false` and the walk, with its four register writes per code, leaves the binary.
- The corner code to frequency mapping is not in any public source, so the log prints the **raw code** and the trend is read by a person:

```text
[AUDIO] HPF corner walk starting: 8 codes, 3000 ms each, back on corner 2 after
[AUDIO] HPF corner 0 reads -26 dBFS, floor -45 dBFS
...
[AUDIO] HPF corner 7 reads -33 dBFS, floor -38 dBFS
[AUDIO] HPF corner walk done, back on corner 2
```

Measured while sitting still on 2026-09-28, `dBFS` loudest / median floor:

| Corner code | Panel reads | Median floor |
| --- | --- | --- |
| `0x0` | `-26` | `-45` |
| `0x1` | `-33` | `-44` |
| `0x2` (shipped) | `-35` | `-42` |
| `0x3` | `-35` | `-42` |
| `0x4` | `-36` | `-41` |
| `0x5` | `-36` | `-41` |
| `0x6` | `-35` | `-40` |
| `0x7` | `-33` | `-38` |

**The floor is broadband: not DC offset, and nothing low-frequency to filter.** The floor rises *monotonically* with the code (`-45` → `-38`), which says a larger code is a higher corner passing more of the low end — the shape a high-pass gives broadband noise. Were the floor dominated by low frequencies (mains hum, rumble, offset leakage), dropping the corner to its lowest code would have crushed it, and instead the whole range spans 7 dB with the shipped `0x2` only **3 dB** off the quietest code. So all the corner can buy is 3 dB, which is not worth stepping off the `esp-bsp` baseline: the shipped code stays `0x2`.

It also shows why the statistic was worth choosing: at `0x0` the panel reads `-26` while the floor is `-45`, and those 19 dB are the transient of the boot itself. Measured with `loudest_rms()` the floor would have read `-26`; the median is unmoved. That gap is also where the panel's `-30` and the real floor of `-42` come from.

The base of the SPL column can therefore be read as a **trustworthy level**: it is not a projection of an offset, it is the sound pressure this room is actually at.

## Testing

```bash
moon run iot:test        # core: metering maths, contracts, the state machine
moon run iot:test-bsp    # board: I2S configuration and buffering
moon run iot:smoke-host  # end to end: source → state → diagnostics
```

There are 342 tests; the count is not the point of this section. **The point is which invariant each one holds.** The table below is an index: a new test must land in some row, and a test that lands in none is an invariant nothing covers.

The "break verified" column reads as: deliberately break that invariant and confirm the test goes red. It is the only way to know a test is actually watching — running green proves nothing.

| Invariant | Test that holds it | Break verified |
| --- | --- | --- |
| every synth table entry is within 1 LSB of `sinf` | `the_table_holds_the_oscillator_it_replaced` | ✅ a 4 LSB table error turns it red |
| still within 1 LSB after interpolation | `the_interpolated_oscillator_tracks_sinf_within_an_lsb` | ✅ as above |
| the oscillator phase never leaves the table | `a_chime_stays_inside_the_table_it_indexes` | ✅ mismatching `WAVE_TURN` with `PHASE_BITS` turns it red |
| the envelope is capped by the ramp (no overshoot into clipping) | `a_chime_opens_and_closes_on_silence` | ✅ dropping `min(ramp)` turns it red |
| a brief idle ring is not a drained stream | `a_late_poll_within_the_silence_limit_is_not_a_stall` | — |
| only a genuine drain triggers a rebuild | `a_brief_idle_between_feeds_is_not_a_drained_stream` | ✅ deciding straight off `tx_idle` turns it red |
| the microphone's DC-blocking write order cannot be swapped | `es7210`'s MockI2c trace test | ✅ swapping `0x0A`/`0x2A` turns it red |
| register writes are read-modify-write, not overwrite | `es7210`'s stored-byte test | ✅ changing `merged` to `value` turns it red |
| ES8311 needs a second write after power-up | `es8311`'s power-on value test | ✅ dropping the second write turns it red |
| a partly filled column is not committed | `a_partial_window_leaves_the_column_untouched` | ✅ setting `committed` straight to full turns it red |
| an uncommitted window has a floor of zero | `a_window_of_silence_has_a_floor_of_zero` | ✅ starting the floor at 1 turns it red |
| bar height and dB share one source (so cannot disagree) | `dbfs_agrees_with_the_band_it_is_drawn_on` | ✅ skewing the height range to 255/200 turns it red |
| dB, SPL and bar height are each monotonic | `dbfs_…` / `spl_…` / `scope_height_never_dips…` | — |
| the A-weighting cascade matches the integer prototype LSB for LSB | `weighting`'s band test | ✅ a 0.25 dB mid-band coefficient error turns it red |
| an invalid motion sample advances nothing | `an_invalid_sample_advances_nothing` | ✅ removing the `valid` early return turns it red |
| a knock needs the residual to fall back under the quiet bar | `recognizer`'s knock/turn tests | ✅ dropping `TAP_QUIET_MG2` turns it red |
| gestures pair by position | `input`'s double-tap window test | ✅ widening the pairing distance by 200 px turns it red |
| gesture tallies accumulate | `state`'s `off_tap_…keeps_off` | ✅ resetting the tally each time turns it red |

`overlay.rs` covers column layout on its own: a whole blank cell fits between a unit and the widest reading, the three columns never touch, and the SPL reading stays within `LEVEL_VALUE_GLYPHS`. The layout lives in core rather than at board level because a column laid out wrongly shows on screen only as "looks intentional" — a unit pushed out of line by its digits is a bug that does not look like a defect while being obviously the wrong answer — and the board side carries `esp-hal`, which the host cannot compile at all.

`smoke-host`'s fake source supplies data in **whole polls** and asserts the dB reading really passed through the state layer; its peak counter is seeded from `isize::MIN` — the level is always ≤ 0 dBFS, and seeding from 0 would leave the maximum stuck at "no level at all".

> The full break-verification procedure is recorded in the [Playback Implementation Record](@/records/iot/playback.en.md). This table is a contract, not a list: the day a row stops turning red, it is no longer a test but decoration.
