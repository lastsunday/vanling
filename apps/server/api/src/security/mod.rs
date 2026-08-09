use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::{
    body::Body,
    debug_handler,
    extract::{ConnectInfo, Extension, Query, Request, State, connect_info::MockConnectInfo},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::{Local, Timelike};
use entity::{
    api_access_log,
    prelude::*,
    security_event::{self, SecurityEventType},
};
use framework::{
    auth::{Jwt, Principal},
    data::{ApiResponse, PageData, PageParam, paginate},
    error::{AppResult, framework_code::FrameworkErrorCode},
    middleware::get_auth_layer,
    rate_limit::{BucketSnapshot, RateLimitDecision, SlidingWindowConfig, SlidingWindowCounter},
};
use http_body::Frame;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::config::security::{RateLimitKeyBy, RateLimitMatcher, SecurityConfig};

const TAG: &str = "security";

fn login_fail_counter(security: &SecurityConfig) -> &'static SlidingWindowCounter {
    static COUNTER: std::sync::OnceLock<SlidingWindowCounter> = std::sync::OnceLock::new();
    COUNTER.get_or_init(|| {
        SlidingWindowCounter::new(SlidingWindowConfig::new(
            security.login_fail_limit,
            Duration::from_secs(security.login_fail_window_secs),
        ))
    })
}

/// Resolved rate-limit bucket: resource name + identity key for the usage
/// registry, plus the authenticated principal identity and display name for the
/// audit trail. Public auth/ota endpoints are keyed per-IP and carry no
/// principal; authenticated `/api/*` endpoints are keyed per-user
/// (`user:{id}`) with the principal fields recorded. `GET
/// /api/security/rate_limit` never counts.
struct BucketKey {
    resource: String,
    key: String,
    principal_id: Option<String>,
    name: Option<String>,
}

fn resolve_bucket(
    path: &str,
    ip: &str,
    principal: Option<&Principal>,
    matchers: &[RateLimitMatcher],
) -> Option<BucketKey> {
    if path == "/api/security/rate_limit" {
        return None;
    }
    let matcher = matchers.iter().find(|matcher| matcher.matches(path))?;
    if !matcher.count {
        return None;
    }
    let (key, principal_id, name) = match matcher.key_by {
        RateLimitKeyBy::Ip => (format!("ip:{ip}"), None, None),
        RateLimitKeyBy::Principal => {
            let principal = principal?;
            (
                format!("user:{}", principal.id),
                Some(principal.id.clone()),
                principal.name.clone(),
            )
        }
    };
    Some(BucketKey {
        resource: matcher.name.clone(),
        key,
        principal_id,
        name,
    })
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

/// Metadata captured at request entry and persisted once the response body has
/// been fully streamed. Kept in memory while the response streams so that the
/// recorded response size covers the whole payload.
#[derive(Clone)]
struct PendingAccessLog {
    conn: DatabaseConnection,
    request_id: String,
    method: String,
    path: String,
    query: Option<String>,
    ip: String,
    principal_id: Option<String>,
    name: Option<String>,
    status: i32,
    duration_ms: i64,
    user_agent: Option<String>,
}

/// Wraps the response body, counting streamed bytes and persisting the access
/// log once the stream completes. Recording at stream end guarantees the stored
/// response size matches what the client actually received.
struct CountedBody<B> {
    inner: B,
    pending: Arc<Mutex<Option<PendingAccessLog>>>,
    size: Arc<AtomicU64>,
    done: Arc<AtomicBool>,
}

impl<B> http_body::Body for CountedBody<B>
where
    B: http_body::Body + Unpin,
    B::Data: AsRef<[u8]>,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let poll = Pin::new(&mut self.inner).poll_frame(cx);
        match &poll {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    self.size
                        .fetch_add(data.as_ref().len() as u64, Ordering::Relaxed);
                }
            }
            Poll::Ready(None) if !self.done.swap(true, Ordering::Relaxed) => {
                let size = self.size.load(Ordering::Relaxed);
                if let Some(log) = self.pending.lock().unwrap().take() {
                    tokio::spawn(async move {
                        insert_access_log(&log, size).await;
                    });
                }
            }
            _ => {}
        }
        poll
    }
}

