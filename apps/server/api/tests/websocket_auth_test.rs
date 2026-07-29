use std::sync::Arc;

use api::ws::verify_device_token;
use axum::http::{HeaderMap, header};
use framework::auth::{Jwt, Principal};
use framework::config::auth::AuthConfig;

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

fn headers_with_token(token: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(t) = token {
        headers.insert(
            header::AUTHORIZATION,
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
        device_id: Some(String::from("11:22:33:44:55:66")),
        token_type: String::from("device"),
    };
    let token = Jwt::global().access_token_encode(&principal).unwrap();
    let result = verify_device_token(&headers_with_token(Some(&token)));
    assert!(result.is_ok());
    let p = result.unwrap();
    assert_eq!(p.id, "test-device");
    assert_eq!(p.device_id.unwrap(), "11:22:33:44:55:66");
}
