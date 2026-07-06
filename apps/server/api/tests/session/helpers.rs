use api::{
    AppState,
    asr::AsrFactory,
    config::{
        AsrModel, LlmModel, TtsModel, VadModel, asr::AsrConfig, audio::AudioConfig, llm::LlmConfig,
        session::SessionConfig, tts::TtsConfig, vad::VadConfig,
    },
    llm::LlmFactory,
    mcp::{
        client::server::ServerMcpClient,
        mcp_host::{McpHost, UnionMcpHost},
    },
    setup_mcp,
    tts::TtsFactory,
    util::audio::pcm_decode,
    vad::VadFactory,
    ws::session::SessionBuilder,
    ws::session::round::OutputMessage,
    ws::session::{Session, listener::DefaultListener},
};
use framework::id::gen_id;
use rmcp::{
    model::{JsonObject, JsonRpcMessage, JsonRpcResponse, JsonRpcVersion2_0, RequestId, object},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde::Serialize;
use service::ws::frame::Frame;

use std::{
    cmp,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Absolute workspace root, derived from CARGO_MANIFEST_DIR (compile-time constant).
/// CARGO_MANIFEST_DIR = <root>/apps/server/api → 3 `.parent()` calls = root.
fn workspace_root() -> PathBuf {
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

use crate::common::{router_client::RouterClient, setup_database};

/// Full session pipeline test at 16000Hz output.
/// Uses Void VAD/ASR + Echo LLM + Matcha TTS.
pub async fn create_session() -> Result<
    (
        Session,
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
    let mcp_config = StreamableHttpClientTransportConfig {
        uri: "/mcp".into(),
        ..Default::default()
    };
    let client = RouterClient { router };
    let transport = StreamableHttpClientTransport::with_client(client, mcp_config);
    let mut server_client = ServerMcpClient::new(transport).await?;
    server_client.init().await?;
    let session_id = gen_id();
    let mut mcp_host = UnionMcpHost::new(Some(session_id.clone()));
    mcp_host.add_client(Box::new(server_client)).await;

    let audio_config = Arc::new(AudioConfig {
        output_sample_rate: Some(16000),
        output_channel: Some(1),
        output_frame_duration: Some(20_u64),
    });
    let (session, input_tx, output_rx) = SessionBuilder::new()
        .with_listener(Box::new(DefaultListener::new(
            VadFactory::create_model(&Arc::new(VadConfig {
                model: Some(VadModel::Earshot),
                ..Default::default()
            })),
            Arc::new(Mutex::new(AsrFactory::create_model(&AsrConfig {
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
        .with_id(session_id.clone())
        .with_model(
           Arc::new( LlmFactory::create_model(&LlmConfig {
                model: Some(LlmModel::Echo),
                ..Default::default()
            }))
        )
            .with_tts(Arc::new(TtsFactory::create_model(&TtsConfig {
                model: Some(TtsModel::MatchaTts),
                path: Some(
                    workspace_root()
                        .join("data/tts/model/matcha/matcha-icefall-zh-en/")
                        .to_string_lossy()
                        .into_owned(),
                ),
                ..Default::default()
            }, &audio_config).await.unwrap()))
        .with_mcp_host(Arc::new(Mutex::new(mcp_host)))
        .with_config(Arc::new(SessionConfig {
            close_connection_no_voice_time: Some(3000),
            silence_voice_timeout: Some(1200),
            system_prompt: Some(String::from(
                "你是一个助手，所有回答必须使用纯文本自然语言，禁止使用任何Markdown符号如#、-、*等。",
            )),
            max_prompt_len: Some(6000),
        }))
        .with_audio_config(audio_config.clone())
        .build();
    Ok((session, input_tx, output_rx, container, state))
}

pub async fn create_mini_session_channel() -> (
    mpsc::UnboundedSender<Frame>,
    mpsc::UnboundedReceiver<OutputMessage>,
) {
    let audio_config = Arc::new(AudioConfig {
        output_sample_rate: Some(16000),
        output_channel: Some(1),
        output_frame_duration: Some(20_u64),
    });
    let session_id = gen_id();
    let (session, input_tx, output_rx) = SessionBuilder::new()
        .with_listener(Box::new(DefaultListener::new(
            VadFactory::create_model(&Arc::new(VadConfig {
                model: Some(VadModel::Earshot),
                ..Default::default()
            })),
            Arc::new(Mutex::new(AsrFactory::create_model(&AsrConfig {
                model:Some(AsrModel::Void),
                ..Default::default()
            }))),
        )))
        .with_id(session_id.clone())
        .with_model(
           Arc::new( LlmFactory::create_model(&LlmConfig {
            model: Some(LlmModel::Echo),
            ..Default::default()
            }))
        )
            .with_tts(Arc::new(TtsFactory::create_model(&TtsConfig {
            model: Some(TtsModel::Mute),
            ..Default::default()
        }, &audio_config).await.unwrap()))
        .with_mcp_host(Arc::new(Mutex::new(UnionMcpHost::new(Some(session_id.clone())))))
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
    (input_tx, output_rx)
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
    let mut encoder = opus::Encoder::new(
        ENCODE_SAMPLE_RATE,
        opus::Channels::Mono,
        opus::Application::Audio,
    )
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
        let packet = encoder
            .encode_vec_float(&pcm_data[start..end], size)
            .unwrap();
        audio.push(packet);
    }
    audio
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