async fn insert_access_log(log: &PendingAccessLog, response_size: u64) {
    let conn = log.conn.clone();
    let model = api_access_log::ActiveModel {
        request_id: Set(log.request_id.clone()),
        method: Set(log.method.clone()),
        path: Set(log.path.clone()),
        query: Set(log.query.clone()),
        ip: Set(Some(log.ip.clone())),
        principal_id: Set(log.principal_id.clone()),
        name: Set(log.name.clone()),
        status: Set(log.status),
        duration_ms: Set(log.duration_ms),
        response_size: Set(Some(response_size as i64)),
        user_agent: Set(log.user_agent.clone()),
        ..Default::default()
    };
    if let Err(e) = model.insert(&conn).await {
        tracing::warn!(
            component = "ACCESS",
            event = "record_access_log_failed",
            request_id = %log.request_id,
            path = %log.path,
            error = %e,
            "failed to persist api access log"
        );
    }
}

/// Fills in the response status/duration and swaps in the counting body. Any
/// response body (including the rate-limited early returns) can be passed.
fn finish_access_log(
    access_log: Option<PendingAccessLog>,
    started: Instant,
    response: Response,
) -> Response {
    let mut log = match access_log {
        Some(log) => log,
        None => return response,
    };
    log.status = response.status().as_u16() as i32;
    log.duration_ms = started.elapsed().as_millis() as i64;
    tracing::info!(
        component = "ACCESS",
        event = "access_log",
        request_id = %log.request_id,
        method = %log.method,
        path = %log.path,
        ip = %log.ip,
        principal_id = %log.principal_id.as_deref().unwrap_or("-"),
        name = %log.name.as_deref().unwrap_or("-"),
        status = log.status,
        duration_ms = log.duration_ms,
        "api request access logged"
    );
    let pending = Arc::new(Mutex::new(Some(log)));
    let size = Arc::new(AtomicU64::new(0));
    let done = Arc::new(AtomicBool::new(false));
    response.map(move |body| {
        Body::new(CountedBody {
            inner: body,
            pending: pending.clone(),
            size: size.clone(),
            done: done.clone(),
        })
    })
}

/// Unified security middleware: applies GitHub-style per-bucket rate limits,
/// the per-account login failure lockout, response usage headers and login
/// outcome audit events.
pub async fn security_middleware(
    State(AppState {
        conn,
        security_config,
        usage_registry,
        rate_limit_matchers,
        ..
    }): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();
    let ip = extract_ip(&request);
    let is_login = path == "/api/auth/login";
    let principal = bearer_principal(&request);

    let mut access_log = if security_config.api_access_log_enabled
        && security_config
            .access_log_path_prefixes
            .iter()
            .any(|prefix| path.starts_with(prefix))
    {
        Some(PendingAccessLog {
            conn: conn.clone(),
            request_id: xid::new().to_string(),
            method: request.method().as_str().to_string(),
            path: path.clone(),
            query: request.uri().query().map(String::from),
            ip: ip.clone(),
            principal_id: principal.as_ref().map(|p| p.id.clone()),
            name: principal.as_ref().and_then(|p| p.name.clone()),
            status: 0,
            duration_ms: 0,
            user_agent: request
                .headers()
                .get(axum::http::header::USER_AGENT)
                .and_then(|value| value.to_str().ok())
                .map(String::from),
        })
    } else {
        None
    };
    let access_started = Instant::now();

    let bucket = resolve_bucket(&path, &ip, principal.as_ref(), &rate_limit_matchers);

    let allowed_bucket = match &bucket {
        Some(bucket) => match usage_registry.check(&bucket.resource, &bucket.key) {
            Some(RateLimitDecision::Limited { limit, retry_after }) => {
                tracing::warn!(
                    component = "RATELIMIT",
                    event = "rate_limited",
                    ip = %ip,
                    path = %path,
                    principal_id = %bucket.principal_id.as_deref().unwrap_or("-"),
                    name = %bucket.name.as_deref().unwrap_or("-"),
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
                        bucket.principal_id.as_deref(),
                        bucket.name.as_deref(),
                        LimitInfo {
                            retry_after,
                            limit,
                            remaining: 0,
                            window_secs: usage_registry.window_secs(&bucket.resource).unwrap_or(0),
                        },
                    ),
                );
                return finish_access_log(
                    access_log,
                    access_started,
                    build_limited_response(&bucket.resource, limit, 0, retry_after),
                );
            }
            Some(RateLimitDecision::Allowed { .. }) => {
                usage_registry.peek(&bucket.resource, &bucket.key)
            }
            None => None,
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
            if let Some(log) = access_log.as_mut() {
                log.name = Some(account.clone());
            }
            if let Err(e) = login_fail_counter(&security_config).record(account) {
                tracing::warn!(
                    component = "RATELIMIT",
                    event = "rate_limited",
                    account = %account,
                    ip = %ip,
                    path = %path,
                    retry_after_ms = e.retry_after.as_millis() as i64,
                    limit = security_config.login_fail_limit as i64,
                    "account temporarily locked after repeated login failures"
                );
                record_event(
                    &conn,
                    rate_limited_model(
                        SecurityEventType::RateLimited,
                        &ip,
                        &path,
                        None,
                        Some(account),
                        LimitInfo {
                            retry_after: e.retry_after,
                            limit: security_config.login_fail_limit,
                            remaining: 0,
                            window_secs: security_config.login_fail_window_secs,
                        },
                    ),
                );
                let resource = rate_limit_matchers
                    .iter()
                    .find(|matcher| matcher.matches("/api/auth/login"))
                    .map(|matcher| matcher.name.clone())
                    .unwrap_or_else(|| String::from("auth"));
                return finish_access_log(
                    access_log,
                    access_started,
                    build_limited_response(
                        &resource,
                        security_config.login_fail_limit,
                        0,
                        e.retry_after,
                    ),
                );
            }
        }
        request
    } else {
        request
    };

    let response = next.run(request).await;
    let status = response.status();
    let mut response = response;

    if let (Some(bucket), Some(snapshot)) = (&bucket, allowed_bucket) {
        apply_usage_headers(response.headers_mut(), &bucket.resource, snapshot);
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
                    bucket.principal_id.as_deref(),
                    bucket.name.as_deref(),
                    LimitInfo {
                        retry_after: snapshot.reset_after,
                        limit: snapshot.limit,
                        remaining: snapshot.remaining,
                        window_secs: usage_registry.window_secs(&bucket.resource).unwrap_or(0),
                    },
                ),
            );
        }
    }

    if is_login && let Some(account) = login_account {
        if status.is_success() {
            login_fail_counter(&security_config).clear(&account);
            record_login_success(&conn, &account, &ip, &path);
        } else if status == StatusCode::BAD_REQUEST {
            record_login_failure(&conn, &account, &ip, &path);
        }
    }

    finish_access_log(access_log, access_started, response)
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
    principal_id: Option<&str>,
    account: Option<&str>,
    info: LimitInfo,
) -> security_event::ActiveModel {
    security_event::ActiveModel {
        event_type: Set(event_type),
        ip: Set(Some(ip.to_string())),
        path: Set(Some(path.to_string())),
        principal_id: Set(principal_id.map(String::from)),
        account: Set(account.map(String::from)),
        retry_after_ms: Set(Some(info.retry_after.as_millis() as i64)),
        limit: Set(Some(info.limit as i64)),
        remaining: Set(Some(info.remaining as i64)),
        window_secs: Set(Some(info.window_secs as i64)),
        ..Default::default()
    }
}

