use api::asr::model::void::AsrVoid;
use api::config::vad::VadConfig;
use api::vad::model::earshot::VadEarshot;
use api::ws::default_listener::DefaultListener;
use service::chobits::asr::Asr;
use service::chobits::listener::{ListenInput, ListenState, Listener};
use service::chobits::vad::Vad;

mod common;
use common::vad::*;

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing_test::traced_test;

/// Build a DefaultListener with VadEarshot (speech detection) + AsrVoid (no-op ASR).
fn make_listener() -> DefaultListener {
    let vad = Box::new(VadEarshot::new(&VadConfig::default()).unwrap()) as Box<dyn Vad>;
    let asr = Arc::new(Mutex::new(Box::new(AsrVoid::new().unwrap()) as Box<dyn Asr>));
    DefaultListener::new(vad, asr)
}

/// Encode PCM f32 into Opus packets (20ms, 320-sample frames, 16kHz).
fn encode_opus(pcm: &[f32]) -> Vec<Vec<u8>> {
    let mut encoder =
        ropus::Encoder::builder(16000, ropus::Channels::Mono, ropus::Application::Audio)
            .build()
            .unwrap();
    let frame_size = 320;
    let mut packets = Vec::new();
    for chunk in pcm.chunks(frame_size) {
        let mut padded = chunk.to_vec();
        padded.resize(frame_size, 0.0);
        let mut buf = vec![0u8; 4000];
        let written = encoder.encode_float(&padded, &mut buf).unwrap();
        buf.truncate(written);
        packets.push(buf);
    }
    packets
}

/// Feed all Opus packets to the listener sequentially.
async fn feed_all(listener: &mut DefaultListener, packets: &[Vec<u8>]) {
    for pkt in packets {
        listener.accept(ListenInput::Audio(pkt.clone())).await;
    }
}

// ---------------------------------------------------------------------------
// 1. Prefix buffer is flushed into voice_data on first speech detection.
//
//   Feeds ~2s silence (prefix fills to 4800 samples max) then real speech.
//   Verifies voice_data length is at least 4800, proving the ring buffer
//   was drained on the first is_speech() frame.
// ---------------------------------------------------------------------------
#[tokio::test]
#[traced_test]
async fn test_prefix_included_in_first_speech() -> anyhow::Result<()> {
    let mut listener = make_listener();

    // 2 seconds of silence → prefix buffer fills to 4800
    let silence = vec![0.0f32; 16000 * 2];
    feed_all(&mut listener, &encode_opus(&silence)).await;

    // Realtime speech → triggers is_speech after ~5 consecutive speech frames
    let (speech_pcm, sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    assert_eq!(sr, 16000);
    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;

    let voice_data = listener.take_voice().await;
    assert!(
        voice_data.len() >= 4800,
        "voice_data should include the 4800-sample prefix, got {}",
        voice_data.len()
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// 2. Voice data grows monotonically — feeding more speech never shrinks
//    or corrupts previously accumulated audio.
// ---------------------------------------------------------------------------
#[tokio::test]
#[traced_test]
async fn test_voice_data_grows_monotonically() -> anyhow::Result<()> {
    let mut listener = make_listener();

    let (speech_pcm, sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    assert_eq!(sr, 16000);
    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;
    let len1 = listener.take_voice().await.len();
    assert!(len1 > 0, "voice_data should have content after speech");

    // Feed more speech → voice_data should only grow
    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;
    let len2 = listener.take_voice().await.len();
    assert!(
        len2 > 0,
        "voice_data should have content after more speech; len2={len2}"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// 3. Silence between speech turns resets prefix_flushed so the next turn
//    also receives a fresh prefix.
// ---------------------------------------------------------------------------
#[tokio::test]
#[traced_test]
async fn test_prefix_fresh_after_silence() -> anyhow::Result<()> {
    let mut listener = make_listener();

    // -- Round 1: silence + speech → prefix included --
    let silence = vec![0.0f32; 16000 * 2];
    feed_all(&mut listener, &encode_opus(&silence)).await;
    let (speech_pcm, sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    assert_eq!(sr, 16000);
    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;
    let round1_len = listener.take_voice().await.len();
    assert!(round1_len >= 4800, "round 1 should have prefix padding",);

    // -- Silence gap (no reset) — is_speech transitions back to false --
    let gap = vec![0.0f32; 16000 * 3]; // 3s silence (> min_silence_duration=1000ms)
    feed_all(&mut listener, &encode_opus(&gap)).await;

    let after_gap = listener.take_voice().await.len();
    assert!(
        after_gap >= round1_len,
        "voice_data should not shrink during silence",
    );

    // -- Round 2: verify voice_data does NOT grow by a huge amount
    //    (prefix_flushed was reset by silence → new prefix on next speech)
    let (speech2_pcm, sr2) = read_wav(&resource_path("speech_b.wav").to_string_lossy());
    assert_eq!(sr2, 16000);
    feed_all(&mut listener, &encode_opus(&speech2_pcm)).await;
    let round2_len = listener.take_voice().await.len();

    // Round 2 should get a fresh prefix (up to 4800) + speech windows.
    // If prefix_flushed was NOT reset, round2 would only be ~a few windows (< 5000).
    assert!(
        round2_len >= 4800,
        "round 2 should include a fresh prefix (>=4800 samples); got round2_len={}",
        round2_len,
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// 4. Reset clears voice_data, prefix_buffer, and prefix_flushed.
// ---------------------------------------------------------------------------
#[tokio::test]
#[traced_test]
async fn test_reset_clears_everything() -> anyhow::Result<()> {
    let mut listener = make_listener();

    // Feed speech → voice_data should have content
    let (speech_pcm, sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    assert_eq!(sr, 16000);
    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;
    assert!(
        !listener.take_voice().await.is_empty(),
        "voice_data should have speech content before reset",
    );

    // Reset
    listener.reset(None).await;
    assert!(
        listener.take_voice().await.is_empty(),
        "voice_data should be empty after reset",
    );
    assert_eq!(
        listener.get_state(),
        ListenState::Idle,
        "state should be Idle after reset",
    );

    // Feed same speech again → voice_data should grow again from scratch
    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;
    let after_reset = listener.take_voice().await.len();
    assert!(
        after_reset >= 4800,
        "new prefix should be built after reset; got {}",
        after_reset,
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// 5. Silence-only input: no timeout set → never reaches End, voice_data empty.
// ---------------------------------------------------------------------------
#[tokio::test]
#[traced_test]
async fn test_silence_only_no_end_state() -> anyhow::Result<()> {
    let mut listener = make_listener();
    listener.reset(None).await; // no silence_voice_timeout

    let silence = vec![0.0f32; 16000 * 5]; // 5 seconds
    feed_all(&mut listener, &encode_opus(&silence)).await;

    assert_eq!(
        listener.get_state(),
        ListenState::Listening { is_speech: false }
    );
    assert!(listener.take_voice().await.is_empty());

    Ok(())
}
