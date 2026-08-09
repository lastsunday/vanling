mod common;

use api::create_router;
use axum::{
    Router,
    body::Body,
    extract::connect_info::MockConnectInfo,
    http::{Request, Response, StatusCode},
};
use framework::{
    auth::{Jwt, Principal},
    config::auth::AuthConfig,
    rate_limit::{FixedWindowConfig, UsageConfig, UsageRegistry},
};
use http_body_util::BodyExt;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

use common::{
    get_json, get_json_paging_result_items, get_json_with_token, post_json, post_json_without_body,
    response_to_json, setup_database, tear_down, wait_until,
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

fn auditor_token() -> String {
    Jwt::global()
        .access_token_encode(&framework::auth::Principal {
            id: String::from("test-auditor"),
            name: Some(String::from("auditor")),
            token_type: String::from("user"),
        })
        .expect("encode auditor token")
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

    wait_until(
        Duration::from_secs(5),
        Duration::from_millis(50),
        "login security events recorded",
        || async {
            let response =
                get_json_with_token(app.clone(), "/api/security/events", Some(token.clone())).await;
            let value = response_to_json(response).await;
            let items = get_json_paging_result_items(&value);
            items
                .iter()
                .any(|item| item["event_type"] == "auth_login_failure" && item["account"] == "root")
                && items.iter().any(|item| {
                    item["event_type"] == "auth_login_success" && item["account"] == "root"
                })
        },
    )
    .await
    .expect("login security events recorded");

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

    wait_until(
        Duration::from_secs(5),
        Duration::from_millis(50),
        "rate_limited security event recorded",
        || async {
            let response = get_json_with_token(
                app.clone(),
                "/api/security/events?event_type=rate_limited",
                Some(token.clone()),
            )
            .await;
            let value = response_to_json(response).await;
            let items = get_json_paging_result_items(&value);
            items.len() == 1 && items[0]["ip"] == "10.0.0.1"
        },
    )
    .await
    .expect("rate_limited security event recorded");

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

    let token = admin_token();
    wait_until(
        Duration::from_secs(5),
        Duration::from_millis(50),
        "lockout rate_limited security event recorded",
        || async {
            let response = get_json_with_token(
                app.clone(),
                "/api/security/events?event_type=rate_limited",
                Some(token.clone()),
            )
            .await;
            let value = response_to_json(response).await;
            let items = get_json_paging_result_items(&value);
            items.len() == 1
                && items[0]["account"] == "lockout-probe"
                && items[0]["ip"] == "10.0.0.1"
        },
    )
    .await
    .expect("lockout rate_limited security event recorded");

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
    state.usage_registry = Arc::new(UsageRegistry::new(UsageConfig::new(vec![
        (
            "auth".to_string(),
            FixedWindowConfig::new(PER_IP_LIMIT, Duration::from_secs(15 * 60)),
        ),
        (
            "ota".to_string(),
            FixedWindowConfig::new(30, Duration::from_secs(60)),
        ),
        (
            "core".to_string(),
            FixedWindowConfig::new(3, Duration::from_secs(60 * 60)),
        ),
    ])));
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

    let response =
        get_json_with_token(app.clone(), "/api/security/events", Some(token.clone())).await;
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

    wait_until(
        Duration::from_secs(5),
        Duration::from_millis(50),
        "core rate_limited event stores principal id and name",
        || async {
            let response = get_json_with_token(
                app.clone(),
                "/api/security/events?event_type=rate_limited",
                Some(auditor_token()),
            )
            .await;
            let value = response_to_json(response).await;
            let items = get_json_paging_result_items(&value);
            items.len() == 1
                && items[0]["account"] == "root"
                && items[0]["principal_id"] == "test-admin"
        },
    )
    .await
    .expect("core rate_limited event stores principal id and name");

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
    let resources = value["data"]["resources"]
        .as_array()
        .expect("resources array");
    let find = |name: &str| {
        resources
            .iter()
            .find(|resource| resource["name"] == name)
            .unwrap_or_else(|| panic!("resource {name} present"))
    };
    let auth = find("auth");
    assert_eq!(auth["limit"], 20);
    let ota = find("ota");
    assert_eq!(ota["limit"], 30);
    let mcp = find("mcp");
    assert_eq!(mcp["limit"], 1000);
    assert_eq!(mcp["used"], 0);
    let core = find("core");
    assert_eq!(core["limit"], 1000);
    assert_eq!(core["used"], 0);
    assert!(core["reset"].as_i64().is_some());

    let response =
        get_json_with_token(app.clone(), "/api/security/events", Some(token.clone())).await;
    assert_eq!(response.status(), StatusCode::OK);

    let response =
        get_json_with_token(app.clone(), "/api/security/rate_limit", Some(token.clone())).await;
    let value = response_to_json(response).await;
    let resources = value["data"]["resources"]
        .as_array()
        .expect("resources array");
    let core = resources
        .iter()
        .find(|resource| resource["name"] == "core")
        .expect("core present");
    assert_eq!(core["used"], 1);

    let response =
        get_json_with_token(app.clone(), "/api/security/rate_limit", Some(token.clone())).await;
    let value = response_to_json(response).await;
    let resources = value["data"]["resources"]
        .as_array()
        .expect("resources array");
    let core = resources
        .iter()
        .find(|resource| resource["name"] == "core")
        .expect("core present");
    assert_eq!(core["used"], 1, "introspection must not consume quota");

    tear_down(container).await;
}

#[tokio::test]
async fn test_access_logs_requires_auth() {
    let (app, container) = build_app().await;
    let response = get_json(app, "/api/security/access_logs").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    tear_down(container).await;
}

#[tokio::test]
async fn test_access_logs_recorded_and_queryable() {
    let (app, container) = build_app().await;
    let token = admin_token();

    let failed = post_json(
        app.clone(),
        "/api/auth/login",
        &json!({"account": "root", "password": "WrongPass1"}),
    )
    .await;
    assert!(failed.status().is_client_error());
    response_to_json(failed).await;

    let success = post_json(
        app.clone(),
        "/api/auth/login",
        &json!({"account": "root", "password": "Change_Me"}),
    )
    .await;
    assert_eq!(success.status(), StatusCode::OK);
    response_to_json(success).await;

    let mcp = post_json_without_body(app.clone(), "/mcp").await;
    assert_eq!(mcp.status(), StatusCode::UNAUTHORIZED);
    mcp.into_body().collect().await.unwrap();

    let events =
        get_json_with_token(app.clone(), "/api/security/events", Some(token.clone())).await;
    assert_eq!(events.status(), StatusCode::OK);
    response_to_json(events).await;

    wait_until(
        Duration::from_secs(5),
        Duration::from_millis(50),
        "access logs recorded",
        || async {
            let response = get_json_with_token(
                app.clone(),
                "/api/security/access_logs",
                Some(token.clone()),
            )
            .await;
            let value = response_to_json(response).await;
            let items = get_json_paging_result_items(&value);
            items.iter().any(|item| {
                item["method"] == "POST"
                    && item["path"] == "/api/auth/login"
                    && item["name"] == "root"
                    && item["principal_id"].is_null()
            }) && items.iter().any(|item| {
                item["method"] == "GET"
                    && item["path"] == "/api/security/events"
                    && item["name"] == "root"
                    && item["principal_id"] == "test-admin"
            }) && items
                .iter()
                .any(|item| item["method"] == "POST" && item["path"] == "/mcp")
        },
    )
    .await
    .expect("access logs recorded");

    let response = get_json_with_token(
        app.clone(),
        "/api/security/access_logs",
        Some(token.clone()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let value = response_to_json(response).await;
    let items = get_json_paging_result_items(&value);
    assert!(items.iter().any(|item| item["method"] == "POST"
        && item["path"] == "/api/auth/login"
        && item["name"] == "root"
        && item["principal_id"].is_null()));
    assert!(items.iter().any(|item| {
        item["method"] == "GET"
            && item["path"] == "/api/security/events"
            && item["name"] == "root"
            && item["principal_id"] == "test-admin"
    }));
    assert!(
        items
            .iter()
            .any(|item| item["method"] == "POST" && item["path"] == "/mcp")
    );
    assert!(
        items
            .iter()
            .all(|item| item["duration_ms"].as_i64().is_some())
    );

    let response = get_json_with_token(
        app.clone(),
        "/api/security/access_logs?method=GET",
        Some(token.clone()),
    )
    .await;
    let value = response_to_json(response).await;
    let items = get_json_paging_result_items(&value);
    assert!(!items.is_empty());
    assert!(items.iter().all(|item| item["method"] == "GET"));

    let response = get_json_with_token(
        app.clone(),
        "/api/security/access_logs?path=%2Fapi%2Fauth%2Flogin",
        Some(token.clone()),
    )
    .await;
    let value = response_to_json(response).await;
    let items = get_json_paging_result_items(&value);
    assert!(!items.is_empty());
    assert!(items.iter().all(|item| item["path"] == "/api/auth/login"));

    let response = get_json_with_token(
        app,
        "/api/security/access_logs?principal_id=test-admin",
        Some(token),
    )
    .await;
    let value = response_to_json(response).await;
    let items = get_json_paging_result_items(&value);
    assert!(!items.is_empty());
    assert!(
        items
            .iter()
            .all(|item| item["principal_id"] == "test-admin")
    );

    tear_down(container).await;
}

#[tokio::test]
async fn test_usage_stats_reports_allowed_and_limited() {
    let (container, mut state) = setup_database().await;
    Jwt::init(Arc::new(auth_config()));
    state.usage_registry = Arc::new(UsageRegistry::new(UsageConfig::new(vec![
        (
            "auth".to_string(),
            FixedWindowConfig::new(3, Duration::from_secs(15 * 60)),
        ),
        (
            "ota".to_string(),
            FixedWindowConfig::new(30, Duration::from_secs(60)),
        ),
        (
            "core".to_string(),
            FixedWindowConfig::new(1000, Duration::from_secs(60 * 60)),
        ),
    ])));
    let (app, _ct) = create_router(state, CancellationToken::new());
    let app = app.layer(MockConnectInfo(SocketAddr::from(([10, 0, 0, 3], 1337))));
    let token = admin_token();

    for i in 0..3 {
        let response = post_json(
            app.clone(),
            "/api/auth/login",
            &json!({"account": format!("usage-probe-{i}"), "password": "WrongPass1"}),
        )
        .await;
        assert_ne!(
            response.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "request {i} should be allowed"
        );
        response_to_json(response).await;
    }

    let response = post_json(
        app.clone(),
        "/api/auth/login",
        &json!({"account": "usage-over", "password": "WrongPass1"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    response_to_json(response).await;

    let response = get_json_with_token(
        app.clone(),
        "/api/security/usage_stats",
        Some(token.clone()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let value = response_to_json(response).await;
    let resources = value["data"]["resources"]
        .as_array()
        .expect("resources array");
    let auth = resources
        .iter()
        .find(|resource| resource["name"] == "auth")
        .expect("auth present");
    assert_eq!(auth["allowed"], 3);
    assert_eq!(auth["limited"], 1);
    assert_eq!(auth["active_keys"], 1);

    tear_down(container).await;
}

async fn post_mcp(app: Router, token: &str) -> Response<Body> {
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(
            axum::http::header::ACCEPT,
            "application/json, text/event-stream",
        )
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"unknown_method","params":{}}"#,
        ))
        .unwrap();
    app.oneshot(request).await.unwrap()
}

async fn drain(response: Response<Body>) {
    tokio::time::timeout(Duration::from_secs(5), response.into_body().collect())
        .await
        .expect("response body must finish")
        .unwrap();
}

#[tokio::test]
async fn test_mcp_rate_limited_per_principal() {
    let (container, mut state) = setup_database().await;
    Jwt::init(Arc::new(auth_config()));
    state.usage_registry = Arc::new(UsageRegistry::new(UsageConfig::new(vec![
        (
            "auth".to_string(),
            FixedWindowConfig::new(20, Duration::from_secs(15 * 60)),
        ),
        (
            "ota".to_string(),
            FixedWindowConfig::new(30, Duration::from_secs(60)),
        ),
        (
            "mcp".to_string(),
            FixedWindowConfig::new(3, Duration::from_secs(60 * 60)),
        ),
        (
            "core".to_string(),
            FixedWindowConfig::new(1000, Duration::from_secs(60 * 60)),
        ),
    ])));
    let (app, _ct) = create_router(state, CancellationToken::new());
    let app = app.layer(MockConnectInfo(SocketAddr::from(([10, 0, 0, 5], 1337))));

    let token = Jwt::global()
        .access_token_encode(&Principal {
            id: String::from("rate-limit-device"),
            name: Some(String::from("probe")),
            token_type: String::from("mcp"),
        })
        .expect("encode mcp token");

    for i in 0..3 {
        let response = post_mcp(app.clone(), &token).await;
        assert_ne!(
            response.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "request {i} should be allowed"
        );
        drain(response).await;
    }

    let response = post_mcp(app, &token).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response.headers().get("x-ratelimit-resource").unwrap(),
        "mcp"
    );
    drain(response).await;

    tear_down(container).await;
}

#[tokio::test]
async fn test_dashboard_summary_includes_security_metrics() {
    let (app, container) = build_app().await;
    let token = admin_token();

    let failed = post_json(
        app.clone(),
        "/api/auth/login",
        &json!({"account": "root", "password": "WrongPass1"}),
    )
    .await;
    response_to_json(failed).await;

    wait_until(
        Duration::from_secs(5),
        Duration::from_millis(50),
        "security event visible in dashboard summary",
        || async {
            let response =
                get_json_with_token(app.clone(), "/api/stats/summary", Some(token.clone())).await;
            let value = response_to_json(response).await;
            value["data"]["security_events_today"].as_u64().unwrap_or(0) > 0
        },
    )
    .await
    .expect("security event visible in dashboard summary");

    let response = get_json_with_token(app.clone(), "/api/stats/summary", Some(token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let value = response_to_json(response).await;
    let data = &value["data"];
    assert!(data["security_events_today"].as_u64().is_some());
    assert!(data["security_events_7d"].as_u64().is_some());
    assert!(data["rate_limited_today"].as_u64().is_some());
    assert!(data["recent_security_events"].is_array());

    tear_down(container).await;
}

#[tokio::test]
async fn test_security_events_filter_by_time_range() {
    let (app, container) = build_app().await;
    let token = admin_token();

    let before = chrono::Local::now().fixed_offset();

    let failed = post_json(
        app.clone(),
        "/api/auth/login",
        &json!({"account": "range-probe", "password": "WrongPass1"}),
    )
    .await;
    response_to_json(failed).await;

    wait_until(
        Duration::from_secs(5),
        Duration::from_millis(50),
        "login security event recorded",
        || async {
            let response =
                get_json_with_token(app.clone(), "/api/security/events", Some(token.clone())).await;
            let value = response_to_json(response).await;
            !get_json_paging_result_items(&value).is_empty()
        },
    )
    .await
    .expect("login security event recorded");

    let after = chrono::Local::now().fixed_offset();

    let response = get_json_with_token(
        app.clone(),
        &format!(
            "/api/security/events?start={}&end={}",
            before.to_rfc3339().replace('+', "%2B"),
            after.to_rfc3339().replace('+', "%2B")
        ),
        Some(token.clone()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let value = response_to_json(response).await;
    let items = get_json_paging_result_items(&value);
    assert!(
        !items.is_empty(),
        "event recorded between before/after should be returned"
    );

    let response = get_json_with_token(
        app.clone(),
        &format!(
            "/api/security/events?start={}&end={}",
            after.to_rfc3339().replace('+', "%2B"),
            after.to_rfc3339().replace('+', "%2B")
        ),
        Some(token.clone()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let value = response_to_json(response).await;
    let items = get_json_paging_result_items(&value);
    assert!(items.is_empty(), "future range should not match the event");

    let response = get_json_with_token(
        app.clone(),
        &format!(
            "/api/security/events?start={}&end={}",
            (before - chrono::Duration::days(1))
                .to_rfc3339()
                .replace('+', "%2B"),
            before.to_rfc3339().replace('+', "%2B")
        ),
        Some(token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let value = response_to_json(response).await;
    assert!(
        get_json_paging_result_items(&value).is_empty(),
        "range before the event should not match it"
    );

    tear_down(container).await;
}

#[tokio::test]
async fn test_security_events_filter_by_ip_account_path() {
    let (app, container) = build_app().await;
    let token = admin_token();

    let response = post_json(
        app.clone(),
        "/api/auth/login",
        &json!({"account": "probe-acc-a", "password": "WrongPass1"}),
    )
    .await;
    response_to_json(response).await;
    let response = post_json(
        app.clone(),
        "/api/auth/login",
        &json!({"account": "probe-acc-b", "password": "WrongPass1"}),
    )
    .await;
    response_to_json(response).await;

    wait_until(
        Duration::from_secs(5),
        Duration::from_millis(50),
        "login security events recorded",
        || async {
            let response =
                get_json_with_token(app.clone(), "/api/security/events", Some(token.clone())).await;
            let value = response_to_json(response).await;
            get_json_paging_result_items(&value).len() >= 2
        },
    )
    .await
    .expect("login security events recorded");

    let response = get_json_with_token(
        app.clone(),
        "/api/security/events?account=probe-acc-a",
        Some(token.clone()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let items = get_json_paging_result_items(&response_to_json(response).await);
    assert!(!items.is_empty());
    assert!(items.iter().all(|item| item["account"] == "probe-acc-a"));

    let response = get_json_with_token(
        app.clone(),
        "/api/security/events?path=%2Fapi%2Fauth%2Flogin",
        Some(token.clone()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let items = get_json_paging_result_items(&response_to_json(response).await);
    assert!(items.iter().all(|item| item["path"] == "/api/auth/login"));

    let response = get_json_with_token(app, "/api/security/events?ip=10.0.0.1", Some(token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let items = get_json_paging_result_items(&response_to_json(response).await);
    assert!(!items.is_empty());
    assert!(items.iter().all(|item| item["ip"] == "10.0.0.1"));

    tear_down(container).await;
}
