+++
title = "TODO Triage Record"
weight = 401
sort_by = "weight"
[extra]
source_file_hash = "7913ce62e5ec905e686041c05287342faba18ddd"
translated_at = "2026-10-25T00:00:00Z"
+++

<!-- doc-audience: ai -->

# TODO Triage Record

## The problem

`development/server/TODO.md` used to be the server's work entry point: one 86-row table with a priority column of its own. It was a second copy of the same axis as `roadmap/_index.md`:

- Both carried priority columns, and they disagreed; the roadmap says "priorities are assigned by humans" while the TODO never said where its came from.
- Location columns rot. Paths such as `api/src/ws/default_listener.rs` and `api/src/mcp/mcp_host.rs` no longer exist after the refactor, yet their rows stayed.
- Eighteen "suggested: new crate" entries sat in a table nobody adjudicated, which amounts to silent approval.

A document is not a state machine. What a backlog must track is independently changing state, so it belongs in something that reports that state automatically.

## Where it went

The 86 data rows fell into four classes:

| Class | Rows | Destination |
| --- | --- | --- |
| Unfinished | 73 | Still-valid items merged into the matching module tables in the [roadmap](@/roadmap/_index.en.md), 46 net new rows; the rest duplicated existing roadmap rows or had been solved by the architecture |
| Done | 11 | git log; the roadmap already had matching ✅ rows, so they were not copied again |
| Dropped | 2 | "Dropped conclusions" below |
| Suggested new crate | 18 | [Dependency candidates](@/discussions/dependency-candidates.en.md), marked "not approved" |

## Dropped conclusions

| Item | Why it was dropped |
| --- | --- |
| Hann-window crossfade against clicks (512 samples) | The input PCM is already continuous, so chunk boundaries carry no discontinuity; windowing only changes the energy envelope. Rejected 2026-07-23 |
| First-packet silence trim + 40 ms preroll | Incompatible with the two-stage streaming architecture: the first packet leaves as soon as a token arrives, while trimming requires seeing audio that comes later, which would delay the first packet to the end of the sentence. Rejected 2026-07-23 |

## Expired items that no longer hold

These rows would mislead if left in the TODO; they were checked and do not hold:

| Item | Finding |
| --- | --- |
| MCP authentication | Implemented. The `/mcp` route in `component/mcp/mod.rs` carries `get_auth_layer()`, and the security config adds per-principal rate limiting |
| Audio hot-path cloning | The old `default_listener.rs` is gone. `OpusDecodeNode` now decodes once per 20 ms and slices one `to_vec` per VAD window (60 per second), not the hot path the row described |
| Clock overflow | `service/src/session/` no longer uses `Local::now()` in timeout arithmetic |
| VAD at non-16 kHz | `OpusDecodeNode` resamples every configured rate to 16 kHz through a rubato FFT, and the chain has no second audio entry, so VAD cannot receive non-16 kHz input |
| MCP ABBA lock ordering | The old `api/src/mcp/mcp_host.rs` no longer exists and the current component takes no cross-instance locks; it is now recorded in the roadmap as "re-check before reintroducing multi-endpoint aggregation" rather than kept as a defect row |

## What did not become a test

The plan was to turn three items into `#[ignore]` tests; only one survived:

| Item | Disposition |
| --- | --- |
| Opus sample rate zero | Added `opus_configure_zero_sample_rate_does_not_panic` (`api/tests/pipeline_nodes_test.rs`, `#[ignore]`). `Configure(sample_rate = 0)` panics at the `unwrap` of `ropus::Decoder::new(0, …)`; the test asserts "must not panic", and removing `#[ignore]` once fixed shows it |
| VAD at non-16 kHz | Does not hold (see above); there is no failing path to assert |
| TTS tail hold buffer | A missing feature rather than a behavioural defect, kept as the roadmap row "Tail hold buffer"; a failing ignored test cannot say when it counts as fixed |

## Rules left behind

- **One priority authority for the backlog**: `roadmap/_index.md`. Agents start there; there is no second table.
- **Behavioural defects that can be asserted** are carried by `#[ignore = "reason"]` tests, so a refactor reports whether the bug is still there.
- **"Why it was abandoned"** goes into this section, not the roadmap.
- **A new dependency** goes through [Dependency candidates](@/discussions/dependency-candidates.en.md) and needs human approval before `Cargo.toml` changes.
