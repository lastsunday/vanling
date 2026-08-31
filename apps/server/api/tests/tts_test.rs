use std::sync::Arc;

use api::{
    component::tts::TtsManager,
    config::{TtsModel, audio::AudioConfig, tts::TtsConfig},
};
use tokio_stream::StreamExt;
use tracing::info;
use tracing_test::traced_test;

mod common;
use common::tts::*;

#[tokio::test]
#[traced_test]
async fn test_tts_default() -> anyhow::Result<()> {
    const ENCODE_SAMPLE_RATE: u32 = 16000;
    const MONO_20MS: usize = ENCODE_SAMPLE_RATE as usize * 20 / 1000;
    let size = MONO_20MS;
    TtsManager::init(
        Arc::new(TtsConfig {
            model: Some(TtsModel::Mute),
            ..Default::default()
        }),
        Arc::new(AudioConfig {
            ..Default::default()
        }),
    )
    .await?;
    let tts = TtsManager::global().default();
    let text_stream = tts_stream(String::from(TEST_TTS_TEXT));
    let cancel = tokio_util::sync::CancellationToken::new();
    let mut tts_stream = tts.stream(Box::pin(text_stream), cancel).await;

    let mut audio: Vec<Vec<u8>> = Vec::new();
    while let Some(data) = tts_stream.next().await {
        match data {
            Ok(data) => {
                info!("{:?}", data.text);
                audio.append(&mut data.audio.to_vec());
            }
            Err(e) => {
                panic!("{:?}", e);
            }
        }
    }
    let audio_len = audio.len();
    info!("audio len = {}", audio_len);

    let mut decoder = ropus::Decoder::new(ENCODE_SAMPLE_RATE, ropus::Channels::Mono).unwrap();
    let mut decode_data: Vec<f32> = Vec::new();
    for n in 0..audio_len {
        let mut samples = vec![0f32; size];
        let data = audio.get(n).unwrap();
        let len = decoder
            .decode_float(data, &mut samples, ropus::DecodeMode::Normal)
            .unwrap();
        decode_data.append(&mut samples[..len].to_vec());
    }

    info!("decode_data len = {}", decode_data.len());
    std::fs::create_dir_all("./test_data")?;
    let _ = wavers::write("./test_data/test_tts_default.wav", &decode_data, 16000, 1);
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_tts_mute() -> anyhow::Result<()> {
    TtsManager::init(
        Arc::new(TtsConfig {
            model: Some(TtsModel::Mute),
            ..Default::default()
        }),
        Arc::new(AudioConfig {
            ..Default::default()
        }),
    )
    .await?;
    let tts = TtsManager::global().default();
    let text_stream = tts_stream(String::from(TEST_TTS_TEXT));
    let cancel = tokio_util::sync::CancellationToken::new();
    let mut tts_stream = tts.stream(Box::pin(text_stream), cancel).await;
    let mut audio: Vec<Vec<u8>> = Vec::new();
    while let Some(data) = tts_stream.next().await {
        match data {
            Ok(data) => {
                assert_eq!(data.text.as_ref(), TEST_TTS_TEXT);
                audio.append(&mut data.audio.to_vec());
            }
            Err(e) => {
                panic!("{:?}", e);
            }
        }
    }
    let audio_len = audio.len();
    assert_eq!(0, audio_len);
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_tts_matcha_zh_en() -> anyhow::Result<()> {
    let path = ws_root()
        .join("data/tts/model/matcha/matcha-icefall-zh-en/")
        .to_string_lossy()
        .into_owned();
    run_tts_test(
        &TtsConfig {
            model: Some(TtsModel::MatchaTts),
            path: Some(path),
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
        "./test_data/test_tts_matcha_zh_en.wav",
    )
    .await
}
