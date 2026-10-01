+++
title = "Playback Implementation Record"
weight = 10
sort_by = "weight"
[extra]
source_file_hash = "49afa729fa864b844dc3660a4c382882f979fc8f"
translated_at = "2026-10-01T07:37:04Z"
+++

<!-- doc-audience: ai -->

# Playback Implementation Record

Judgements and rejected approaches from the playback path in `apps/iot`. **AI-produced, not reviewed by a human** — see the [section above](@/records/_index.en.md). The system description (contract, panel readout, registers) is in [Playback Semantics](@/development/iot/playback.en.md).

Each judgement is marked with its basis: **`[test]`** a unit test backs it, **`[measured]`** hardware measurement with a way to reproduce, **`[reported]`** not reproduced, kept only for the record.

---

## `Cp0Disabled`: a synthesised sound crashes in the feed interrupt

### Symptom

Tapping the Speaker page to play a chime stops the device outright — screen and sound both stop, and **it does not reset**. A silent board is worse than a crashed one: it looks like "it hung", and "it hung" usually means scheduling.

### Root cause

`Tone::fill` executed an FPU instruction inside the Priority2 feed interrupt, raising the coprocessor exception `Cp0Disabled` (`EXCCAUSE: 0x20`). The board stopped on the first chime frame.

Symbolised call stack (`addr2line`, Xtensa toolchain in a container):

```
Tone::fill
  ← Es8311Tx::feed
    ← feed_task::poll
      ← Executor::poll
        ← esp_rtos::embassy::handle_interrupt::<1>
```

**The key property: this is a property of the interrupt context, not of the arithmetic.** The same code runs perfectly on the cooperative executor.

`[measured]` Reproduction:

1. The firmware sends `Intent::Business(BusinessIntent::PlayNext)` into `INTENT_BUS` every 1.2 s after boot — the same path a real tap takes
2. `[PLAY] Asset` is fine; `[PLAY] Chime` is immediately followed by a panic
3. The first chime frame crashes it — there is no cumulative process

**Why automate the reproduction**: the bug needs "tap the chime" to fire, and a human tap is both slow and irreproducible. With a self-trigger, one flash plus one log capture decides it — which is what made every later fix verifiable automatically.

### Two rejected hypotheses

Recorded because both were highly plausible, and both lead to the wrong fix.

#### Hypothesis one: synthesis is too expensive and starves the cooperative executor

`[measured]` The first attempt measured a worst-case chime feed of **4434 µs** against a 5000 µs cadence — 88.7%. Asset, with the same frame count, is a memcpy at **5 µs**. The conclusion looked clear: make synthesis cheap.

So: a 512-entry table with linear interpolation. `[measured]` **5 µs** — a 887× improvement.

**The board still hung.** The first chime frame crashed it, and no amount of "ran long enough to starve it" applies. The cost hypothesis is incompatible with the symptom.

#### Hypothesis two: a table with interpolation is enough

The table is `f32`, and so is the interpolation, so FPU instructions are still executed.

`[measured]` To separate "cost" from "floating point", the oscillator was temporarily reverted to per-sample `sinf` — same call site, the only variable changed: **same crash, identical exception**.

That step is what pinned the root cause to floating point itself.

### Final fix: integers at run time

`[test]` Three tests hold the fix in place:

| Test                                                      | What it holds                                                        |
| --------------------------------------------------------- | -------------------------------------------------------------------- |
| `the_table_holds_the_oscillator_it_replaced`              | every entry within 1 LSB of `sinf`                                     |
| `the_interpolated_oscillator_tracks_sinf_within_an_lsb`  | still within 1 LSB after interpolation                                 |
| `a_chime_stays_inside_the_table_it_indexes`              | the phase never leaves the table (interpolation reads `whole + 1`)      |

Implementation: the phase is an 8-bit fixed-point index into a 1024-entry static table, the envelope is Q16, and `inv_ramp` is computed once per note. The per-sample path is integer multiplies and shifts only. The table is built at compile time by `sine()` (`libm::sinf` is not a `const fn`); **fidelity is pinned by tests, not asserted in a comment**.

`[measured]` After: 40 automated taps (20 chime, 20 asset), **0 panics**, worst single feed **786 µs** (12% of the cadence), `[PLAY]` steady at 200 kHz, 0 ring dry / restart / watchdog.

> Two bugs in my own tests were caught this way: integer interpolation's `low + (…) >> B` shifted the whole sum right by 8 because `+` binds tighter than `>>`, leaving the chime nearly silent; and a reference value missing its TAU factor, reporting a 38831 LSB error. Both were found by the tests, not by the code.

### The lesson

`audio-probe` stayed green throughout, because its feed runs under `SpeakerRunner::Inline` on the **cooperative** executor — where floating point is fine.

**This class of bug only shows up on the interrupt executor.** The same is true of `HostSpeaker` in the host tests. Both the investigation and the regression test have to run in the real interrupt context.

---

## Why the feed owns its executor

