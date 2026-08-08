mod common;

use api::create_router;
use axum::{Router, extract::connect_info::MockConnectInfo, http::StatusCode};
use framework::{
    auth::Jwt,
    config::auth::AuthConfig,
    rate_limit::{FixedWindowConfig, UsageConfig, UsageRegistry},
};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use tokio_util::sync::CancellationToken;

use common::{
    get_json, get_json_paging_result_items, get_json_with_token, post_json, response_to_json,
    setup_database, tear_down,
};

const LOGIN_FAIL_LIMIT: u32 = 5;
const PER_IP_LIMIT: u32 = 20;

fn auth_config() -> AuthConfig {
    AuthConfig {
        access_token_secret: Some(String::from("test-secret")),
        access_token_expires_in: Some(28800),
        refresh_token_secret: Some(String::from("test-refresh-secret")),
        refresh_token_expires_in: Some(15897600),
        audience: Some(String::from("test-aud")),
        issuer: Some(String::from("test-iss")),
        client_id: Some(String::from("d1aicsr57dijo7h963ig")),
        client_secret: Some(String::from("ujTgh2lEQYy0PXhK")),
    }
}

fn admin_token() -> String {
    Jwt::global()
        .access_token_encode(&framework::auth::Principal {
            id: String::from("test-admin"),
            name: Some(String::from("root")),
            token_type: String::from("user"),
        })
        .expect("encode admin token")
}

async fn build_app() -> (Router, Option<ContainerAsync<Postgres>>) {
    let (container, state) = setup_database().await;
    Jwt::init(Arc::new(auth_config()));
    let (app, _ct) = create_router(state, CancellationToken::new());
    let app = app.layer(MockConnectInfo(SocketAddr::from(([10, 0, 0, 1], 1337))));
    (app, container)
}

#[tokio::test]
async fn test_security_events_requires_auth() {
    let (app, container) = build_app().await;
    let response = get_json(app, "/api/security/events").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    tear_down(container).await;
}

#[tokio::test]
async fn test_login_records_security_events() {
    let (app, container) = build_app().await;
    let token = admin_token();

    let failed = post_json(
        app.clone(),
        "/api/auth/login",
        &json!({"account": "root", "password": "WrongPass1"}),
    )
    .await;
    assert!(failed.status().is_client_error());

    let success = post_json(
        app.clone(),
        "/api/auth/login",
        &json!({"account": "root", "password": "Change_Me"}),
    )
    .await;
    assert_eq!(success.status(), StatusCode::OK);

    tokio::time::sleep(Duration::from_millis(300)).await;

    let response =
        get_json_with_token(app.clone(), "/api/security/events", Some(token.clone())).await;
    assert_eq!(response.status(), StatusCode::OK);
    let value = response_to_json(response).await;
    let items = get_json_paging_result_items(&value);
    assert!(
        items
            .iter()
            .any(|item| item["event_type"] == "auth_login_failure" && item["account"] == "root")
    );
    assert!(
        items
            .iter()
            .any(|item| item["event_type"] == "auth_login_success" && item["account"] == "root")
    );

    let response = get_json_with_token(
        app.clone(),
        "/api/security/events?event_type=auth_login_success",
        Some(token),
    )
    .await;
    let value = response_to_json(response).await;
    let items = get_json_paging_result_items(&value);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["event_type"], "auth_login_success");

    tear_down(container).await;
}

