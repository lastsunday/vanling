pub mod device;
pub mod device_transport;
pub mod external;

use rmcp::transport::{
    StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
};

use external::ExternalMcpClient;

pub async fn create_external_mcp_client(uri: String) -> anyhow::Result<ExternalMcpClient> {
    let config = StreamableHttpClientTransportConfig::with_uri(uri);
    let transport = StreamableHttpClientTransport::from_config(config);
    let mut external_mcp_client = ExternalMcpClient::new(transport).await?;
    external_mcp_client.init().await?;
    Ok(external_mcp_client)
}
