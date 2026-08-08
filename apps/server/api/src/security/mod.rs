use std::net::SocketAddr;
use std::time::Duration;

use axum::{
    debug_handler,
    extract::{ConnectInfo, Extension, Query, Request, State, connect_info::MockConnectInfo},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::Local;
use entity::{
    prelude::*,
    security_event::{self, SecurityEventType},
};
use framework::{
    auth::{Jwt, Principal},
    data::{ApiResponse, PageData, PageParam, paginate},
    error::{AppResult, framework_code::FrameworkErrorCode},
    middleware::get_auth_layer,
    rate_limit::{
        BucketSnapshot, RateLimitDecision, Resource, SlidingWindowConfig, SlidingWindowCounter,
    },
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, QueryFilter, QueryOrder,
    prelude::*,
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;

const TAG: &str = "security";

/// Per-account login failure lockout: 5 failures within 15 minutes.
const LOGIN_FAIL_LIMIT: u32 = 5;
const LOGIN_FAIL_WINDOW: Duration = Duration::from_secs(15 * 60);

const RETENTION_DAYS: i64 = 30;
const CLEANUP_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

fn login_fail_counter() -> &'static SlidingWindowCounter {
    static COUNTER: std::sync::OnceLock<SlidingWindowCounter> = std::sync::OnceLock::new();
    COUNTER.get_or_init(|| {
        SlidingWindowCounter::new(SlidingWindowConfig::new(
            LOGIN_FAIL_LIMIT,
            LOGIN_FAIL_WINDOW,
        ))
    })
}

/// Resolves the rate-limit bucket (resource + identity key + account) for a
/// request. Public auth/ota endpoints are keyed per-IP; authenticated `/api/*`
/// endpoints are keyed per-user and only counted when the bearer token is
/// valid. `GET /api/security/rate_limit` never counts against any bucket.
fn resolve_bucket(
    path: &str,
    request: &Request,
    ip: &str,
) -> Option<(Resource, String, Option<String>)> {
    match path {
        "/api/auth/login" | "/api/auth/access_token" => {
            Some((Resource::Auth, format!("ip:{ip}"), None))
        }
        "/api/ota" | "/api/ota/activate" => Some((Resource::Ota, format!("ip:{ip}"), None)),
        "/api/security/rate_limit" => None,
        _ if path.starts_with("/api/") => bearer_principal(request).map(|principal| {
            (
                Resource::Core,
                format!("user:{}", principal.id),
                Some(principal.id),
            )
        }),
        _ => None,
    }
}

fn extract_ip(request: &Request) -> String {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip().to_string())
        .or_else(|| {
            request
                .extensions()
                .get::<MockConnectInfo<SocketAddr>>()
                .map(|info| info.0.ip().to_string())
        })
        .unwrap_or_else(|| String::from("unknown"))
}

fn bearer_principal(request: &Request) -> Option<Principal> {
    let token = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?;
    Jwt::try_global()?.access_token_decode(token).ok()
}

