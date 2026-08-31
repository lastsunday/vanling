use std::future::Future;

use anyhow::Context;
use rmcp::{
    RoleClient,
    model::{JsonObject, JsonRpcRequest, Request, RequestId, ServerJsonRpcMessage},
    service::{RxJsonRpcMessage, TxJsonRpcMessage},
    transport::Transport,
};
use serde_json::Value;
use service::frame::{FrameResult, OutputMessage};
use service::message::mcp::McpRequest;
use tokio::sync::mpsc::{Receiver, UnboundedSender};

pub struct DeviceMcpTransportError(pub anyhow::Error);

impl std::fmt::Debug for DeviceMcpTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl std::fmt::Display for DeviceMcpTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DeviceMcpTransportError: {}", self.0)
    }
}

impl std::error::Error for DeviceMcpTransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

pub struct DeviceMcpTransport {
    inbound_rx: Receiver<ServerJsonRpcMessage>,
    outbound_tx: UnboundedSender<OutputMessage>,
    session_id: String,
}

impl DeviceMcpTransport {
    pub fn new(
        inbound_rx: Receiver<ServerJsonRpcMessage>,
        outbound_tx: UnboundedSender<OutputMessage>,
        session_id: String,
    ) -> Self {
        Self {
            inbound_rx,
            outbound_tx,
            session_id,
        }
    }
}

impl Transport<RoleClient> for DeviceMcpTransport {
    type Error = DeviceMcpTransportError;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let tx = self.outbound_tx.clone();
        let session_id = self.session_id.clone();
        async move {
            let json = serde_json::to_value(&item)
                .context("serialize ClientJsonRpcMessage")
                .map_err(DeviceMcpTransportError)?;

            let method = json
                .get("method")
                .and_then(|v| v.as_str())
                .context("missing method")
                .map_err(DeviceMcpTransportError)?;

            let params_val = match json.get("params") {
                Some(Value::Object(obj)) => Value::Object(obj.clone()),
                _ => Value::Object(Default::default()),
            };

            let mut map = serde_json::Map::new();
            map.insert("method".into(), Value::String(method.to_string()));
            map.insert("params".into(), params_val);
            let clean = Value::Object(map);

            let request: Request<String, JsonObject> = serde_json::from_value(clean)
                .context("build Request<String, JsonObject>")
                .map_err(DeviceMcpTransportError)?;

            let id: Option<RequestId> = json
                .get("id")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .context("parse id")
                .map_err(DeviceMcpTransportError)?;

            tracing::trace!(
                session_id = %session_id,
                method = %method,
                direction = "outbound",
                "mcp send"
            );

            let mcp_request = McpRequest::new(
                Some(session_id.clone()),
                JsonRpcRequest::new(id.unwrap_or(RequestId::Number(-1)), request),
            );

            let output = OutputMessage {
                epoch: 0,
                round_id: None,
                session_id,
                payload: FrameResult::McpResult(mcp_request),
            };

            tx.send(output)
                .map_err(|e| DeviceMcpTransportError(anyhow::anyhow!("channel send: {}", e)))
        }
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleClient>> {
        let msg = self.inbound_rx.recv().await;
        if let Some(ref msg) = msg {
            let method = serde_json::to_value(msg)
                .ok()
                .and_then(|v| {
                    v.get("method")
                        .and_then(|m| m.as_str())
                        .map(|m| m.to_string())
                })
                .unwrap_or_default();
            tracing::trace!(
                session_id = %self.session_id,
                method = %method,
                direction = "inbound",
                "mcp receive"
            );
        }
        msg
    }

    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        std::future::ready(Ok(()))
    }
}
