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
    ws::session::{SessionBuilder, listener::DefaultListener},
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
use std::{path::Path, sync::Arc};
use tokio::sync::Mutex;
use tracing::debug;
use tracing_test::traced_test;

use crate::session::helpers::get_audio;

#[tokio::test]
#[traced_test]
async fn test_asr_voice_input_manual() -> anyhow::Result<()> {
    let audio = get_audio();
    let audio_config = Arc::new(AudioConfig {
        output_sample_rate: Some(16000),
        output_channel: Some(1),
        output_frame_duration: Some(20_u64),
    });
    let session_id = gen_id();
    let (session, input_tx, mut output_rx) = SessionBuilder::new()
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
                ..Default::default()
            }))),
        )))
        .with_id(session_id.clone())
        .with_model(Arc::new(LlmFactory::create_model(&LlmConfig {
            model: Some(LlmModel::Echo),
            ..Default::default()
        })))
        .with_tts(Arc::new(
            TtsFactory::create_model(
                &TtsConfig {
                    model: Some(TtsModel::Mute),
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

    tokio::spawn(session.start());

    input_tx.send(Frame::Hello(HelloMessage {
        ..Default::default()
    }))?;
    assert!(matches!(
        output_rx.recv().await.unwrap().payload,
        FrameResult::HelloResult(..)
    ));

    input_tx.send(Frame::Listen(ListenMessage {
        state: ListenState::Start,
        mmod: Some(ListenMode::Manual),
        ..Default::default()
    }))?;

    for n in 0..audio.len() {
        input_tx.send(Frame::Voice {
            data: audio.get(n).unwrap().to_vec(),
        })?;
    }

    input_tx.send(Frame::Listen(ListenMessage {
        state: ListenState::Stop,
        mmod: Some(ListenMode::Manual),
        ..Default::default()
    }))?;

    let mut frames = Vec::new();
    loop {
        let frame = output_rx.recv().await.unwrap().payload;
        let is_stop =
            matches!(&frame, FrameResult::TTSResult(msg) if msg.state == Some(TtsState::Stop));
        frames.push(frame);
        if is_stop {
            break;
        }
    }

    let stt_text = frames
        .iter()
        .find_map(|f| {
            if let FrameResult::STTResult(msg) = f {
                msg.text.clone()
            } else {
                None
            }
        })
        .expect("STTResult not found");
    debug!("ASR: {stt_text}");
    assert_eq!(
        stt_text,
        "And so my fellow Americans ask not what your country can do for you, ask what you can do for your country.",
        "ASR transcription mismatch"
    );

    let echo_text = frames
        .iter()
        .find_map(|f| {
            if let FrameResult::TTSResult(msg) = f {
                if msg.state == Some(TtsState::SentenceStart) {
                    msg.text.clone()
                } else {
                    None
                }
            } else {
                None
            }
        })
        .expect("TTSResult(SentenceStart) not found");
    assert_eq!(echo_text, stt_text, "Echo should match STT exactly");
    debug!("Echo: {echo_text}");

    assert!(
        frames.iter().any(
            |f| matches!(f, FrameResult::TTSResult(msg) if msg.state == Some(TtsState::Start))
        ),
        "Missing TTSResult(Start)"
    );
    assert!(
        frames
            .iter()
            .any(|f| matches!(f, FrameResult::LLMResult(..))),
        "Missing LLMResult"
    );

    drop(input_tx);
    Ok(())
}