#[tokio::test]
async fn test_per_ip_rate_limit_returns_429() {
    let (app, container) = build_app().await;
    let token = admin_token();

    for i in 0..PER_IP_LIMIT {
        let account = format!("probe-{:02}", i);
        let response = post_json(
            app.clone(),
            "/api/auth/login",
            &json!({"account": account, "password": "Change_Me"}),
        )
        .await;
        assert!(
            response.status().is_client_error(),
            "request {} should be client error, got {:?}",
            i,
            response.status()
        );
        assert_ne!(
            response.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "request {} should not be rate limited yet",
            i
        );
        if i == 0 {
            assert_eq!(
                response.headers().get("x-ratelimit-resource").unwrap(),
                "auth"
            );
            assert_eq!(response.headers().get("x-ratelimit-used").unwrap(), "1");
        }
    }

    let response = post_json(
        app.clone(),
        "/api/auth/login",
        &json!({"account": "probe-over", "password": "Change_Me"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response.headers().get("x-ratelimit-remaining").unwrap(),
        "0"
    );
    assert_eq!(
        response.headers().get("x-ratelimit-limit").unwrap(),
        PER_IP_LIMIT.to_string().as_str()
    );
    assert_eq!(
        response.headers().get("x-ratelimit-used").unwrap(),
        PER_IP_LIMIT.to_string().as_str()
    );
    assert_eq!(
        response.headers().get("x-ratelimit-resource").unwrap(),
        "auth"
    );
    assert!(response.headers().get("retry-after").is_some());
    let value = response_to_json(response).await;
    assert_eq!(value["code"], 301006);

    tokio::time::sleep(Duration::from_millis(300)).await;

    let response = get_json_with_token(
        app,
        "/api/security/events?event_type=rate_limited",
        Some(token),
    )
    .await;
    let value = response_to_json(response).await;
    let items = get_json_paging_result_items(&value);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["ip"], "10.0.0.1");

    tear_down(container).await;
}

#[tokio::test]
async fn test_per_account_login_lockout() {
    let (app, container) = build_app().await;

    for i in 0..LOGIN_FAIL_LIMIT {
        let response = post_json(
            app.clone(),
            "/api/auth/login",
            &json!({"account": "lockout-probe", "password": "WrongPass1"}),
        )
        .await;
        assert_ne!(
            response.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "failure {} should not be rate limited yet",
            i
        );
    }

    let response = post_json(
        app.clone(),
        "/api/auth/login",
        &json!({"account": "lockout-probe", "password": "Change_Me"}),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "6th login attempt should be rejected even with correct password"
    );
    assert_eq!(
        response.headers().get("x-ratelimit-limit").unwrap(),
        LOGIN_FAIL_LIMIT.to_string().as_str()
    );
    assert_eq!(
        response.headers().get("x-ratelimit-remaining").unwrap(),
        "0"
    );
    assert_eq!(
        response.headers().get("x-ratelimit-resource").unwrap(),
        "auth"
    );

    tokio::time::sleep(Duration::from_millis(300)).await;

    let token = admin_token();
    let response = get_json_with_token(
        app,
        "/api/security/events?event_type=rate_limited",
        Some(token),
    )
    .await;
    let value = response_to_json(response).await;
    let items = get_json_paging_result_items(&value);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["account"], "lockout-probe");
    assert_eq!(items[0]["ip"], "10.0.0.1");

    tear_down(container).await;
}

#[tokio::test]
async fn test_per_user_core_rate_limit_returns_429() {
    let (container, mut state) = setup_database().await;
    Jwt::init(Arc::new(auth_config()));
    state.usage_registry = Arc::new(UsageRegistry::new(UsageConfig::new(
        FixedWindowConfig::new(PER_IP_LIMIT, Duration::from_secs(15 * 60)),
        FixedWindowConfig::new(30, Duration::from_secs(60)),
        FixedWindowConfig::new(3, Duration::from_secs(60 * 60)),
    )));
    let (app, _ct) = create_router(state, CancellationToken::new());
    let app = app.layer(MockConnectInfo(SocketAddr::from(([10, 0, 0, 2], 1337))));
    let token = admin_token();

    for i in 1..=3 {
        let response =
            get_json_with_token(app.clone(), "/api/security/events", Some(token.clone())).await;
        assert_eq!(response.status(), StatusCode::OK, "request {i} should pass");
        assert_eq!(
            response.headers().get("x-ratelimit-resource").unwrap(),
            "core"
        );
        assert_eq!(
            response.headers().get("x-ratelimit-used").unwrap(),
            i.to_string().as_str()
        );
    }

    let response = get_json_with_token(app.clone(), "/api/security/events", Some(token)).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers().get("x-ratelimit-limit").unwrap(), "3");
    assert_eq!(
        response.headers().get("x-ratelimit-remaining").unwrap(),
        "0"
    );
    assert_eq!(
        response.headers().get("x-ratelimit-resource").unwrap(),
        "core"
    );
    let value = response_to_json(response).await;
    assert_eq!(value["code"], 301006);

    tear_down(container).await;
}

#[tokio::test]
async fn test_rate_limit_introspection_does_not_consume_quota() {
    let (app, container) = build_app().await;
    let token = admin_token();

    let response =
        get_json_with_token(app.clone(), "/api/security/rate_limit", Some(token.clone())).await;
    assert_eq!(response.status(), StatusCode::OK);
    let value = response_to_json(response).await;
    let resources = &value["data"]["resources"];
    assert_eq!(resources["auth"]["limit"], 20);
    assert_eq!(resources["ota"]["limit"], 30);
    assert_eq!(resources["core"]["limit"], 1000);
    assert_eq!(resources["core"]["used"], 0);
    assert!(resources["core"]["reset"].as_i64().is_some());

    let response =
        get_json_with_token(app.clone(), "/api/security/events", Some(token.clone())).await;
    assert_eq!(response.status(), StatusCode::OK);

    let response =
        get_json_with_token(app.clone(), "/api/security/rate_limit", Some(token.clone())).await;
    let value = response_to_json(response).await;
    let core = &value["data"]["resources"]["core"];
    assert_eq!(core["used"], 1);

    let response =
        get_json_with_token(app.clone(), "/api/security/rate_limit", Some(token.clone())).await;
    let value = response_to_json(response).await;
    let core = &value["data"]["resources"]["core"];
    assert_eq!(core["used"], 1, "introspection must not consume quota");

    tear_down(container).await;
}
