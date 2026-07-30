use std::sync::Arc;

use async_trait::async_trait;
use serde_json;
use service::ling::frame::{Frame, OutputMessage};
use tokio::sync::Mutex;

use crate::config::mcp::McpConfig;
use crate::mcp::client::{
    create_external_mcp_client, device::DeviceMcpClient, device_transport::DeviceMcpTransport,
};
use crate::ws::filter::{FilterAction, FilterCtx, InputFilter};

pub(crate) use rmcp::model::ServerJsonRpcMessage;

pub(crate) struct McpContext {
    pub registry: Arc<Mutex<service::ling::mcp::McpRegistry>>,
    pub input_tx: tokio::sync::mpsc::Sender<ServerJsonRpcMessage>,
    pub output_rx: tokio::sync::mpsc::UnboundedReceiver<OutputMessage>,
}

pub(crate) async fn setup_mcp_session(session_id: String, mcp_config: &McpConfig) -> McpContext {
    let (mcp_input_tx, mcp_input_rx) = tokio::sync::mpsc::channel::<ServerJsonRpcMessage>(64);
    let (mcp_output_tx, mcp_output_rx) = tokio::sync::mpsc::unbounded_channel::<OutputMessage>();

    let registry = Arc::new(Mutex::new(service::ling::mcp::McpRegistry::new(Some(
        session_id.clone(),
    ))));

    if let Some(uri_list) = &mcp_config.uri_list {
        for uri in uri_list {
            let external_mcp_client = create_external_mcp_client(uri.to_string()).await;
            match external_mcp_client {
                Ok(client) => {
                    registry.lock().await.add_client(Arc::new(client)).await;
                }
                Err(e) => {
                    tracing::warn!(
                        component = "MCP", event = "mcp_server_init_failed",
                        session_id = %session_id,
                        uri = %uri,
                        error = %e,
                        "mcp server init failed"
                    );
                }
            }
        }
    }

    let mcp_registry = registry.clone();
    let mcp_session_id = session_id.clone();
    let transport = DeviceMcpTransport::new(mcp_input_rx, mcp_output_tx, mcp_session_id.clone());
    tokio::spawn(async move {
        match DeviceMcpClient::new(transport).await {
            Ok(client) => {
                let client: Arc<dyn service::ling::mcp::McpClient> = Arc::new(client);
                let mut reg = mcp_registry.lock().await;
                reg.add_client(client).await;
                tracing::debug!(
                    component = "MCP", event = "device_mcp_client_initialized",
                    session_id = %mcp_session_id,
                    "device mcp client initialized"
                );
            }
            Err(e) => {
                tracing::warn!(
                    component = "MCP", event = "device_mcp_client_init_failed",
                    session_id = %mcp_session_id,
                    error = %e,
                    "device mcp client init failed"
                );
            }
        }
        std::future::pending::<()>().await;
    });

    McpContext {
        registry,
        input_tx: mcp_input_tx,
        output_rx: mcp_output_rx,
    }
}

pub(crate) struct McpRouterFilter {
    input_tx: tokio::sync::mpsc::Sender<ServerJsonRpcMessage>,
}

impl McpRouterFilter {
    pub(crate) fn new(input_tx: tokio::sync::mpsc::Sender<ServerJsonRpcMessage>) -> Self {
        Self { input_tx }
    }
}

#[async_trait]
impl InputFilter for McpRouterFilter {
    async fn process(&self, _ctx: &FilterCtx, frame: Frame) -> FilterAction<Frame> {
        let Frame::Mcp(mcp_msg) = frame else {
            return FilterAction::Continue(frame);
        };

        match serde_json::to_value(&mcp_msg.payload) {
            Ok(value) => match serde_json::from_value(value) {
                Ok(server_msg) => {
                    if self.input_tx.send(server_msg).await.is_err() {
                        return FilterAction::Break;
                    }
                }
                Err(e) => {
                    tracing::warn!(component = "MCP", event = "mcp_payload_convert_failed", error = %e, "failed to convert mcp payload");
                }
            },
            Err(e) => {
                tracing::warn!(component = "MCP", event = "mcp_payload_serialize_failed", error = %e, "failed to serialize mcp payload");
            }
        }

        FilterAction::Consumed
    }
}
