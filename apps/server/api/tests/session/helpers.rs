use api::{
    AppState,
    component::asr::AsrManager,
    component::ling::LingCoreBuilder,
    component::llm::LlmManager,
    component::mcp::client::external::ExternalMcpClient,
    component::tts::TtsManager,
    component::vad::pool::VadPool,
    config::{
        AsrModel, LlmProvider, TtsModel, VadModel, asr::AsrConfig, audio::AudioConfig,
        llm::LlmConfig, tts::TtsConfig, vad::VadConfig,
    },
    setup_mcp,
};
use framework::auth::{Jwt, Principal};
use framework::config::auth::AuthConfig;
use framework::id::gen_id;
use rmcp::transport::{
    StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
};
use service::component::mcp::McpRegistry;
use service::frame::{Frame, FrameResult, OutputMessage};
use service::message::tts::{TtsMessage, TtsState};
use service::pipeline::{AsrNode, LingNode, OpusDecodeNode, TtsNode, TurnNode, VadNode};
use service::session::{SessionBuilder, SessionConfig as ServiceSessionConfig};

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
        service::session::Session,
        mpsc::UnboundedSender<Frame>,
        mpsc::UnboundedReceiver<OutputMessage>,
        Option<ContainerAsync<Postgres>>,
        AppState,
    ),
    anyhow::Error,
