use api::{
    asr::AsrFactory,
    config::{
        AsrModel, LlmModel, TtsModel, VadModel, asr::AsrConfig, audio::AudioConfig, llm::LlmConfig,
        session::SessionConfig, tts::TtsConfig, vad::VadConfig,
    },
    llm::LlmFactory,
    mcp::mcp_host::UnionMcpHost,
    tts::TtsFactory,
    vad::VadFactory,
    ws::session::SessionBuilder,
    ws::session::listener::DefaultListener,
};
use framework::id::gen_id;
use service::{
    chobits::message::{
        hello::HelloMessage,
        listen::{ListenMessage, ListenMode, ListenState},
        tts::TtsState,
    },
    ws::frame::{Frame, FrameResult},
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_stream::StreamExt;
use tracing::info;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
/// Collect full TTS audio through complete session pipeline (Void VAD/ASR + Echo LLM + Matcha TTS)
async fn test_tts_audio_collect() -> anyhow::Result<()> {
    use std::path::Path;

    let audio_config = Arc::new(AudioConfig {
        input_sample_rate: Some(16000),
        input_frame_duration: Some(20_u64),
        input_channel: Some(1),
        output_sample_rate: Some(16000),
        output_channel: Some(1),
        output_frame_duration: Some(60_u64),
    });

    let ws_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let model_path = ws_root
        .join("data/tts/model/matcha/matcha-icefall-zh-en/")
        .to_string_lossy()
        .into_owned();

    let session_id = gen_id();
    let mut session = SessionBuilder::new()
        .with_listener(Box::new(DefaultListener::new(
            VadFactory::create_model(&Arc::new(VadConfig {
                model: Some(VadModel::Void),
                ..Default::default()
            })),
            Arc::new(Mutex::new(AsrFactory::create_model(&AsrConfig {
                model: Some(AsrModel::Void),
                ..Default::default()
            }))),
            audio_config.clone(),
        )))
        .with_id(session_id.clone())
        .with_model(Arc::new(LlmFactory::create_model(&LlmConfig {
            model: Some(LlmModel::Echo),
            ..Default::default()
        })))
        .with_tts(Arc::new(
            TtsFactory::create_model(
                &TtsConfig {
                    model: Some(TtsModel::MatchaTts),
                    path: Some(model_path),
                    options: Some(serde_json::json!({
                        "num_threads": 2,
                        "noise_scale": 0.667,
                        "length_scale": 1.0,
                        "speed": 1.0,
                        "debug": false,
                    })),
                    ..Default::default()
                },
                &audio_config,
            )
            .await
            .unwrap(),
        ))
        .with_mcp_host(Arc::new(Mutex::new(UnionMcpHost::new(Some(
            session_id.clone(),
        )))))
        .with_config(Arc::new(SessionConfig {
            close_connection_no_voice_time: Some(3000),
            silence_voice_timeout: Some(1200),
            system_prompt: Some(String::from(
                "你是一个助手，所有回答必须使用纯文本自然语言，禁止使用任何Markdown符号如#、-、*等。",
            )),
            max_prompt_len: Some(3000),
        }))
        .with_audio_config(audio_config.clone())
        .build();

    session.start().await?;
    let mut output = session.output_frame().await;

    session
        .accept_frame(&Frame::Hello(HelloMessage {
            ..Default::default()
        }))
        .await;
    assert!(matches!(
        output.next().await.unwrap().payload.unwrap(),
        FrameResult::HelloResult(..)
    ));

    let text = "对于有媒体报道称，“特朗普说，如果中国不在霍尔木兹海峡护航问题上提供协助，他将推迟访华”，林剑说，中方注意到美方已就媒体不实报道公开作出澄清，表示有关报道是完全错误的，强调访问与霍尔木兹海峡通航问题无关。";
    session
        .accept_frame(&Frame::Listen(ListenMessage {
            state: ListenState::Detect,
            mmod: Some(ListenMode::Manual),
            text: Some(text),
            ..Default::default()
        }))
        .await;

    let mut all_packets: Vec<Vec<u8>> = Vec::new();
    loop {
        let data = output.next().await.unwrap().payload.unwrap();
        match data {
            FrameResult::TTSResult(msg) => {
                if msg.state == Some(TtsState::Stop) {
                    break;
                }
            }
            FrameResult::AudioResult(audio) => {
                all_packets.push(audio.data);
            }
            _ => {}
        }
    }
    info!("collected {} opus packets", all_packets.len());

    session.stop().await;

    let mut decoder = opus::Decoder::new(16000, opus::Channels::Mono).unwrap();
    let mut decoded = Vec::new();
    for packet in &all_packets {
        let mut samples = vec![0f32; 960];
        if let Ok(len) = decoder.decode_float(packet, &mut samples, false) {
            decoded.extend_from_slice(&samples[..len]);
        }
    }
    info!("decoded {} PCM samples", decoded.len());

    assert!(!decoded.is_empty(), "no audio decoded");
    std::fs::create_dir_all("./test_data")?;
    wavers::write("./test_data/test_tts_collect_16k.wav", &decoded, 16000, 1)?;
    info!("saved test_data/test_tts_collect_16k.wav");
    Ok(())
}
