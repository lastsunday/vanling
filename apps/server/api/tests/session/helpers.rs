use api::{
    AppState,
    asr::AsrManager,
    config::{
        AsrModel, LlmProvider, TtsModel, VadModel, asr::AsrConfig, audio::AudioConfig,
        llm::LlmConfig, tts::TtsConfig, vad::VadConfig,
    },
    mcp::client::external::ExternalMcpClient,
    setup_mcp,
    tts::TtsManager,
    vad::VadManager,
    ws::default_listener::DefaultListener,
    {chii::ChiiCoreBuilder, llm::LlmManager},
};
use framework::id::gen_id;
use rmcp::transport::{
    StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
};
use service::chobits::{
    frame::{Frame, FrameResult, OutputMessage},
    mcp::McpRegistry,
    message::tts::{TtsMessage, TtsState},
    session::{
        AudioConfig as ServiceAudioConfig, SessionBuilder, SessionConfig as ServiceSessionConfig,
    },
};

use futures::StreamExt;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

/// Absolute workspace root, derived from CARGO_MANIFEST_DIR (compile-time constant).
/// CARGO_MANIFEST_DIR = <root>/apps/server/api → 3 `.parent()` calls = root.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use tokio::sync::{Mutex, mpsc};
use utoipa_axum::router::OpenApiRouter;

use crate::common::{router_client::RouterClient, setup_database, tts::tts_stream};

/// Full session pipeline test at 16000Hz output.
/// Uses Void VAD/ASR + Echo LLM + Matcha TTS.
pub async fn create_session() -> Result<
    (
        service::chobits::session::Session,
        mpsc::UnboundedSender<Frame>,
        mpsc::UnboundedReceiver<OutputMessage>,
        Option<ContainerAsync<Postgres>>,
        AppState,
    ),
    anyhow::Error,
> {
    let (container, state) = setup_database().await;
    // server client
    let router = OpenApiRouter::new();
    let ct = tokio_util::sync::CancellationToken::new();
    let router = setup_mcp(router, state.clone(), ct.child_token())
        .split_for_parts()
        .0;
    let mcp_config = StreamableHttpClientTransportConfig::with_uri("/mcp");
    let client = RouterClient { router };
    let transport = StreamableHttpClientTransport::with_client(client, mcp_config);
    let mut external_client = ExternalMcpClient::new(transport).await?;
    external_client.init().await?;
    let session_id = gen_id();
    let mcp_registry = Arc::new(Mutex::new(McpRegistry::new(Some(session_id.clone()))));
    mcp_registry
        .lock()
        .await
        .add_client(Arc::new(external_client))
        .await;

    let audio_config = Arc::new(AudioConfig {
        output_sample_rate: Some(16000),
        output_channel: Some(1),
        output_frame_duration: Some(20_u64),
    });

    let chii: Arc<dyn service::chobits::chii::Chii> = Arc::new(
        ChiiCoreBuilder::new()
            .with_session_id(Some(session_id.clone()))
            .with_model(LlmManager::create_model(&LlmConfig {
                provider: Some(LlmProvider::LocalEcho),
                ..Default::default()
            }))
            .with_mcp_registry(mcp_registry)
            .build(),
    );

    let tts: Arc<dyn service::chobits::tts::Tts> = Arc::from(
        TtsManager::create_model(
            &TtsConfig {
                model: Some(TtsModel::MatchaTts),
                path: Some(
                    workspace_root()
                        .join("data/tts/model/matcha/matcha-icefall-zh-en/")
                        .to_string_lossy()
                        .into_owned(),
                ),
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
            session_id.clone(),
            VadManager::create_model(&Arc::new(VadConfig {
                model: Some(VadModel::Earshot),
                ..Default::default()
            })),
            Arc::new(Mutex::new(AsrManager::create_model(&AsrConfig {
                model: Some(AsrModel::SenseVoice),
                path: Some(
                    workspace_root()
                        .join("data/asr/model/sense_voice/default/")
                        .to_string_lossy()
                        .into_owned(),
                ),
                variant: None,
            }))),
            Some(1200),
        )))
        .with_chii(chii)
        .with_tts(tts)
        .with_config(ServiceSessionConfig {
            close_connection_no_activity_time: Some(3000),
            silence_voice_timeout: Some(1200),
            system_prompt: Some(String::from(
                "你是一个助手，所有回答必须使用纯文本自然语言，禁止使用任何Markdown符号如#、-、*等。",
            )),
            max_prompt_len: Some(6000),
            barge_in_lockout_ms: Some(250),
        })
        .with_audio_config(ServiceAudioConfig {
            output_sample_rate: 16000,
            output_channel: 1,
            output_frame_duration: 20,
        })
        .build();
    Ok((
        session_ctx.session,
        session_ctx.input_tx,
        session_ctx.output_rx,
        container,
        state,
    ))
}

pub async fn create_mini_session_channel() -> (
    mpsc::UnboundedSender<Frame>,
    mpsc::UnboundedReceiver<OutputMessage>,
) {
    create_mini_session_with_timeout(3000).await
}

