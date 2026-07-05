use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

use crate::mcp::client::McpClient;
use crate::ws::session::round::OutputMessage;
use anyhow::Context;
use async_trait::async_trait;
use rig::{
    OneOrMany,
    completion::ToolDefinition,
    message::{ToolCall, ToolResult, ToolResultContent},
};
use rmcp::model::{
    CallToolRequest, CallToolRequestParams, CallToolResult, ClientCapabilities, ConstString,
    Implementation, InitializeRequest, InitializeRequestParams, InitializeResult, JsonObject,
    JsonRpcMessage, JsonRpcRequest, JsonRpcVersion2_0, ListToolsRequest, ListToolsResult,
    PaginatedRequestParams, ProtocolVersion, RawContent, Request, RequestId, Tool, object,
};
use serde::Serialize;
use service::chobits::message::{
    hello::HelloMessage,
    mcp::{McpMessage, McpRequest},
};
use service::ws::frame::FrameResult;
use tokio::sync::{
    Mutex,
    mpsc::{Receiver, UnboundedSender},
};

use tracing::{error, info};

#[derive(Debug, Clone)]
pub enum DeviceMcpPhase {
    Initialize,
    GetToolList,
    ToolCall,
}

pub struct DeviceMcpClient {
    session_id: Option<String>,
    current_request_id: Option<RequestId>,
    request_id: AtomicI64,
    next_cursor: Option<String>,
    pub tools: Vec<Tool>,
    pub phase: DeviceMcpPhase,
    output_tx: UnboundedSender<OutputMessage>,
    call_tool_result_rx: Arc<Mutex<Receiver<anyhow::Result<ToolResult>>>>,
}

#[async_trait]
impl McpClient for DeviceMcpClient {
    async fn get_tool(&self) -> anyhow::Result<Vec<ToolDefinition>> {
        let mut result = vec![];
        for tool in &self.tools {
            result.push(ToolDefinition {
                name: tool.name.to_string(),
                description: tool.description.clone().unwrap_or_default().to_string(),
                parameters: serde_json::to_value(tool.input_schema.clone())?,
            });
        }
        Ok(result)
    }

    async fn call_tool(&self, param: ToolCall) -> anyhow::Result<ToolResult> {
        let id = RequestId::Number(self.request_id.fetch_add(1, Ordering::Relaxed));
        let request = CallToolRequest::new(CallToolRequestParams {
            meta: None,
            name: param.function.name.clone().into(),
            arguments: Some(to_json_object(param.function.arguments.clone())),
            task: None,
        });
        let tx = self.output_tx.clone();
        let session_id = self.session_id.clone();
        let result = tx.send(OutputMessage {
            epoch: 0,
            round_id: None,
            session_id: session_id.clone().unwrap_or_default(),
            payload: Ok(FrameResult::McpResult(McpRequest::new(
                session_id,
                JsonRpcRequest {
                    jsonrpc: JsonRpcVersion2_0,
                    id,
                    request: Request {
                        method: request.method.as_str().to_string(),
                        params: to_json_object(request.params),
                        ..Default::default()
                    },
                },
            ))),
        });
        if result.is_err() {
            Err(anyhow::anyhow!(
                "tx send mcp tool call failure,name = {}",
                param.function.name
            ))
        } else {
            let call_tool_result_rx = self.call_tool_result_rx.clone();
            let mut call_tool_result_rx = call_tool_result_rx.lock().await;
            let result = call_tool_result_rx.recv().await;
            if let Some(result) = result {
                result
            } else {
                Err(anyhow::anyhow!("call tool result is none"))
            }
        }
    }
}

impl DeviceMcpClient {
    pub fn new(
        session_id: Option<String>,
        output_tx: UnboundedSender<OutputMessage>,
        call_tool_result_rx: Arc<Mutex<Receiver<anyhow::Result<ToolResult>>>>,
    ) -> Self {
        Self {
            session_id,
            current_request_id: None,
            request_id: AtomicI64::new(0),
            next_cursor: None,
            tools: Vec::new(),
            phase: DeviceMcpPhase::Initialize,
            output_tx,
            call_tool_result_rx,
        }
    }

    pub async fn create_initialize_request(&mut self) -> McpRequest {
        let id = RequestId::Number(self.request_id.fetch_add(1, Ordering::Relaxed));
        self.current_request_id = Some(id.clone());
        let request = InitializeRequest::new(InitializeRequestParams {
            meta: None,
            protocol_version: ProtocolVersion::V_2025_06_18,
            capabilities: ClientCapabilities {
                ..Default::default()
            },
            client_info: Implementation::from_build_env(),
        });
        let method = request.method.as_str().to_string();
        let params = object(serde_json::to_value(request.params).unwrap());
        McpRequest::new(
            self.session_id.clone(),
            JsonRpcRequest {
                jsonrpc: JsonRpcVersion2_0,
                id,
                request: Request {
                    method,
                    params,
                    ..Default::default()
                },
            },
        )
    }

