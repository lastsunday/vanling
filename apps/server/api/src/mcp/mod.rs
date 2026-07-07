use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio_util::sync::CancellationToken;
use utoipa_axum::router::OpenApiRouter;

use crate::{AppState, mcp::tool::administrator::Administrator};

pub mod client;
pub mod mcp_host;
pub mod provider;
pub mod tool;

pub fn create_routes(state: AppState, cancellation_token: CancellationToken) -> OpenApiRouter {
    let service = StreamableHttpService::new(
        || Ok(Administrator::new()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig {
            cancellation_token,
            ..Default::default()
        },
    );

    OpenApiRouter::new()
        .nest_service("/mcp", service)
        //.layer(get_auth_layer())
        .with_state(state)
}
