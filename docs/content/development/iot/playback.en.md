+++
title = "Playback Semantics"
weight = 270
sort_by = "weight"
[extra]
source_file_hash = "f19e622a2127b92fd17fa49d38e52d2897bdebda"
translated_at = "2026-10-15T02:20:27Z"
+++

# Playback Semantics

The ES8311 is only one peripheral. What the page exposes is **the start and the end of a sound** — what is playing now, and whether it has finished — not a progress bar and not a volume. This page covers the contract, the panel readout, and the "why it is like this" behind every part of it.

This page describes what the system is now; how it became so, and what was rejected along the way, is in the [Playback Implementation Record](@/records/iot/playback.en.md) (AI-produced, not reviewed by a human).

The capture side is in [Audio Metering Semantics](@/development/iot/audio.en.md). The two pages share one MCLK but their contracts are opposite: capture never stops, while playback is over when the one-shot is done. Which is why playback has a phase and capture does not.

## The contract

A sound is a one-shot: one tap plays one sound, it reports completion when it runs out, and there is no "pause" or "progress" in between.

So the `Speaker` driver is three steps rather than one — `play()` starts it, `feed()` moves it along, and the driver says it is `done()`.

The loop that ties the three together is the app's playback module (`crates/app/src/playback.rs`), because only it knows the cadence. The board owns the ring, because only it knows the DMA.

That loop is split into two tasks on two executors, because a cadence and a blocking call are two things that cannot be mixed.

`feed_loop` does nothing but the 5 ms cadence and one memory write, and is spawned onto the S3's `FROM_CPU_INTR1` at `Priority2`. `control_loop` handles commands, the I2C behind `play()`/`set_muted()`, and reporting `finished` back on the intent bus, and stays on the cooperative `FROM_CPU_INTR0`.

The driver is shared as a `Box<dyn Speaker>`, so `play()` and `feed()` cannot tread on the ring the other is writing.

That is why `Speaker` demands `Send + 'static`, and why its error is the named `SpeakerFault` rather than each implementation's own type: a trait object cannot spell an unnamed associated type.

The C6 and the host have no second software interrupt, so they take `SpeakerRunner::Inline` and the two loops are `join4`ed on one executor, with the same semantics.

`PlaybackFinished` travels back over the intent bus from the playback task; it is not a polled flag.

The phase leaves `Playing` only when the driver itself says so. Letting the page and the driver each tell their own story produces "it still looks like it is playing, but it finished ages ago", and every later tap is then dropped as a replay of a sound that had already ended.

A trigger that lands on `Playing` only increments `dropped`; it does not interrupt the current sound.

Interrupting leaves a step between two stretches of audio — that is a click, and a click is much harder to explain than "that tap went unheard". The drop has to be visible, so it gets a readout of its own (`DRP`) instead of being quietly folded into another number.

`ToggleMute` latches the output; it does not stop playback.

A trigger while muted still plays to the end and still reports completion: muting is the ear's business, not the sound's. Conflating the two makes `ST` read `IDLE` while the sound is still going.

The Speaker page and the Audio page are two pages.

With no ES8311 found, `playback_enabled` is false and the page does not enter the page loop (`DisplayPage::next_page` skips pages no capability backs); capture and every other page carry on. Making sound is an addition, not a precondition for capture.

## Where the sound comes from

The catalogue holds five sounds, and **each one is a MIDI file** (`bsp-esp/src/assets/*.mid`) in the same format: a Standard MIDI File, which any DAW can open, play and edit.

MIDI is not here for its synthesis abilities — it carries no timbre and no per-note envelope in either version — it is here for **provenance**. Stored as PCM blobs, the sounds are untraceable binaries with a recipe in a comment that the build does not run; stored as MIDI, anyone who opens a file can tell what it is and change it.

