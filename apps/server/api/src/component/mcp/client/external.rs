use async_trait::async_trait;
use framework::error::AppError;
use rmcp::{
    RoleClient, ServiceExt,
    model::{
        CallToolRequestParams, ClientCapabilities, ContentBlock, Implementation,
        InitializeRequestParams, PaginatedRequestParams,
    },
    service::RunningService,
    transport::IntoTransport,
};
use service::component::llm::ToolDef;
use service::component::mcp::McpClient;

pub struct ExternalMcpClient {
    client: RunningService<RoleClient, InitializeRequestParams>,
    tools: Vec<ToolDef>,
}

impl ExternalMcpClient {
    pub async fn new<T, E, A>(transport: T) -> anyhow::Result<Self>
    where
        T: IntoTransport<RoleClient, E, A>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let client_info = InitializeRequestParams::new(
            ClientCapabilities::default(),
            Implementation::new("Server mcp client", "0.0.1"),
        );
        let client = client_info.serve(transport).await?;
        Ok(Self {
            client,
            tools: vec![],
        })
    }

    pub async fn init(&mut self) -> anyhow::Result<()> {
        let mut cursor = None;
        loop {
            let tools_result = self
                .client
                .list_tools(Some(PaginatedRequestParams::default().with_cursor(cursor)))
                .await?;
            for tool in tools_result.tools {
                self.tools.push(ToolDef {
                    name: tool.name.to_string(),
                    description: tool.description.unwrap_or_default().to_string(),
                    input_schema: serde_json::to_value(tool.input_schema)?,
                });
            }
            if let Some(next_cursor) = tools_result.next_cursor {
                cursor = Some(next_cursor);
            } else {
                break;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl McpClient for ExternalMcpClient {
    async fn get_tool(&self) -> Result<Vec<ToolDef>, AppError> {
        Ok(self.tools.clone())
    }

    async fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, AppError> {
        let function_json_text =
            serde_json::to_string(&serde_json::json!({"name": tool_name, "arguments": arguments}))
                .map_err(|e| AppError::from(anyhow::anyhow!(e)))?;
        let request: CallToolRequestParams = serde_json::from_str(function_json_text.as_str())
            .map_err(|e| AppError::from(anyhow::anyhow!(e)))?;
        let response = self
            .client
            .call_tool(request)
            .await
            .map_err(|e| AppError::from(anyhow::anyhow!(e)))?;

        let content = &response.content;
        match content.len() {
            0 => Err(AppError::from(anyhow::anyhow!(
                "call tool result must be not empty"
            ))),
            _ => {
                let item = content.first().unwrap();
                match item {
                    ContentBlock::Text(text_content) => Ok(text_content.text.clone()),
                    _ => Err(AppError::from(anyhow::anyhow!(
                        "tool call unsupported result type"
                    ))),
                }
            }
        }
    }
}
