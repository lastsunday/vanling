+++
title = "Audio Implementation Record"
weight = 40
sort_by = "weight"
[extra]
source_file_hash = "c17eb2ded6aabda7de2ca2c6b8b08d10f2541b56"
translated_at = "2026-10-18T00:00:00Z"
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

## One place may stop the capture chain, and dropping it kills the microphone

`[measured]` To reproduce: flash the default firmware, tap the screen fast enough to enter and leave the Camera page repeatedly (no `IOT_BOOT_PAGE` — the touch path is the real one), and read 220 s of serial. The tell in the failing run was `[INPUT] held … worst source: … capture N ms` going from 17-20ms to **0 ms** and staying there.

**`0 ms` is not "cheaper", it is "no ring to drain".** That run had 130 seconds with no `[AUDIO]` line at all, input occupancy dropping from 52% to 27%, and all 46 `[DEMAND]` lines saying `capture on` — the demand had not switched it off, the chain was simply gone.

The failure shape is worth writing down in full, because it is quiet and irreversible:

1. the watchdog decides the capture has gone quiet and fires a repair
2. the repair path **takes its own chain first**, then goes to arm — so arm finds nothing
3. the ring and the codec side are dropped together, with no log
4. from then on `sample()` returns early on "no chain", and the watchdog sits *below* that return, so the watchdog that should have noticed is never reached
5. so it answers every poll with silence, forever, having said nothing

Step 4 is the one that matters: **an early return hides the watchdog behind itself**, which is why "the chain is gone" presented as "everything is fine, there is just no sound".

The fix makes the invariant structural rather than conventional: **only `arm()` may stop a running chain.** `Chain<A>` holds both the on-the-wire half and the assembled-but-parked half, and its only emptying method, `take_to_arm()`, always stops what it takes. The type lives in `iot-core` (which already hosts the host-testable `CaptureWatchdog`, `CaptureBacklog` and `SampleStream`), because the type worth keeping is an I2S DMA transfer — precisely the thing that cannot be built off hardware — and `set_collecting` / `arm` / `restart` had **no tests at all** before this.

`[test]` `parking_and_arming_returns_the_same_chain`, `repairing_a_stalled_capture_never_drops_the_chain`, `a_capture_with_the_demand_off_is_parked_and_not_lost`, `a_re_arm_that_refuses_reports_itself_rather_than_going_quiet`, `parking_nothing_is_not_a_second_park` (`core/src/drivers/audio.rs`). The second of those fails against the code as it was.

`[reported]` Not reproduced on hardware after the fix: under the same gesture load, which was *heavier* (183 page switches against 46, 74 camera chain re-arms against 9), capture stayed at 12-20ms and never reached 0, and the watchdog never fired once — **so the fix itself is unverified on hardware**; all that has changed is that it can no longer drop a chain silently.

Also added: `sample()` now distinguishes "parked by demand" from "the chain is lost", logging the second once. From inside `sample()` the two are **identical** — neither has a running chain — and the only difference is whether something is still parked.

---

## The calibration walk hangs off a demand-gated poll

`[measured]` To reproduce: flash `IOT_DEMAND='audio=off'` and read 40 s of serial; then flash `IOT_BOOT_PAGE=camera IOT_DEMAND='motion=on:30000,audio=on:30000'` and read 40 s.

> Every "batched to 30 s" row here was measured under the **removed `Budget` mechanism** (`IOT_BUDGET=thrift`: `Secondary` → 30 s, `Primary` → 10 s). That layer was identical to writing the cadence directly, because this board's IMU has no FIFO, so it has been removed — see the [architecture document](@/development/iot/architecture.en.md#The_demand_model_(what_/_how_often_/_how_sensitive)). **The numbers are unchanged**, and each place gives the equivalent `IOT_DEMAND` spelling.

`[AUDIO] HPF corner walk` is a one-shot at boot (`HPF_CORNER_WALK`, off in production builds): it walks all 8 corner codes and returns to the working one. It is stepped from `sample()`, and `sample()` is a demand-gated poll. So the walk's **completion time is set by budget**:

| Build | audio demand | Walk should take | Measured corner reports |
|---|---|---|---|
| default | `Continuous@Secondary`, 20ms | 24s | one every 3s |
| Camera page + motion batched to 30 s | 30s | ~240s | 0 in 40s |
| `audio=off` | chain parked | never | 0 in 40s |

