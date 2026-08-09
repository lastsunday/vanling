use serde::{Deserialize, Serialize};

/// A single external MCP server endpoint.
///
/// Exactly one auth mode is applied:
/// - `token`: static token sent to the server. It is a bare token: the Streamable
///   HTTP transport adds the `Bearer ` prefix itself.
/// - `self_signed = true`: server signs its own token against the local `/mcp`.
/// - neither: no Authorization header is sent (open server).
#[derive(Serialize, Deserialize, Clone)]
pub struct McpServerConfig {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(default)]
    pub self_signed: bool,
}

impl std::fmt::Debug for McpServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServerConfig")
            .field("uri", &self.uri)
            .field(
                "token",
                &self.token.as_ref().map(|_| "***********").unwrap_or("None"),
            )
            .field("self_signed", &self.self_signed)
            .finish()
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct McpConfig {
    #[serde(default)]
    pub server_list: Vec<McpServerConfig>,
}
