pub mod device_transport;
pub mod rmcp_device;
pub mod server;

use rmcp::transport::{
    StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
};

use server::ServerMcpClient;

pub async fn create_server_mcp_client(uri: String) -> anyhow::Result<ServerMcpClient> {
    let config = StreamableHttpClientTransportConfig::with_uri(uri);
    let transport = StreamableHttpClientTransport::from_config(config);
    let mut server_mcp_client = ServerMcpClient::new(transport).await?;
    server_mcp_client.init().await?;
    Ok(server_mcp_client)
}
