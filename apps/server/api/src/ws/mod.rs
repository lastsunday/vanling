pub mod default_listener;
pub mod input_proxy;
pub mod input_sender;
pub mod mcp_handler;
pub mod output_proxy;
pub mod output_sender;
pub mod protocol_translator;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{
    RequestPartsExt, debug_handler,
    extract::{ConnectInfo, FromRequestParts, Path, State, WebSocketUpgrade, ws::Message},
    http::{HeaderMap, StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use axum_extra::{TypedHeader, headers};
use framework::id::gen_id;
use framework::prelude::error as error_code;
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use service::chobits::frame::Frame;
use service::chobits::message::close::CloseMessage;
use tracing::{Instrument, Level, span};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    AppState,
    asr::AsrFactory,
    config::{audio::AudioConfig, mcp::McpConfig, session::SessionConfig, vad::VadConfig},
    record::recorder::Recorder,
    tts::TtsFactory,
    vad::VadFactory,
    ws::{
        default_listener::DefaultListener,
        input_proxy::InputProxy,
        input_sender::InputSender,
        mcp_handler::{
            McpContext, McpFrameAction, ServerJsonRpcMessage, handle_mcp_frame, setup_mcp_handler,
        },
        output_proxy::OutputProxy,
        output_sender::OutputSender,
        protocol_translator::ProtocolTranslator,
    },
    {chii::ChiiCoreBuilder, llm::LlmFactory},
};

const TAG: &str = "ws";

pub fn create_routes(state: AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(ws_handler))
        .with_state(state)
}

#[debug_handler]
#[tracing::instrument(name="ws",skip_all,fields(ip = %addr))]
#[utoipa::path(get,
    path = "/chobits/{version}",
    tag=TAG,
    security(()),
    params(
        ("version" = Version, Path,example="v1", description = "Version"),
        ("Protocol-Version" = String,Header,description="",example="1"),
        ("Device-Id" = String,Header,description="设备的唯一标识符（使用MAC地址或由硬件ID生成的伪MAC地址）",example="11:22:33:44:55:66"),
        ("Client-Id" = String,Header,description="客户端的唯一标识符，由软件自动生成的UUID v4（擦除FLASH或重装后会变化）",example="7b94d69a-9808-4c59-9c9b-704333b38aff"),
    )
)]
async fn ws_handler(
    _version: Version,
    ws: WebSocketUpgrade,
    _user_agent: Option<TypedHeader<headers::UserAgent>>,
    _headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(AppState {
        conn,
        session_config,
        mcp_config,
        vad_config,
        audio_config,
        ..
    }): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        let (write, read) = socket.split();
        handle_socket(
            SocketContext {
                session_id: gen_id(),
                conn,
                session_config,
                mcp_config,
                vad_config,
                audio_config,
            },
            write,
            read,
        )
    })
}

pub(crate) struct SocketContext {
    session_id: String,
    conn: sea_orm::DatabaseConnection,
    session_config: Arc<SessionConfig>,
    mcp_config: Arc<McpConfig>,
    vad_config: Arc<VadConfig>,
    audio_config: Arc<AudioConfig>,
}

