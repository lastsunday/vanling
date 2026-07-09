use api::AppState;
use api::config::ws::WsConfig;
use api::setup_default;
use api::setup_ota;
use axum::body::Body;
use axum::extract::Request;
use axum::extract::connect_info::MockConnectInfo;
use axum::http;
use common::response_to_json;
use core::option::Option;
use cucumber::then;
use cucumber::when;
use cucumber::{World, given};
use framework::auth::Jwt;
use framework::config::auth::AuthConfig;
use futures::FutureExt;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceExt;
use utoipa_axum::router::OpenApiRouter;
mod common;
use common::{setup_database, tear_down};
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;

use axum::{Router, http::StatusCode};

use crate::common::get_string_from_value;

const OTA_API_URL: &str = "/api/ota";

#[given("含有连接所需要的基本信息")]
async fn prepare_connect_info(world: &mut TestWorld) {
    world.prepare_connect_info_value = json!({
      "application": {
        "elf_sha256": "c8a8ecb6d6fbcda682494d9675cd1ead240ecf38bdde75282a42365a0e396033",
        "version": "1.0.1"
      },
      "board": {
        "channel": 1,
        "ip": "192.168.1.11",
        "mac": "11:22:33:44:55:66",
        "name": "bread-compact-wifi-128x64",
        "rssi": -55,
        "ssid": "卧室",
        "type": "bread-compact-wifi"
      }
    });
    world.device_id = String::from("11:22:33:44:55:66");
    world.client_id = String::from("7b94d69a-9808-4c59-9c9b-704333b38aff");
    world.user_agent = String::from("cube-1.54tft-wifi/1.0.1");
}

#[when(expr = "所有者进行连接信息查询")]
async fn query_connect_info(world: &mut TestWorld) {
    let builder = Request::builder()
        .method("POST")
        .uri(OTA_API_URL)
        .header(http::header::HOST, String::from("127.0.0.1:3000"))
        .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
        .header("Device-Id", world.device_id.clone())
        .header("Client-Id", world.client_id.clone())
        .header("User-Agent", world.user_agent.clone());
    let request = builder
        .body(Body::from(
            serde_json::to_string(&world.prepare_connect_info_value).unwrap(),
        ))
        .unwrap();
    let response = world.app.clone().unwrap().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let data = &response_to_json(response).await;
    let websocket = data.get("websocket").unwrap();
    world.ws_url = get_string_from_value(websocket, "url");
}

#[then(expr = "所有者获得连接地址")]
async fn get_connect_info(world: &mut TestWorld) {
    assert_eq!("ws://127.0.0.1:3000/chobits/v1", world.ws_url)
}

#[derive(Debug, Default, World)]
pub struct TestWorld {
    prepare_connect_info_value: serde_json::Value,
    device_id: String,
    client_id: String,
    user_agent: String,
    ws_url: String,
    container: Option<ContainerAsync<Postgres>>,
    app: Option<Router>,
    state: Option<AppState>,
}

#[tokio::test]
async fn main() {
    TestWorld::cucumber()
        .before(|_feature, _rule, _scenario, world| {
            async move {
                let (container, mut state) = setup_database().await;
                state.ws_config = Arc::new(WsConfig {
                    schema: Some(String::from("ws")),
                });
                world.container = container;
                world.state = Some(state.clone());
                Jwt::init(Arc::new(AuthConfig {
                    access_token_secret: Some(String::from("QLjJTeVblAlM47de")),
                    access_token_expires_in: Some(28800),
                    refresh_token_secret: Some(String::from("N8lI0uitNzJl6vYK")),
                    refresh_token_expires_in: Some(15897600),
                    audience: Some(String::from("audience")),
                    issuer: Some(String::from("issuer")),
                    client_id: Some(String::from("d1aicsr57dijo7h963ig")),
                    client_secret: Some(String::from("ujTgh2lEQYy0PXhK")),
                }));
                let app = OpenApiRouter::new();
                let app = setup_ota(app, state).split_for_parts().0;
                let app = setup_default(app);
                let app = app.layer(MockConnectInfo(SocketAddr::from(([0, 0, 0, 0], 1337))));
                world.app = Some(app);
            }
            .boxed()
        })
        .after(|_feature, _rule, _scenario, _ev, world| {
            async move {
                if let Some(world) = world {
                    tear_down(world.container.take()).await;
                }
            }
            .boxed()
        })
        .run("tests/features/communication/connect.feature")
        .await;
}
