use api::{
    component::asr::AsrManager,
    config::{AsrModel, asr::AsrConfig},
};

use tracing::info;
use tracing_test::traced_test;

mod common;
use common::asr::analyze_asr;
use common::tts::ws_root;
use common::vad::{read_wav, resource_path};

#[tokio::test]
#[traced_test]
async fn test_asr_model_void() {
    let model = AsrManager::create_model(&AsrConfig {
        model: Some(AsrModel::Void),
        ..Default::default()
    });
    let result = model.transcribe(16000, &[]).await.unwrap();
    assert_eq!("void", result.text);
    assert_eq!(1.0, result.prob);
}

#[tokio::test]
#[traced_test]
async fn test_asr_model_void_stream() {
    let model = AsrManager::create_model(&AsrConfig {
        model: Some(AsrModel::Void),
        ..Default::default()
    });
    let stream = model.create_stream();
    stream.accept_waveform(&[]);
    stream.decode();
    assert!(!stream.is_endpoint());
    assert!(stream.get_partial().is_none());
    let result = stream.finish().unwrap();
    assert_eq!("void", result.text);
    assert_eq!(1.0, result.prob);
}

#[tokio::test]
#[traced_test]
async fn test_asr_model_x_asr_transcribe() {
    let model_path = ws_root()
        .join("data/asr/model/x_asr/default/")
        .to_string_lossy()
        .into_owned();

    let model = AsrManager::create_model(&AsrConfig {
        model: Some(AsrModel::XAsr),
        path: Some(model_path),
        ..Default::default()
    });

    let (pcm, sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    assert_eq!(sr, 16000);

    let start = std::time::Instant::now();
    let result = model.transcribe(sr, &pcm).await.unwrap();
    let elapsed = start.elapsed();

    let audio_dur = pcm.len() as f64 / sr as f64;
    let diag = analyze_asr(audio_dur, elapsed, "", &result.text);

    assert!(
        !result.text.is_empty(),
        "Transcribe should return non-empty text"
    );
    assert!(result.prob > 0.0);
    tracing::info!(
        "[X-ASR transcribe] {} | ASR 诊断: {diag}",
        result.text.trim()
    );
}

#[tokio::test]
#[traced_test]
async fn test_asr_model_x_asr_streaming() {
    let model_path = ws_root()
        .join("data/asr/model/x_asr/default/")
        .to_string_lossy()
        .into_owned();

    let model = AsrManager::create_model(&AsrConfig {
        model: Some(AsrModel::XAsr),
        path: Some(model_path),
        ..Default::default()
    });

    let (pcm, sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    assert_eq!(sr, 16000);

    let stream = model.create_stream();
    let chunk_size = 1600_usize; // 100ms at 16kHz

    let start = std::time::Instant::now();
    let mut partial_count = 0;
    let mut last_partial = String::new();
    for chunk in pcm.chunks(chunk_size) {
        stream.accept_waveform(chunk);
        stream.decode();
        if let Some(partial) = stream.get_partial() {
            partial_count += 1;
            last_partial = partial;
        }
    }

    let result = stream.finish();
    let total_elapsed = start.elapsed();

    assert!(result.is_some(), "finish should return a result");
    let result = result.unwrap();
    let audio_dur = pcm.len() as f64 / sr as f64;
    let diag = analyze_asr(audio_dur, total_elapsed, "", &result.text);

    assert!(!result.text.is_empty(), "Final text should be non-empty");
    assert!(
        partial_count > 0,
        "Streaming should produce at least one partial result (got {partial_count})"
    );
    tracing::info!(
        "[X-ASR stream] {} | partial={} last_partial=[{}] final=[{}] | ASR 诊断: {diag}",
        result.text.trim(),
        partial_count,
        last_partial.trim(),
        result.text.trim(),
    );
}

#[tokio::test]
#[traced_test]
async fn test_asr_transcribe_short_audio() {
    let model_path = ws_root()
        .join("data/asr/model/x_asr/default/")
        .to_string_lossy()
        .into_owned();

    let model = AsrManager::create_model(&AsrConfig {
        model: Some(AsrModel::XAsr),
        path: Some(model_path),
        ..Default::default()
    });

    let (pcm, sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    assert_eq!(sr, 16000);

    let full_dur = pcm.len() as f64 / sr as f64;
    info!(
        "[short-audio] full audio: {:.2}s ({} samples)",
        full_dur,
        pcm.len()
    );

    let short_samples = sr as usize; // 1s
    let short_pcm = &pcm[..short_samples.min(pcm.len())];
    let short_dur = short_pcm.len() as f64 / sr as f64;
    info!(
        "[short-audio] trimmed audio: {:.2}s ({} samples)",
        short_dur,
        short_pcm.len()
    );

    let start = std::time::Instant::now();
    let r1 = model.transcribe(sr, short_pcm).await;
    let elapsed = start.elapsed();
    let text1 = r1.as_ref().map(|r| r.text.as_str()).unwrap_or("(error)");
    let diag1 = analyze_asr(short_dur, elapsed, "And so", text1);
    info!(
        "[short-audio] without padding: \"{}\" | {}",
        text1.trim(),
        diag1
    );

    let mut padded = short_pcm.to_vec();
    padded.extend_from_slice(&vec![0.0; sr as usize * 300 / 1000]);
    let padded_dur = padded.len() as f64 / sr as f64;
    let start = std::time::Instant::now();
    let r2 = model.transcribe(sr, &padded).await;
    let elapsed = start.elapsed();
    let text2 = r2.as_ref().map(|r| r.text.as_str()).unwrap_or("(error)");
    let diag2 = analyze_asr(padded_dur, elapsed, "And so", text2);
    info!(
        "[short-audio] with 300ms padding: \"{}\" | {}",
        text2.trim(),
        diag2
    );

    let start = std::time::Instant::now();
    let r3 = model.transcribe(sr, &pcm).await;
    let elapsed = start.elapsed();
    let text3 = r3.as_ref().map(|r| r.text.as_str()).unwrap_or("(error)");
    let diag3 = analyze_asr(full_dur, elapsed, "And so my fellow american", text3);
    info!("[short-audio] full audio: \"{}\" | {}", text3.trim(), diag3);
}
