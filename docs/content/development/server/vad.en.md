+++
title = "VAD and Turn Detection"
weight = 203
sort_by = "weight"
[extra]
source_file_hash = "4eb40226df5b77db7394a81e93555eeff39e2da2"
translated_at = "2026-10-25T00:00:00Z"
+++

# VAD and Turn Detection

VAD decides which stretch of audio is speech; ASR decides when that speech is finished. They are two gates at different layers: VAD judges "speech ended", ASR judges "turn ended".

## Where it sits in the chain

Each dialogue turn is one chain, and VAD is its second link:

```text
AudioFrame → OpusDecodeNode → VadNode → AsrNode → TurnNode → LingNode → TtsNode
```

| Node | What it does | Key constraint |
| --- | --- | --- |
| `OpusDecodeNode` | Opus decode, rubato FFT resample to 16 kHz | Per-frame cap `MAX_OPUS_FRAME_MS = 120`; both VAD and ASR below hard-code 16 kHz input, and the chain has no second resampling point |
| `VadNode` | Slice frames at the model's window, keep a speech prefix, emit boundary events | A serial state machine, taken from the pool per turn |
| `AsrNode` | Feed the streaming recognizer, emit partials and final text | 200 ms silence confirmation (see below) |
| `TurnNode` | Close `TurnText` into `TurnComplete` | Boundary closure only, no judgement |

Events shared across the chain: `PcmFrame` / `SpeechStarted` / `SpeechEnded` / `PartialTranscript` / `TurnText` / `TurnComplete`. The control events `ListenMode` and `FinishTurn` travel the same stream.

## The Vad contract and the object pool

Defined in `service/src/component/vad.rs`:

```rust
pub trait Vad: Send + Sync {
    fn accept_waveform(&mut self, samples: &[f32]) -> Result<f32, AppError>;
    fn is_speech(&mut self) -> bool;
    fn clear(&mut self);
    fn window_size(&self) -> usize;
}
```

`accept_waveform` takes **exactly one `window_size`** window, returns a score and advances the state machine; `is_speech` reads the current state; `clear` drops state left over from the previous turn.

`VadPool` has only `acquire` / `release`. The pool is not there to save memory: **VAD is a stateful serial object**, so exactly one Round may hold it at a time and it cannot be shared like a stateless service. `acquire` always calls `clear()` first and `release` returns it; `VadNode` uses `ReleaseMode::Deferred` to return it at the end of the turn.

## Earshot criteria

`VadEarshot` (`api/src/component/vad/model/earshot/`) is the default model: embedded Silero VAD weights, pure-Rust inference, no ONNX runtime.

Processing runs in two stages:

1. Input is sliced into 160-sample (10 ms) frames, denoised through RNNoise and clamped to `[-1, 1]`;
2. Denoised samples accumulate into 256-sample (16 ms) windows, and each window gets a score and an RMS.

A window counts as "voiced" only when both hold:

```text
rms >= 0.003  &&  score >= threshold(0.4)
```

**Onset** (`is_speech` turning true) additionally requires all four:

| Condition | Default | Purpose |
| --- | --- | --- |
| Consecutive voiced frames | ≥ 3 | Reject single-frame spikes |
| Accumulated predictions | ≥ 5 | Reject transients that do not persist |
| Accumulated speech duration | ≥ `min_speech_duration` = 150 ms | A short breath is not a turn |
| Voiced share of the last 40 windows | ≥ 0.55 | Dithering window, so on/off chatter does not trigger |

While not in speech, more than 80 ms of consecutive silence clears the accumulated predictions and speech duration; gaps shorter than that do not, so speaking slowly does not keep restarting the count.

**Offset** uses hysteresis: during speech the "still active" bar drops to `deactivation_threshold` (0.2). Any active window resets the silence timer; only silence **beyond** `min_silence_duration` (550 ms) calls `clear_state()` and returns to non-speech.

`VadVoid` is the test double: `is_speech` always true, score always 1, window 512 samples (only so there is a value), used to bypass VAD decisions when testing downstream stages.

## VadNode behaviour

`VadNode` does not judge; it slices, buffers and translates events.

- **Slicing**: every `PcmFrame` is cut at `vad.window_size()`; a trailing partial window is emitted as-is rather than carried into the next frame.
- **Prefix buffer**: every slice enters `prefix_buffer`, capped at `PREFIX_SAMPLES_MAX = 9600` (about 600 ms at 16 kHz); over the cap the oldest samples are dropped. This compensates VAD's detection delay — by the time VAD says "speech", the speaker already started.
- **Event order**: each window's `PcmFrame` is passed downstream first, and `SpeechStarted` immediately **follows** the triggering slice. That way ASR's own prefix already contains the trigger window when it receives `SpeechStarted`, so the onset is not lost. `SpeechEnded` likewise follows the slice on a falling edge.
- **Release**: `ReleaseMode::Deferred`. `on_acquire` takes from the pool, `on_release` returns it, and `Drop` is the leak backstop; `stream()` may be called more than once, with state inside `Arc<Mutex<_>>`.
- **Errors**: if `accept_waveform` returns an error, remaining windows of the current `PcmFrame` are dropped and events already produced are still emitted.

## Two silence gates

