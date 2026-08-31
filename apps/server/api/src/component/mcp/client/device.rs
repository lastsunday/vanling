use std::sync::Arc;

use async_trait::async_trait;
use framework::error::AppError;
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, ContentBlock},
    service::RunningService,
};
use service::component::llm::ToolDef;
use service::component::mcp::McpClient;

use super::device_transport::DeviceMcpTransport;

pub struct DeviceMcpClient {
    service: RunningService<RoleClient, ()>,
}

impl DeviceMcpClient {
    pub async fn new(transport: DeviceMcpTransport) -> Result<Self, AppError> {
        let service = ()
            .serve(transport)
            .await
            .map_err(|e| AppError::from(anyhow::anyhow!("device mcp init: {:?}", e)))?;
        Ok(Self { service })
    }

    pub fn into_arc_client(self) -> Arc<dyn McpClient> {
        Arc::new(self)
    }
}

#[async_trait]
impl McpClient for DeviceMcpClient {
    async fn get_tool(&self) -> Result<Vec<ToolDef>, AppError> {
        let result = self
            .service
            .list_all_tools()
            .await
            .map_err(|e| AppError::from(anyhow::anyhow!("list tools: {}", e)))?;
        Ok(result
            .into_iter()
            .map(|tool| ToolDef {
                name: tool.name.to_string(),
                description: tool.description.unwrap_or_default().to_string(),
                input_schema: serde_json::to_value(tool.input_schema).unwrap_or_default(),
            })
            .collect())
    }

    async fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, AppError> {
        let request: CallToolRequestParams = serde_json::from_value(serde_json::json!({
            "name": tool_name,
            "arguments": arguments,
        }))
        .map_err(|e| AppError::from(anyhow::anyhow!("build params: {}", e)))?;

        let response = self
            .service
            .call_tool(request)
            .await
            .map_err(|e| AppError::from(anyhow::anyhow!("call tool: {}", e)))?;

        match response.content.first() {
            Some(ContentBlock::Text(text)) => Ok(text.text.clone()),
            _ => Err(AppError::from(anyhow::anyhow!(
                "unsupported tool result type"
            ))),
        }
    }
}
