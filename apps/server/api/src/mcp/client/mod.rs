pub mod device;
pub mod device_transport;
pub mod external;

use framework::auth::{Jwt, Principal};
use rmcp::transport::{
    StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
};

use external::ExternalMcpClient;

use crate::config::mcp::McpServerConfig;

pub async fn create_external_mcp_client(
    uri: String,
    auth_token: Option<String>,
) -> anyhow::Result<ExternalMcpClient> {
    let mut config = StreamableHttpClientTransportConfig::with_uri(uri);
    config.auth_header = auth_token;
    let transport = StreamableHttpClientTransport::from_config(config);
    let mut external_mcp_client = ExternalMcpClient::new(transport).await?;
    external_mcp_client.init().await?;
    Ok(external_mcp_client)
}

/// Resolve the raw auth token for an MCP server entry:
/// explicit token, otherwise a locally self-signed token when `self_signed` is
/// set, otherwise no token (open server).
///
/// `subject_id` is embedded in the self-signed token so the local `/mcp`
/// endpoint can attribute requests to the real device/user identity (audit
/// trail and rate limiting).
///
/// The returned token is bare (without a `Bearer ` prefix): the rmcp Streamable
/// HTTP transport adds the `Bearer ` prefix itself via `bearer_auth()`.
pub fn resolve_mcp_auth_token(server: &McpServerConfig, subject_id: &str) -> Option<String> {
    if let Some(token) = &server.token {
        return Some(token.clone());
    }
    if server.self_signed {
        return Jwt::global()
            .access_token_encode(&Principal {
                id: subject_id.to_string(),
                name: None,
                token_type: String::from("mcp"),
            })
            .ok();
    }
    None
}
