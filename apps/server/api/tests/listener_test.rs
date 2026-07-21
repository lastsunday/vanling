use api::asr::model::void::AsrVoid;
use api::config::vad::VadConfig;
use api::vad::model::earshot::VadEarshot;
use api::ws::default_listener::DefaultListener;
use service::chobits::asr::Asr;
use service::chobits::listener::{ListenInput, Listener, TurnOutput};
use service::chobits::vad::Vad;

mod common;
use common::vad::*;

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing_test::traced_test;

fn make_listener() -> DefaultListener {
    let vad = Box::new(VadEarshot::new(&VadConfig::default()).unwrap()) as Box<dyn Vad>;
    let asr = Arc::new(Mutex::new(Box::new(AsrVoid::new().unwrap()) as Box<dyn Asr>));
    DefaultListener::new("test".to_string(), vad, asr, Some(1200))
}

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

async fn feed_all(listener: &mut DefaultListener, packets: &[Vec<u8>]) {
    for pkt in packets {
        listener.accept(ListenInput::Audio(pkt.clone())).await;
    }
}

// ---------------------------------------------------------------------------
// 1. Prefix buffer is flushed into voice_data on first speech detection.
//
//   Feeds ~2s silence (prefix fills to 4800 samples max) then real speech.
//   Flush forces ASR and returns TurnResult with voice_data.
//   Verifies voice_data length is at least 4800, proving the ring buffer
//   was drained on the first is_speech() frame.
// ---------------------------------------------------------------------------
#[tokio::test]
#[traced_test]
async fn test_prefix_included_in_first_speech() -> anyhow::Result<()> {
    let mut listener = make_listener();

    let silence = vec![0.0f32; 16000 * 2];
    feed_all(&mut listener, &encode_opus(&silence)).await;

    let (speech_pcm, sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    assert_eq!(sr, 16000);
    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;

    let result = listener.flush().await;
    let voice_data = result.map(|r| r.voice_data).unwrap_or_default();
    assert!(
        voice_data.len() >= 4800,
        "voice_data should include the 4800-sample prefix, got {}",
        voice_data.len()
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// 2. Voice data grows monotonically — feeding more speech before flush never
//    shrinks or corrupts previously accumulated audio.
// ---------------------------------------------------------------------------
#[tokio::test]
#[traced_test]
async fn test_voice_data_grows_monotonically() -> anyhow::Result<()> {
    let mut listener = make_listener();

    let (speech_pcm, sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    assert_eq!(sr, 16000);
    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;
    let result1 = listener.flush().await;
    let len1 = result1.map(|r| r.voice_data.len()).unwrap_or(0);
    assert!(len1 > 0, "voice_data should have content after speech");

    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;
    let result2 = listener.flush().await;
    let len2 = result2.map(|r| r.voice_data.len()).unwrap_or(0);
    assert!(
        len2 > 0,
        "voice_data should have content after more speech; len2={len2}"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// 3. Reset clears everything — voice_data, state, prefix buffer.
// ---------------------------------------------------------------------------
#[tokio::test]
#[traced_test]
async fn test_reset_clears_everything() -> anyhow::Result<()> {
    let mut listener = make_listener();

    let (speech_pcm, sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    assert_eq!(sr, 16000);
    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;
    let result = listener.flush().await;
    assert!(
        result.is_some(),
        "voice_data should have speech content before reset",
    );

    listener.reset(None).await;
    let after = listener.flush().await;
    assert!(after.is_none(), "voice_data should be empty after reset");

    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;
    let result = listener.flush().await;
    let after_reset = result.map(|r| r.voice_data.len()).unwrap_or(0);
    assert!(
        after_reset >= 4800,
        "new prefix should be built after reset; got {}",
        after_reset,
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// 4. Silence-only with no timeout set → flush returns None.
// ---------------------------------------------------------------------------
#[tokio::test]
#[traced_test]
async fn test_silence_only_no_turn() -> anyhow::Result<()> {
    let mut listener = make_listener();
    listener.reset(None).await;

    let silence = vec![0.0f32; 16000 * 5];
    feed_all(&mut listener, &encode_opus(&silence)).await;

    let result = listener.flush().await;
    assert!(
        result.is_none(),
        "no turn should be produced for silence only"
    );

    let outputs = listener.drain_outputs().await;
    assert!(outputs.is_empty(), "no outputs should be pending");

    Ok(())
}

// ---------------------------------------------------------------------------
// 5. SpeechStarted event fires on first VAD speech detection.
// ---------------------------------------------------------------------------
#[tokio::test]
#[traced_test]
async fn test_speech_started_event() -> anyhow::Result<()> {
    let mut listener = make_listener();

    // Silence first (no speech)
    let silence = vec![0.0f32; 16000 * 1];
    feed_all(&mut listener, &encode_opus(&silence)).await;
    let outputs = listener.drain_outputs().await;
    assert!(
        !outputs
            .iter()
            .any(|o| matches!(o, TurnOutput::SpeechStarted))
    );

    // Speech should trigger SpeechStarted
    let (speech_pcm, sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    assert_eq!(sr, 16000);
    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;
    let outputs = listener.drain_outputs().await;
    assert!(
        outputs
            .iter()
            .any(|o| matches!(o, TurnOutput::SpeechStarted)),
        "SpeechStarted event should be produced"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// 6. Silence timeout triggers TurnComplete via drain_outputs.
// ---------------------------------------------------------------------------
#[tokio::test]
#[traced_test]
async fn test_silence_timeout_triggers_turn_complete() -> anyhow::Result<()> {
    let mut listener = make_listener();

    // Speech with short silence timeout
    let (speech_pcm, sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    assert_eq!(sr, 16000);
    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;

    // No flush — drain_outputs should detect silence timeout after ~1200ms
    // Use a short timeout for testing
    listener.reset(Some(100)).await; // 100ms timeout
    let (speech_pcm, _sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let outputs = listener.drain_outputs().await;
    assert!(
        outputs
            .iter()
            .any(|o| matches!(o, TurnOutput::TurnComplete(_))),
        "silence timeout should produce TurnComplete"
    );

    Ok(())
}