fn build_limited_response(
    resource: &str,
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
        HeaderValue::from_str(resource).expect("resource is valid"),
    );
    response
}

fn apply_usage_headers(headers: &mut HeaderMap, resource: &str, snapshot: BucketSnapshot) {
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
        HeaderValue::from_str(resource).expect("resource is valid"),
    );
}

pub fn record_login_failure(conn: &DatabaseConnection, account: &str, ip: &str, path: &str) {
    record_event(
        conn,
        security_event::ActiveModel {
            event_type: Set(SecurityEventType::AuthLoginFailure),
            ip: Set(Some(ip.to_string())),
            path: Set(Some(path.to_string())),
            account: Set(Some(account.to_string())),
            ..Default::default()
        },
    );
}

pub fn record_login_success(conn: &DatabaseConnection, account: &str, ip: &str, path: &str) {
    record_event(
        conn,
        security_event::ActiveModel {
            event_type: Set(SecurityEventType::AuthLoginSuccess),
            ip: Set(Some(ip.to_string())),
            path: Set(Some(path.to_string())),
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
    /// 账号模糊匹配
    pub account: Option<String>,
    /// 路径模糊匹配
    pub path: Option<String>,
    /// 起始时间（含），RFC3339，如 2026-08-01T00:00:00+08:00
    pub start: Option<String>,
    /// 结束时间（不含），RFC3339
    pub end: Option<String>,
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
    if let Some(ref account) = params.account {
        query = query.filter(security_event::Column::Account.like(format!("%{account}%")));
    }
    if let Some(ref path) = params.path {
        query = query.filter(security_event::Column::Path.like(format!("%{path}%")));
    }
    if let Some(start) = params
        .start
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
    {
        query = query.filter(security_event::Column::CreateDatetime.gte(start));
    }
    if let Some(end) = params
        .end
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
    {
        query = query.filter(security_event::Column::CreateDatetime.lt(end));
    }

    let query = query
        .order_by_desc(security_event::Column::CreateDatetime)
        .order_by_desc(security_event::Column::Id);
    let result = paginate(query, &conn, &pagination).await?;

    Ok(ApiResponse::success(Some(result)))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AccessLogListParams {
    #[param(example = "1")]
    pub page: Option<u64>,
    #[param(example = "20")]
    pub page_size: Option<u64>,
    /// HTTP 方法：GET / POST / PUT / DELETE
    pub method: Option<String>,
    /// 路径模糊匹配
    pub path: Option<String>,
    /// IP 模糊匹配
    pub ip: Option<String>,
    /// 身份名称（用户登录名 / 设备类型）
    pub name: Option<String>,
    /// 身份编号
    pub principal_id: Option<String>,
    /// 状态码
    pub status: Option<i32>,
}

#[debug_handler]
#[utoipa::path(get, path = "/security/access_logs", tag = TAG, params(AccessLogListParams), responses(
    (status=OK,body=ApiResponse<PageData<api_access_log::Model>>)
))]
async fn list_access_logs(
    State(AppState { conn, .. }): State<AppState>,
    Query(params): Query<AccessLogListParams>,
) -> AppResult<ApiResponse<PageData<api_access_log::Model>>> {
    let pagination = PageParam::new(params.page, params.page_size);

    let mut query = ApiAccessLog::find();

    if let Some(ref method) = params.method {
        query = query.filter(api_access_log::Column::Method.eq(method));
    }
    if let Some(ref path) = params.path {
        query = query.filter(api_access_log::Column::Path.like(format!("%{path}%")));
    }
    if let Some(ref ip) = params.ip {
        query = query.filter(api_access_log::Column::Ip.like(format!("%{ip}%")));
    }
    if let Some(ref name) = params.name {
        query = query.filter(api_access_log::Column::Name.eq(name));
    }
    if let Some(ref principal_id) = params.principal_id {
        query = query.filter(api_access_log::Column::PrincipalId.eq(principal_id));
    }
    if let Some(status) = params.status {
        query = query.filter(api_access_log::Column::Status.eq(status));
    }

    let query = query
        .order_by_desc(api_access_log::Column::CreateDatetime)
        .order_by_desc(api_access_log::Column::Id);
    let result = paginate(query, &conn, &pagination).await?;

    Ok(ApiResponse::success(Some(result)))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AccessLogNameCount {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AccessLogPrincipalCount {
    pub id: String,
    pub name: Option<String>,
    pub count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AccessLogHourlyPoint {
    pub hour: String,
    pub total: i64,
    pub count_2xx: i64,
    pub count_3xx: i64,
    pub count_4xx: i64,
    pub count_5xx: i64,
    pub avg_ms: f64,
    pub p95_ms: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AccessLogStats {
    pub total: i64,
    pub today: i64,
    pub last_24h: i64,
    pub avg_duration_24h_ms: f64,
    pub p95_duration_24h_ms: i64,
    pub error_4xx_24h: i64,
    pub error_5xx_24h: i64,
    pub requests_by_hour: Vec<AccessLogHourlyPoint>,
    pub status_classes: Vec<AccessLogNameCount>,
    pub top_methods: Vec<AccessLogNameCount>,
    pub top_paths: Vec<AccessLogNameCount>,
    pub top_ips: Vec<AccessLogNameCount>,
    pub top_principals: Vec<AccessLogPrincipalCount>,
}

fn status_class(status: i32) -> &'static str {
    match status / 100 {
        2 => "2xx",
        3 => "3xx",
        4 => "4xx",
        5 => "5xx",
        _ => "other",
    }
}

fn percentile(sorted: &[i64], p: f64) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[index]
}

fn hour_label(dt: chrono::DateTime<chrono::FixedOffset>) -> String {
    dt.with_timezone(&Local).format("%m-%d %H:00").to_string()
}

/// Top-N values for a non-null access log column within the given cutoff.
async fn top_access_log_dimension(
    conn: &DatabaseConnection,
    cutoff: chrono::DateTime<chrono::FixedOffset>,
    column: api_access_log::Column,
    limit: u64,
) -> AppResult<Vec<AccessLogNameCount>> {
    let rows: Vec<(String, i64)> = ApiAccessLog::find()
        .select_only()
        .column_as(column, "name")
        .column_as(api_access_log::Column::Id.count(), "count")
        .filter(api_access_log::Column::CreateDatetime.gte(cutoff))
        .group_by(column)
        .order_by_desc(api_access_log::Column::Id.count())
        .limit(limit)
        .into_tuple()
        .all(conn)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(name, count)| AccessLogNameCount { name, count })
        .collect())
}

