pub mod input_proxy;
pub mod input_sender;
pub mod message_converter;
pub mod output_proxy;
pub mod output_sender;
pub mod session;

use crate::{
    AppState,
    asr::AsrFactory,
    config::{audio::AudioConfig, mcp::McpConfig, session::SessionConfig, vad::VadConfig},
    llm::LlmFactory,
    mcp::{
        client::server::ServerMcpClient,
        mcp_host::{McpHost, UnionMcpHost},
    },
    record::collector::RecordCollector,
    tts::TtsFactory,
    vad::VadFactory,
    ws::{
        input_proxy::InputProxy, input_sender::InputSender, output_proxy::OutputProxy,
        output_sender::OutputSender, session::SessionBuilder, session::listener::DefaultListener,
    },
};

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
use message_converter::convert_to_frame;
use rmcp::transport::{
    StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
};
use serde::Serialize;
use service::chobits::message::close::CloseMessage;
use service::ws::frame::{Frame, FrameResult};
use tokio::sync::Mutex;
use tracing::{Instrument, Level, debug, error, span, warn};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use std::{collections::HashMap, net::SocketAddr, sync::Arc};

#[derive(Serialize)]
pub(crate) struct ErrorFrame {
    #[serde(rename = "type")]
    mtype: &'static str,
    code: u32,
    message: String,
}

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
    user_agent: Option<TypedHeader<headers::UserAgent>>,
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
    debug!("user_agent = {:?}", user_agent);
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

pub(crate) async fn handle_socket<W, R>(ctx: SocketContext, write: W, read: R)
where
    W: Sink<Message> + Unpin + Send + 'static,
    R: Stream<Item = Result<Message, axum::Error>> + Unpin + Send + 'static,
{
    let span = span!(Level::DEBUG, "socket", id=%ctx.session_id);
    let _guard = span.enter();

    let (session, input_tx, output_rx) = SessionBuilder::new()
        .with_id(ctx.session_id.clone())
        .with_listener(Box::new(DefaultListener::new(
            VadFactory::create_model(&ctx.vad_config),
            AsrFactory::global().default().clone(),
            ctx.audio_config.clone(),
        )))
        .with_model(LlmFactory::global().default())
        .with_tts(TtsFactory::global().default())
        .with_mcp_host(Arc::new(Mutex::new(
            create_mcp_host(ctx.session_id.clone(), ctx.mcp_config.clone()).await,
        )))
        .with_config(ctx.session_config.clone())
        .with_audio_config(ctx.audio_config.clone())
        .build();

    let collector = Arc::new(RecordCollector::new(ctx.conn.clone()));

    let session_handle = tokio::spawn(
        session
            .start()
            .instrument(span!(parent: &span, Level::DEBUG, "session")),
    );
    let output_handle = tokio::spawn(
        ws_output(
            write,
            OutputProxy::new(output_rx, Some(collector.clone()), ctx.session_id.clone()),
        )
        .instrument(span!(parent: &span, Level::DEBUG, "output")),
    );
    let input_handle = tokio::spawn(
        ws_input(
            read,
            InputProxy::new(ctx.session_id.clone(), Some(collector.clone()), input_tx),
        )
        .instrument(span!(parent: &span, Level::DEBUG, "input")),
    );

    let _ = tokio::join!(session_handle, output_handle, input_handle);
}

async fn ws_input<R>(mut read: R, input_sender: impl InputSender)
where
    R: Stream<Item = Result<Message, axum::Error>> + Unpin + Send + 'static,
{
    while let Some(Ok(msg)) = read.next().await {
        let result = convert_to_frame(&msg);
        if result.is_break() {
            if let Some(frame) = result.break_value().flatten() {
                input_sender.send(frame);
            }
            input_sender.send(Frame::Close(CloseMessage::new(1000, String::new())));
            return;
        }
        if result.is_continue() {
            if let Some(frame) = result.continue_value().flatten() {
                input_sender.send(frame);
            } else {
                warn!("unknown continue message");
            }
        }
    }
    input_sender.send(Frame::Close(CloseMessage::new(1000, String::new())));
}

async fn ws_output<W>(mut write: W, mut output_sender: impl OutputSender)
where
    W: Sink<Message> + Unpin + Send + 'static,
{
    while let Some(frame) = output_sender.recv().await {
        let result = match &frame {
            Ok(FrameResult::AudioResult(audio)) => {
                write.send(Message::Binary(audio.data.clone().into())).await
            }
            Ok(other) => {
                let text = serde_json::to_string(other).unwrap_or_default();
                write.send(Message::Text(text.into())).await
            }
            Err(e) => {
                let error_frame = ErrorFrame {
                    mtype: "error",
                    code: WsErrorCode::InternalError as u32,
                    message: e.to_string(),
                };
                let text = serde_json::to_string(&error_frame).unwrap_or_default();
                write.send(Message::Text(text.into())).await
            }
        };
        if result.is_err() {
            break;
        }
    }
    let _ = write.close().await;
}

async fn create_server_mcp_client(uri: String) -> anyhow::Result<ServerMcpClient> {
    let config = StreamableHttpClientTransportConfig::with_uri(uri);
    let transport = StreamableHttpClientTransport::from_config(config);
    let mut server_mcp_client = ServerMcpClient::new(transport).await?;
    server_mcp_client.init().await?;
    Ok(server_mcp_client)
}

async fn create_mcp_host(session_id: String, mcp_config: Arc<McpConfig>) -> UnionMcpHost {
    let mut mcp_host = UnionMcpHost::new(Some(session_id));
    let uri_list = &mcp_config.uri_list;
    if let Some(uri_list) = uri_list {
        for uri in uri_list {
            let server_mcp_client = create_server_mcp_client(uri.to_string()).await;
            match server_mcp_client {
                Ok(server_mcp_client) => {
                    mcp_host.add_client(Box::new(server_mcp_client)).await;
                }
                Err(e) => {
                    error!("{:?}", e);
                }
            }
        }
    }
    mcp_host
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