> {
    let (container, state) = setup_database().await;
    Jwt::init(Arc::new(AuthConfig {
        access_token_secret: Some(String::from("test-secret")),
        access_token_expires_in: Some(28800),
        refresh_token_secret: Some(String::from("test-refresh-secret")),
        refresh_token_expires_in: Some(15897600),
        audience: Some(String::from("test-aud")),
        issuer: Some(String::from("test-iss")),
        ..Default::default()
    }));
    let admin_token = Jwt::global()
        .access_token_encode(&Principal {
            id: String::from("test-admin"),
            name: Some(String::from("root")),
            token_type: String::from("user"),
        })
        .expect("encode admin token");
    // server client
    let router = OpenApiRouter::new();
    let ct = tokio_util::sync::CancellationToken::new();
    let router = setup_mcp(router, state.clone(), ct.child_token())
        .split_for_parts()
        .0;
    let mut mcp_config = StreamableHttpClientTransportConfig::with_uri("/mcp");
    mcp_config.auth_header = Some(admin_token);
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

    let ling: Arc<dyn service::ling::Ling> = Arc::new(
        LingCoreBuilder::new()
            .with_session_id(Some(session_id.clone()))
            .with_model(LlmManager::create_model(&LlmConfig {
                provider: Some(LlmProvider::LocalEcho),
                ..Default::default()
            }))
            .with_mcp_registry(mcp_registry)
            .build(),
    );

    let tts: Arc<dyn service::component::tts::Tts> = Arc::from(
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

    let templates: Vec<Arc<dyn service::pipeline::Node>> = vec![
        Arc::new(OpusDecodeNode::new()) as Arc<dyn service::pipeline::Node>,
        Arc::new(VadNode::new(Arc::new(VadPool::new(Arc::new(VadConfig {
            model: Some(VadModel::Earshot),
            ..Default::default()
        }))))) as Arc<dyn service::pipeline::Node>,
        Arc::new(AsrNode::new(Arc::from(AsrManager::create_model(
            &AsrConfig {
                model: Some(AsrModel::XAsr),
                path: Some(
                    workspace_root()
                        .join("data/asr/model/x_asr/default/")
                        .to_string_lossy()
                        .into_owned(),
                ),
                variant: None,
            },
        )))) as Arc<dyn service::pipeline::Node>,
        Arc::new(TurnNode::new()) as Arc<dyn service::pipeline::Node>,
        Arc::new(LingNode::new(ling)) as Arc<dyn service::pipeline::Node>,
        Arc::new(TtsNode::new(tts)) as Arc<dyn service::pipeline::Node>,
    ];

    let session_ctx = SessionBuilder::new()
        .with_id(session_id.clone())
        .with_node_templates(templates)
        .with_config(ServiceSessionConfig {
            close_connection_no_activity_time: Some(3000),
            silence_voice_timeout: Some(1200),
            barge_in_lockout_ms: Some(250),
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

/// 无 TTS 模板的会话通道。模板只含听段节点（opus→vad→asr→turn），
/// 无 `TtsNode` ⇒ `audio_spec` 为 `None` ⇒ 握手 `audio_params: None`（不声明下行语音能力）。
pub async fn create_no_tts_session_channel() -> (
    mpsc::UnboundedSender<Frame>,
    mpsc::UnboundedReceiver<OutputMessage>,
) {
    let session_id = gen_id();
    let mcp_registry = Arc::new(Mutex::new(McpRegistry::new(Some(session_id.clone()))));

    let ling: Arc<dyn service::ling::Ling> = Arc::new(
        LingCoreBuilder::new()
            .with_session_id(Some(session_id.clone()))
            .with_model(LlmManager::create_model(&LlmConfig {
                provider: Some(LlmProvider::LocalEcho),
                ..Default::default()
            }))
            .with_mcp_registry(mcp_registry)
            .build(),
    );

    let templates: Vec<Arc<dyn service::pipeline::Node>> = vec![
        Arc::new(OpusDecodeNode::new()) as Arc<dyn service::pipeline::Node>,
        Arc::new(VadNode::new(Arc::new(VadPool::new(Arc::new(VadConfig {
            model: Some(VadModel::Earshot),
            ..Default::default()
        }))))) as Arc<dyn service::pipeline::Node>,
        Arc::new(AsrNode::new(Arc::from(AsrManager::create_model(
            &AsrConfig {
                model: Some(AsrModel::Void),
                ..Default::default()
            },
        )))) as Arc<dyn service::pipeline::Node>,
        Arc::new(TurnNode::new()) as Arc<dyn service::pipeline::Node>,
        Arc::new(LingNode::new(ling)) as Arc<dyn service::pipeline::Node>,
    ];

    let session_ctx = SessionBuilder::new()
        .with_id(session_id.clone())
        .with_node_templates(templates)
        .with_config(ServiceSessionConfig {
            close_connection_no_activity_time: Some(3000),
            silence_voice_timeout: Some(1200),
            barge_in_lockout_ms: Some(250),
        })
        .build();
    tokio::spawn(session_ctx.session.start());
    (session_ctx.input_tx, session_ctx.output_rx)
}

pub async fn create_mini_session_with_timeout(
    close_connection_no_activity_time_ms: i64,
) -> (
    mpsc::UnboundedSender<Frame>,
    mpsc::UnboundedReceiver<OutputMessage>,
) {
    let session_id = gen_id();
    let mcp_registry = Arc::new(Mutex::new(McpRegistry::new(Some(session_id.clone()))));

    let ling: Arc<dyn service::ling::Ling> = Arc::new(
        LingCoreBuilder::new()
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

    let tts: Arc<dyn service::component::tts::Tts> = Arc::from(
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

    let templates: Vec<Arc<dyn service::pipeline::Node>> = vec![
        Arc::new(OpusDecodeNode::new()) as Arc<dyn service::pipeline::Node>,
        Arc::new(VadNode::new(Arc::new(VadPool::new(Arc::new(VadConfig {
            model: Some(VadModel::Earshot),
            ..Default::default()
        }))))) as Arc<dyn service::pipeline::Node>,
        Arc::new(AsrNode::new(Arc::from(AsrManager::create_model(
            &AsrConfig {
                model: Some(AsrModel::Void),
                ..Default::default()
            },
        )))) as Arc<dyn service::pipeline::Node>,
        Arc::new(TurnNode::new()) as Arc<dyn service::pipeline::Node>,
        Arc::new(LingNode::new(ling)) as Arc<dyn service::pipeline::Node>,
        Arc::new(TtsNode::new(tts)) as Arc<dyn service::pipeline::Node>,
    ];

    let session_ctx = SessionBuilder::new()
        .with_id(session_id.clone())
        .with_node_templates(templates)
        .with_config(ServiceSessionConfig {
            close_connection_no_activity_time: Some(close_connection_no_activity_time_ms),
            silence_voice_timeout: Some(1200),
            barge_in_lockout_ms: Some(250),
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

/// 把 16k opus 音频重采样到目标采样率，重新编码为 opus 帧（每帧 20ms）。
/// 用于模拟客户端声明非 16k 上行采样率（浏览器 AudioContext 常见 48k）时，
/// 上行仍是真实语音（XAsr 可识别），从而覆盖 opus 节点重采样路径。
pub fn resample_opus_audio(audio: &[Vec<u8>], target_rate: u32) -> Vec<Vec<u8>> {
    const SRC_RATE: u32 = 16000;
    let mut decoder =
        ropus::Decoder::new(SRC_RATE, ropus::Channels::Mono).expect("build opus decoder");
    let max_frame = (SRC_RATE as usize * 120) / 1000;
    let mut src_pcm: Vec<f32> = Vec::new();
    for frame in audio {
        let mut samples = vec![0f32; max_frame];
        let len = decoder
            .decode_float(frame, &mut samples, ropus::DecodeMode::Normal)
            .expect("decode 16k opus");
        src_pcm.extend_from_slice(&samples[..len]);
    }
    let ratio = target_rate as f64 / SRC_RATE as f64;
    let mut dst_pcm: Vec<f32> = Vec::with_capacity((src_pcm.len() as f64 * ratio) as usize);
    for n in 0..((src_pcm.len() as f64 * ratio) as usize) {
        let f = n as f64 / ratio;
        let i0 = f.floor() as usize;
        let i1 = (i0 + 1).min(src_pcm.len().saturating_sub(1));
        let frac = f - i0 as f64;
        dst_pcm.push(src_pcm[i0] * (1.0 - frac as f32) + src_pcm[i1] * frac as f32);
    }
    let frame_samples = (target_rate as usize * 20) / 1000;
    let mut encoder = ropus::Encoder::builder(
        target_rate,
        ropus::Channels::Mono,
        ropus::Application::Audio,
    )
    .build()
    .expect("build opus encoder");
    let mut out = vec![0u8; 4000];
    dst_pcm
        .chunks(frame_samples)
        .map(|chunk| {
            let mut pcm = vec![0i16; frame_samples];
            for (i, s) in pcm.iter_mut().enumerate() {
                let v = chunk.get(i).copied().unwrap_or(0.0);
                *s = (v.clamp(-1.0, 1.0) * 32767.0) as i16;
            }
            let n = encoder.encode(&pcm, &mut out).expect("encode opus frame");
            out[..n].to_vec()
        })
        .collect()
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