pub async fn create_mini_session_with_timeout(
    close_connection_no_activity_time_ms: i64,
) -> (
    mpsc::UnboundedSender<Frame>,
    mpsc::UnboundedReceiver<OutputMessage>,
) {
    let session_id = gen_id();
    let mcp_registry = Arc::new(Mutex::new(McpRegistry::new(Some(session_id.clone()))));

    let chii: Arc<dyn service::chobits::chii::Chii> = Arc::new(
        ChiiCoreBuilder::new()
            .with_session_id(Some(session_id.clone()))
            .with_model(LlmManager::create_model(&LlmConfig {
                provider: Some(LlmProvider::LocalEcho),
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
        TtsManager::create_model(
            &TtsConfig {
                model: Some(TtsModel::Mute),
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
            session_id.clone(),
            VadManager::create_model(&Arc::new(VadConfig {
                model: Some(VadModel::Earshot),
                ..Default::default()
            })),
            Arc::new(Mutex::new(AsrManager::create_model(&AsrConfig {
                model: Some(AsrModel::Void),
                ..Default::default()
            }))),
            Some(1200),
        )))
        .with_chii(chii)
        .with_tts(tts)
        .with_config(ServiceSessionConfig {
            close_connection_no_activity_time: Some(close_connection_no_activity_time_ms),
            silence_voice_timeout: Some(1200),
            system_prompt: Some(String::from(
                "你是一个助手，所有回答必须使用纯文本自然语言，禁止使用任何Markdown符号如#、-、*等。",
            )),
            max_prompt_len: Some(3000),
            barge_in_lockout_ms: Some(250),
        })
        .with_audio_config(ServiceAudioConfig {
            output_sample_rate: 16000,
            output_channel: 1,
            output_frame_duration: 20,
        })
        .build();
    tokio::spawn(session_ctx.session.start());
    (session_ctx.input_tx, session_ctx.output_rx)
}

pub async fn create_session_channel() -> (
    mpsc::UnboundedSender<Frame>,
    mpsc::UnboundedReceiver<OutputMessage>,
    Option<ContainerAsync<Postgres>>,
    AppState,
) {
    let (session, input_tx, output_rx, container, state) = create_session().await.unwrap();
    tokio::spawn(session.start());
    (input_tx, output_rx, container, state)
}

/// Generate Opus-encoded audio frames from text via Matcha TTS.
/// Uses the same TTS config as `create_session_channel()` so the audio is
/// compatible with the session's ASR pipeline.
pub async fn get_tts_audio(text: &str) -> Vec<Vec<u8>> {
    let audio_config = Arc::new(AudioConfig {
        output_sample_rate: Some(16000),
        output_channel: Some(1),
        output_frame_duration: Some(20_u64),
    });
    let tts = TtsManager::create_model(
        &TtsConfig {
            model: Some(TtsModel::MatchaTts),
            path: Some(
                workspace_root()
                    .join("data/tts/model/matcha/matcha-icefall-zh-en/")
                    .to_string_lossy()
                    .into_owned(),
            ),
            ..Default::default()
        },
        &audio_config,
    )
    .await
    .unwrap();

    let text_stream = tts_stream(text.to_string());
    let cancel = CancellationToken::new();
    let mut stream = tts.stream(Box::pin(text_stream), cancel).await;
    let mut all_audio = Vec::new();
    while let Some(packet) = stream.next().await {
        all_audio.extend(packet.unwrap().audio);
    }
    all_audio
}

/// Default timeout for recv operations.
const DEFAULT_RECV_TIMEOUT: Duration = Duration::from_secs(60);

/// Receive the next frame with a per-step timeout.
/// Panics on timeout or channel close.
/// Set `TEST_RECV_TIMEOUT` env var (seconds) to override default 60s timeout.
pub async fn recv_frame(
    rx: &mut mpsc::UnboundedReceiver<OutputMessage>,
    step: &str,
) -> FrameResult {
    let timeout = std::env::var("TEST_RECV_TIMEOUT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_RECV_TIMEOUT);
    let msg = tokio::time::timeout(timeout, rx.recv())
        .await
        .unwrap_or_else(|_| panic!("timeout at: {step}"))
        .unwrap_or_else(|| panic!("closed at: {step}"));
    tracing::debug!(payload = %msg.payload, epoch = msg.epoch, step, "<<< recv");
    msg.payload
}

/// Send a frame with tracing log.
pub fn send_frame(tx: &mpsc::UnboundedSender<Frame>, frame: Frame) {
    tracing::debug!(%frame, ">>> send");
    tx.send(frame).expect("send frame failed");
}

/// Drain the LLM+TTS sentence loop until TTSResult(Stop).
/// Each iteration: LLMResult → SentenceStart → AudioResult* → SentenceEnd.
/// Returns the number of sentences processed.
pub async fn recv_llm_tts_loop(
    rx: &mut mpsc::UnboundedReceiver<OutputMessage>,
    round: &str,
) -> usize {
    let mut sentences = 0;
    loop {
        match recv_frame(rx, &format!("{round}/llm or stop")).await {
            FrameResult::LLMResult(..) => {
                sentences += 1;
                let f = recv_frame(rx, &format!("{round}/sentence start")).await;
                assert!(
                    matches!(
                        f,
                        FrameResult::TTSResult(TtsMessage {
                            state: Some(TtsState::SentenceStart),
                            ..
                        })
                    ),
                    "expected TTSResult(SentenceStart), got {f}"
                );
                loop {
                    match recv_frame(rx, &format!("{round}/audio or end")).await {
                        FrameResult::AudioResult(_) => {}
                        FrameResult::TTSResult(TtsMessage {
                            state: Some(TtsState::SentenceEnd),
                            ..
                        }) => break,
                        other => {
                            panic!("{round}: expected AudioResult or SentenceEnd, got {other}")
                        }
                    }
                }
            }
            FrameResult::TTSResult(TtsMessage {
                state: Some(TtsState::Stop),
                ..
            }) => break,
            other => {
                panic!("{round}: expected LLMResult or TTSResult(Stop), got {other}")
            }
        }
    }
    sentences
}
