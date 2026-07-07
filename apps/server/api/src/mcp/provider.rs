use std::sync::Arc;

use async_trait::async_trait;
use framework::error::AppError;
use rig::message::{ToolCall, ToolFunction, ToolResult, ToolResultContent};
use service::chobits::frame::OutputMessage;
use service::chobits::mcp::{Mcp, ToolDef};
use service::chobits::message::hello::HelloMessage;
use service::chobits::message::mcp::McpMessage;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{Sender, UnboundedSender, channel};

use crate::mcp::client::McpClient;
use crate::mcp::client::device::{DeviceMcpClient, DeviceMcpPhase};
use crate::mcp::mcp_host::{McpHost, UnionMcpHost};

pub struct McpProviderImpl {
    inner: Arc<Mutex<dyn McpHost>>,
    session_id: String,
    device_mcp_phase: DeviceMcpPhase,
    device_mcp_call_tool_result_tx: Option<Sender<anyhow::Result<ToolResult>>>,
}

impl McpProviderImpl {
    pub fn new(session_id: String) -> Self {
        Self {
            inner: Arc::new(Mutex::new(UnionMcpHost::new(Some(session_id.clone())))),
            session_id,
            device_mcp_phase: DeviceMcpPhase::Initialize,
            device_mcp_call_tool_result_tx: None,
        }
    }

    pub async fn add_client(&mut self, mcp_client: Box<dyn McpClient>) {
        self.inner.lock().await.add_client(mcp_client).await;
    }

    pub fn mcp_host(&self) -> Arc<Mutex<dyn McpHost>> {
        self.inner.clone()
    }
}

#[async_trait]
impl Mcp for McpProviderImpl {
    async fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, AppError> {
        let result = self
            .inner
            .lock()
            .await
            .call_tool(ToolCall {
                id: String::new(),
                call_id: None,
                function: ToolFunction {
                    name: tool_name.to_string(),
                    arguments: arguments.clone(),
                },
                signature: None,
                additional_params: None,
            })
            .await
            .map_err(AppError::from)?;

        let text = result
            .content
            .into_iter()
            .filter_map(|c| match c {
                ToolResultContent::Text(t) => Some(t.text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(text)
    }

    async fn get_tools(&self) -> Result<Vec<ToolDef>, AppError> {
        let tools = self
            .inner
            .lock()
            .await
            .get_tool()
            .await
            .map_err(AppError::from)?;
        Ok(tools
            .into_iter()
            .map(|t| ToolDef {
                name: t.name,
                description: t.description,
                input_schema: t.parameters,
            })
            .collect())
    }

    async fn handle_hello(
        &mut self,
        hello: &HelloMessage,
        output_tx: &UnboundedSender<OutputMessage>,
    ) {
        let (result_tx, result_rx) = channel::<anyhow::Result<ToolResult>>(1);
        self.device_mcp_call_tool_result_tx = Some(result_tx);

        let device = DeviceMcpClient::new(
            Some(self.session_id.clone()),
            output_tx.clone(),
            Arc::new(Mutex::new(result_rx)),
        );
        {
            let mut inner = self.inner.lock().await;
            inner.set_device_client(Arc::new(Mutex::new(device))).await;
        }
        let device_mcp_client = {
            let mut inner = self.inner.lock().await;
            inner
                .get_device_client()
                .await
                .clone()
                .expect("device mcp not exists")
        };
        let mut device_mcp_client = device_mcp_client.lock().await;
        device_mcp_client.request_mcp_initialize(hello).await;
    }

    async fn handle_frame(
        &mut self,
        msg: &McpMessage,
        _output_tx: &UnboundedSender<OutputMessage>,
    ) {
        match self.device_mcp_phase {
            DeviceMcpPhase::ToolCall => {
                let result = DeviceMcpClient::handle_mcp_tool_call_result(msg).await;
                let tx = self
                    .device_mcp_call_tool_result_tx
                    .clone()
                    .expect("device mcp call tool result tx not exists");
                if let Err(e) = tx.send(result).await {
                    tracing::error!(error = %e, "send device mcp tool call result");
                }
            }
            _ => {
                let device_mcp_client = {
                    let mut inner = self.inner.lock().await;
                    inner
                        .get_device_client()
                        .await
                        .clone()
                        .expect("device mcp not exists")
                };
                let mut device_mcp_client = device_mcp_client.lock().await;
                let phase = device_mcp_client.handle_mcp(msg).await.clone();
                self.device_mcp_phase = phase;
            }
        }
    }
}
