pub mod default_listener;
pub mod input_proxy;
pub mod input_sender;
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
use tokio::sync::Mutex;
use tracing::{Instrument, Level, span};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    AppState,
    asr::AsrFactory,
    config::{audio::AudioConfig, mcp::McpConfig, session::SessionConfig, vad::VadConfig},
    llm::{LlmFactory, client::ClientBuilder},
    mcp::{client::create_server_mcp_client, provider::McpProviderImpl},
    record::recorder::Recorder,
    tts::TtsFactory,
    vad::VadFactory,
    ws::{
        default_listener::DefaultListener, input_proxy::InputProxy, input_sender::InputSender,
        output_proxy::OutputProxy, output_sender::OutputSender,
        protocol_translator::ProtocolTranslator,
    },
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

#[allow(dead_code)]
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

    let mut mcp_impl = McpProviderImpl::new(ctx.session_id.clone());
    let mcp_host = mcp_impl.mcp_host();
    let uri_list = &ctx.mcp_config.uri_list;
    if let Some(uri_list) = uri_list {
        for uri in uri_list {
            let server_mcp_client = create_server_mcp_client(uri.to_string()).await;
            match server_mcp_client {
                Ok(client) => {
                    mcp_impl.add_client(Box::new(client)).await;
                }
                Err(e) => {
                    tracing::warn!(session_id = %ctx.session_id, uri = %uri, error = %e, "mcp server init failed")
                }
            }
        }
    }
    let mcp_provider: Arc<Mutex<dyn service::chobits::mcp::Mcp>> = Arc::new(Mutex::new(mcp_impl));

    let llm: Arc<dyn service::chobits::llm::Llm> = Arc::new(
        ClientBuilder::new()
            .with_session_id(Some(ctx.session_id.clone()))
            .with_model(LlmFactory::global().default())
            .with_mcp_host(mcp_host)
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
        .with_llm(llm)
        .with_tts(tts)
        .with_mcp(mcp_provider)
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
    translator: impl ProtocolTranslator,
) where
    R: Stream<Item = Result<Message, axum::Error>> + Unpin + Send + 'static,
{
    while let Some(Ok(msg)) = read.next().await {
        let frame = translator.input(msg);
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
    mut output_sender: impl OutputSender,
    translator: impl ProtocolTranslator,
) where
    W: Sink<Message> + Unpin + Send + 'static,
{
    while let Some(result) = output_sender.recv().await {
        let msg = translator.output(result);
        if write.send(msg).await.is_err() {
            break;
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
