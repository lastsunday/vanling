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
use tracing::debug;
use tracing_test::traced_test;

use crate::session::helpers::get_audio;

#[tokio::test]
#[traced_test]
async fn test_asr_voice_input_manual() -> anyhow::Result<()> {
    let audio = get_audio();
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

    let audio_config = Arc::new(AudioConfig {
        output_sample_rate: Some(16000),
        output_channel: Some(1),
        output_frame_duration: Some(20_u64),
    });

    let tts: Arc<dyn service::chobits::tts::Tts> = Arc::from(
        TtsFactory::create_model(
            &TtsConfig {
                model: Some(TtsModel::Mute),
                ..Default::default()
            },
            &audio_config,
        )
        .await
        .unwrap(),
    );

    let (session, input_tx, output_rx) = SessionBuilder::new()
        .with_id(session_id.clone())
        .with_listener(Box::new(DefaultListener::new(
            VadFactory::create_model(&Arc::new(VadConfig {
                model: Some(VadModel::Earshot),
                ..Default::default()
            })),
            Arc::new(Mutex::new(AsrFactory::create_model(&AsrConfig {
                model: Some(AsrModel::SenseVoice),
                path: Some(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .parent()
                        .unwrap()
                        .parent()
                        .unwrap()
                        .parent()
                        .unwrap()
                        .join("data/asr/model/sense_voice/default/")
                        .to_string_lossy()
                        .into_owned(),
                ),
                variant: None,
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
            max_prompt_len: Some(6000),
        })
        .with_audio_config(ServiceAudioConfig {
            output_sample_rate: 16000,
            output_channel: 1,
            output_frame_duration: 20,
        })
        .build();
    tokio::spawn(session.start());

    let hello = Frame::Hello(HelloMessage {
        session_id: Some(session_id.clone()),
        ..Default::default()
    });
    input_tx.send(hello).unwrap();

    for packet in &audio {
        input_tx
            .send(Frame::Voice {
                data: packet.clone(),
            })
            .unwrap();
    }

    // give it some time to process
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

    // now send a listen stop since we're not using input-based timing
    input_tx.send(Frame::ListenStop).unwrap();

    // wait for up to n seconds for the first non-hello output
    let result = tokio::time::timeout(
        tokio::time::Duration::from_secs(30),
        collect_results(output_rx, vec!["hello".to_string()], 20),
    )
    .await;
    match result {
        Ok(outputs) => {
            debug!("outputs = {:?}", outputs);
            assert!(!outputs.is_empty(), "expected at least one output");
        }
        Err(_) => {
            panic!("Test timed out waiting for LLM result");
        }
    }

    Ok(())
}

async fn collect_results(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<service::chobits::frame::OutputMessage>,
    exclude: Vec<String>,
    max: usize,
) -> Vec<String> {
    let mut results = Vec::new();
    loop {
        match rx.recv().await {
            Some(msg) => {
                if exclude.contains(&"hello".to_string())
                    && matches!(msg.payload, FrameResult::HelloResult(_))
                {
                    continue;
                }
                results.push(msg.payload.to_string());
                if results.len() >= max {
                    break;
                }
                if matches!(msg.payload, FrameResult::TTSResult(ref tts) if tts.state == Some(TtsState::Stop))
                {
                    break;
                }
            }
            None => break,
        }
    }
    results
}
