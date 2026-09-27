+++
title = "Audio Metering Semantics"
weight = 260
sort_by = "weight"
[extra]
source_file_hash = "1bcd0274a7199fcdc4a117eec341f78344db215f"
translated_at = "2026-09-27T00:00:00Z"
+++

# Audio Metering Semantics

The ES7210 is only one source. What the page exposes is **level** — how many dBFS right now, and how much of it persisted over the last two seconds — not a count of LSBs and certainly not raw samples. This page covers the contract, the meaning of every readout, and why the numbers are drawn the way they are.

## The contract

The capture driver **never stops**. It is always folding the samples the I2S delivers into the envelope; the `ST` phase only decides which envelope the panel draws and whether the sweep follows the live one. Reading `ST` as "the recorder is running / stopped" gets two things wrong at once: recording makes the driver do no additional work, and stopping it makes the driver do no less.

A capture poll is pure data plane: it refreshes the envelope and raises **zero** semantics. Starting and stopping are decided only by a business-side click (`ToggleRecord` in `intent.rs`), and only on the Audio page — a click on any other page still cycles the color.

There are three phases: `IDLE` (nothing captured, the panel draws only `TAP TO REC` and no sweep), `REC` (follows the live envelope), and `STOP` (draws the latched copy). A click on either `IDLE` or `STOP` goes straight back to `REC`; there is no "resume" semantic, because what gets latched is **the window at the moment of stopping**, not a recording.

When the board finds no ES7210, `audio_enabled` is false and the `AUDIO` page never enters the page cycle (`DisplayPage::next_page` skips pages no capability backs); every other page is unaffected. Getting to the page is a triple-click of the button or a three-finger tap (`TogglePage`).

## Parameter lookup

Work the bench against these two tables. Every metering parameter lives in core (`drivers/audio.rs`) and is independent of the hardware; the board only decides how many bytes to feed it.

### `AUDIO` readout rows

| Tag | Field | Unit | Meaning | Parameter |
| --- | --- | --- | --- | --- |
| `ST` | `audio.phase` | — | `IDLE` not capturing / `REC` follows the live envelope / `STOP` draws the latched envelope | — |
| `MS` | `audio.elapsed_ms` | ms | Wall-clock time elapsed, **not** a sample count | `CAPTURE_MS` |
| `PK` | `envelope.loudest()` | dBFS | Loudest peak in the window | `SCOPE_FLOOR_DECIBELS` |
| `RMS` | `envelope.loudest_rms()` | dBFS | Largest per-column RMS in the window | `SCOPE_FLOOR_DECIBELS` |
| `COL` | `envelope.committed()` | columns | Columns committed so far, capped at 200 | `ENVELOPE_COLUMNS` |
| `RST` | `audio.restarts` | times | Re-arms after a stalled DMA | `CaptureWatchdog` |

Two more marks ride at the end of rows: the `DBFS` on the `PK` and `RMS` rows is the unit, and the `CLIP` at the end of the `COL` row is the clip latch.

`COL` climbing while `PK` does not is what a dead capture path looks like: in a quiet room `PK` climbs with it, and a reading parked at `-60` says no sample arrived for this whole window. Read `RST` together with `COL` — both moving while `PK` sits at `-60` means the driver is re-arming over and over; neither moving means the page is not running.

`MS` counts wall-clock time rather than samples because every other readout on the panel is stamped from the same clock, and a late poll must not make the capture look shorter than it was.

### Scale and graphics

| Element | Meaning | Parameter |
| --- | --- | --- |
| Left `0` | 0 dBFS, the top of a bar | `SCOPE_FLOOR_DECIBELS` |
| Left `-24` / `-48` | Ruler ticks; a dotted grid line every 12 dB | `GRID_DECIBELS` |
| Left `-60` | The noise floor, which is the centre line (solid) | `SCOPE_FLOOR_DECIBELS` |
| Centre line | Silence. Bars grow upward and downward from it, **mirrored** | `WAVE_CY` |
| One column per 10 ms | Time width of a column | `COLUMN_MS` |
| 200 columns | The whole window, 2.0 s | `ENVELOPE_COLUMNS` |
| Solid core in a column | That column's RMS, i.e. perceived loudness | `loudest_rms()` |
| Dithered shoulder in a column | The part of the peak above the RMS | `released_peaks()` |
| Block at the top | The column where clipping happened | `FULL_SCALE_LSB` |
| Short dash on the tallest bar | Peak hold: the window's tallest bar and where it sits | `loudest()` |
| `2.0S` at bottom left | Window length, computed from the constants | `COLUMN_MS` × `ENVELOPE_COLUMNS` |
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

`RELEASE_SHIFT = 4` is derived rather than guessed: each column falls by 1/16, which is 0.561 dB per column and 56 dB/s, so walking the full 60 dB of scale takes 107 columns, comfortably inside a 200-column window. The first column still has to hold a 3 ms transient, so the first column cannot be slower either. A core test asserts the "columns until back at the floor < window length" requirement directly, which ties the parameter to that demand.

### The log map's error is a chord error, not a quantization error

Reading the top bits of `log₂` is the usual trick, and the error it leaves behind is commonly called a quantization error. It is not: quantization is worth 1/16 of an octave (0.38 dB), while the **chord deviation** between a linear mantissa and true `log₂` approaches 0.5 dB half an octave up, which is **right at −20 dBFS** — the loudness of someone speaking. A bare chord reads −20 dBFS as −21.

The 16-byte correction table (`LOG2_16_CHORD_ERROR`, indexed by the recovered mantissa) pulls the error across the whole scale inside 0.02 dB. What matters more is that `scope_height()` and `dbfs()` share one `log2_16()`, so a bar's height and the number next to it **cannot** contradict each other; the tests pin that down too, including the invariant that the tallest drawn bar's height is exactly the `PK` row — release only ever pulls a bar down, so the tallest bar can neither exceed the window's loudest peak nor be dragged below it.

`PK` and `RMS` matching at their window maximum is a steady state; a peak far above the RMS is a knock or a transient. The two are peers because a peak is a property of one sample while RMS is perceived loudness, which is why audio tools draw both tiers instead of choosing one.

## Testing

```bash
moon run iot:test        # core: metering maths, contract, state machine
moon run iot:test-bsp    # board: I2S configuration and buffering
moon run iot:smoke-host  # end to end: source → state → diagnostics
```

Core covers: dB and bar height sharing one map, dB monotonic and never over the rail, the release's per-column floor and its "back to the floor within the window", RMS distinguishing a steady signal from a single spike, the RMS of a two-slot full-scale signal, the clip latch and its mark ageing out with the column, chronological order across a wrapped window, the 2 s window length, and the tallest bar being the `PK`.

The `smoke-host` fake source feeds **whole polls** and asserts the decibel reading genuinely travels through the state layer; its peak counter is seeded at `isize::MIN` — every reading a level meter can report is at or below 0 dBFS, so a zero seed would leave the maximum at "no level at all" and the smoke would pass on a capture that never measured anything.