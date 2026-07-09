pub mod registry;

pub use registry::*;

use async_trait::async_trait;
use framework::error::AppError;

pub use crate::chobits::llm::ToolDef;

#[async_trait]
pub trait McpClient: Send + Sync {
    async fn get_tool(&self) -> Result<Vec<ToolDef>, AppError>;

    async fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, AppError>;
}