/// Top-N values for a nullable access log column (nulls excluded) within cutoff.
async fn top_access_log_dimension_nullable(
    conn: &DatabaseConnection,
    cutoff: chrono::DateTime<chrono::FixedOffset>,
    column: api_access_log::Column,
    limit: u64,
) -> AppResult<Vec<AccessLogNameCount>> {
    let rows: Vec<(Option<String>, i64)> = ApiAccessLog::find()
        .select_only()
        .column_as(column, "name")
        .column_as(api_access_log::Column::Id.count(), "count")
        .filter(api_access_log::Column::CreateDatetime.gte(cutoff))
        .filter(column.is_not_null())
        .group_by(column)
        .order_by_desc(api_access_log::Column::Id.count())
        .limit(limit)
        .into_tuple()
        .all(conn)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(name, count)| name.map(|name| AccessLogNameCount { name, count }))
        .collect())
}

#[debug_handler]
#[utoipa::path(get, path = "/security/access_logs/stats", tag = TAG, responses(
    (status=OK,body=ApiResponse<AccessLogStats>)
))]
async fn access_log_stats(
    State(AppState { conn, .. }): State<AppState>,
) -> AppResult<ApiResponse<AccessLogStats>> {
    let now = Local::now().fixed_offset();
    let today_start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|d| d.and_local_timezone(Local).unwrap())
        .map(|d| d.fixed_offset());
    let twenty_four_hours_ago = now - chrono::Duration::hours(24);

    let total = ApiAccessLog::find().count(&conn).await? as i64;
    let today = match today_start {
        Some(ts) => {
            ApiAccessLog::find()
                .filter(api_access_log::Column::CreateDatetime.gte(ts))
                .count(&conn)
                .await? as i64
        }
        None => 0,
    };
    let last_24h = ApiAccessLog::find()
        .filter(api_access_log::Column::CreateDatetime.gte(twenty_four_hours_ago))
        .count(&conn)
        .await? as i64;
    let error_4xx_24h = ApiAccessLog::find()
        .filter(api_access_log::Column::CreateDatetime.gte(twenty_four_hours_ago))
        .filter(api_access_log::Column::Status.gte(400))
        .filter(api_access_log::Column::Status.lte(499))
        .count(&conn)
        .await? as i64;
    let error_5xx_24h = ApiAccessLog::find()
        .filter(api_access_log::Column::CreateDatetime.gte(twenty_four_hours_ago))
        .filter(api_access_log::Column::Status.gte(500))
        .filter(api_access_log::Column::Status.lte(599))
        .count(&conn)
        .await? as i64;

    let rows: Vec<(Option<chrono::DateTime<chrono::FixedOffset>>, i32, i64)> = ApiAccessLog::find()
        .select_only()
        .column(api_access_log::Column::CreateDatetime)
        .column(api_access_log::Column::Status)
        .column(api_access_log::Column::DurationMs)
        .filter(api_access_log::Column::CreateDatetime.gte(twenty_four_hours_ago))
        .into_tuple()
        .all(&conn)
        .await?;

    let cur_hour = now
        .with_minute(0)
        .and_then(|d| d.with_second(0))
        .and_then(|d| d.with_nanosecond(0))
        .unwrap();
    let mut buckets: Vec<AccessLogHourlyPoint> = (0..24)
        .map(|i| AccessLogHourlyPoint {
            hour: hour_label(cur_hour - chrono::Duration::hours(23 - i as i64)),
            total: 0,
            count_2xx: 0,
            count_3xx: 0,
            count_4xx: 0,
            count_5xx: 0,
            avg_ms: 0.0,
            p95_ms: 0,
        })
        .collect();
    let mut durations_by_bucket: Vec<Vec<i64>> = vec![Vec::new(); 24];

    for (dt, status, duration_ms) in &rows {
        let Some(dt) = dt else {
            continue;
        };
        let hour = dt
            .with_minute(0)
            .and_then(|d| d.with_second(0))
            .and_then(|d| d.with_nanosecond(0))
            .unwrap();
        let diff = (cur_hour - hour).num_hours();
        if !(0..=23).contains(&diff) {
            continue;
        }
        let idx = 23 - diff as usize;
        buckets[idx].total += 1;
        match status_class(*status) {
            "2xx" => buckets[idx].count_2xx += 1,
            "3xx" => buckets[idx].count_3xx += 1,
            "4xx" => buckets[idx].count_4xx += 1,
            "5xx" => buckets[idx].count_5xx += 1,
            _ => {}
        }
        durations_by_bucket[idx].push(*duration_ms);
    }

    let mut all_durations = Vec::with_capacity(rows.len());
    for (_, _, duration_ms) in &rows {
        all_durations.push(*duration_ms);
    }
    all_durations.sort_unstable();
    let avg_duration_24h_ms = if all_durations.is_empty() {
        0.0
    } else {
        all_durations.iter().sum::<i64>() as f64 / all_durations.len() as f64
    };
    let p95_duration_24h_ms = percentile(&all_durations, 0.95);

    for i in 0..24 {
        durations_by_bucket[i].sort_unstable();
        let durs = &durations_by_bucket[i];
        buckets[i].p95_ms = percentile(durs, 0.95);
        buckets[i].avg_ms = if durs.is_empty() {
            0.0
        } else {
            durs.iter().sum::<i64>() as f64 / durs.len() as f64
        };
    }

    let status_rows: Vec<(i32, i64)> = ApiAccessLog::find()
        .select_only()
        .column_as(api_access_log::Column::Status, "status")
        .column_as(api_access_log::Column::Id.count(), "count")
        .filter(api_access_log::Column::CreateDatetime.gte(twenty_four_hours_ago))
        .group_by(api_access_log::Column::Status)
        .into_tuple()
        .all(&conn)
        .await?;
    let mut status_by_class: HashMap<&'static str, i64> = HashMap::new();
    for (status, count) in status_rows {
        *status_by_class.entry(status_class(status)).or_default() += count;
    }
    let mut status_classes: Vec<AccessLogNameCount> = status_by_class
        .into_iter()
        .map(|(class, count)| AccessLogNameCount {
            name: class.to_string(),
            count,
        })
        .collect();
    status_classes.sort_by(|a, b| a.name.cmp(&b.name));

    let top_methods = top_access_log_dimension(
        &conn,
        twenty_four_hours_ago,
        api_access_log::Column::Method,
        10,
    )
    .await?;
    let top_paths = top_access_log_dimension(
        &conn,
        twenty_four_hours_ago,
        api_access_log::Column::Path,
        10,
    )
    .await?;
    let top_ips = top_access_log_dimension_nullable(
        &conn,
        twenty_four_hours_ago,
        api_access_log::Column::Ip,
        10,
    )
    .await?;
    let top_principals = top_access_log_dimension_nullable(
        &conn,
        twenty_four_hours_ago,
        api_access_log::Column::PrincipalId,
        10,
    )
    .await?;
    let principal_ids: Vec<String> = top_principals.iter().map(|p| p.name.clone()).collect();
    let mut name_by_id: HashMap<String, String> = HashMap::new();
    if !principal_ids.is_empty() {
        let principal_names: Vec<(String, String)> = ApiAccessLog::find()
            .select_only()
            .column(api_access_log::Column::PrincipalId)
            .column(api_access_log::Column::Name)
            .filter(api_access_log::Column::PrincipalId.is_in(principal_ids))
            .filter(api_access_log::Column::Name.is_not_null())
            .into_tuple()
            .all(&conn)
            .await?;
        for (principal_id, name) in principal_names {
            name_by_id.entry(principal_id).or_insert(name);
        }
    }
    let top_principals: Vec<AccessLogPrincipalCount> = top_principals
        .into_iter()
        .map(|hit| {
            let id = hit.name;
            let name = name_by_id.get(&id).cloned();
            AccessLogPrincipalCount {
                id,
                name,
                count: hit.count,
            }
        })
        .collect();

    Ok(ApiResponse::success(Some(AccessLogStats {
        total,
        today,
        last_24h,
        avg_duration_24h_ms,
        p95_duration_24h_ms,
        error_4xx_24h,
        error_5xx_24h,
        requests_by_hour: buckets,
        status_classes,
        top_methods,
        top_paths,
        top_ips,
        top_principals,
    })))
}

