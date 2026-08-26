use api::asr::model::void::AsrVoid;
use api::config::vad::VadConfig;
use api::vad::model::earshot::VadEarshot;
use api::ws::default_listener::DefaultListener;
use service::ling::asr::Asr;
use service::ling::listener::{ListenInput, Listener, TurnOutput};
use service::ling::vad::Vad;

mod common;
use common::vad::*;

use std::sync::Arc;
use tracing_test::traced_test;

fn make_listener() -> DefaultListener {
    let vad = Box::new(VadEarshot::new(&VadConfig::default()).unwrap()) as Box<dyn Vad>;
    let asr: Arc<dyn Asr> = Arc::new(AsrVoid::new().unwrap());
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
// 1. Prefix buffer content is fed into ASR stream on first speech detection.
//
//   Feeds ~2s silence (prefix fills to 4800 samples max) then real speech.
//   Flush triggers stream finish and returns a TurnResult.
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
    assert!(result.is_some(), "flush should return a TurnResult");
    let turn = result.unwrap();
    assert_eq!("void", turn.text);
    assert!(
        turn.voice_data.is_empty(),
        "voice_data is empty in streaming mode"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// 2. [removed] Voice data accumulation test no longer applies in streaming mode.
//    Audio is fed directly to the ASR stream, not accumulated as voice_data.

// ---------------------------------------------------------------------------
// 3. Reset clears everything — state, prefix buffer, stream.
// ---------------------------------------------------------------------------
#[tokio::test]
#[traced_test]
async fn test_reset_clears_everything() -> anyhow::Result<()> {
    let mut listener = make_listener();

    let (speech_pcm, sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    assert_eq!(sr, 16000);
    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;
    let result = listener.flush().await;
    assert!(result.is_some(), "should have result before reset");

    listener.reset(None).await;
    let after = listener.flush().await;
    assert!(after.is_none(), "should be empty after reset");

    // After reset, new speech should produce a result
    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;
    let after_reset = listener.flush().await;
    assert!(
        after_reset.is_some(),
        "new speech should produce result after reset",
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
    let silence = vec![0.0f32; 16000];
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
    listener.reset(Some(100)).await; // 100ms audio-silence timeout

    let (speech_pcm, sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    assert_eq!(sr, 16000);
    feed_all(&mut listener, &encode_opus(&speech_pcm)).await;

    // +300ms silence crosses the threshold instantly.
    let silence = vec![0.0f32; 16000 * 3 / 10];
    feed_all(&mut listener, &encode_opus(&silence)).await;

    let outputs = listener.drain_outputs().await;
    assert!(
        outputs
            .iter()
            .any(|o| matches!(o, TurnOutput::TurnComplete(_))),
        "audio silence timeout should produce TurnComplete"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// 7. Turn finishing is driven by consumed audio time, not wall clock —
//    no finish while active speech is consumed, exactly one after the
//    clip's own decay. (Earshot ignores hard zeros after speech, hence
//    the full-clip feed.)
// ---------------------------------------------------------------------------
#[tokio::test]
#[traced_test]
async fn test_turn_finish_is_audio_time_driven() -> anyhow::Result<()> {
    let mut listener = make_listener();
    listener.reset(None).await;

    let (speech_pcm, sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    assert_eq!(sr, 16000);
    let packets = encode_opus(&speech_pcm);

    let speech_end_pkt = (2600 * 16000 / 1000) / 320;
    for pkt in &packets[..speech_end_pkt] {
        listener.accept(ListenInput::Audio(pkt.clone())).await;
        let outputs = listener.drain_outputs().await;
        assert!(
            !outputs
                .iter()
                .any(|o| matches!(o, TurnOutput::TurnComplete(_))),
            "turn must not finish while active speech is being consumed"
        );
    }

    // Rest of clip + margin: decay finishes the turn exactly once.
    let mut finished = 0;
    for pkt in &packets[speech_end_pkt..] {
        listener.accept(ListenInput::Audio(pkt.clone())).await;
        finished += listener
            .drain_outputs()
            .await
            .iter()
            .filter(|o| matches!(o, TurnOutput::TurnComplete(_)))
            .count();
    }
    let margin = vec![0.0f32; 16000 * 2 / 5];
    for pkt in encode_opus(&margin) {
        listener.accept(ListenInput::Audio(pkt)).await;
        finished += listener
            .drain_outputs()
            .await
            .iter()
            .filter(|o| matches!(o, TurnOutput::TurnComplete(_)))
            .count();
    }
    assert_eq!(
        finished, 1,
        "clip decay should finish the turn exactly once"
    );

    Ok(())
}
