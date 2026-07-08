use api::{
    asr::AsrFactory,
    config::{
        AsrModel, LlmModel, TtsModel, VadModel, asr::AsrConfig, audio::AudioConfig, llm::LlmConfig,
        tts::TtsConfig, vad::VadConfig,
    },
    tts::TtsFactory,
    vad::VadFactory,
    ws::default_listener::DefaultListener,
    {chii::ChiiCoreBuilder, llm::LlmFactory},
};
use framework::id::gen_id;
use service::chobits::{
    frame::{Frame, FrameResult},
    mcp::McpRegistry,
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
    let mcp_registry = Arc::new(Mutex::new(McpRegistry::new(Some(session_id.clone()))));

    let chii: Arc<dyn service::chobits::chii::Chii> = Arc::new(
        ChiiCoreBuilder::new()
            .with_session_id(Some(session_id.clone()))
            .with_model(LlmFactory::create_model(&LlmConfig {
                model: Some(LlmModel::Echo),
                ..Default::default()
            }))
            .with_mcp_registry(mcp_registry)
            .build(),
    );

    let tts: Arc<dyn service::chobits::tts::Tts> = Arc::from(
        TtsFactory::create_model(
            &TtsConfig {
                model: Some(TtsModel::MatchaTts),
                path: Some(model_path.clone()),
                ..Default::default()
            },
            &audio_config,
        )
        .await
        .unwrap(),
    );

    let session_ctx = SessionBuilder::new()
        .with_id(session_id.clone())
        .with_listener(Box::new(DefaultListener::new(
            VadFactory::create_model(&Arc::new(VadConfig {
                model: Some(VadModel::Earshot),
                ..Default::default()
            })),
            Arc::new(Mutex::new(AsrFactory::create_model(&AsrConfig {
                model: Some(AsrModel::Void),
                ..Default::default()
            }))),
        )))
        .with_chii(chii)
        .with_tts(tts)
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
    tokio::spawn(session_ctx.session.start());

    let hello = Frame::Hello(HelloMessage {
        session_id: Some(session_id.clone()),
        ..Default::default()
    });
    session_ctx.input_tx.send(hello).unwrap();

    // send the text
    session_ctx
        .input_tx
        .send(Frame::Input {
            text: "今天天气怎么样".to_string(),
        })
        .unwrap();

    // wait for TTS stop
    let mut output_rx = session_ctx.output_rx;
    let mut audio_frames: Vec<Vec<u8>> = Vec::new();
    let mut tts_text: String = String::new();
    loop {
        match output_rx.recv().await {
            Some(msg) => {
                info!("{:?}", msg.payload.to_string());
                match msg.payload {
                    FrameResult::HelloResult(_) => continue,
                    FrameResult::AudioResult(audio) => {
                        audio_frames.push(audio.data);
                    }
                    FrameResult::TTSResult(tts) => {
                        if let Some(text) = tts.text {
                            tts_text.push_str(&text);
                        }
                        if tts.state == Some(TtsState::Stop) {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            None => break,
        }
    }

    assert!(audio_frames.len() > 0, "no audio collected");
    info!(
        "TTS text: {:?}, audio frames: {}",
        tts_text,
        audio_frames.len()
    );
    Ok(())
}