#[derive(Debug, Serialize, ToSchema, Default)]
pub struct EventTypeCounts {
    pub rate_limited: i64,
    pub rate_limit_near: i64,
    pub auth_login_success: i64,
    pub auth_login_failure: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EventIpHit {
    pub ip: String,
    pub count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SecurityEventStats {
    pub today: EventTypeCounts,
    pub last_7d: EventTypeCounts,
    pub total: EventTypeCounts,
    pub top_ips_last_24h: Vec<EventIpHit>,
}

async fn count_by_type_since(
    conn: &DatabaseConnection,
    cutoff: Option<chrono::DateTime<chrono::FixedOffset>>,
) -> AppResult<EventTypeCounts> {
    let mut query = SecurityEvent::find()
        .select_only()
        .column(security_event::Column::EventType)
        .column_as(security_event::Column::Id.count(), "count")
        .group_by(security_event::Column::EventType);
    if let Some(cutoff) = cutoff {
        query = query.filter(security_event::Column::CreateDatetime.gte(cutoff));
    }
    let rows: Vec<(SecurityEventType, i64)> = query
        .into_tuple::<(SecurityEventType, i64)>()
        .all(conn)
        .await?;
    let mut counts = EventTypeCounts::default();
    for (event_type, count) in rows {
        match event_type {
            SecurityEventType::RateLimited => counts.rate_limited = count,
            SecurityEventType::RateLimitNear => counts.rate_limit_near = count,
            SecurityEventType::AuthLoginSuccess => counts.auth_login_success = count,
            SecurityEventType::AuthLoginFailure => counts.auth_login_failure = count,
        }
    }
    Ok(counts)
}

#[debug_handler]
#[utoipa::path(get, path = "/security/stats", tag = TAG, responses(
    (status=OK,body=ApiResponse<SecurityEventStats>)
))]
async fn event_stats(
    State(AppState { conn, .. }): State<AppState>,
) -> AppResult<ApiResponse<SecurityEventStats>> {
    let now = Local::now().fixed_offset();
    let today_start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|d| d.and_local_timezone(Local).unwrap())
        .map(|d| d.fixed_offset());
    let seven_days_ago = now - chrono::Duration::days(7);
    let twenty_four_hours_ago = now - chrono::Duration::hours(24);

    let today = count_by_type_since(&conn, today_start).await?;
    let last_7d = count_by_type_since(&conn, Some(seven_days_ago)).await?;
    let total = count_by_type_since(&conn, None).await?;

    let top_ips = SecurityEvent::find()
        .select_only()
        .column_as(security_event::Column::Ip, "ip")
        .column_as(security_event::Column::Id.count(), "count")
        .filter(security_event::Column::CreateDatetime.gte(twenty_four_hours_ago))
        .filter(security_event::Column::Ip.is_not_null())
        .group_by(security_event::Column::Ip)
        .order_by_desc(security_event::Column::Id.count())
        .limit(10)
        .into_tuple::<(String, i64)>()
        .all(&conn)
        .await?;
    let top_ips_last_24h = top_ips
        .into_iter()
        .map(|(ip, count)| EventIpHit { ip, count })
        .collect();

    Ok(ApiResponse::success(Some(SecurityEventStats {
        today,
        last_7d,
        total,
        top_ips_last_24h,
    })))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RateLimitBucketInfo {
    pub name: String,
    pub limit: u32,
    pub used: u32,
    pub remaining: u32,
    pub reset: i64,
}

