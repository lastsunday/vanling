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
    util::audio::pcm_decode,
    vad::VadManager,
    ws::default_listener::DefaultListener,
    {chii::ChiiCoreBuilder, llm::LlmManager},
};
use framework::id::gen_id;
use rmcp::{
    model::{JsonObject, JsonRpcMessage, JsonRpcResponse, JsonRpcVersion2_0, RequestId, object},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde::Serialize;
use service::chobits::{
    frame::{Frame, FrameResult, OutputMessage},
    mcp::McpRegistry,
    session::{
        AudioConfig as ServiceAudioConfig, SessionBuilder, SessionConfig as ServiceSessionConfig,
    },
};

use futures::StreamExt;
use std::{
    cmp,
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
use tracing::debug;
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
            VadManager::create_model(&Arc::new(VadConfig {
                model: Some(VadModel::Earshot),
                ..Default::default()
            })),
            Arc::new(Mutex::new(AsrManager::create_model(&AsrConfig {
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

pub fn get_audio() -> Vec<Vec<u8>> {
    use std::path::PathBuf;

    let wav_file: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "resources",
        "test",
        "samples_jfk.wav",
    ]
    .iter()
    .collect();
    debug!("{}", wav_file.display());
    let (pcm_data, sample_rate) = pcm_decode(wav_file).unwrap();
    debug!(
        "pcm_data len = {},sample_rate = {}",
        pcm_data.len(),
        sample_rate
    );

    const ENCODE_SAMPLE_RATE: u32 = 16000;
    let mut encoder = ropus::Encoder::builder(
        ENCODE_SAMPLE_RATE,
        ropus::Channels::Mono,
        ropus::Application::Audio,
    )
    .build()
    .unwrap();

    // 16000Hz * 1 channel * 20 ms / 1000 = 320
    const MONO_20MS: usize = ENCODE_SAMPLE_RATE as usize * 20 / 1000;
    let size = MONO_20MS;
    debug!("size = {}", size);
    let len = pcm_data.len();
    let mut count = len / size;
    if len % size > 0 {
        count += 1;
    }
    debug!("count = {}", count);
    let mut audio: Vec<Vec<u8>> = Vec::new();

    for n in 0..count {
        let start = n * size;
        let end = cmp::min((n + 1) * size, len);
        let mut buf = vec![0u8; size * 4];
        let written = encoder
            .encode_float(&pcm_data[start..end], &mut buf)
            .unwrap();
        buf.truncate(written);
        audio.push(buf);
    }
    audio
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

/// Receive the next frame with a per-step timeout.
/// Panics on timeout or channel close.
pub async fn recv_frame(
    rx: &mut mpsc::UnboundedReceiver<OutputMessage>,
    step: &str,
) -> FrameResult {
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .unwrap_or_else(|_| panic!("timeout at: {}", step))
        .unwrap_or_else(|| panic!("closed at: {}", step))
        .payload
}

pub fn to_json_rpc_response<T>(id: i64, result: T) -> JsonRpcMessage
where
    T: Serialize,
{
    JsonRpcMessage::Response(JsonRpcResponse {
        jsonrpc: JsonRpcVersion2_0,
        id: RequestId::Number(id),
        result: to_json_object(result),
    })
}

pub fn to_json_object<T>(value: T) -> JsonObject
where
    T: Serialize,
{
    object(serde_json::to_value(value).unwrap())
}
