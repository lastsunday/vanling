pub mod filter;
pub mod mcp_session;
pub mod protocol_translator;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{
    RequestPartsExt, debug_handler,
    extract::{ConnectInfo, FromRequestParts, Path, State, WebSocketUpgrade, ws::Message},
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header, request::Parts},
    response::{IntoResponse, Response},
};
use axum_extra::{TypedHeader, headers};
use framework::{
    auth::{Jwt, Principal},
    id::gen_id,
};
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use service::frame::{Frame, FrameResult, OutputMessage};
use service::pipeline::Node;
use service::session::{self, SessionBuilder};
use tokio::sync::mpsc::UnboundedSender;
use tokio_stream::StreamMap;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_util::sync::CancellationToken;

use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use service::pipeline::{AsrNode, LingNode, OpusDecodeNode, TtsNode, TurnNode, VadNode};

use crate::{
    AppState,
    component::asr::AsrManager,
    component::ling::LingCoreBuilder,
    component::llm::LlmManager,
    component::tts::TtsManager,
    component::vad::pool::VadPool,
    config::{ling::LingConfig, mcp::McpConfig, session::SessionConfig, vad::VadConfig},
    record::recorder::Recorder,
    ws::{
        filter::{
            FilterCtx, FilterStep, InputFilter, OutputFilter, RecorderInputFilter,
            RecorderOutputFilter, run_input_filters, run_output_filters,
        },
        mcp_session::{McpRouterFilter, setup_mcp_session},
        protocol_translator::ProtocolTranslator,
    },
};

const TAG: &str = "ws";

pub fn create_routes(state: AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(ws_handler))
        .with_state(state)
}

fn extract_bearer_from_query(query: Option<&str>) -> Option<HeaderValue> {
    let query = query?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts.next()? == "authorization" {
            let value = parts.next()?;
            return HeaderValue::from_str(&value.replace('+', " ")).ok();
        }
    }
    None
}

pub fn verify_device_token(headers: &HeaderMap) -> Result<Principal, Box<Response>> {
    let Some(principal) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .and_then(|token| Jwt::global().access_token_decode(token).ok())
    else {
        return Err(Box::new(
            (StatusCode::UNAUTHORIZED, "unauthorized").into_response(),
        ));
    };
    Ok(principal)
}

#[debug_handler]
#[utoipa::path(get,
    path = "/vanling/{version}",
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
    uri: Uri,
    _version: Version,
    ws: WebSocketUpgrade,
    _user_agent: Option<TypedHeader<headers::UserAgent>>,
    mut headers: HeaderMap,
    ConnectInfo(_addr): ConnectInfo<SocketAddr>,
    State(AppState {
        conn,
        session_config,
        ling_config,
        mcp_config,
        vad_config,
        cancellation_token,
        ..
    }): State<AppState>,
) -> Response {
    // Browser WebSocket API can't set custom headers; fallback to query param
    if !headers.contains_key(header::AUTHORIZATION)
        && let Some(val) = extract_bearer_from_query(uri.query())
    {
        headers.insert(header::AUTHORIZATION, val);
    }

    let Ok(principal) = verify_device_token(&headers) else {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    };

    ws.on_upgrade(move |socket| {
        let (write, read) = socket.split();
        handle_socket(
            SocketContext {
                session_id: gen_id(),
                device_id: Some(principal.id.clone()),
                conn,
                session_config,
                ling_config,
                mcp_config,
                vad_config,
                cancellation_token,
            },
            write,
            read,
        )
    })
    .into_response()
}

pub(crate) struct SocketContext {
    session_id: String,
    device_id: Option<String>,
    conn: sea_orm::DatabaseConnection,
    session_config: Arc<SessionConfig>,
    ling_config: Arc<LingConfig>,
    mcp_config: Arc<McpConfig>,
    vad_config: Arc<VadConfig>,
    cancellation_token: CancellationToken,
}

impl SocketContext {
    fn to_session_config(&self) -> session::SessionConfig {
        session::SessionConfig {
            silence_voice_timeout: self.session_config.silence_voice_timeout,
            close_connection_no_activity_time: self
                .session_config
                .close_connection_no_activity_time,
            barge_in_lockout_ms: self.session_config.barge_in_lockout_ms,
        }
    }
}