/// Unified security middleware: applies GitHub-style per-bucket rate limits,
/// the per-account login failure lockout, response usage headers and login
/// outcome audit events.
pub async fn security_middleware(
    State(AppState {
        conn,
        usage_registry,
        ..
    }): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();
    let ip = extract_ip(&request);
    let is_login = path == "/api/auth/login";

    let bucket = resolve_bucket(&path, &request, &ip);

    let allowed_bucket = match &bucket {
        Some((resource, key, account)) => match usage_registry.check(*resource, key) {
            RateLimitDecision::Limited { limit, retry_after } => {
                tracing::warn!(
                    component = "RATELIMIT",
                    event = "rate_limited",
                    ip = %ip,
                    path = %path,
                    account = %account.as_deref().unwrap_or("-"),
                    retry_after_ms = retry_after.as_millis() as i64,
                    limit = limit as i64,
                    "request rate limited"
                );
                record_event(
                    &conn,
                    rate_limited_model(
                        SecurityEventType::RateLimited,
                        &ip,
                        &path,
                        account.as_deref(),
                        LimitInfo {
                            retry_after,
                            limit,
                            remaining: 0,
                            window_secs: usage_registry.window_secs(*resource),
                        },
                    ),
                );
                return build_limited_response(*resource, limit, 0, retry_after);
            }
            RateLimitDecision::Allowed { .. } => usage_registry.peek(*resource, key),
        },
        None => None,
    };

    // Buffer the login body so the per-account lockout can be checked before
    // the handler runs, then rebuild the request for the downstream handler.
    let mut login_account: Option<String> = None;
    let request = if is_login {
        let (parts, body) = request.into_parts();
        let bytes = axum::body::to_bytes(body, usize::MAX)
            .await
            .unwrap_or_default();
        let account = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|value| {
                value
                    .get("account")
                    .and_then(|a| a.as_str())
                    .map(String::from)
            });
        let request = Request::from_parts(parts, axum::body::Body::from(bytes));
        if let Some(ref account) = account {
            login_account = Some(account.clone());
            if let Err(e) = login_fail_counter().record(account) {
                tracing::warn!(
                    component = "RATELIMIT",
                    event = "rate_limited",
                    account = %account,
                    ip = %ip,
                    path = %path,
                    retry_after_ms = e.retry_after.as_millis() as i64,
                    limit = LOGIN_FAIL_LIMIT as i64,
                    "account temporarily locked after repeated login failures"
                );
                record_event(
                    &conn,
                    rate_limited_model(
                        SecurityEventType::RateLimited,
                        &ip,
                        &path,
                        Some(account),
                        LimitInfo {
                            retry_after: e.retry_after,
                            limit: LOGIN_FAIL_LIMIT,
                            remaining: 0,
                            window_secs: LOGIN_FAIL_WINDOW.as_secs(),
                        },
                    ),
                );
                return build_limited_response(Resource::Auth, LOGIN_FAIL_LIMIT, 0, e.retry_after);
            }
        }
        request
    } else {
        request
    };

    let response = next.run(request).await;
    let status = response.status();
    let mut response = response;

    if let (Some((resource, _, _)), Some(snapshot)) = (&bucket, allowed_bucket) {
        apply_usage_headers(response.headers_mut(), *resource, snapshot);
        if snapshot.remaining <= 1 {
            tracing::info!(
                component = "RATELIMIT",
                event = "rate_limit_near",
                ip = %ip,
                path = %path,
                remaining = snapshot.remaining,
                limit = snapshot.limit as i64,
                "request near rate limit"
            );
            record_event(
                &conn,
                rate_limited_model(
                    SecurityEventType::RateLimitNear,
                    &ip,
                    &path,
                    None,
                    LimitInfo {
                        retry_after: snapshot.reset_after,
                        limit: snapshot.limit,
                        remaining: snapshot.remaining,
                        window_secs: usage_registry.window_secs(*resource),
                    },
                ),
            );
        }
    }

    if is_login && let Some(account) = login_account {
        if status.is_success() {
            login_fail_counter().clear(&account);
            record_login_success(&conn, &account, &ip);
        } else if status == StatusCode::BAD_REQUEST {
            record_login_failure(&conn, &account, &ip);
        }
    }

    response
}

struct LimitInfo {
    retry_after: Duration,
    limit: u32,
    remaining: u32,
    window_secs: u64,
}

fn rate_limited_model(
    event_type: SecurityEventType,
    ip: &str,
    path: &str,
    account: Option<&str>,
    info: LimitInfo,
) -> security_event::ActiveModel {
    security_event::ActiveModel {
        event_type: Set(event_type),
        ip: Set(Some(ip.to_string())),
        path: Set(Some(path.to_string())),
        account: Set(account.map(String::from)),
        retry_after_ms: Set(Some(info.retry_after.as_millis() as i64)),
        limit: Set(Some(info.limit as i64)),
        remaining: Set(Some(info.remaining as i64)),
        window_secs: Set(Some(info.window_secs as i64)),
        ..Default::default()
    }
}

