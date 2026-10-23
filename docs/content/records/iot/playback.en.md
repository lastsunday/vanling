+++
title = "Playback Implementation Record"
weight = 10
sort_by = "weight"
[extra]
source_file_hash = "f5263efc4ce0d7d84fdf3f76857d134ab60af2b2"
translated_at = "2026-10-22T12:29:02Z"
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

1. The firmware sends `Intent::Business(BusinessIntent::PlayNext)` into `INTENT_BUS` every 1.2 s after boot — the name of the day; it is `Playback(PlaybackAction::Next)` today — the same path a real tap takes
2. `[PLAY] Asset` is fine; `[PLAY] Chime` is immediately followed by a panic (both names are the ones of the day; the catalogue grew to five sounds later, see [Sound recipes](#Sound_recipes))
3. The first chime frame crashes it — there is no cumulative process

**Why automate the reproduction**: the bug needs "tap the chime" to fire, and a human tap is both slow and irreproducible. With a self-trigger, one flash plus one log capture decides it — which is what made every later fix verifiable automatically.

### Two rejected hypotheses

Recorded because both were highly plausible, and both lead to the wrong fix.

#### Hypothesis one: synthesis is too expensive and starves the cooperative executor

`[measured]` The first attempt measured a worst-case chime feed of **4434 µs** against a 5000 µs cadence — 88.7%. Asset — a stored PCM blob — has the same frame count and is a memcpy at **5 µs**. The conclusion looked clear: make synthesis cheap.

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
| `every_partial_stays_inside_the_table_it_indexes`              | the phase never leaves the table (interpolation reads `whole + 1`)      |

Implementation: the phase is an 8-bit fixed-point index into a 1024-entry static table, the envelope is Q16, and `inv_ramp` is computed once per note. The per-sample path is integer multiplies and shifts only. The table is built at compile time by `sine()` (`libm::sinf` is not a `const fn`); **fidelity is pinned by tests, not asserted in a comment**.

`[measured]` After: 40 automated taps (20 chime, 20 asset), **0 panics**, worst single feed **786 µs** (12% of the cadence), `[PLAY]` steady at 200 kHz, 0 ring dry / restart / watchdog.

> Two bugs in my own tests were caught this way: integer interpolation's `low + (…) >> B` shifted the whole sum right by 8 because `+` binds tighter than `>>`, leaving the chime nearly silent; and a reference value missing its TAU factor, reporting a 38831 LSB error. Both were found by the tests, not by the code.

### The lesson

`audio-probe` stayed green throughout, because its feed runs on the **cooperative** executor — where floating point is fine.

**This class of bug only shows up on the interrupt executor.** The same is true of `HostSpeaker` in the host tests. Both the investigation and the regression test have to run in the real interrupt context.

The probe joins its feed straight onto the main task (`run_audio_only`) rather than going through `SpeakerRunner` — that enum appears only in the product firmware. The conclusion stands; the mechanism is as above.

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

## Two decisions the host cannot see, so they are pure functions with explicit tests

`still_sounding(source_done, arm_sounded, arm_in_flight)` answers whether a sound is still audible.

**Rejected direction**: treat a spent source as the end of the sound. **Wrong**: a spent source is not the end — the arm carrying its last frames has to play out first. Inverting it is worse: a spent source keeps arming silent rings, each of which answers "still going", so the phase never leaves `Playing` and every later tap is tallied as dropped against a sound already over. And this error is **entirely invisible from the host**, because the arm only exists on the board.

The watchdog's direction mistake is the same shape but costs more: `tx_idle` goes high whenever the FIFO is momentarily empty, which is the ordinary state between two feeds. Treated as a fault, it tears the stream down and rebuilds it mid-sound, restarting the codec's clocks and replaying a ring of stale audio.

`[test]` `a_spent_source_never_reports_playing_again` and `a_brief_idle_between_feeds_is_not_a_drained_stream` (`core/src/drivers/playback.rs`) pin the two directions; the second walks a whole ring's playthrough feed by feed, so no unlucky arrangement of cadences can hide it.

---

## Sound recipes

The recipe **is** the file: the five `assets/*.mid` come to 488 bytes together, and any DAW will open one and show what note it holds and how long it lasts.

**The source of truth for the recipe is the `RECIPES` table in `crates/tool/src/midi.rs`**, and the `.mid` files are what it builds — they are checked in, but they are products. Changing a sound means editing one row of that table and running `moon run iot:gen-midi-assets`, not editing a binary. A file edited in a DAW without the table updated fails `the_files_are_what_the_recipes_generate` — **that is what replaces "the recipe lives in a comment the build does not run."**

`bsp-esp` still has an `#[ignore]`d `write_the_catalogue_as_a_wav`, which means **you can listen without flashing a board**: it renders the files to a WAV through the driver's own synthesis path (`Tone` over the parsed table). A change to a batch of notes costs a second to judge instead of twenty minutes of flashing and reading a serial console.

### Why the recipes are in a tool rather than in the board

Generating assets is not a test, so the table and the `.mid` generator moved out of `bsp-esp`'s test module into an `iot-tool` with no dependencies.

The cost is that **the two sides cannot see each other**: the recipes no longer read `Sound`, and the firmware cannot read the recipes. What is left is the filename convention (`Rise` becomes `rise.mid`, and `label_of` has to say the same thing).

One test holds it: `the_board_names_the_same_files` reads the `include_bytes!` list out of `bsp-esp/src/sounds.rs` and checks it against the recipes. Without it a rename done on one side only passes silently — both files exist, both are claimed by something, and only the board plays the wrong one.

### The cost of moving it out: bare-metal `--workspace`

`lefthook.yml`'s `iot-clippy` is the last bare-metal `--workspace` in the tree (`cargo clippy --workspace --target riscv32imac-unknown-none-elf`). It had always worked only because every member happened to be firmware code. `iot-tool` made the workspace's first std crate, a bare-metal target has no std, and the pre-commit hook failed immediately with `E0463: can't find crate for std`.

**A feature cannot fix this, and that was tested.** With `required-features` on `[lib]`, `[[bin]]` and `[[test]]`, `cargo clippy --workspace --target <riscv>` still compiled the crate and still failed — `required-features` decides which of a package's *targets* build, not whether the package builds. The one direction a feature helps is the reverse: `cargo run -p iot-tool` without the feature says `target ... requires the features` instead of failing halfway through.

The fix is `--exclude iot-tool`, which finishes a migration that was already specified: item 4 of the [adding-a-chip-family checklist](@/development/iot/architecture.en.md) says `moon`/`lefthook`'s `--workspace --target` is to be scoped to the firmware, and `moon.yml` was converted to an explicit `-p iot-app` long ago — `lefthook` was simply never converted and never needed to be. The cost is that **a second host tool has to be added there too**, which is why the comment says so.

## Why the first version's five sounds did not sound like five

`[实测]` On the board the first version came back "not clear enough". Three causes, each measured rather than guessed, and each one a guideline:

**One: `TAP` and `ERROR` used the same pitch — E6 = 1319 Hz, zero semitones apart.** The only pair in the catalogue that even shared a note, differing only in whether it went up or down.

**Two: across all ten pairs, the nearest pair of notes fell within 0–5 semitones** — not one further than a perfect fourth. The whole fundamental range was two octaves and four of the five sounds sat inside E5–G6. The guideline asks for "gross differences ... two or three octaves".

**Three: the note counts were `1/2/2/2/3`.** The most effective dimension in the guideline, and three of five sounds on the same value. The lengths clustered too: 290 / 330 / 338 ms.

The fix follows the guideline itself — counts spread to 1/2/2/3/4, registers laid out a band per sound three semitones apart, no pitch used twice, lengths spread to 200 / 262 / 312 / 350 / 384 ms. The one repeated value, the two-note count, falls on `ERROR` and `RISE`, the only pair whose contours are opposite.

### Why it fits at all: five sounds spend the whole usable range

`[实测]` This was not designed, it was searched. The constraints are fixed: 277–1760 Hz (below that the speaker will not produce it, above that the ×2.76 partial passes 5 kHz), and neighbouring registers at least three semitones apart.

The result: **a bare octave, a falling third and a rising third come to twenty semitones, four gaps add twelve, and that is exactly the thirty-two available — not one spare.** Raise the gap to four semitones and there is no solution.

So this catalogue has no slack: move any one register a band and it has to take that band from another. **The only ways to more distinctness are a wider frequency range — below 277 Hz, or past the partial's ceiling — or fewer sounds.** The literature backs the second: McGookin & Brewster (2004) measured about 30% correct identification when four earcons were presented concurrently.

### Why `TAP` went from two notes to three

This is the most expensive concession in the version. `TAP` is both the default sound and the boot sound, the most frequent interaction in the catalogue, and Google's rule is precisely that **the more often an interaction happens, the less intrusive that sound should be**. Three notes run 30 ms longer than two.

The guidelines want note counts; Google wants restraint; each sits on the other's territory. `[判断]` The guidelines won, because what came back from the user was that the sounds could not be told apart. `[实测]` And before this version has been listened to on the board, it is the one change here backed by numbers rather than by an ear — **the only part I cannot hear.**

### Why not an arpeggio

`RISE` was originally two rising notes like `TAP`. The earcon guidelines warn that such a pair is easily confused, and their fix is "make the rhythms as different as possible" — **so the first version did exactly that and turned `RISE` into a four-note arpeggio.** (A second version pushed the distinctness problem back the other way; see "Why the first version's five sounds did not sound like five" above.)

**That was wrong, and Google's own ten years of data say why.** Their camera shutter once "didn't sound anything like a camera; it had a melody, which made it very expressive", and they then deliberately subtracted:

> dialing up all individual sounds to that level often comes at the expense of the user experience. **Today, people don't really want to be hearing melodies when they're taking photos.**

> Good multisensory design often **disappears** — users might not actively notice it.

> **The more often an interaction happens, the less intrusive** that sound should be.

A functional sound that may fire dozens of times a day turned into an arpeggio is a sound that draws attention, which weakens the moments that matter. **The earcon literature measures whether users can tell A from B — that is a data display. vanling is a desk companion and none of its five sounds encodes a data parameter, so by Google's standard they should get quieter, not richer.**

The fix was a **bare octave**, placed in the lowest register so it does not compete with `TAP`. The pitches are in the [catalogue table](@/development/iot/playback.en.md#Where_the_sound_comes_from).

**Why this is written down**: the decision rests on another company's practice (a twelve-person sound team at Google, shipping across all of its products), not on an experiment of ours. The next person to see `TAP` and `RISE` overlap in register and think "make it more obviously different" will do it again — **overlapping registers on two sounds with different contours is the design, not a defect.**

### Why only two partials: the count is a cost, not a taste

The partial count multiplies straight into the per-sample cost. `[measured]` On the board: a cadence of 199.8–200.3 kHz and a worst single feed of **583 µs** (11.7% of the cadence), leaving four times the headroom even with two partials.

583 µs is *lower* than the 786 µs on record, because `render_slice` changed from one frame per call to a whole slice per call — 240 fewer entries into the source per feed, **which more than covered the two extra partials**.

Eight partials would push the per-sample work past the 5 ms cadence. Past that there are only two roads: a cached wavetable (which accepts harmonic ratios only, so the buzzer returns), or a longer interval. **Re-measure the feed cost before adding a third partial** rather than treating this extrapolation as a licence.

### Three names in the catalogue have changed, each for its own reason

| Now | Was | Why |
| --- | --- | --- |
| `Tap` | `Chime` | `Chime` and `Bell` are near-synonyms in English, so two of those side by side are hard to tell apart; and `Tap` names the trigger rather than the shape of the sound |
| `Notice` | `Notify` | the old name is a verb while `Warn` and `Error` in the same column are nouns |
| `Rise` | `Bell` | it is a bare octave rising, not a bell; and all five share one timbre, so naming one of them after a timbre mixes up the column. `Rise` names the pitch motion |

Against Google's own sound library (`material_product_sounds`) we line up as:

| Ours | Material |
| --- | --- |
| `TAP` | `ui_tap-variant-*` |
| `NOTICE` | `notification_simple-*` |
| `WARN` | `alert_high-intensity` |
| `ERROR` | `alert_error-*` |
| `RISE` | none (Material has `hero_simple-celebration` here) |

**The one Material has that we do not**: its `ui_lock`/`ui_unlock` are a paired state flip, while `PlaybackAction::SetMuted` is entirely silent today — nothing on the speaker, only a `MUTE` on the panel. Adding that pair is a feature change and outside this one.

Material's three-part naming (`notification_simple`) cannot be copied: the panel's value slot is seven glyphs and that is twenty.

The `Cp0Disabled` section above **keeps the old `Chime` spelling**: it is the record of a real fault, and renaming it would stop matching the serial log it was read from.

### Why MIDI rather than samples

The rejected direction: store the five sounds as PCM blobs. **The recipe at the time was a string of numbers in a comment in `audio_out.rs`**, which the build does not run, so the recipe and the code could drift apart with nothing noticing; and 31 KB of binary shows nothing at all in a `git diff`.

MIDI resolves no synthesis problem — it carries no timbre and no per-note envelope in either version — it resolves **provenance**: the file is a standard format, it can be opened and edited, and a change shows up in a pull request as something readable.

The cost is the `ramp_ms` retreat above.

### Why the partials are inharmonic

`TIMBRE = [(100, 180), (276, 76)]`. **2.76 is a struck bell's second-partial ratio**; integer multiples only sound like a buzzer, which is why the sound this replaced had to be stored as samples rather than generated.

`[测试]` `the_family_timbres_partials_are_inharmonic` pins the 276 and asserts it is not a whole multiple of 100, so nobody "simplifies" it to 3. `[测试]` `the_family_timbre_cannot_outrun_the_rail` pins that the gains sum to full scale: two partials that line up and push past the rail is a click at the end of a sound.

### A single partial has to be the synthesis it replaced

`[测试]` `a_lone_partial_is_the_oscillator_it_replaced`: `Tone::on(score, &PLAIN)` speaks through one partial at full gain, and the result is compared sample for sample against **the synthesis it replaced, written out in the test**, across the whole score — 16 000 samples.

A reference implementation rather than sixteen pasted numbers: a pasted array pins sixteen samples and says nothing about the other 16 000, whereas this test catches a ramp or an interpolation that drifted anywhere in the score.

---

## Whether the tests actually watch: the break test

A green test run says nothing — it only proves the code was executed. There is one way to know a test is doing its job: **break the invariant it holds, on purpose, and confirm it goes red.**

Procedure: temporarily change the product code, run that test, restore, then compare a shasum baseline across all 47 files to confirm the working tree returned to its original state.

`[measured]` 15 candidates — **all 15 turned red**:

| What was broken | Test |
| --- | --- |
| one synth table entry off by 4 LSB | `the_table_holds_the_oscillator_it_replaced` |
| `WAVE_TURN` inconsistent with `PHASE_BITS` | `every_partial_stays_inside_the_table_it_indexes` |
| envelope drops its `min(ramp)` cap | `a_tap_opens_and_closes_on_silence` |
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

**[measured] The tests added since were break-tested separately, and all four turned red**:

| Break | Test |
| --- | --- |
| partial ratio 2.76 changed to the integer 3 | `the_family_timbres_partials_are_inharmonic` |
| gains summing past full scale | `the_family_timbre_cannot_outrun_the_rail` |
| one pitch changed in the `RECIPES` table | `the_files_are_what_the_recipes_generate` |
| ramp inference from `min(ms/4, 40)` to `min(ms/2, 40)` | `a_note_ramps_at_both_of_its_ends` |

**Conclusion: none of the 342 are decorative. Not one was deleted.**

An earlier pass grouped four tests as "suspected duplicates" by name similarity (the three `never_runs_backwards` monotonicity tests, and the two `off_…` full-path ones). Reading each showed they cover different objects — `spl()` calls `dbfs()` internally, so it exercises a second path, and the two `off_…` tests assert different tallies. **Deleting tests by name deletes real coverage.**

The count was never the problem; readability was. 342 tests cannot be read, so the "Testing" sections of `audio.md` and `playback.md` became invariant index tables: one row per invariant, naming the test that holds it, with the verification column above. A new test must land in some row, and the day a row stops turning red it is no longer a test but decoration.
