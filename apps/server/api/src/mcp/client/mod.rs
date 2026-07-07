use async_trait::async_trait;
use rig::{
    completion::ToolDefinition,
    message::{ToolCall, ToolResult},
};
use rmcp::transport::{
    StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
};

pub mod device;
pub mod server;

use server::ServerMcpClient;

#[async_trait]
pub trait McpClient: Send + Sync {
    async fn get_tool(&self) -> anyhow::Result<Vec<ToolDefinition>>;

    async fn call_tool(&self, param: ToolCall) -> anyhow::Result<ToolResult>;
}

pub async fn create_server_mcp_client(uri: String) -> anyhow::Result<ServerMcpClient> {
    let config = StreamableHttpClientTransportConfig::with_uri(uri);
    let transport = StreamableHttpClientTransport::from_config(config);
    let mut server_mcp_client = ServerMcpClient::new(transport).await?;
    server_mcp_client.init().await?;
    Ok(server_mcp_client)
}