#[derive(Debug, Serialize, ToSchema, Default)]
pub struct RateLimitResult {
    resources: Vec<RateLimitBucketInfo>,
}

#[debug_handler]
#[utoipa::path(get, path = "/security/rate_limit", tag = TAG, security(()), responses(
    (status=OK,body=ApiResponse<RateLimitResult>)
))]
async fn rate_limit(
    State(AppState {
        usage_registry,
        rate_limit_matchers,
        ..
    }): State<AppState>,
    Extension(principal): Extension<Principal>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResult<ApiResponse<RateLimitResult>> {
    let ip = addr.ip().to_string();
    let mut resources = Vec::with_capacity(rate_limit_matchers.len());
    for matcher in rate_limit_matchers.iter() {
        let key = match matcher.key_by {
            RateLimitKeyBy::Ip => format!("ip:{ip}"),
            RateLimitKeyBy::Principal => format!("user:{}", principal.id),
        };
        let limit = usage_registry.limit(&matcher.name).unwrap_or(matcher.limit);
        let window_secs = usage_registry
            .window_secs(&matcher.name)
            .unwrap_or(matcher.window_secs);
        resources.push(match usage_registry.peek(&matcher.name, &key) {
            Some(snapshot) => RateLimitBucketInfo {
                name: matcher.name.clone(),
                limit: snapshot.limit,
                used: snapshot.used,
                remaining: snapshot.remaining,
                reset: Local::now().timestamp() + snapshot.reset_after.as_secs() as i64,
            },
            None => RateLimitBucketInfo {
                name: matcher.name.clone(),
                limit,
                used: 0,
                remaining: limit,
                reset: Local::now().timestamp() + window_secs as i64,
            },
        });
    }
    Ok(ApiResponse::success(Some(RateLimitResult { resources })))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ResourceUsageInfo {
    pub name: String,
    pub limit: u32,
    pub window_secs: u64,
    pub active_keys: usize,
    pub allowed: u64,
    pub limited: u64,
    pub top_keys: Vec<BucketUsageInfo>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BucketUsageInfo {
    pub key: String,
    pub used: u32,
    pub remaining: u32,
    pub reset_after_secs: u64,
}

#[derive(Debug, Serialize, ToSchema, Default)]
pub struct UsageStatsResult {
    pub resources: Vec<ResourceUsageInfo>,
}

fn resource_usage_info(
    name: String,
    usage: &framework::rate_limit::ResourceUsage,
) -> ResourceUsageInfo {
    ResourceUsageInfo {
        name,
        limit: usage.limit,
        window_secs: usage.window_secs,
        active_keys: usage.active_keys,
        allowed: usage.allowed,
        limited: usage.limited,
        top_keys: usage
            .top_keys
            .iter()
            .map(|(key, snapshot)| BucketUsageInfo {
                key: key.clone(),
                used: snapshot.used,
                remaining: snapshot.remaining,
                reset_after_secs: snapshot.reset_after.as_secs(),
            })
            .collect(),
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct UsageStatsParams {
    /// 每个资源返回的 top keys 数量上限，默认 10。
    #[param(example = "10", maximum = 200, default = 10)]
    pub top_n: Option<usize>,
}

#[debug_handler]
#[utoipa::path(get, path = "/security/usage_stats", tag = TAG, params(UsageStatsParams), responses(
    (status=OK,body=ApiResponse<UsageStatsResult>)
))]
async fn usage_stats(
    State(AppState { usage_registry, .. }): State<AppState>,
    Query(params): Query<UsageStatsParams>,
) -> AppResult<ApiResponse<UsageStatsResult>> {
    let top_n = params.top_n.unwrap_or(10).clamp(1, 200);
    let stats = usage_registry.usage_stats(top_n);
    Ok(ApiResponse::success(Some(UsageStatsResult {
        resources: stats
            .into_iter()
            .map(|(name, usage)| resource_usage_info(name, &usage))
            .collect(),
    })))
}

pub fn create_routes(state: AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(list_events))
        .routes(routes!(list_access_logs))
        .routes(routes!(access_log_stats))
        .routes(routes!(event_stats))
        .routes(routes!(usage_stats))
        .routes(routes!(rate_limit))
        .route_layer(get_auth_layer())
        .with_state(state)
}

/// Deletes security events older than the retention window.
async fn cleanup_old_events(
    conn: &DatabaseConnection,
    retention_days: i64,
) -> Result<u64, sea_orm::DbErr> {
    let cutoff = Local::now().fixed_offset() - chrono::Duration::days(retention_days);
    SecurityEvent::delete_many()
        .filter(security_event::Column::CreateDatetime.lt(cutoff))
        .exec(conn)
        .await
        .map(|result| result.rows_affected)
}

/// Deletes access logs older than the retention window.
async fn cleanup_old_access_logs(
    conn: &DatabaseConnection,
    retention_days: i64,
) -> Result<u64, sea_orm::DbErr> {
    let cutoff = Local::now().fixed_offset() - chrono::Duration::days(retention_days);
    ApiAccessLog::delete_many()
        .filter(api_access_log::Column::CreateDatetime.lt(cutoff))
        .exec(conn)
        .await
        .map(|result| result.rows_affected)
}

/// Background retention task for the append-only security event and access
/// log tables.
pub async fn cleanup_loop(
    conn: DatabaseConnection,
    ct: CancellationToken,
    retention_days: i64,
    access_retention_days: i64,
    interval_secs: u64,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    loop {
        tokio::select! {
            _ = ct.cancelled() => {
                tracing::info!(component = "SECURITY", event = "cleanup_stopped", "security event cleanup stopped");
                break;
            }
            _ = interval.tick() => {
                match cleanup_old_events(&conn, retention_days).await {
                    Ok(deleted) => tracing::info!(component = "SECURITY", event = "cleanup_done", deleted = deleted, "security event cleanup done"),
                    Err(e) => tracing::warn!(component = "SECURITY", event = "cleanup_failed", error = %e, "security event cleanup failed"),
                }
                match cleanup_old_access_logs(&conn, access_retention_days).await {
                    Ok(deleted) => tracing::info!(component = "ACCESS", event = "access_log_cleanup_done", deleted = deleted, "api access log cleanup done"),
                    Err(e) => tracing::warn!(component = "ACCESS", event = "access_log_cleanup_failed", error = %e, "api access log cleanup failed"),
                }
            }
        }
    }
}
