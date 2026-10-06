+++
title = "Playback Semantics"
weight = 270
sort_by = "weight"
[extra]
source_file_hash = "a668539bef7d20a3c967b31c6a217d2bafc19a33"
translated_at = "2026-10-01T07:37:04Z"
+++

# Playback Semantics

The ES8311 is only one peripheral. What the page exposes is **the start and the end of a sound** — what is playing now, and whether it has finished — not a progress bar and not a volume. This page covers the contract, the panel readout, and the "why it is like this" behind every part of it.

This page describes what the system is now; how it became so, and what was rejected along the way, is in the [Playback Implementation Record](@/records/iot/playback.en.md) (AI-produced, not reviewed by a human).

The capture side is in [Audio Metering Semantics](@/development/iot/audio.en.md). The two pages share one MCLK but their contracts are opposite: capture **never stops**, while playback is **over when the one-shot is done** — which is why playback has a phase and capture does not.

> This page was written by AI and has not been reviewed by a human. It describes what the system **should be**; how the implementation became so, and what was rejected along the way, is in the [development records](@/records/_index.en.md).

## The contract

A sound is a **one-shot**: one tap plays one sound, it reports completion when it runs out, and there is no "pause" or "progress" in between. So the `Speaker` driver is three steps rather than one — `play()` starts it, `feed()` moves it along, and the driver says it is `done()` — and the loop that ties the three together is the app's playback module (`crates/app/src/playback.rs`), because only it knows the cadence; the board owns the ring, because only it knows the DMA.