#[tracing::instrument(skip_all, fields(session_id = %ctx.session_id))]
pub(crate) async fn handle_socket<W, R>(ctx: SocketContext, write: W, read: R)
where
    W: Sink<Message> + Unpin + Send + 'static,
    R: Stream<Item = Result<Message, axum::Error>> + Unpin + Send + 'static,
{
    tracing::info!("session started");

    let McpContext {
        registry,
        inbound_tx: mcp_inbound_tx,
        outbound_rx: mcp_outbound_rx,
    } = setup_mcp_handler(ctx.session_id.clone(), &ctx.mcp_config).await;

    let chii: Arc<dyn service::chobits::chii::Chii> = Arc::new(
        ChiiCoreBuilder::new()
            .with_session_id(Some(ctx.session_id.clone()))
            .with_model(LlmFactory::global().default())
            .with_mcp_registry(registry.clone())
            .build(),
    );

    let tts: Arc<dyn service::chobits::tts::Tts> = TtsFactory::global().default();

    use service::chobits::session::{AudioConfig, SessionBuilder, SessionConfig};

    let session_config = SessionConfig {
        system_prompt: ctx.session_config.system_prompt.clone(),
        max_prompt_len: ctx.session_config.max_prompt_len,
        silence_voice_timeout: ctx.session_config.silence_voice_timeout,
        close_connection_no_voice_time: ctx.session_config.close_connection_no_voice_time,
    };
    let audio_config = AudioConfig {
        output_sample_rate: ctx
            .audio_config
            .output_sample_rate
            .expect("output sample rate is empty"),
        output_channel: ctx
            .audio_config
            .output_channel
            .expect("output channel is empty"),
        output_frame_duration: ctx
            .audio_config
            .output_frame_duration
            .expect("output frame duration is empty"),
    };

    let (session, input_tx, output_rx) = SessionBuilder::new()
        .with_id(ctx.session_id.clone())
        .with_listener(Box::new(DefaultListener::new(
            VadFactory::create_model(&ctx.vad_config),
            AsrFactory::global().default().clone(),
        )))
        .with_chii(chii)
        .with_tts(tts)
        .with_config(session_config)
        .with_audio_config(audio_config)
        .build();

    let recorder = Arc::new(Recorder::new(ctx.conn.clone()));
    let translator = protocol_translator::XiaozhiProtocolTranslator;

    let session_handle = tokio::spawn(session.start().instrument(span!(Level::DEBUG, "session")));
    let output_handle = tokio::spawn(
        ws_output(
            write,
            OutputProxy::new(output_rx, Some(recorder.clone()), ctx.session_id.clone()),
            mcp_outbound_rx,
            translator,
        )
        .instrument(span!(Level::DEBUG, "output")),
    );

    let input_handle = tokio::spawn(
        ws_input(
            read,
            InputProxy::new(
                ctx.session_id.clone(),
                Some(recorder.clone()),
                input_tx,
                20,
                1,
            ),
            mcp_inbound_tx,
            translator,
        )
        .instrument(span!(Level::DEBUG, "input")),
    );

    let _ = tokio::join!(session_handle, output_handle, input_handle);

    tracing::info!("session ended");
}

async fn ws_input<R>(
    mut read: R,
    input_sender: impl InputSender,
    mcp_inbound_tx: tokio::sync::mpsc::Sender<ServerJsonRpcMessage>,
    translator: impl ProtocolTranslator,
) where
    R: Stream<Item = Result<Message, axum::Error>> + Unpin + Send + 'static,
{
    while let Some(Ok(msg)) = read.next().await {
        let frame = translator.input(msg);
        match handle_mcp_frame(&frame, &mcp_inbound_tx).await {
            McpFrameAction::Handled => continue,
            McpFrameAction::ChannelClosed => break,
            McpFrameAction::NotMcp => {}
        }
        let is_close = matches!(&frame, Frame::Close(_));
        input_sender.send(frame);
        if is_close {
            return;
        }
    }
    input_sender.send(Frame::Close(CloseMessage::new(1000, String::new())));
}

async fn ws_output<W>(
    mut write: W,
    mut session_output: impl OutputSender,
    mut mcp_outbound_rx: tokio::sync::mpsc::UnboundedReceiver<
        service::chobits::frame::OutputMessage,
    >,
    translator: impl ProtocolTranslator,
) where
    W: Sink<Message> + Unpin + Send + 'static,
{
    loop {
        let payload = tokio::select! {
            result = session_output.recv() => result,
            msg = mcp_outbound_rx.recv() => msg.map(|m| m.payload),
        };
        match payload {
            Some(payload) => {
                let msg = translator.output(payload);
                if write.send(msg).await.is_err() {
                    break;
                }
            }
            None => break,
        }
    }
    let _ = write.close().await;
}

#[derive(Debug, PartialEq, Eq, ToSchema)]
enum Version {
    V1,
}

impl<S> FromRequestParts<S> for Version
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let params: Path<HashMap<String, String>> =
            parts.extract().await.map_err(IntoResponse::into_response)?;

        let version = params
            .get("version")
            .ok_or_else(|| (StatusCode::NOT_FOUND, "version param missing").into_response())?;

        match version.as_str() {
            "v1" => Ok(Version::V1),
            _ => Err((StatusCode::NOT_FOUND, "unknown version").into_response()),
        }
    }
}

#[error_code]
pub enum WsErrorCode {
    ListenFailure = 504001,
    TtsEncode = 504002,
    TtsText = 504003,
    AsrFailure = 504004,
    LlmFailure = 504005,
    InternalError = 504006,
}