The layout is not a matter of taste — it was searched for under the earcon guidelines: **thirty-two semitones have to hold five sounds, and the table below is the only arrangement that fits.** Why these five numbers is in the [playback record](@/records/iot/playback.en.md#why-the-first-versions-five-sounds-did-not-sound-like-five).

| `Sound` | `SRC` | Score | Notes | Register | Length | Meaning |
| --- | --- | --- | --- | --- | --- | --- |
| `Rise` | `RISE` | D#4 137 ms / rest 25 / D#5 150 ms | 2 | lowest | 312 ms | ascent |
| `Error` | `ERROR` | A5 112 ms / rest 25 / E5 125 ms | 2 | low-middle | 262 ms | it failed |
| `Notice` | `NOTICE` | B5 200 ms | **1** | middle | 200 ms | something happened |
| `Tap` (the default) | `TAP` | D6 100 / rest 25 / E6 100 / rest 25 / F#6 100 | **3** | high-middle | 350 ms | confirmation |
| `Warn` | `WARN` | A6 87 ms ×4, 12 ms apart | **4** | highest | 384 ms | needs attention |

Neighbouring registers are **exactly three semitones apart**, no pitch is used by two sounds, and all five lengths differ. The only repetition is the two-note count, and it falls on `ERROR` and `RISE` because they are the one pair whose contours are opposite — falling against rising is the single difference the ear cannot get wrong.

`RISE`'s **bare octave** is the shape of Google's brand sound `Super G`, placed in the lowest register so it does not compete with `TAP`. The reasoning is in the [Playback Implementation Record](@/records/iot/playback.en.md#why-not-an-arpeggio).

Moving `TAP` from two notes to three is the largest concession in this version: it is both the default sound and the boot sound, and Google's rule is that **the more often an interaction happens, the less intrusive that sound should be** — while three notes run 30 ms longer than two. The reasoning is in the [playback record](@/records/iot/playback.en.md#why-tap-went-from-two-notes-to-three).

`D#4` is the note this speaker reproduces worst, so `RISE` is carried by its 2.76 partial at 765 Hz rather than by its fundamental. That one is **not measured** — it follows from how a small speaker rolls off, and it wants a listen on the board.

`Sweep` is a sixth sound but is **not in the tap cycle**: it is the bench excitation, generated from a band of frequencies rather than played from a file.

Parsing runs through `from_midi` inside `play()`, which is on the cooperative side and can allocate; the feed interrupt walks the parsed note table and never touches a byte of the file.

`from_midi` skips every event an earcon does not use — track names, program changes, controllers, sysex — rather than refusing the file. A DAW writes those by default, so refusing a file for carrying them would make the one authoring tool in use the one that does not work. Tracks are concatenated in file order: a piece split across tracks is one sound, not several, and reading them apart would silently play only the first.

### The ramp is inferred, not declared

MIDI has note on and note off, and no envelope — and a note starting or ending on a step edge is a click.

So `ramp_of(ms) = min(ms / 4, 40 ms)` is applied at both ends of every note and written into `Note` as `from_midi` reads the file. The cap matters: a proportional ramp spends three quarters of a three-second note fading rather than sounding.

This is a real retreat from "hand-declared" to "inferred by rule" — there is no way to say of one particular note that it wants a 40 ms release. The rule is pinned by `a_note_ramps_at_both_of_its_ends`.

### Timbre: one family, two inharmonic partials

`TIMBRE = [(100, 180), (276, 76)]`, a ratio in hundredths and a Q8 gain.

**Inharmonic on purpose**: a real bell's partials are not integer multiples of its fundamental, and integer multiples are exactly what makes a synthesised tone a buzzer — which is why the sound this replaced had to be stored as samples rather than generated.

**"The five share one timbre" was only half true.** The partial ratio is fixed at ×2.76, but it rides on the fundamental, so `RISE`'s partial sits at 765 Hz and `WARN`'s at 4858 Hz. The five spectra differ by an order of magnitude and already sound like five timbres rather than one. The docs used to call it one because "same partial ratio" got read as "same timbre" — which is also the good news: `WARN`'s 4858 Hz partial lands right on a small speaker's roll-off, and any higher would have forced the catalogue to give ground.

The gains **sum to exactly full scale** in Q8 (180 + 76 = 256), so two partials together cannot reach further than one alone. That cap is what stops a note whose partials happen to line up from pushing the sum past the rail.

Past three partials the arithmetic has to be redone: the per-sample cost grows with the partial count, and eight would eat the 5 ms cadence. Past that point the choice is a cached wavetable — which only accepts harmonic ratios, and so returns to the buzzer — or a longer interval.

`Tone::on(score, &PLAIN)` speaks with a single partial, and `a_lone_partial_is_the_oscillator_it_replaced` uses it to compare, sample for sample, against **the synthesis it replaced written out in the test**.

`interleave_mono` copies each sample into both slots. A mono source into a two-slot frame has to look stereo, or only half the frame is ours; a tail too short for a whole frame writes nothing — half a frame on the wire is a click.

`render_slice` does this in two steps: ask the source for a whole slice of mono frames, then widen it with `interleave_mono`. The fused version it replaced called in one frame at a time, re-entering the source 240 times a feed, and its interleaving was a hand-copy inside `audio_out` that no test could reach.

The peak is `TONE_PEAK_LSB = 23_000` (the 16-bit wire's own LSB), deliberately short of the rail: the level control is the DAC's volume register (`0x32`), and these 23 000 are only the headroom left to it.

Synthesis is fixed point too, like the [capture side](@/development/iot/audio.en.md), but for a different reason: metering chose fixed point, whereas synthesis was forced to.

`Tone::next_sample` runs in the feed interrupt, and an FPU instruction in that context raises `Cp0Disabled` — the board stops on the first chime frame, with no reset. The same code on the cooperative executor is fine, so this is a property of the interrupt context rather than of the arithmetic.

`Tone`'s phase is therefore a fixed-point index into a `WAVE_ENTRIES`-entry static table (8 fractional bits) and its envelope is Q16: `inv_ramp` is computed once per note, leaving each sample with integer multiplies and shifts alone.

The measured per-feed cost on the synthesis side is in the [Playback Implementation Record](@/records/iot/playback.en.md).

The table is built at compile time by `sine()` (a whole turn folded to a quarter, then Taylor), because `libm::sinf` is not a `const fn`.

Fidelity is pinned by tests, not by this comment: every entry and every interpolated result must land within one LSB of `sinf`, and the phase must stay inside the table.

## Parameters at a glance

| Label | Field | Unit | Meaning |
| --- | --- | --- | --- |
| `ST` | `playback.phase` | — | `IDLE` nothing playing / `PLAY` playing; only the driver pushes it back to `IDLE` |
| `SRC` | `playback.sound` | — | The sound the next tap will play, `TAP` / `NOTICE` / `WARN` / `ERROR` / `RISE`, cycled by a click |
| `MUTE` | `playback.muted` | — | Stamped only while latched, the same way `CLIP` appears: it is a state, not an event |
| `PLY` | `playback.plays` | times | How many sounds actually made it out |
| `DRP` | `playback.dropped` | times | Taps that landed on `PLAY` and were dropped |

`PLY` and `DRP` are two numbers because they answer two questions — "did it make a sound" and "did it hear me". A page that combined them into one tally could not tell a quiet speaker from a busy finger.

The Speaker page has no animation, and that is why it is not in `is_live_page()`: its phase flips twice per play, and both flips come from intents (so both bring a fresh diagnostics).

A page repainting 320×240 pixels every frame to say the same sentence is pure waste. The Audio page is the opposite — its sweep is always moving.

## One resident ring, 120 ms of runway

The board's playback driver (`virtual_components/audio_out.rs`) builds one **streaming ring** at bring-up and never touches it again: 24 descriptors of 960 bytes, 23 040 bytes in total = **120 ms of audio on the wire**. The DMA walks that ring from end to end, and each feed tops up **the slice the DMA has already played** rather than swapping in a fresh one.

So the "allocate a ring per arm" design is wrong here, and wrong twice over. On the TX side descriptors never come back from the DMA to the CPU: the writable byte count `I2s::write()` hands over is a one-shot budget with no path that returns the ring to you once the chain runs, so a second use of the same ring either hits `available_bytes == 0` or plays the previous round again. A streaming ring works precisely because it **tops up and never swaps**.

A descriptor's length — 960 bytes = 5 ms — is chosen by the app's cadence, not by the DMA. Dividing evenly means every feed lands on its own grid line instead of a beat late, and a beat late quantizes the moment playback starts, which plays a stored sound a few percent flat: exactly the kind of thing that sounds wrong with nothing to point at.

The playback task never waits for the DMA. A feed is a plain memory write; what waits is the DMA.

The 120 ms of runway is not an estimate, it is the ring's length — and being pushed late once is enough to drain it. That is why the ring is resident rather than allocated on demand.

Why the feed gets a preemptible executor of its own is that under cooperative scheduling "nothing else is running" is not a state you get to assume.

On the same cooperative executor, a capture fold, a panel write and input diagnostics each hold the CPU for stretches well beyond the 5 ms cadence, so a feed is eventually pushed late. The playback ring has 120 ms of runway, and being pushed late once is enough to drain it.

The per-task measurements, the warm-up soak, and the 158-second record taken after the move are in the [Playback Implementation Record](@/records/iot/playback.en.md).

With `feed_loop` moved to `FROM_CPU_INTR1` at `Priority2`, the same board in the same environment over 158 continuous seconds: cadence 199.6–202.9 kHz against a 200 kHz target, worst gap 6 ms in any window, zero gaps over 20 ms, zero ring drys, zero DMA restarts.

The worst gap uses 5% of the 120 ms of runway.

Capture still took 17 ms and the panel still took 16 ms throughout. They just can no longer postpone the cadence, which is the entire reason for the two executors.

`Priority2` is a deliberate ceiling: any higher and the feed would starve input and rendering in turn, becoming the thing everything else waits on.

`play()` does not cut what is already on the ring either: the step where two sounds meet is a click. The cost is up to one descriptor = 5 ms from tap to sound.

## Clocks: one MCLK, RX hung off the TX's clocks

Both codecs share a 12.288 MHz MCLK and 48 kHz (`MCLK_HZ = 12_288_000` in `components/audio_clock.rs`, with `const _: ()` pinning `MCLK_HZ / 256 == SAMPLE_RATE_HZ`).

Sharing is not "two sides happen to be configured the same" — one side genuinely takes its clocks from the other.

When the ES8311 is found the board builds the TDM with `TdmConfig::with_signal_loopback(true)`: TX drives BCLK/WS (`GPIO14`/`GPIO13`), and RX connects only its own DIN (`GPIO12`), hanging off the TX's clocks as a slave.

The name `signal_loopback` invites reading it as a data loopback, but it does not carry data — it only handles clocks.

TX and RX inside one I2S peripheral run on their own independent dividers, so sharing BCLK/WS requires putting RX into slave mode to follow TX.

In the esp-hal HIL test the data is looped back physically (via `dout.split()`, which cuts one physical pin into DOUT and DIN), which is precisely what shows that `signal_loopback` does not push playback samples into RX.

This was once written the wrong way round, and the evidence is kept here.

This section used to say `with_signal_loopback(false)` and assert that `signal_loopback(true)` "restarts the capture DMA over and over and, measured on the device, breaks capture".

That measurement was taken while RX still owned the BCLK/WS pins: `rx_slave_mod=1` then makes RX drive the pins while simultaneously being a slave, which is self-contradictory. What broke was the pin-ownership-plus-slave pairing, not loopback.

Either arrangement can only satisfy one codec:

| Configuration | TX | RX | Result (HPF corner walk, same environment) |
| --- | --- | --- | --- |
| Before | own divider | owns pins, self-driven | microphone −18…−28 dBFS (peak≠floor); speaker crackles |
| RX pins + `loopback=true` | slave | self-driven yet forced to slave | capture dead |
| TX pins + `loopback=false` | owns pins, self-driven | own divider | microphone constant −3 dBFS (peak==floor — a straight line); speaker clean |
| **TX pins + `loopback=true`** | **owns pins, self-driven** | **slave, following** | **microphone −33…−37 dBFS (peak≠floor); speaker clean** |

`signal_loopback` writes both `tx_conf.sig_loopback` and `rx_conf.rx_slave_mod` (esp-hal 1.2.0 `i2s/master/low_level/v3.rs`), so "shared clocks" and "slave mode" have to be enabled together — leave either one out and half the chain breaks.

With no ES8311 the board falls back to the capture's self-driven configuration: RX in slave mode with no clock source gets no samples at all, so that path must not be given up just to serve a board with no speaker.

Nothing guards this bit automatically, because `audio_out` sits behind the `audio-out` feature, which only `lckfb-szpi-esp32s3` enables.

`test-bsp` runs on the host and deliberately carries no esp-hal feature at all (esp-hal does not build for a host target), and `with_signal_loopback` / `signal_loopback` are not `const fn`, so `const _: () = assert!(…)` is unavailable too.

The comment above and the table are the only guard on this bit.

One GDMA channel serves both halves of RX and TX (`I2s::new` splits the channel and `split` gives the two halves), which is the same shape the SPI driver already uses bidirectionally, so the board stays on `DMA_CH1` and needs no second channel.

To be confirmed on the bench: `tx_stop()` only clears `TX_START` and does not shut the peripheral down, so BCLK/WS are expected to keep coming out between arms.

That is the foundation of the whole design and has to be measured on real silicon.

The alternative is a driver that keeps topping up silent arms (and an app that can no longer park), which costs a wake-up every 5 ms to refill a silence nobody will ever hear.

The stakes grew once TX became the clock master: the microphone now follows the TX's clocks as a slave, so if `tx_stop()` really drops BCLK it is not just the speaker that breaks — the microphone breaks with it.

The latest 60-second verification recorded zero playback drains, so this is still unmeasured.

## The ES8311 register rows

The register layout is transcribed from Espressif's `es8311_adf.c` in `esp-audio-dev`. The ES8311 datasheet is not distributed, so **the file does as it does, and so does this**: everywhere the meaning cannot be looked up, the value is kept as a mask rather than guessed.

| Stage | What is written | Why a mask and not a whole byte |
| --- | --- | --- |
| Power-on row | Defaults for `0x01`–`0x11`, including `0x03`/`0x04` = `0x10` | The reference driver writes whole bytes here too |
| Slave row | `0x00` reset with the master bit cleared, `0x01` = `0x3F` to open every clock gate | The master bit is the only field of `0x00` that is not a power-on value |
| Clock row | `0x02` mask `0x07`, `0x03`/`0x04` mask `0xF0`, `0x06` mask `0x1F`, `0x07` mask `0xC0` | The masks match the reference driver's `read & mask` one for one |
| DAC row | `0x09` frame format merged, analog power-up | The frame is a bit field, not a whole byte to overwrite |
| Start row | `0x09` masked `0x4F` to pin the 16-bit word width and clear the run bit; volume `0x32` = `0xBF`, `0x31` mask `0x60` to clear the mute | The word width is a bit field too: the reference driver writes it for every sample format, and a width left at its power-on value makes the DAC swallow 16-bit frames as another word length and stay silent |

One row of the clock table (`COEFF_48K`) is the `12288000/48000` row of `coeff_div[]`: pre 1, mult 1, adc_div 1, dac_div 1, fs 0, lrck `0x00:0xFF`, bclk 4, and an OSR of `0x10` on both converters.

A test in core pins `MCLK_HZ / (lrck_l + 1) == SAMPLE_RATE_HZ`, so a mistyped row fails at compile or test time instead of turning into "the sound is 4% slow".

Why `0x03`'s mask is `0xF0` and not `0x80`: when the reference driver writes `0x03` it first does `read & 0x80` to hold bit 7, and the very next line is `regv |= fs_mode << 6`. Bit 7 is inside that field, so the value it "held" is overwritten immediately.

`0xF0` covers the two fs bits and the OSR: every OSR in the reference table is `0x10` or `0x20`, so the low nibble is zero anyway.

Writing the byte outright therefore produces the same bytes as the reference driver while actually being able to change the OSR. With mask `0x80` a new OSR would be OR-ed onto the old one and could never be changed.

`the_row_lands_the_bytes_the_reference_driver_writes` pins this: it asserts the three bytes the reference driver writes (`0x10`/`0x10`/`0xC3`), not numbers that merely look plausible.

The address is `0x18`, with `0x19` tried if the probe fails; the chip ID is read from `0xFD`/`0xFE` and must be `0x83`/`0x11`.

The ID is read, not trusted: the address is the one thing on this bus that a board can get wrong silently, and a codec whose identity has not been verified can only be treated as "assumed working".

`PA_EN` is bit 1 of the PCA9557 output register (`Pca9557::set_output_bit` is a read-modify-write; the argument is the pin number, not a mask).

The output comes up at `0x05` with config `0xF8`; when `set_output(DVP_PWDN_BIT)` drops the LCD chip-select the baseline is `0x04`, and enabling the PA takes it to `0x06`.

The amplifier is not in `Speaker`: it is board wiring, not a driver contract.

## Tests

```bash
moon run iot:test        # core: MIDI parsing, additive synthesis, the state machine, UnwiredSpeaker
moon run iot:test-bsp    # board: the sound catalogue (what note each file holds), the ES8311 register rows (MockI2c)
moon run iot:smoke-host  # end to end: the catalogue, a drop, completion, mute
```

They are in two places, because they need different things.

The seven in `sounds.rs` under `moon run iot:test-bsp` walk the catalogue over `Sound::ALL` and assert of every sound that it opens on zero, ends near zero, does not reach the rail, is a one-shot of 100–400 ms, has no note under 82.5 ms, and ramps by the inferred rule. **They run against the real files**, so a file that will not parse, or that was edited into something the synthesiser cannot play, goes red here.

The six under `moon run iot:test-tool` are about **files and recipes**: that the `.mid` on disk is byte for byte what the recipes generate, that the directory holds no `.mid` no recipe claims, that the table's own shape rules hold (registers three semitones apart, lengths all different), and that the board's `include_bytes!` filenames match the recipes. **A sound edited in a DAW without the `RECIPES` table updated makes `moon run iot:test-tool` go red.**

The cross-layer invariant index is in [Audio Metering Semantics · Testing](@/development/iot/audio.en.md#testing) — one table covering the invariants shared by both sides, with the "deliberately break it, confirm red" column. The few unique to playback:

| Invariant | Test that holds it | Break verified |
| --- | --- | --- |
| every synth table entry is within 1 LSB of `sinf` | `the_table_holds_the_oscillator_it_replaced` | ✅ a 4 LSB table error turns it red |
| still within 1 LSB after interpolation | `the_interpolated_oscillator_tracks_sinf_within_an_lsb` | ✅ as above turns it red |
| the phase never leaves the table | `every_partial_stays_inside_the_table_it_indexes` | ✅ mismatched units turn it red |
| the envelope is capped by the ramp | `a_tap_opens_and_closes_on_silence` | ✅ dropping `min(ramp)` turns it red |
| a brief idle ring is not a drained stream | `a_brief_idle_between_feeds_is_not_a_drained_stream` | ✅ deciding straight off `tx_idle` turns it red |
| ES8311 needs a second write after power-up | `es8311`'s power-on value test | ✅ dropping the second write turns it red |
| a tap over a sounding one is dropped | `smoke-host`'s `dropped` tally | — |
| a board with no speaker reports `feed()` false always | `UnwiredSpeaker` | — |

The ES8311 board tests all run on `MockI2c`: the power-on values, the stored byte after a masked write, chip ID verification, the repeated write (the reference driver issues it twice, because the first write after power-up is occasionally lost), the slave address, and the mute bit's read-modify-write.

The board's register sequence being verifiable without hardware is what allowed this driver into the repository at all.

`UnwiredSpeaker` (core) is what a board with no speaker must report through `HasPlayback`: an absent driver does not mean the surface is absent, so `run()`'s one copy still runs on the C6 (via `Inline`) with `feed()` permanently false and `play()` silent.

A silent fallback would let the page offer a sound nobody can produce.

After the split into two tasks, the `smoke-host` case for "a tap over a sounding one is dropped" is the only assertion that catches a specific regression.

`feed()` reports "nothing to feed" whenever no sound is on it, and it is already running before `play()`. So the "finished" report it left stands in `done` and is read as the end of the new sound: the sound starts and is immediately judged finished, and the page returns to `Idle`.

The next trigger therefore plays on rather than dropping, seen as `played = [Notice, Tap]`, `dropped = 0`.

The `play()` that is accepted must therefore clear `done` while holding the lock: both halves in one critical section, so the feed cannot set it again in between.
