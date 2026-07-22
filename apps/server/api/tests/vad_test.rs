use api::config::vad::VadConfig;

mod common;
use api::vad::model::earshot::VadEarshot;
use common::vad::*;
use service::chobits::vad::Vad;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
async fn test_state_machine() -> anyhow::Result<()> {
    let config = VadConfig {
        threshold: Some(0.5),
        deactivation_threshold: Some(0.5),
        min_silence_duration: Some(1000.0),
        ..Default::default()
    };
    let mut vad = VadEarshot::new(&config)?;
    let (speech1, sr1) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    let (speech2, _sr2) = read_wav(&resource_path("speech_b.wav").to_string_lossy());
    assert_eq!(sr1, SAMPLE_RATE);

    // ── Phase 1: feed speech_a → should trigger speech state ──
    for chunk in speech1.chunks(WINDOW_SIZE) {
        let mut frame = chunk.to_vec();
        if frame.len() < WINDOW_SIZE {
            frame.resize(WINDOW_SIZE, 0.0);
        }
        vad.accept_waveform(&frame)?;
    }
    assert!(vad.is_speech(), "Expected speech=true after speech_a");

    // ── Phase 2: feed 2s of silence → should clear after min_silence_duration=1000ms ──
    let silence_frames_count = (2 * SAMPLE_RATE as usize) / WINDOW_SIZE;
    for _ in 0..silence_frames_count {
        vad.accept_waveform(&silence_frame())?;
    }
    assert!(!vad.is_speech(), "Expected speech=false after 2s silence");

    // ── Phase 3: feed speech_b → should detect new speech segment ──
    for chunk in speech2.chunks(WINDOW_SIZE) {
        let mut frame = chunk.to_vec();
        if frame.len() < WINDOW_SIZE {
            frame.resize(WINDOW_SIZE, 0.0);
        }
        vad.accept_waveform(&frame)?;
    }
    assert!(vad.is_speech(), "Expected speech=true after speech_b");

    Ok(())
}
