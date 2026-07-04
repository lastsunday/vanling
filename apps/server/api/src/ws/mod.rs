pub mod message_converter;
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
    tts::TtsFactory,
    vad::VadFactory,
    ws::session::Session,
};

use axum::{
    RequestPartsExt, debug_handler,
    extract::{ConnectInfo, FromRequestParts, Path, State, WebSocketUpgrade, ws::Message},
    http::{HeaderMap, StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use axum_extra::{TypedHeader, headers};
use framework::error::AppError;
use framework::id::gen_id;
use framework::prelude::error as error_code;
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use message_converter::convert_to_frame;
use rmcp::transport::{
    StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
};
use serde::Serialize;
use service::ws::frame::FrameResult;
use session::round::OutputMessage;
use session::{SessionBuilder, listener::DefaultListener};

use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::Mutex;
use tracing::{Instrument, Level, debug, error, info, span, trace, warn};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

#[derive(Serialize)]
struct ErrorFrame {
    #[serde(rename = "type")]
    mtype: &'static str,
    code: u32,
    message: String,
}

const TAG: &str = "ws";

pub fn create_routes(state: AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(ws_handler))
        //.layer(get_auth_layer())
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

pub(crate) async fn handle_socket<W, R>(ctx: SocketContext, mut write: W, read: R)
where
    W: Sink<Message> + Unpin + Send + 'static,
    R: Stream<Item = Result<Message, axum::Error>> + Unpin + Send + 'static,
{
    let span = span!(Level::DEBUG, "socket", id=%ctx.session_id);
    let _guard = span.enter();
    let mut session = SessionBuilder::new()
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
    if let Err(e) = session.start().instrument(span.clone()).await {
        error!("{}", e);
        let result = write.close().await;
        if result.is_err() {
            info!("write close failure");
        }
        return;
    }
    let session_id_clone = ctx.session_id.clone();
    let output_stream = session.output_frame().await;
    tokio::spawn(async move {
        let span = span!(parent:None,Level::DEBUG, "socket", id=%session_id_clone);
        on_send(output_stream, write).instrument(span).await
    });
    tokio::spawn(async move {
        let span = span!(parent:None,Level::DEBUG, "socket", id=%ctx.session_id);
        on_recv(session, read).instrument(span).await
    });
}

async fn on_recv<R>(mut session: Session, mut read: R)
where
    R: Stream<Item = Result<Message, axum::Error>> + Unpin + Send + 'static,
{
    while let Some(Ok(msg)) = read.next().await {
        let result = convert_to_frame(&msg);
        if result.is_break() {
            if let Some(item) = result.break_value() {
                match item {
                    Some(frame) => session.accept_frame(&frame).await,
                    None => trace!("break value none"),
                }
            }
            session.stop().await;
            return;
        }
        if result.is_continue()
            && let Some(item) = result.continue_value()
            && let Some(frame) = item
        {
            session.accept_frame(&frame).await
        } else {
            warn!("unknown continue message");
        }
    }
    session.stop().await;
}

async fn on_send<W>(
    mut output: impl Stream<Item = OutputMessage> + Unpin + Send + 'static,
    mut write: W,
) where
    W: Sink<Message> + Unpin + Send + 'static,
{
    while let Some(msg) = output.next().await {
        match msg.payload {
            Ok(frame) => match frame {
                FrameResult::AudioResult(msg) => {
                    if write.send(Message::Binary(msg.data.into())).await.is_err() {
                        break;
                    }
                }
                FrameResult::CloseResult => {
                    if write.close().await.is_err() {
                        break;
                    }
                }
                _ => {
                    let data = serde_json::to_string(&frame).expect("frame to json failure");
                    if write.send(Message::Text(data.into())).await.is_err() {
                        break;
                    }
                }
            },
            Err(api_err) => {
                api_err.log();
                let AppError::App { code, message, .. } = &api_err;
                let data = serde_json::to_string(&ErrorFrame {
                    mtype: "error",
                    code: *code,
                    message: message.clone(),
                })
                .expect("error frame to json failure");
                if write.send(Message::Text(data.into())).await.is_err() {
                    break;
                }
            }
        }
    }
    if write.close().await.is_err() {
        info!("write close failure");
    }
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