`[measured]` This explains an earlier observation nobody attributed: the `build-s3-thrift` log (`audio=off`) shows no corner reports after `capture stopped`. It was read as "not advancing" when in fact it **can never advance**.

Not fatal for a bounded one-shot field aid, but the coupling has to be written down rather than discovered in a log. **A calibration that must finish on time cannot live inside `sample()`** — that function is switched off by the demand.

---

## A Plane's Marginal Cost Moves With Whether Another Task Is Awake

`[measured]` To reproduce: from `apps/iot/`, build five firmware variants that differ only in `IOT_DEMAND`, flash each, and read 45 s of `[INPUT] held` (8 windows of 5 s, median).

| `IOT_DEMAND` | `[INPUT] held` | Less the floor | Yields |
|---|---|---|---|
| `motion=off,audio=off,display=off` | 10.87% | — | interaction floor |
| `audio=off,display=off` | 23.4% | 12.52% | motion alone |
| `audio=off,display=on-change` | 26.6% | 15.73% | motion with a task awake |
| `motion=off,display=off` | 34.0% | 23.13% | capture alone |
| default (`motion Continuous + audio Continuous + display Continuous`) | 49.4% | — | — |

Two conclusions:

**One — contention tracks whether another task is awake, not how much it does.** `display=on-change` and `display=off` sit far closer together than `OnChange` and `Continuous` do, because `Mode::Off` parks the render loop (`Activity::Idle`), so the panel stops waking the executor every tick; `OnChange` and `Continuous` both keep it waking and pay contention anyway. **Doing less is not waking less.**

**Two — so a fixed conversion factor cannot work.** `motion Continuous` alone is 12.52% and 15.73% with an awake task beside it, +26%, while `probe-demand-s3` measures 11.40% in isolation, which is a third figure again. Any budget shaped as `isolated figure × some factor` is wrong by more the more planes a scenario turns on, which is the only reason a budget exists.

There is also a trap in the method itself: differencing yields a **marginal** cost, and it depends on context. The 12.52% above was measured with the panel dark and the 15.73% with it awake — one configuration, two numbers. So a differenced row is not "what this plane costs" but "what this plane adds in this scenario", and cited as the former it stops being true as soon as the scenario changes. Take the lit reading (157_300), because the expensive context is the one that counts.

`[test]` (**removed with that model**, kept here as a record) `an_isolated_row_would_still_not_be_addable_to_a_device_row`, `the_row_is_the_device_reading_and_the_probe_only_found_the_floor`, `the_differenced_rows_land_near_the_whole_device_build_they_were_taken_from` (the `core/src/demand.rs` of the day) used to pin that the three units cannot be mixed; with the cost model gone the constraint is left as a requirement on the method, with no code keeping it.

## Throttling moves executor from one place to another, it does not save any

`[measured]` Batching only motion to 30 s (`IOT_DEMAND='motion=on:30000,audio=on:30000,display=on:10000'`; the old spelling was `IOT_BUDGET=thrift`), 8 windows of 5 s per build against the default:

| Build | `[INPUT] held` | `[DISPLAY] held` (per 2 s window) | Both sides |
|---|---|---|---|
| default | 2473/5000 = 49.5% | 923 ms = 45.3% (35 frames) | 94.8% |
| motion batched to 30 s | 1774/5000 = 35.5% (motion 30 s) | 1179 ms = 58.5% (45 frames) | 94.0% |

Motion gave up 14.0 points and the render loop took 13.2 of them, with the device total barely moving. Every frame is still 26 ms (10 ms render + a 16 ms whole-frame SPI write); what changed is the frame count, because a cooperative executor gives its time to whoever is awake and the panel's animation clock will draw more frames whenever it has CPU.

**So adding a latency ceiling in percent was the wrong shape**: the two percentages are what add up to the device, and they trade against each other. The two sides must be read separately, and the panel side is a residual that may not be added to the polled side — the sum exceeds 100% of a core.

## The capture plane cannot be batched, because a stretched poll stops the clock

`[measured]` With capture also stretched to 30 s (`IOT_DEMAND='motion=on:30000,audio=on:30000,display=on:10000'`):

