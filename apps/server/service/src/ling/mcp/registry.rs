use std::collections::HashMap;

use anyhow::Context;
use framework::error::AppError;
use std::sync::Arc;

use super::McpClient;
use crate::ling::llm::ToolDef;

pub struct McpRegistry {
    pub session_id: Option<String>,
    mcp_client_list: Vec<Arc<dyn McpClient>>,
}

impl McpRegistry {
    pub fn new(session_id: Option<String>) -> Self {
        Self {
            session_id: session_id.clone(),
            mcp_client_list: vec![],
        }
    }

    pub async fn add_client(&mut self, mcp_client: Arc<dyn McpClient>) {
        self.mcp_client_list.push(mcp_client);
    }

    pub async fn get_tool(&self) -> Result<Vec<ToolDef>, AppError> {
        let mut tools = Vec::<ToolDef>::new();
        for item in &self.mcp_client_list {
            let mut sub_items = item.get_tool().await?;
            tools.append(&mut sub_items);
        }
        Ok(tools)
    }

    pub async fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, AppError> {
        let mut function_name_and_client_map = HashMap::<String, Arc<dyn McpClient>>::new();

        for mcp_client in &self.mcp_client_list {
            let tools = mcp_client.get_tool().await?;
            for tool in tools {
                function_name_and_client_map.insert(tool.name.clone(), mcp_client.clone());
            }
        }

        let client = function_name_and_client_map
            .get(tool_name)
            .with_context(|| anyhow::anyhow!(format!("can't find function name = {}", tool_name)))
            .map_err(AppError::from)?;
        client.call_tool(tool_name, arguments).await
    }
}
