use async_trait::async_trait;
use framework::error::AppError;
use tokio::sync::mpsc::UnboundedSender;

use crate::chobits::frame::OutputMessage;
use crate::chobits::message::hello::HelloMessage;
use crate::chobits::message::mcp::McpMessage;

pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[async_trait]
pub trait Mcp: Send + Sync {
    async fn get_tools(&self) -> Result<Vec<ToolDef>, AppError>;
    async fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, AppError>;
    async fn handle_hello(
        &mut self,
        hello: &HelloMessage,
        output_tx: &UnboundedSender<OutputMessage>,
    );
    async fn handle_frame(&mut self, msg: &McpMessage, output_tx: &UnboundedSender<OutputMessage>);
}
