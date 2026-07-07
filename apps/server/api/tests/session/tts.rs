use api::{
    asr::AsrFactory,
    config::{
        AsrModel, LlmModel, TtsModel, VadModel, asr::AsrConfig, audio::AudioConfig, llm::LlmConfig,
        tts::TtsConfig, vad::VadConfig,
    },
    mcp::provider::McpProviderImpl,
    tts::TtsFactory,
    vad::VadFactory,
    ws::default_listener::DefaultListener,
    {chii::ChiiCoreBuilder, llm::LlmFactory},
};
use framework::id::gen_id;
use service::chobits::{
    frame::{Frame, FrameResult},
    message::{hello::HelloMessage, tts::TtsState},
    session::{
        AudioConfig as ServiceAudioConfig, SessionBuilder, SessionConfig as ServiceSessionConfig,
    },
};
use std::{path::Path, sync::Arc};
use tokio::sync::Mutex;
use tracing::info;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
/// Collect full TTS audio through complete session pipeline (Void VAD/ASR + Echo LLM + Matcha TTS)
async fn test_tts_audio_collect() -> anyhow::Result<()> {
    let audio_config = Arc::new(AudioConfig {
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
    let mcp_impl = McpProviderImpl::new(session_id.clone());
    let mcp_host = mcp_impl.mcp_host();
    let mcp_provider: Arc<Mutex<dyn service::chobits::mcp::Mcp>> = Arc::new(Mutex::new(mcp_impl));

    let chii: Arc<dyn service::chobits::chii::Chii> = Arc::new(
        ChiiCoreBuilder::new()
            .with_session_id(Some(session_id.clone()))
            .with_model(Arc::new(LlmFactory::create_model(&LlmConfig {
                model: Some(LlmModel::Echo),
                ..Default::default()
            })))
            .with_mcp_host(mcp_host)
            .build(),
    );

    let tts: Arc<dyn service::chobits::tts::Tts> = Arc::from(
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
    );

    let (session, input_tx, mut output_rx) = SessionBuilder::new()
        .with_id(session_id.clone())
        .with_listener(Box::new(DefaultListener::new(
            VadFactory::create_model(&Arc::new(VadConfig {
                model: Some(VadModel::Void),
                ..Default::default()
            })),
            Arc::new(Mutex::new(AsrFactory::create_model(&AsrConfig {
                model: Some(AsrModel::Void),
                ..Default::default()
            }))),
        )))
        .with_chii(chii)
        .with_tts(tts)
        .with_mcp(mcp_provider)
        .with_config(ServiceSessionConfig {
            close_connection_no_voice_time: Some(3000),
            silence_voice_timeout: Some(1200),
            system_prompt: Some(String::from(
                "你是一个助手，所有回答必须使用纯文本自然语言，禁止使用任何Markdown符号如#、-、*等。",
            )),
            max_prompt_len: Some(3000),
        })
        .with_audio_config(ServiceAudioConfig {
            output_sample_rate: 16000,
            output_channel: 1,
            output_frame_duration: 60,
        })
        .build();

    tokio::spawn(session.start());

    input_tx.send(Frame::Hello(HelloMessage {
        ..Default::default()
    }))?;
    assert!(matches!(
        output_rx.recv().await.unwrap().payload,
        FrameResult::HelloResult(..)
    ));

    let text = "对于有媒体报道称，“特朗普说，如果中国不在霍尔木兹海峡护航问题上提供协助，他将推迟访华”，林剑说，中方注意到美方已就媒体不实报道公开作出澄清，表示有关报道是完全错误的，强调访问与霍尔木兹海峡通航问题无关。";
    input_tx.send(Frame::Input {
        text: text.to_string(),
    })?;

    let mut all_packets: Vec<Vec<u8>> = Vec::new();
    loop {
        let data = output_rx.recv().await.unwrap().payload;
        match data {
            FrameResult::TTSResult(msg) if msg.state == Some(TtsState::Stop) => {
                break;
            }
            FrameResult::TTSResult(_) => {}
            FrameResult::AudioResult(audio) => {
                all_packets.push(audio.data);
            }
            _ => {}
        }
    }
    info!("collected {} opus packets", all_packets.len());

    drop(input_tx);

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