fn build_limited_response(
    resource: Resource,
    limit: u32,
    remaining: u32,
    retry_after: Duration,
) -> Response {
    let body = axum::Json(ApiResponse::<()>::error(
        FrameworkErrorCode::RateLimited.code() as i32,
        FrameworkErrorCode::RateLimited.message(),
    ));
    let retry_after_secs = retry_after.as_secs().max(1);
    let mut response = (StatusCode::TOO_MANY_REQUESTS, body).into_response();
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::RETRY_AFTER,
        HeaderValue::from_str(&retry_after_secs.to_string()).expect("retry-after is valid"),
    );
    headers.insert(
        "x-ratelimit-limit",
        HeaderValue::from_str(&limit.to_string()).expect("limit is valid"),
    );
    headers.insert(
        "x-ratelimit-remaining",
        HeaderValue::from_str(&remaining.to_string()).expect("remaining is valid"),
    );
    headers.insert(
        "x-ratelimit-used",
        HeaderValue::from_str(&(limit.saturating_sub(remaining)).to_string())
            .expect("used is valid"),
    );
    headers.insert(
        "x-ratelimit-reset",
        HeaderValue::from_str(&(Local::now().timestamp() + retry_after_secs as i64).to_string())
            .expect("reset is valid"),
    );
    headers.insert(
        "x-ratelimit-resource",
        HeaderValue::from_str(resource.as_str()).expect("resource is valid"),
    );
    response
}

fn apply_usage_headers(headers: &mut HeaderMap, resource: Resource, snapshot: BucketSnapshot) {
    let reset = Local::now().timestamp() + snapshot.reset_after.as_secs() as i64;
    headers.insert(
        "x-ratelimit-limit",
        HeaderValue::from_str(&snapshot.limit.to_string()).expect("limit is valid"),
    );
    headers.insert(
        "x-ratelimit-remaining",
        HeaderValue::from_str(&snapshot.remaining.to_string()).expect("remaining is valid"),
    );
    headers.insert(
        "x-ratelimit-used",
        HeaderValue::from_str(&snapshot.used.to_string()).expect("used is valid"),
    );
    headers.insert(
        "x-ratelimit-reset",
        HeaderValue::from_str(&reset.to_string()).expect("reset is valid"),
    );
    headers.insert(
        "x-ratelimit-resource",
        HeaderValue::from_str(resource.as_str()).expect("resource is valid"),
    );
}

pub fn record_login_failure(conn: &DatabaseConnection, account: &str, ip: &str) {
    record_event(
        conn,
        security_event::ActiveModel {
            event_type: Set(SecurityEventType::AuthLoginFailure),
            ip: Set(Some(ip.to_string())),
            account: Set(Some(account.to_string())),
            ..Default::default()
        },
    );
}

pub fn record_login_success(conn: &DatabaseConnection, account: &str, ip: &str) {
    record_event(
        conn,
        security_event::ActiveModel {
            event_type: Set(SecurityEventType::AuthLoginSuccess),
            ip: Set(Some(ip.to_string())),
            account: Set(Some(account.to_string())),
            ..Default::default()
        },
    );
}

fn record_event(conn: &DatabaseConnection, model: security_event::ActiveModel) {
    let conn = conn.clone();
    tokio::spawn(async move {
        if let Err(e) = model.insert(&conn).await {
            tracing::warn!(component = "SECURITY", event = "record_event_failed", error = %e, "failed to persist security event");
        }
    });
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SecurityEventListParams {
    #[param(example = "1")]
    pub page: Option<u64>,
    #[param(example = "20")]
    pub page_size: Option<u64>,
    /// 事件类型筛选：rate_limited / rate_limit_near / auth_login_success / auth_login_failure
    pub event_type: Option<SecurityEventType>,
    /// IP 模糊匹配
    pub ip: Option<String>,
}

#[debug_handler]
#[utoipa::path(get, path = "/security/events", tag = TAG, params(SecurityEventListParams), responses(
    (status=OK,body=ApiResponse<PageData<security_event::Model>>)
))]
async fn list_events(
    State(AppState { conn, .. }): State<AppState>,
    Query(params): Query<SecurityEventListParams>,
) -> AppResult<ApiResponse<PageData<security_event::Model>>> {
    let pagination = PageParam::new(params.page, params.page_size);

    let mut query = SecurityEvent::find();

    if let Some(event_type) = params.event_type {
        query = query.filter(security_event::Column::EventType.eq(event_type));
    }
    if let Some(ref ip) = params.ip {
        query = query.filter(security_event::Column::Ip.like(format!("%{ip}%")));
    }

    let query = query
        .order_by_desc(security_event::Column::CreateDatetime)
        .order_by_desc(security_event::Column::Id);
    let result = paginate(query, &conn, &pagination).await?;

    Ok(ApiResponse::success(Some(result)))
}