`[measured]` Under cooperative scheduling, "nothing else is running" does not hold: a capture fold occupies 17 ms, one panel write 16 ms, and input diagnostics reported 2470 ms of a 5000 ms window. All of these are larger than the 5 ms cadence.

The playback ring has 120 ms of runway. `[measured]` In a 150-second warm-up soak it was eaten twice, the worst late feed being **167 ms** — which drains the DMA outright.

Hence `feed_loop` on `FROM_CPU_INTR1` at `Priority2`, and `control_loop` (which does I2C) left on the cooperative executor.

`[measured]` After the move, on the same board and the same environment over 158 continuous seconds: cadence 199.6–202.9 kHz against a 200 kHz target, worst gap 6 ms, zero gaps over 20 ms, 0 ring dry, 0 DMA restart. Capture still occupied 17 ms and the panel still 16 ms in that window — they simply could no longer defer the feed.

`Priority2` is a deliberate ceiling: one level higher and the feed would starve input and render instead.

---

## The boundary between ISR and cooperative side

`feed()` early on did two things in the interrupt that it must not: log (taking the logger's lock) and restart the DMA (allocating a fresh ring). An interrupt can preempt a cooperative task holding either of those locks, and that task cannot release until the feed returns — **a deadlock with no reset to clear it**.

The fix was to split them: `feed()` only *records* a `Recovery`, and `recover()` performs the rebuild on the cooperative side.

One ordering in that fix is counter-intuitive and the easiest thing to break later:

1. `feed()` must **push before it records** — writing into a live transfer has exactly one route, and a just-drained ring is entirely free, so that push is what primes the restart
2. the buffer `stop()` returns must **never** be pushed into — it comes back whole with `pre_filled` set, so a push is handed the empty tail past the ring's end and writes nothing
3. `write()` replays that whole ring from the first descriptor — which is why priming first is what makes the replay this cadence's audio rather than the sound that stalled

---

## The asset recipe

`asset.pcm` is binary and does not describe itself, so the recipe lives in the `ASSET` comment in `audio_out.rs`: 660 Hz for 120 ms then 880 Hz for 160 ms, each note a sum of partials at 1.0, 2.76 and 5.40 of its fundamental.

- **2.76 and 5.40**: a struck bell's overtones are not integer multiples of the fundamental, and integer multiples only sound like a buzzer
- **660 and 880** were chosen against the chime's 880 and 1320 so the catalogue's two sounds are distinguishable by ear
- **silence at both ends**: each note decays to ~1% of its own, the first sample fades in over 2 ms, and 45 ms of silence follows the last — a stored file cannot grow a ramp the way `Tone` does, and a waveform starting on a non-zero voltage is a click, and a click is louder than an ugly sound

---

## Whether the tests actually watch: the break test

A green test run says nothing — it only proves the code was executed. There is one way to know a test is doing its job: **break the invariant it holds, on purpose, and confirm it goes red.**

Procedure: temporarily change the product code, run that test, restore, then compare a shasum baseline across all 47 files to confirm the working tree returned to its original state.

`[measured]` 15 candidates — **all 15 turned red**:

| What was broken | Test |
| --- | --- |
| one synth table entry off by 4 LSB | `the_table_holds_the_oscillator_it_replaced` |
| `WAVE_TURN` inconsistent with `PHASE_BITS` | `a_chime_stays_inside_the_table_it_indexes` |
| envelope drops its `min(ramp)` cap | `a_chime_opens_and_closes_on_silence` |
| watchdog decides straight off `tx_idle` | `a_brief_idle_between_feeds_is_not_a_drained_stream` |
| HPF write order `0x0A`/`0x2A` swapped | `es7210`'s MockI2c trace test |
| masked write becomes an overwrite | `es7210`'s stored-byte test |
| the second write after power-up dropped | `es8311`'s power-on value test |
| a partly filled column marked committed | `a_partial_window_leaves_the_column_untouched` |
| uncommitted window's floor returns 1 | `a_window_of_silence_has_a_floor_of_zero` |
| bar height range skewed to 255/200 | `dbfs_agrees_with_the_band_it_is_drawn_on` |
| invalid sample no longer returns early | `an_invalid_sample_advances_nothing` |
| knock drops the quiet residual bar | `recognizer`'s knock/turn tests |
| gesture pairing distance widened by 200 px | `input`'s double-tap window test |
| tap tally reset each time | `state`'s `off_tap_…keeps_off` |
| mid-band biquad coefficient off by 0.25 dB | `weighting`'s band test |

**Conclusion: none of the 342 are decorative. Not one was deleted.**

An earlier pass grouped four tests as "suspected duplicates" by name similarity (the three `never_runs_backwards` monotonicity tests, and the two `off_…` full-path ones). Reading each showed they cover different objects — `spl()` calls `dbfs()` internally, so it exercises a second path, and the two `off_…` tests assert different tallies. **Deleting tests by name deletes real coverage.**

The count was never the problem; readability was. 342 tests cannot be read, so the "Testing" sections of `audio.md` and `playback.md` became invariant index tables: one row per invariant, naming the test that holds it, with the verification column above. A new test must land in some row, and the day a row stops turning red it is no longer a test but decoration.