    pub async fn handle_initialize_result(&mut self, message: &JsonRpcMessage) {
        // info!("message = {:?}", message.clone());
        let result = message.clone().into_response();
        match result {
            Some((response, id)) => {
                if let Some(current_request_id) = &self.current_request_id {
                    if current_request_id.clone().eq(&id) {
                        let response: InitializeResult =
                            serde_json::from_value(serde_json::Value::Object(response)).unwrap();
                        info!(
                            "name = {}, version = {}",
                            response.server_info.name, response.server_info.version
                        );
                        self.phase = DeviceMcpPhase::GetToolList;
                    } else {
                        error!(
                            "invalid id,current = {:?}, reponse = {:?}",
                            self.current_request_id, id
                        );
                    }
                } else {
                    error!("invalid id,current is None, reponse = {:?}", id);
                }
            }
            None => {
                error!("invalid mpc message = {:?}", message);
            }
        }
    }

    pub async fn create_tools_list_request(&mut self) -> McpRequest {
        let id = RequestId::Number(self.request_id.fetch_add(1, Ordering::Relaxed));
        self.current_request_id = Some(id.clone());
        let request = ListToolsRequest::with_param(PaginatedRequestParams {
            meta: None,
            cursor: self.next_cursor.clone(),
        });
        McpRequest::new(
            self.session_id.clone(),
            JsonRpcRequest {
                jsonrpc: JsonRpcVersion2_0,
                id,
                request: Request {
                    method: request.method.as_str().to_string(),
                    params: to_json_object(request.params),
                    ..Default::default()
                },
            },
        )
    }

    pub async fn handle_tools_list_result(&mut self, message: &JsonRpcMessage) -> bool {
        // info!("message = {:?}", message.clone());
        let result = message.clone().into_response();
        match result {
            Some((response, id)) => {
                if let Some(current_request_id) = &self.current_request_id {
                    if current_request_id.clone().eq(&id) {
                        let response: ListToolsResult =
                            serde_json::from_value(serde_json::Value::Object(response)).unwrap();
                        let next_cursor = response.next_cursor;
                        let mut tools = response.tools;
                        self.tools.append(&mut tools);
                        self.next_cursor = next_cursor.clone();
                        return next_cursor.is_some();
                    } else {
                        error!(
                            "invalid id,current = {:?}, reponse = {:?}",
                            self.current_request_id, id
                        );
                    }
                } else {
                    error!("invalid id,current is None, reponse = {:?}", id);
                }
            }
            None => {
                error!("invalid mpc message = {:?}", message);
            }
        }
        false
    }

    pub async fn handle_mcp(&mut self, message: &McpMessage) -> &DeviceMcpPhase {
        match self.phase {
            DeviceMcpPhase::Initialize => {
                self.handle_mcp_initialize_result(message).await;
                self.request_mcp_tools_list().await;
            }
            DeviceMcpPhase::GetToolList => {
                let has_next = self.handle_mcp_tools_list_result(message).await;
                if has_next {
                    self.request_mcp_tools_list().await;
                } else {
                    self.phase = DeviceMcpPhase::ToolCall;
                }
            }
            DeviceMcpPhase::ToolCall => {
                panic!("not support handle tool call in device mcp client");
            }
        }
        &self.phase
    }

    pub async fn handle_mcp_tool_call_result(message: &McpMessage) -> anyhow::Result<ToolResult> {
        let message = &message.payload;
        if let Some((response, id)) = message.clone().into_response() {
            let result: CallToolResult =
                serde_json::from_value(serde_json::Value::Object(response))
                    .with_context(|| anyhow::anyhow!("parse call tool result failure"))?;
            let content = result.content.first();
            if let Some(content) = content {
                match &content.raw {
                    RawContent::Text(raw_text_content) => {
                        let tool_result = ToolResult {
                            id: id.to_string(),
                            call_id: Some(id.to_string()),
                            content: OneOrMany::one(ToolResultContent::text(
                                raw_text_content.text.to_string(),
                            )),
                        };
                        Ok(tool_result)
                    }
                    _ => Err(anyhow::anyhow!(
                        "mcp tool call result content is not text type"
                    )),
                }
            } else {
                Err(anyhow::anyhow!("mcp tool call result content is none"))
            }
        } else {
            Err(anyhow::anyhow!("mcp tool call result is none"))
        }
    }

    pub async fn request_mcp_initialize(&mut self, _hello_message: &HelloMessage) {
        let tx = self.output_tx.clone();
        let request = self.create_initialize_request().await;
        let result = tx.send(OutputMessage {
            epoch: 0,
            round_id: None,
            session_id: self.session_id.clone().unwrap_or_default(),
            payload: Ok(FrameResult::McpResult(request)),
        });
        if result.is_err() {
            info!("tx send mcp initialize reqeust failure");
        }
    }

    async fn handle_mcp_initialize_result(&mut self, message: &McpMessage) {
        self.handle_initialize_result(&message.payload).await;
    }

    async fn request_mcp_tools_list(&mut self) {
        let tx = self.output_tx.clone();
        let result = tx.send(OutputMessage {
            epoch: 0,
            round_id: None,
            session_id: self.session_id.clone().unwrap_or_default(),
            payload: Ok(FrameResult::McpResult(
                self.create_tools_list_request().await,
            )),
        });
        if result.is_err() {
            info!("tx send mcp tools list reqeust failure");
        }
    }

    async fn handle_mcp_tools_list_result(&mut self, message: &McpMessage) -> bool {
        return self.handle_tools_list_result(&message.payload).await;
    }
}

fn to_json_object<T>(value: T) -> JsonObject
where
    T: Serialize,
{
    object(serde_json::to_value(value).unwrap())
}
