use anyhow::Context;
use api::{
    mcp::client::{device::DeviceMcpClient, device_transport::DeviceMcpTransport},
    setup_mcp,
};
use rmcp::{
    ServiceExt as _rmcp_ServiceExt,
    model::{
        CallToolRequestParams, ClientCapabilities, Implementation, InitializeRequestParams,
        InitializeResult, JsonRpcResponse, JsonRpcVersion2_0, ServerCapabilities,
        ServerJsonRpcMessage, ServerResult,
    },
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use service::chobits::frame::{FrameResult, OutputMessage};
use tracing_test::traced_test;
use utoipa_axum::router::OpenApiRouter;

mod common;
use common::{setup_database, tear_down};

use crate::common::router_client::RouterClient;

#[tokio::test]
#[traced_test]
/// Validates DeviceMcpTransport + DeviceMcpClient initialization handshake
/// through mock channels, verifying the rmcp protocol flow:
///   initialize → init response → initialized notification → client ready
async fn test_device_mcp_transport_handshake() -> anyhow::Result<()> {
    let (inbound_tx, inbound_rx) = tokio::sync::mpsc::channel::<ServerJsonRpcMessage>(64);
    let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::unbounded_channel::<OutputMessage>();

    let transport = DeviceMcpTransport::new(inbound_rx, outbound_tx, "test-session".into());

    let client_handle = tokio::spawn(async move { DeviceMcpClient::new(transport).await });

    // ---- Step 1: Receive "initialize" request ----
    let msg = outbound_rx
        .recv()
        .await
        .context("expected initialize request")?;
    let init_req = match msg.payload {
        FrameResult::McpResult(req) => req,
        _ => anyhow::bail!("expected McpResult"),
    };
    assert_eq!(init_req.payload.request.method, "initialize");
    let init_id = init_req.payload.id;

    // ---- Step 2: Send InitializeResult response ----
    let init_result = InitializeResult::new(ServerCapabilities::default())
        .with_server_info(Implementation::new("test-device", "1.0.0"));
    inbound_tx
        .send(ServerJsonRpcMessage::Response(JsonRpcResponse {
            jsonrpc: JsonRpcVersion2_0,
            id: init_id,
            result: ServerResult::InitializeResult(init_result),
        }))
        .await
        .context("send initialize response")?;

    // ---- Step 3: Consume "notifications/initialized" (fire-and-forget, no response) ----
    let msg = outbound_rx
        .recv()
        .await
        .context("expected initialized notification")?;
    let notif = match msg.payload {
        FrameResult::McpResult(req) => req,
        _ => anyhow::bail!("expected McpResult"),
    };
    assert_eq!(notif.payload.request.method, "notifications/initialized");

    // ---- Step 4: Client should be ready ----
    drop(inbound_tx);
    let _client = client_handle
        .await
        .context("client task panicked")?
        .context("client init failed")?;

    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_administrator_mcp() -> anyhow::Result<()> {
    let (container, state) = setup_database().await;
    let router = OpenApiRouter::new();
    let ct = tokio_util::sync::CancellationToken::new();
    let router = setup_mcp(router, state.clone(), ct.child_token())
        .split_for_parts()
        .0;
    let config = StreamableHttpClientTransportConfig::with_uri("/mcp");
    let client = RouterClient { router };
    let transport = StreamableHttpClientTransport::with_client(client, config);
    let client_info = InitializeRequestParams::new(
        ClientCapabilities::default(),
        Implementation::new("test sse client", "0.0.1"),
    );
    let client = client_info.serve(transport).await.inspect_err(|e| {
        tracing::error!("client error: {:?}", e);
    })?;
    // Initialize
    let server_info = client.peer_info();
    tracing::info!("Connected to server: {server_info:#?}");

    // List tools
    let tools = client.list_tools(Default::default()).await?;
    tracing::info!("Available tools: {tools:#?}");

    let tool_name = "sum";
    let tool_result = client
        .call_tool(
            CallToolRequestParams::new(tool_name).with_arguments(
                serde_json::json!({"a": 1, "b": 2})
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            ),
        )
        .await?;
    tracing::info!("Tool({tool_name}) result: {tool_result:#?}");

    let tool_name = "datetime";
    let tool_result = client
        .call_tool(CallToolRequestParams::new(tool_name))
        .await?;
    tracing::info!("Tool({tool_name}) result: {tool_result:#?}");

    client.cancel().await?;

    let _ = &state.conn.close().await.unwrap();
    tear_down(container).await;

    Ok(())
}