```
[00:00:31.023] WARN - [AUDIO] capture ring 10842 B deep, the input loop is behind the codec
[00:01:31.023] INFO - [AUDIO] capture DMA had stopped, re-armed (1)
```

In 90 s the HPF corner walk advanced one step (the default build advances one every 3 s). The 60 ms ring fills and the I2S engine halts on its own, while the watchdog lives inside `sample()` and `stalled()` only fires when a poll found **no** bytes — so it only discovers it on the poll *after* the one that emptied the chain.

That is the same constraint the `audio=off` bug had, reached by two different roads: `Off` takes the chain down deliberately, and batching refuses to make it rarer. The fix is `InputSource::accepts_slower_poll()`, `true` by default and `false` from `Es7210Rx`, with `apply()` folding the plan's cadence back to the board's and printing what is in force. After the fix the same build prints `motion on 30000 ms, capture on 20 ms`, the corner walk is back to every 3 s, and there is no backlog and no re-arm.

`[test]` `a_plane_whose_driver_needs_its_poll_is_not_stretched` (`app/src/input.rs`) pins this.

## The poll side scales by cadence, and agrees with the device at both ends

> This section's **prediction** layer (`Price`/`PolledCadence`: turning a per-poll cost into a rate by the cadence) was removed whole with that model; what replaced it is `MAX_SINGLE_HOLD_MS`, the bound on a single tick (see the [architecture doc](@/development/iot/architecture.md#需求模型（要什么_/_多频繁_/_多敏感）)). The `[DEMAND]` poll-side column below comes from that build, and **today's firmware no longer prints it**; the `[INPUT] held` measurements are unaffected.

A cost row is a rate *at the plane's board cadence*, so a batched plane is cheaper. The premise of that scaling — a cadence changes how often this happens, not what one of them costs — is measured rather than assumed: `probe-demand-s3` reads **2281 µs per poll** at the board's 20 ms against **2282 µs** at 200 ms, 0.04% apart. The probe was changed to run each cell at its own cadence, printing MEASURED where a 20 s window allows it and DERIVED where it does not.

`[measured]` The scaled poll side against whole-device runs, 8 windows of 5 s per build:

| Build | `[DEMAND]` poll side | `[INPUT] held` measured | Error |
|---|---|---|---|
| default | 497,300 | 494,600 | **+0.6%** |
| motion batched to 30 s | 340,104 | 354,800 | **−4.1%** |

Before the scaling, the batched row reported 497,300 (motion counted at 20 ms) against a measured 354,800 — **40% high**.

`[test]` (**removed with that model**, kept here as a record) `the_cadence_scaled_poll_side_predicts_what_the_device_measures` (the `core/src/demand.rs` of the day) used to pin both ends with a 5% tolerance.

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

---

## The full derivation, measured comparison and method are not in this record

Only the conclusions and the code locations are kept here. The measurement tables, why the price is reported on two sides, why `Sensitivity` moves no cadence on this board, why the bound is a single hold rather than a percentage, and every reproduction build and command are in the [architecture document](@/development/iot/architecture.en.md).

Three code-side hard constraints are explained there as well: why `Cost::basis` is a field rather than a comment, why the panel row is a residual that cannot be added, and why the capture plane answers `false` for `InputSource::accepts_slower_poll()` (only `sample()` drains the ring, so throttling does not make a reading staler — it stops the I2S clock).

---

## The two HPF registers cannot be swapped by stage order

`[measured]` Exchanging the values of `0x22` (`REG_ADC12_FILTER_SET`, holding `0x2A`) and `0x23` (`REG_ADC12_FILTER_CLEAR`, holding `0x0A`) raised `PK` in a quiet room from about `-40` to about `-30` — a floor about 10 dB higher, the opposite of what a fix for residual offset would do.

The premise at the time was that the two registers addressed the first and second filter stages, though nothing public determines which stage comes first. The two values differ only in bit 5, and bit 5 carries no cutoff-frequency meaning, so the swap did not exchange stage order at all — it selected a different mode, and the bench says that mode is worse.

The driver is back on the byte order `esp-bsp` emits (the one with bit 5 cleared first, then the set one), and the registers are named after the bits they carry rather than after a stage.