pub(crate) async fn handle_socket<W, R>(ctx: SocketContext, write: W, read: R)
where
    W: Sink<Message> + Unpin + Send + 'static,
    R: Stream<Item = Result<Message, axum::Error>> + Unpin + Send + 'static,
{
    tracing::info!(component = "WS", event = "session_started", session_id = %ctx.session_id, device_id = ?ctx.device_id, "session started");

    let mcp_ctx = setup_mcp_session(
        ctx.session_id.clone(),
        &ctx.mcp_config,
        ctx.device_id.clone(),
    )
    .await;

    let ling = Arc::new(
        LingCoreBuilder::new()
            .with_session_id(Some(ctx.session_id.clone()))
            .with_model(LlmManager::global().default())
            .with_mcp_registry(mcp_ctx.registry)
            .with_preamble(ctx.ling_config.system_prompt.clone())
            .build(),
    );
    let tts = TtsManager::global().default();
    let templates: Vec<Arc<dyn Node>> = vec![
        Arc::new(OpusDecodeNode::new()) as Arc<dyn Node>,
        Arc::new(VadNode::new(Arc::new(VadPool::new(ctx.vad_config.clone())))) as Arc<dyn Node>,
        Arc::new(AsrNode::new(AsrManager::global().default())) as Arc<dyn Node>,
        Arc::new(TurnNode::new()) as Arc<dyn Node>,
        Arc::new(LingNode::new(ling)) as Arc<dyn Node>,
        Arc::new(TtsNode::new(tts)) as Arc<dyn Node>,
    ];

    let session_ctx = SessionBuilder::new()
        .with_id(ctx.session_id.clone())
        .with_node_templates(templates)
        .with_config(ctx.to_session_config())
        .build();

    let recorder = Arc::new(Recorder::new(ctx.conn.clone(), ctx.device_id.clone()));
    let cancel = ctx.cancellation_token.child_token();

    let input_filters: Vec<Box<dyn InputFilter>> = vec![
        Box::new(McpRouterFilter::new(mcp_ctx.input_tx)),
        Box::new(RecorderInputFilter::new(
            Some(recorder.clone()),
            ctx.session_id.clone(),
        )),
    ];
    let output_filters: Vec<Box<dyn OutputFilter>> = vec![Box::new(RecorderOutputFilter::new(
        Some(recorder),
        ctx.session_id.clone(),
    ))];

    let mut output_streams: StreamMap<&str, UnboundedReceiverStream<OutputMessage>> =
        StreamMap::new();
    output_streams.insert(
        "session",
        UnboundedReceiverStream::new(session_ctx.output_rx),
    );
    output_streams.insert("mcp", UnboundedReceiverStream::new(mcp_ctx.output_rx));

    let translator = protocol_translator::XiaozhiProtocolTranslator;

    let session_handle = tokio::spawn(session_ctx.session.start());
    let output_handle = tokio::spawn(ws_output(
        write,
        translator,
        output_streams,
        output_filters,
        cancel.child_token(),
        ctx.session_id.clone(),
    ));
    let input_handle = tokio::spawn(ws_input(
        read,
        translator,
        session_ctx.input_tx,
        input_filters,
        cancel.child_token(),
        ctx.session_id.clone(),
    ));

    let _ = tokio::join!(session_handle, output_handle, input_handle);

    tracing::info!(component = "WS", event = "session_ended", "session ended");
}

async fn ws_input<R>(
    mut read: R,
    translator: impl ProtocolTranslator,
    input_tx: UnboundedSender<Frame>,
    filters: Vec<Box<dyn InputFilter>>,
    cancel: CancellationToken,
    session_id: String,
) where
    R: Stream<Item = Result<Message, axum::Error>> + Unpin + Send + 'static,
{
    loop {
        tokio::select! {
            biased;

            _ = cancel.cancelled() => {
                tracing::debug!(component = "WS", event = "input_cancelled", session_id = %session_id, "input cancelled");
                break;
            }

            msg = read.next() => {
                let Some(Ok(msg)) = msg else {
                    cancel.cancel();
                    break;
                };

                let ctx = FilterCtx { session_id: session_id.clone() };
                let frame = translator.input(msg);

                match run_input_filters(&filters, &ctx, frame).await {
                    FilterStep::Pass(frame) => {
                        let is_close = matches!(&frame, Frame::Close(_));
                        let _ = input_tx.send(frame);
                        if is_close {
                            cancel.cancel();
                            return;
                        }
                    }
                    FilterStep::Skip => continue,
                    FilterStep::Abort => { cancel.cancel(); break; }
                }
            }
        }
    }
}

async fn ws_output<W>(
    mut write: W,
    translator: impl ProtocolTranslator,
    mut output_streams: StreamMap<&str, UnboundedReceiverStream<OutputMessage>>,
    filters: Vec<Box<dyn OutputFilter>>,
    cancel: CancellationToken,
    session_id: String,
) where
    W: Sink<Message> + Unpin + Send + 'static,
{
    loop {
        tokio::select! {
            biased;

            _ = cancel.cancelled() => {
                tracing::debug!(component = "WS", event = "output_cancelled", session_id = %session_id, "output cancelled");
                break;
            }

            output = output_streams.next() => {
                let Some((_source, msg)) = output else { break };

                let ctx = FilterCtx { session_id: session_id.clone() };
                match run_output_filters(&filters, &ctx, msg).await {
                    FilterStep::Pass(msg) => {
                        if matches!(msg.payload, FrameResult::CloseResult) {
                            let close_frame = Message::Close(Some(
                                axum::extract::ws::CloseFrame {
                                    code: axum::extract::ws::close_code::NORMAL,
                                    reason: "session closed".into(),
                                },
                            ));
                            let _ = write.send(close_frame).await;
                            cancel.cancel();
                            break;
                        }
                        let ws_msg = translator.output(msg.payload);
                        if write.send(ws_msg).await.is_err() {
                            cancel.cancel();
                            break;
                        }
                    }
                    FilterStep::Skip => continue,
                    FilterStep::Abort => { cancel.cancel(); break; }
                }
            }
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
