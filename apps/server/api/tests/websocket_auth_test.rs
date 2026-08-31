use std::net::SocketAddr;
use std::sync::Arc;

use api::component::asr::AsrManager;
use api::component::llm::LlmManager;
use api::component::tts::TtsManager;
use api::component::vad::VadManager;
use api::config::AsrModel;
use api::config::LlmProvider;
use api::config::TtsModel;
use api::config::VadModel;
use api::config::asr::AsrConfig;
use api::config::audio::AudioConfig;
use api::config::llm::LlmConfig;
use api::config::tts::TtsConfig;
use api::config::vad::VadConfig;
use api::setup_ws;
use api::ws::verify_device_token;
use axum::extract::connect_info::MockConnectInfo;
use framework::auth::{Jwt, Principal};
use framework::config::auth::AuthConfig;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use utoipa_axum::router::OpenApiRouter;

mod common;

fn jwt_config() -> AuthConfig {
    AuthConfig {
        access_token_secret: Some(String::from("test-secret")),
        access_token_expires_in: Some(28800),
        refresh_token_secret: Some(String::from("test-refresh-secret")),
        refresh_token_expires_in: Some(15897600),
        audience: Some(String::from("test-aud")),
        issuer: Some(String::from("test-iss")),
        ..Default::default()
    }
}

fn headers_with_token(token: Option<&str>) -> axum::http::HeaderMap {
    let mut headers = axum::http::HeaderMap::new();
    if let Some(t) = token {
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {t}").parse().unwrap(),
        );
    }
    headers
}

#[tokio::test]
async fn test_rejects_no_token() {
    Jwt::init(Arc::new(jwt_config()));
    let result = verify_device_token(&headers_with_token(None));
    assert!(result.is_err());
}

#[tokio::test]
async fn test_rejects_invalid_token() {
    Jwt::init(Arc::new(jwt_config()));
    let result = verify_device_token(&headers_with_token(Some("invalid_token")));
    assert!(result.is_err());
}

#[tokio::test]
async fn test_accepts_valid_device_token() {
    Jwt::init(Arc::new(jwt_config()));
    let principal = Principal {
        id: String::from("test-device"),
        name: Some(String::from("test-board")),
        token_type: String::from("device"),
    };
    let token = Jwt::global().access_token_encode(&principal).unwrap();
    let result = verify_device_token(&headers_with_token(Some(&token)));
    assert!(result.is_ok());
    let p = result.unwrap();
    assert_eq!(p.id, "test-device");
}
async fn init_test_managers() {
    TtsManager::init(
        Arc::new(TtsConfig {
            model: Some(TtsModel::Mute),
            ..Default::default()
        }),
        Arc::new(AudioConfig::default()),
    )
    .await
    .ok();
    VadManager::init(Arc::new(VadConfig {
        model: Some(VadModel::Void),
        ..Default::default()
    }))
    .await;
    AsrManager::init(Arc::new(AsrConfig {
        model: Some(AsrModel::Void),
        ..Default::default()
    }))
    .await;
    LlmManager::init(Arc::new(LlmConfig {
        provider: Some(LlmProvider::LocalEcho),
        ..Default::default()
    }))
    .await;
}

async fn start_ws_server() -> SocketAddr {
    Jwt::init(Arc::new(jwt_config()));
    init_test_managers().await;
    let (_, state) = common::setup_database().await;
    let app = OpenApiRouter::new();
    let app = setup_ws(app, state);
    let (app, _) = app.split_for_parts();
    let app = app.layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

async fn send_ws_upgrade_request(addr: SocketAddr, uri: &str, token: Option<&str>) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let mut request = format!(
        "GET {uri} HTTP/1.1\r\n\
         Host: localhost\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n"
    );
    if let Some(t) = token {
        request.push_str(&format!("Authorization: Bearer {t}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line).await.unwrap();
    status_line.trim().to_string()
}

#[tokio::test]
async fn test_ws_upgrade_rejects_no_token() {
    let addr = start_ws_server().await;
    let status = send_ws_upgrade_request(addr, "/vanling/v1", None).await;
    assert!(
        status.contains("401"),
        "Expected 401 Unauthorized, got: {status}"
    );
}

#[tokio::test]
async fn test_ws_upgrade_rejects_invalid_token() {
    let addr = start_ws_server().await;
    let status = send_ws_upgrade_request(addr, "/vanling/v1", Some("invalid_token")).await;
    assert!(
        status.contains("401"),
        "Expected 401 Unauthorized, got: {status}"
    );
}

#[tokio::test]
async fn test_ws_upgrade_accepts_valid_token() {
    let addr = start_ws_server().await;
    let principal = Principal {
        id: String::from("test-device"),
        name: Some(String::from("test-board")),
        token_type: String::from("device"),
    };
    let token = Jwt::global().access_token_encode(&principal).unwrap();
    let status = send_ws_upgrade_request(addr, "/vanling/v1", Some(&token)).await;
    assert!(
        status.contains("101"),
        "Expected 101 Switching Protocols, got: {status}"
    );
}

#[tokio::test]
async fn test_ws_upgrade_accepts_query_param_token() {
    let addr = start_ws_server().await;
    let principal = Principal {
        id: String::from("test-device"),
        name: Some(String::from("test-board")),
        token_type: String::from("device"),
    };
    let token = Jwt::global().access_token_encode(&principal).unwrap();
    let uri = format!("/vanling/v1?authorization=Bearer+{token}");
    let status = send_ws_upgrade_request(addr, &uri, None).await;
    assert!(
        status.contains("101"),
        "Expected 101 Switching Protocols, got: {status}"
    );
}