#[derive(Debug, Serialize, ToSchema, Default)]
pub struct RateLimitBucketInfo {
    limit: u32,
    used: u32,
    remaining: u32,
    reset: i64,
}

#[derive(Debug, Serialize, ToSchema, Default)]
pub struct RateLimitResult {
    resources: RateLimitResources,
}

#[derive(Debug, Serialize, ToSchema, Default)]
pub struct RateLimitResources {
    auth: RateLimitBucketInfo,
    ota: RateLimitBucketInfo,
    core: RateLimitBucketInfo,
}

#[debug_handler]
#[utoipa::path(get, path = "/security/rate_limit", tag = TAG, security(()), responses(
    (status=OK,body=ApiResponse<RateLimitResult>)
))]
async fn rate_limit(
    State(AppState { usage_registry, .. }): State<AppState>,
    Extension(principal): Extension<Principal>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResult<ApiResponse<RateLimitResult>> {
    let ip = addr.ip().to_string();
    let bucket = |resource: Resource, key: String| {
        let limit = usage_registry.limit(resource);
        let window_secs = usage_registry.window_secs(resource);
        match usage_registry.peek(resource, &key) {
            Some(snapshot) => RateLimitBucketInfo {
                limit,
                used: snapshot.used,
                remaining: snapshot.remaining,
                reset: Local::now().timestamp() + snapshot.reset_after.as_secs() as i64,
            },
            None => RateLimitBucketInfo {
                limit,
                used: 0,
                remaining: limit,
                reset: Local::now().timestamp() + window_secs as i64,
            },
        }
    };
    Ok(ApiResponse::success(Some(RateLimitResult {
        resources: RateLimitResources {
            auth: bucket(Resource::Auth, format!("ip:{ip}")),
            ota: bucket(Resource::Ota, format!("ip:{ip}")),
            core: bucket(Resource::Core, format!("user:{}", principal.id)),
        },
    })))
}

pub fn create_routes(state: AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(list_events))
        .routes(routes!(rate_limit))
        .route_layer(get_auth_layer())
        .with_state(state)
}

/// Deletes security events older than the retention window.
async fn cleanup_old_events(conn: &DatabaseConnection) -> Result<u64, sea_orm::DbErr> {
    let cutoff = Local::now().fixed_offset() - chrono::Duration::days(RETENTION_DAYS);
    SecurityEvent::delete_many()
        .filter(security_event::Column::CreateDatetime.lt(cutoff))
        .exec(conn)
        .await
        .map(|result| result.rows_affected)
}

/// Background retention task for the append-only security event table.
pub async fn cleanup_loop(conn: DatabaseConnection, ct: CancellationToken) {
    let mut interval = tokio::time::interval(CLEANUP_INTERVAL);
    loop {
        tokio::select! {
            _ = ct.cancelled() => {
                tracing::info!(component = "SECURITY", event = "cleanup_stopped", "security event cleanup stopped");
                break;
            }
            _ = interval.tick() => {
                match cleanup_old_events(&conn).await {
                    Ok(deleted) => tracing::info!(component = "SECURITY", event = "cleanup_done", deleted = deleted, "security event cleanup done"),
                    Err(e) => tracing::warn!(component = "SECURITY", event = "cleanup_failed", error = %e, "security event cleanup failed"),
                }
            }
        }
    }
}