| Gate | Judge | Threshold | Why this number |
| --- | --- | --- | --- |
| Speech end | Earshot | 550 ms | Hysteresis must be long enough that a normal breath or a drawn-out syllable is not cut |
| Turn end | `AsrNode` | 200 ms | VAD has already confirmed speech ended; this only confirms a little longer, so the user does not wait an extra 350 ms |

`AsrNode` confirms when all three hold:

```text
spoken && !speech_active && silence_samples >= 200ms × sample_rate / 1000
```

`spoken` is set by `SpeechStarted`, `speech_active` flips on `SpeechStarted` / `SpeechEnded`, and `silence_samples` accumulates the sample count of received `PcmFrame`s. On confirmation `finish_stream` runs: text yields `TurnText`, a stream with empty text yields `EmptyInput`, no stream yields `Nothing`.

In streaming mode partials are read every frame and `PartialTranscript` is emitted only when the text changes.

**Push-to-talk** (`ListenMode { streaming: false }`) separates the two actions: audio still feeds the recognizer frame by frame (warming the decoder), but partials and silence confirmation are suppressed, and text is produced only when `FinishTurn` arrives. A mid-sentence pause therefore cannot truncate a push-to-talk utterance.

`TurnNode` only rewrites `TurnText` into `TurnComplete` and passes everything else through; it uses `ReleaseMode::Immediate` and is released when the stream ends.

## Transport stall detection

Silence confirmation can only count samples that have **already arrived**. If the client stops sending audio mid-turn, `silence_samples` stops growing and the chain never wraps up, so the Session falls back to the wall clock: `check_transport_stall()` (`session/mod.rs`) feeds `FinishTurn` into the chain when `spoken` is still active and `last_audio_received.elapsed() >= silence_voice_timeout` (1200 ms).

Push-to-talk skips this check (it returns early when `Listening` and `is_voice_break_detect == false`) — the device's `listen(stop)` is the only turn boundary, and the server never "jumps in" on a silence timeout.

**Why silence confirmation avoids the wall clock**: tests and CI inject audio instantaneously (no real-time pacing), which decouples wall clock from audio-stream time and triggers early on slow machines. Counting samples makes the trigger depend only on decoded content, independent of consumption speed.

**Why transport stall needs the wall clock**: no samples arriving means no samples to count, and a stalled stream can only be noticed in time.

The layering matches OpenAI Realtime's `silence_duration_ms` (audio level) + `idle_timeout_ms` (wall-clock level).

## Turn-termination paths

| Path | Trigger | Raised by |
| --- | --- | --- |
| Silence confirm | streaming mode, ≥ 200 ms of silence | `AsrNode` calls `finish_stream()` itself |
| Transport stall | 1200 ms without new audio while `speaking` | Session feeds `FinishTurn` |
| Device key release | `listen(stop)` | Session feeds `FinishTurn` (the only path for manual) |

Interruption (`Abort` / a new `listen(start)`) is not in this table: it ends the whole Round. See "Interruption (BargeIn)" in [Core Architecture](@/development/server/architecture.en.md).

## Parameter reference

| Parameter | Value | Who reads it |
| --- | --- | --- |
| `threshold` | 0.4 | Earshot onset criterion |
| `deactivation_threshold` | 0.2 | Earshot offset hysteresis |
| `min_speech_duration` | 150 ms | Earshot onset: shorter is not a turn |
| `min_silence_duration` | 550 ms | Earshot offset: how much silence ends speech |
| `rms_threshold` | 0.003 | Earshot: energy gate before the score |
| Sliding ring | 40 windows × 0.55 | Earshot onset: voiced share of the last 640 ms |
| `SILENCE_CONFIRM_MS` | 200 ms | `AsrNode`: how long after speech ends to close the turn |
| `PREFIX_SAMPLES_MAX` | 9600 samples (~600 ms) | Prefix cap shared by `VadNode` and `AsrNode` |
| `VAD_WINDOW_SIZE` | 256 samples (16 ms) | `VadNode` slicing granularity |

## Tests and blind spots

`api/tests/vad_test.rs` has four tests, all on real wav files under `api/resources/test/`:

| Test | Assertion |
| --- | --- |
| `test_noise_burst_no_false_positive` | A noise burst does not trigger |
| `test_keyboard_tap_no_false_positive` | Keyboard taps do not trigger (`min_silence` 550 ms) |
| `test_tap_desk_no_false_positive` | Desk taps do not trigger (first 10 frames asserted first, so a long tail cannot hide it) |
| `test_state_machine` | Speech → 2 s silence → speech, `is_speech` flips correctly |

`api/tests/vad_analysis_test.rs` computes segment precision/recall on the TEN-vad annotation set; it is analysis, not a CI gate.

`api/tests/pipeline_nodes_test.rs` guards two `VadNode` contracts: `SpeechStarted` follows the triggering slice, and the prefix buffer is capped at 9600 samples before the trigger (asserted through the sample count the downstream ASR first receives).

The following has **no** automated coverage today; treat it as a blind spot when changing this area:

- pool reuse across concurrent turns (what happens if two turns hold one instance);
- the 200 ms silence confirmation at real device frame cadence (only synthetic frames are tested);
- the `Deferred` release path when the chain is cancelled early;
- the relationship between the real microphone front end's noise floor and `rms_threshold` (the threshold was tuned on sample files only).
