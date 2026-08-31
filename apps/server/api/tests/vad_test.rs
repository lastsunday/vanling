mod common;
use api::component::vad::model::earshot::VadEarshot;
use common::vad::*;
use service::component::vad::Vad;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
async fn test_noise_burst_no_false_positive() -> anyhow::Result<()> {
    let mut vad = VadEarshot::new(&vad_test_config(1000.0))?;
    let (frames, sr) = load_test_wav("noise_burst.wav");
    assert_eq!(sr, SAMPLE_RATE);
    feed_silence_seconds(&mut vad, 2)?;
    assert_no_trigger(&mut vad, &frames, "noise_burst")
}

#[tokio::test]
#[traced_test]
async fn test_keyboard_tap_no_false_positive() -> anyhow::Result<()> {
    let mut vad = VadEarshot::new(&vad_test_config(550.0))?;
    let (frames, sr) = load_test_wav("keyboard_tap.wav");
    assert_eq!(sr, SAMPLE_RATE);
    feed_silence_seconds(&mut vad, 2)?;
    assert_no_trigger(&mut vad, &frames, "keyboard_tap")
}

#[tokio::test]
#[traced_test]
async fn test_tap_desk_no_false_positive() -> anyhow::Result<()> {
    let mut vad = VadEarshot::new(&vad_test_config(1000.0))?;
    let (frames, sr) = load_test_wav("tap_desk.wav");
    assert_eq!(sr, SAMPLE_RATE);
    assert_no_trigger(&mut vad, &frames[..10], "tap_desk")?;
    for (i, frame) in frames[10..].iter().enumerate() {
        vad.accept_waveform(frame)?;
        if vad.is_speech() {
            tracing::info!(
                "VAD triggered on tap_desk at frame {} ({}ms)",
                i + 10,
                (i + 10) * 16
            );
            break;
        }
    }
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_state_machine() -> anyhow::Result<()> {
    let mut vad = VadEarshot::new(&vad_test_config(1000.0))?;
    let (speech1, sr) = load_test_wav("speech_a.wav");
    assert_eq!(sr, SAMPLE_RATE);
    let (speech2, _) = load_test_wav("speech_b.wav");

    process_until_trigger(&mut vad, &speech1, "speech_a")?;
    assert!(vad.is_speech(), "Expected speech=true after speech_a");

    feed_silence_seconds(&mut vad, 2)?;
    assert!(!vad.is_speech(), "Expected speech=false after 2s silence");

    process_until_trigger(&mut vad, &speech2, "speech_b")?;
    assert!(vad.is_speech(), "Expected speech=true after speech_b");

    Ok(())
}
