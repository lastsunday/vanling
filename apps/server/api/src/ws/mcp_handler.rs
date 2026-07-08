use std::sync::Arc;

use service::chobits::frame::{Frame, OutputMessage};
use tokio::sync::Mutex;

use crate::config::mcp::McpConfig;
use crate::mcp::client::{
    create_server_mcp_client, device_transport::DeviceMcpTransport, rmcp_device::RmcpDeviceClient,
};

pub(crate) use rmcp::model::ServerJsonRpcMessage;

pub(crate) struct McpContext {
    pub registry: Arc<Mutex<service::chobits::mcp::McpRegistry>>,
    pub inbound_tx: tokio::sync::mpsc::Sender<ServerJsonRpcMessage>,
    pub outbound_rx: tokio::sync::mpsc::UnboundedReceiver<OutputMessage>,
}

pub(crate) async fn setup_mcp_handler(session_id: String, mcp_config: &McpConfig) -> McpContext {
    let (mcp_inbound_tx, mcp_inbound_rx) = tokio::sync::mpsc::channel::<ServerJsonRpcMessage>(64);
    let (mcp_outbound_tx, mcp_outbound_rx) =
        tokio::sync::mpsc::unbounded_channel::<OutputMessage>();

    let registry = Arc::new(Mutex::new(service::chobits::mcp::McpRegistry::new(Some(
        session_id.clone(),
    ))));

    if let Some(uri_list) = &mcp_config.uri_list {
        for uri in uri_list {
            let server_mcp_client = create_server_mcp_client(uri.to_string()).await;
            match server_mcp_client {
                Ok(client) => {
                    registry.lock().await.add_client(Arc::new(client)).await;
                }
                Err(e) => {
                    tracing::warn!(
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
    let transport =
        DeviceMcpTransport::new(mcp_inbound_rx, mcp_outbound_tx, mcp_session_id.clone());
    tokio::spawn(async move {
        match RmcpDeviceClient::new(transport).await {
            Ok(client) => {
                let client: Arc<dyn service::chobits::mcp::McpClient> = Arc::new(client);
                let mut reg = mcp_registry.lock().await;
                reg.add_client(client).await;
                tracing::debug!(
                    session_id = %mcp_session_id,
                    "device mcp client initialized"
                );
            }
            Err(e) => {
                tracing::warn!(
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
        inbound_tx: mcp_inbound_tx,
        outbound_rx: mcp_outbound_rx,
    }
}

pub(crate) enum McpFrameAction {
    Handled,
    ChannelClosed,
    NotMcp,
}

pub(crate) async fn handle_mcp_frame(
    frame: &Frame,
    inbound_tx: &tokio::sync::mpsc::Sender<ServerJsonRpcMessage>,
) -> McpFrameAction {
    let Frame::Mcp(mcp_msg) = frame else {
        return McpFrameAction::NotMcp;
    };

    match serde_json::to_value(&mcp_msg.payload) {
        Ok(value) => match serde_json::from_value(value) {
            Ok(server_msg) => {
                if inbound_tx.send(server_msg).await.is_err() {
                    return McpFrameAction::ChannelClosed;
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to convert mcp payload");
            }
        },
        Err(e) => {
            tracing::warn!(error = %e, "failed to serialize mcp payload");
        }
    }
    McpFrameAction::Handled
}
