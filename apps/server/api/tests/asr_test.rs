use api::{
    asr::AsrManager,
    config::{AsrModel, TtsModel, asr::AsrConfig, tts::TtsConfig},
    tts::TtsManager,
};
use tokio_stream::StreamExt;

use tracing::debug;
use tracing_test::traced_test;

mod common;
use common::asr::analyze_asr;
use common::tts::{TEST_TTS_TEXT, test_audio_config, tts_stream, ws_root};
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
async fn test_asr_model_paraformer_transcribe() {
    let model_path = ws_root()
        .join("data/asr/model/paraformer/default/")
        .to_string_lossy()
        .into_owned();

    let model = AsrManager::create_model(&AsrConfig {
        model: Some(AsrModel::Paraformer),
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
        "[PARAFORMER transcribe] {} | ASR 诊断: {diag}",
        result.text.trim()
    );
}

#[tokio::test]
#[traced_test]
async fn test_asr_model_paraformer_streaming() {
    let model_path = ws_root()
        .join("data/asr/model/paraformer/default/")
        .to_string_lossy()
        .into_owned();

    let model = AsrManager::create_model(&AsrConfig {
        model: Some(AsrModel::Paraformer),
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
        "[PARAFORMER stream] {} | partial={} last_partial=[{}] final=[{}] | ASR 诊断: {diag}",
        result.text.trim(),
        partial_count,
        last_partial.trim(),
        result.text.trim(),
    );
}

#[tokio::test]
#[traced_test]
async fn test_tts_asr_loopback() {
    let tts_path = ws_root()
        .join("data/tts/model/matcha/matcha-icefall-zh-en/")
        .to_string_lossy()
        .into_owned();

    let text = TEST_TTS_TEXT;

    let tts = TtsManager::create_model(
        &TtsConfig {
            model: Some(TtsModel::MatchaTts),
            path: Some(tts_path),
            options: Some(serde_json::json!({
                "num_threads": 2,
                "noise_scale": 0.667,
                "length_scale": 1.0,
                "speed": 1.0,
                "debug": false,
            })),
            ..Default::default()
        },
        &test_audio_config(),
    )
    .await
    .unwrap();

    let tts_start = std::time::Instant::now();
    let text_stream = tts_stream(text.to_string());
    let cancel = tokio_util::sync::CancellationToken::new();
    let mut tts_stream = tts.stream(Box::pin(text_stream), cancel).await;

    let mut all_pcm = Vec::new();
    let sample_rate = 16000i32;
    let decode_fs = 320;
    let mut decoder = ropus::Decoder::new(16000, ropus::Channels::Mono).unwrap();
    while let Some(data) = tts_stream.next().await {
        let data = data.unwrap();
        for packet in data.audio {
            let mut samples = vec![0f32; decode_fs];
            if let Ok(len) = decoder.decode_float(&packet, &mut samples, ropus::DecodeMode::Normal)
            {
                all_pcm.extend_from_slice(&samples[..len]);
            }
        }
    }
    let tts_elapsed = tts_start.elapsed();

    assert!(!all_pcm.is_empty(), "No PCM data generated by TTS");

    let model = AsrManager::create_model(&AsrConfig {
        model: Some(AsrModel::Void),
        ..Default::default()
    });

    let asr_start = std::time::Instant::now();
    let result = model
        .transcribe(sample_rate as u32, &all_pcm)
        .await
        .unwrap();
    let asr_elapsed = asr_start.elapsed();

    let audio_dur = all_pcm.len() as f64 / sample_rate as f64;
    let diag = analyze_asr(audio_dur, asr_elapsed, text, &result.text);

    debug!("TTS 生成: {:.2}s", tts_elapsed.as_secs_f64());
    debug!("ASR 诊断: {diag}");
}