That loop is **split into two tasks on two executors**, because a cadence and a blocking call are two things that cannot be mixed. `feed_loop` does nothing but the 5 ms cadence and one memory write, and is spawned onto the S3's `FROM_CPU_INTR1` at `Priority2`; `control_loop` handles commands, the I2C behind `play()`/`set_muted()`, and reporting `finished` back on the intent bus, and stays on the cooperative `FROM_CPU_INTR0`. The driver is shared as a `Box<dyn Speaker>`, so `play()` and `feed()` cannot tread on the arm the other is using — which is why `Speaker` demands `Send + 'static`, and why its error is the named `SpeakerFault` rather than each implementation's own type (a trait object cannot spell an unnamed associated type). The C6 and the host have no second software interrupt, so they take `SpeakerRunner::Inline` and the two loops are `join4`ed on one executor, with the same semantics.

`PlaybackFinished` travels back over the intent bus from the playback task; it is **not** a polled flag. The phase leaves `Playing` only when the driver itself says so: letting the page and the driver each tell their own story produces "it still looks like it is playing, but it finished ages ago", and every later tap is then dropped as a replay of a sound that had already ended.

A trigger that lands on `Playing` only increments `dropped`; it does **not** interrupt the current sound. Interrupting leaves a step between two stretches of audio — that is a click, and a click is much harder to explain than "that tap went unheard". The drop has to be visible, so it gets a readout of its own (`DRP`) instead of being quietly folded into another number.

`ToggleMute` latches the **output**; it does not stop playback. A trigger while muted still plays to the end and still reports completion: muting is the ear's business, not the sound's, and conflating the two makes `ST` read `IDLE` while the sound is still going.

The Speaker page and the Audio page are two pages. With no ES8311 found, `playback_enabled` is false and the page does not enter the page loop (`DisplayPage::next_page` skips pages no capability backs); capture and every other page carry on. Making sound is an addition, not a precondition for capture.

## Where the sound comes from

The catalogue holds two sounds, and they share one format: 48 kHz mono 16-bit, handed to the driver and widened into a two-slot frame. `interleave_mono` copies every sample into both slots — a mono source into a two-slot frame has to look stereo, or only half the frame would be ours — and a tail too short for a whole frame is simply not written, because **a partial frame on the wire is a click**.

- `Chime` (the default): synthesised as it plays by `Tone`. The `CHIME` score is 880 Hz for 120 ms, a 40 ms rest, then 1320 Hz for 180 ms — two notes and a gap, 340 ms all told, an answer you hear rather than a tune you sit through. Each end has a ramp into silence (12 ms and 40 ms): a note that starts or stops on a step edge clicks, and a click is louder than a bad note.
- `Asset`: a PCM blob in flash (`bsp-esp/src/assets/asset.pcm` — 325 ms, peaking at 18 000 LSB, ramped to zero at both ends with 45 ms of trailing silence), read straight out as 16-bit LE by `Pcm`, with **no** check of its rate or width — those are properties of the file that only its author knows, and guessing wrong shows up as "4% slow" rather than as an error. Its recipe lives in the `ASSET` comment in `audio_out.rs` (660 Hz for 120 ms then 880 Hz for 160 ms, each note summing partials at 1.0, 2.76 and 5.40 of its fundamental); there is no generator in the repository and the build does not run one, so changing the sound means re-synthesising from those numbers.
- The peak is `TONE_PEAK_LSB = 23_000` (the 16-bit wire's own LSB), **deliberately short of the rail**: the level control is the DAC's volume register (`0x32`), and these 23 000 are only the headroom left to it.

Synthesis is fixed point too, like the [capture side](@/development/iot/audio.en.md), but for a different reason: metering chose fixed point, whereas synthesis was forced to. `Tone::next_sample` runs in the feed interrupt, and an FPU instruction in that context raises `Cp0Disabled` — the board stops on the first chime frame, with no reset. The same code on the cooperative executor is fine, so this is a property of the interrupt context rather than of the arithmetic.

`Tone`'s phase is therefore a fixed-point index into a `WAVE_ENTRIES`-entry static table (8 fractional bits) and its envelope is Q16: `inv_ramp` is computed once per note, leaving each sample with integer multiplies and shifts alone. The measured per-feed cost on the synthesis side is in the [Playback Implementation Record](@/records/iot/playback.en.md).

The table is built at compile time by `sine()` (a whole turn folded to a quarter, then Taylor), because `libm::sinf` is not a `const fn`. **Fidelity is pinned by tests, not by this comment**: every entry and every interpolated result must land within one LSB of `sinf`, and the phase must stay inside the table.

## Parameters at a glance

| Label | Field | Unit | Meaning |
| --- | --- | --- | --- |
| `ST` | `playback.phase` | — | `IDLE` nothing playing / `PLAY` playing; only the driver pushes it back to `IDLE` |
| `SRC` | `playback.sound` | — | The sound the next tap will play, `CHIME` / `ASSET`, cycled by a click |
| `MUTE` | `playback.muted` | — | Stamped only while latched, the same way `CLIP` appears: it is a state, not an event |
| `PLY` | `playback.plays` | times | How many sounds actually made it out |
| `DRP` | `playback.dropped` | times | Taps that landed on `PLAY` and were dropped |

`PLY` and `DRP` are two numbers because they answer two questions — "did it make a sound" and "did it hear me". A page that combined them into one tally could not tell a quiet speaker from a busy finger.

The Speaker page has no animation, and that is why it is not in `is_live_page()`: its phase flips twice per play, and both flips come from intents (so both bring a fresh diagnostics), and a page repainting 320×240 pixels every frame to say the same sentence is pure waste. The Audio page is the opposite — its sweep is always moving.

## One 20 ms arm, not a resident ring

The board's playback driver (`virtual_components/audio_out.rs`) does not use a "4096-byte resident ring with a cursor" design, and that is forced by how `esp-hal` actually works:

**On the TX side, descriptors never come back from the DMA to the CPU.** The writable byte count `I2s::write()` hands over is a **one-shot** budget, and there is no path that returns the ring to you once the chain runs. So the second use of the same ring runs into one of two bad outcomes: either `available_bytes` hits zero and nothing more can be filled, or the previous round's contents play again. So every "arm" **allocates a fresh** 3840-byte ring (960 bytes × 4 = 960 frames = 20 ms), plays it once, waits for `is_done()`, and after `stop()` drops the ring. Allocation goes through `esp-hal`'s own internal memory pool, and dropping the ring returns the block.

An arm's length is chosen by the app's cadence, not by the DMA. The playback task feeds every 5 ms, and a 20 ms arm is exactly four of them: **dividing evenly** means every arm is re-armed on its own grid line instead of a beat late — and a beat late quantizes the moment playback starts, which plays a stored sound a few percent flat, exactly the kind of thing that sounds wrong with nothing to point at.

The playback task does **not** wait for an arm to finish. A feed is therefore only "is the last arm done", a plain memory write; what waits on the DMA is the DMA.

**Why the feed gets a preemptible executor of its own** is that under cooperative scheduling "nothing else is running" is not a state you get to assume. On the same cooperative executor, a capture fold, a panel write and input diagnostics each hold the CPU for stretches well beyond the 5 ms cadence, so a feed is eventually pushed late. The playback ring has 120 ms of runway, and being pushed late once is enough to drain an arm. The per-task measurements, the warm-up soak, and the 158-second record taken after the move are in the [Playback Implementation Record](@/records/iot/playback.en.md).

With `feed_loop` moved to `FROM_CPU_INTR1` at `Priority2`, the same board in the same environment over 158 continuous seconds: cadence 199.6–202.9 kHz against a 200 kHz target, **worst gap 6 ms in any window, zero gaps over 20 ms, zero ring drys, zero DMA restarts**. The worst gap uses 5% of the 120 ms of runway. Capture still took 17 ms and the panel still took 16 ms throughout — they just can no longer postpone the cadence, which is the entire reason for the two executors.

`Priority2` is a deliberate ceiling: any higher and the feed would starve input and rendering in turn, becoming the thing everything else waits on.

`play()` does not cut the arm already in flight either: the step where arms meet is a click. The cost is up to 20 ms from tap to sound.

## Clocks: one MCLK, RX hung off the TX's clocks

Both codecs share a 12.288 MHz MCLK and 48 kHz (`MCLK_HZ = 12_288_000` in `components/audio_clock.rs`, with `const _: ()` pinning `MCLK_HZ / 256 == SAMPLE_RATE_HZ`). Sharing is not "two sides happen to be configured the same" — one side genuinely takes its clocks from the other: when the ES8311 is found the board builds the TDM with `TdmConfig::with_signal_loopback(true)`, TX drives BCLK/WS (`GPIO14`/`GPIO13`), and RX connects only its own DIN (`GPIO12`), hanging off the TX's clocks as a slave.

The name `signal_loopback` invites reading it as a *data* loopback, but it does not carry data — it only handles clocks. TX and RX inside one I2S peripheral run on their own independent dividers, so sharing BCLK/WS requires putting RX into slave mode to follow TX. In the esp-hal HIL test the data is looped back *physically* (via `dout.split()`, which cuts one physical pin into DOUT and DIN), which is precisely what shows that `signal_loopback` does not push playback samples into RX.

**This was once written the wrong way round, and the evidence is kept here.** This section used to say `with_signal_loopback(false)` and assert that `signal_loopback(true)` "restarts the capture DMA over and over and, measured on the device, breaks capture". That measurement was taken while **RX still owned the BCLK/WS pins**: `rx_slave_mod=1` then makes RX drive the pins while simultaneously being a slave, which is self-contradictory — what broke was the pin-ownership-plus-slave pairing, not loopback. Either arrangement can only satisfy one codec:

| Configuration | TX | RX | Result (HPF corner walk, same environment) |
| --- | --- | --- | --- |
| Before | own divider | owns pins, self-driven | microphone −18…−28 dBFS (peak≠floor); speaker crackles |
| RX pins + `loopback=true` | slave | self-driven yet forced to slave | capture dead |
| TX pins + `loopback=false` | owns pins, self-driven | own divider | microphone constant −3 dBFS (peak==floor — a straight line); speaker clean |
| **TX pins + `loopback=true`** | **owns pins, self-driven** | **slave, following** | **microphone −33…−37 dBFS (peak≠floor); speaker clean** |

`signal_loopback` writes both `tx_conf.sig_loopback` and `rx_conf.rx_slave_mod` (esp-hal 1.2.0 `i2s/master/low_level/v3.rs`), so "shared clocks" and "slave mode" have to be enabled together — leave either one out and half the chain breaks. With no ES8311 the board falls back to the capture's self-driven configuration — RX in slave mode with no clock source gets no samples at all, so that path must not be given up just to serve a board with no speaker.

Nothing guards this bit automatically: `audio_out` sits behind the `audio-out` feature, which only `lckfb-szpi-esp32s3` enables, while `test-bsp` runs on the host and deliberately carries no esp-hal feature at all (esp-hal does not build for a host target); `with_signal_loopback` and `signal_loopback` are not `const fn` either, so `const _: () = assert!(…)` is unavailable too. The comment above and the table are the only guard on this bit.

One GDMA channel serves both halves of RX and TX (`I2s::new` splits the channel and `split` gives the two halves), which is the same shape the SPI driver already uses bidirectionally, so the board stays on `DMA_CH1` and needs no second channel.

**To be confirmed on the bench**: `tx_stop()` only clears `TX_START` and does not shut the peripheral down, so BCLK/WS are expected to keep coming out between arms. That is the foundation of the whole design and has to be measured on real silicon. The alternative is a driver that keeps topping up silent arms (and an app that can no longer park), which costs a wake-up every 5 ms to refill a silence nobody will ever hear. The stakes grew once TX became the clock master: the microphone now follows the TX's clocks as a slave, so if `tx_stop()` really drops BCLK it is not just the speaker that breaks — the microphone breaks with it. The latest 60-second verification recorded zero playback drains, so this is **still unmeasured**.

## The ES8311 register rows

The register layout is transcribed from Espressif's `es8311_adf.c` in `esp-audio-dev`. The ES8311 datasheet is not distributed, so **the file does as it does, and so does this**: everywhere the meaning cannot be looked up, the value is kept as a mask rather than guessed.

| Stage | What is written | Why a mask and not a whole byte |
| --- | --- | --- |
| Power-on row | Defaults for `0x01`–`0x11`, including `0x03`/`0x04` = `0x10` | The reference driver writes whole bytes here too |
| Slave row | `0x00` reset with the master bit cleared, `0x01` = `0x3F` to open every clock gate | The master bit is the only field of `0x00` that is not a power-on value |
| Clock row | `0x02` mask `0x07`, `0x03`/`0x04` mask `0xF0`, `0x06` mask `0x1F`, `0x07` mask `0xC0` | The masks match the reference driver's `read & mask` one for one |
| DAC row | `0x09` frame format merged, analog power-up | The frame is a bit field, not a whole byte to overwrite |
| Start row | `0x09` masked `0x4F` to pin the 16-bit word width and clear the run bit; volume `0x32` = `0xBF`, `0x31` mask `0x60` to clear the mute | The word width is a bit field too: the reference driver writes it for every sample format it configures, and a width left at its power-on value makes the DAC swallow 16-bit frames as another word length and stay silent |

One row of the clock table (`COEFF_48K`) is the `12288000/48000` row of `coeff_div[]`: pre 1, mult 1, adc_div 1, dac_div 1, fs 0, lrck `0x00:0xFF`, bclk 4, and an OSR of `0x10` on both converters. A test in core pins `MCLK_HZ / (lrck_l + 1) == SAMPLE_RATE_HZ`, so a mistyped row fails at compile or test time instead of turning into "the sound is 4% slow".

**Why `0x03`'s mask is `0xF0` and not `0x80`.** When the reference driver writes `0x03` it first does `read & 0x80` to hold bit 7, and the very next line is `regv |= fs_mode << 6` — bit 7 is inside that field, so the value it "held" is overwritten immediately. `0xF0` covers the two fs bits and the OSR: every OSR in the reference table is `0x10` or `0x20`, so the low nibble is zero anyway, and writing the byte outright produces the same bytes as the reference driver while **actually being able to change the OSR** — with mask `0x80` a new OSR would be OR-ed onto the old one and could never be changed. `the_row_lands_the_bytes_the_reference_driver_writes` pins this: it asserts the **three bytes the reference driver writes** (`0x10`/`0x10`/`0xC3`), not numbers that merely look plausible.

The address is `0x18`, with `0x19` tried if the probe fails; the chip ID is read from `0xFD`/`0xFE` and must be `0x83`/`0x11`. **The ID is read, not trusted**: the address is the one thing on this bus that a board can get wrong silently, and a codec whose identity has not been verified can only be treated as "assumed working".

`PA_EN` is bit 1 of the PCA9557 output register (`Pca9557::set_output_bit` is a read-modify-write; **the argument is the pin number, not a mask**). The output comes up at `0x05` with config `0xF8`; when `set_output(DVP_PWDN_BIT)` drops the LCD chip-select the baseline is `0x04`, and enabling the PA takes it to `0x06`. The amplifier is not in `Speaker`: it is board wiring, not a driver contract.

## Tests

```bash
moon run iot:test        # core: sound catalogue, state machine, UnwiredSpeaker, the synthesiser
moon run iot:test-bsp    # board: ES8311 register rows (MockI2c)
moon run iot:smoke-host  # end to end: both sounds, dropping, finishing, mute
```

The cross-layer invariant index is in [Audio Metering Semantics · Testing](@/development/iot/audio.en.md#testing) — one table covering the invariants shared by both sides, with the "deliberately break it, confirm red" column. The few unique to playback:

| Invariant | Test that holds it | Break verified |
| --- | --- | --- |
| every synth table entry is within 1 LSB of `sinf` | `the_table_holds_the_oscillator_it_replaced` | ✅ a 4 LSB table error turns it red |
| still within 1 LSB after interpolation | `the_interpolated_oscillator_tracks_sinf_within_an_lsb` | ✅ as above turns it red |
| the phase never leaves the table | `a_chime_stays_inside_the_table_it_indexes` | ✅ mismatched units turn it red |
| the envelope is capped by the ramp | `a_chime_opens_and_closes_on_silence` | ✅ dropping `min(ramp)` turns it red |
| a brief idle ring is not a drained stream | `a_brief_idle_between_feeds_is_not_a_drained_stream` | ✅ deciding straight off `tx_idle` turns it red |
| ES8311 needs a second write after power-up | `es8311`'s power-on value test | ✅ dropping the second write turns it red |
| a tap over a sounding one is dropped | `smoke-host`'s `dropped` tally | — |
| a board with no speaker reports `feed()` false always | `UnwiredSpeaker` | — |

The ES8311 board tests all run on `MockI2c`: the power-on values, the **stored byte** after a masked write, chip ID verification, the repeated write (the reference driver issues it twice, because the first write after power-up is occasionally lost), the slave address, and the read-modify-write of the mute bit. The board's register sequence being verifiable without hardware is what allowed this driver into the repository at all.

`UnwiredSpeaker` (core) is what a board with no speaker must report through `HasPlayback`: an absent driver does not mean the surface is absent, so `run()`'s one copy still runs on the C6 (via `Inline`) with `feed()` permanently false and `play()` silent — a silent fallback would let the page offer a sound nobody can produce.

After the split into two tasks, the `smoke-host` case for "a tap over a sounding one is dropped" is the only assertion that catches a specific regression: `feed()` reports "nothing to feed" whenever no sound is on it, and it is already running before `play()`, so the "finished" report it left stands in `done` and is read as the **end of the new sound** — the sound starts and is immediately judged finished, the page returns to `Idle`, and the next trigger therefore plays on rather than dropping (seen as `played = [Asset, Chime]`, `dropped = 0`). The `play()` that is accepted must therefore clear `done` while holding the lock: both halves in one critical section, so the feed cannot set it again in between.
