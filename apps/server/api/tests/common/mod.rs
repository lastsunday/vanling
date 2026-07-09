use api::config::audio::AudioConfig;
use api::config::mcp::McpConfig;
use api::config::vad::VadConfig;
use api::config::ws::WsConfig;
use api::{AppState, config::session::SessionConfig};
use axum::{
    Router,
    body::Body,
    http::{self, Request, Response},
};
use chrono::{DateTime, FixedOffset};
use framework::config::auth::AuthConfig;
use http_body_util::BodyExt;
use migration::MigratorTrait;
use serde_json::Value;
use std::{str::FromStr, sync::Arc};
use testcontainers::ContainerAsync;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use tower::ServiceExt;

pub mod asr;
pub mod router_client;
pub mod tts;
pub mod vad;

#[allow(dead_code)]
pub async fn setup_database() -> (Option<ContainerAsync<Postgres>>, AppState) {
    match std::env::var("TEST_DATABASE").as_deref() {
        Ok("pg") => {
            let container = Postgres::default().start().await.unwrap();
            let host_port = container.get_host_port_ipv4(5432).await.unwrap();
            let database_url =
                format!("postgres://postgres:postgres@127.0.0.1:{host_port}/postgres");
            let conn: sea_orm::DatabaseConnection =
                framework::database::establish_connection(&database_url)
                    .await
                    .unwrap();
            migration::Migrator::up(&conn, None).await.unwrap();
            let state = AppState {
                conn,
                session_config: Arc::new(SessionConfig {
                    ..Default::default()
                }),
                mcp_config: Arc::new(McpConfig {
                    ..Default::default()
                }),
                vad_config: Arc::new(VadConfig {
                    ..Default::default()
                }),
                audio_config: Arc::new(AudioConfig {
                    ..Default::default()
                }),
                auth_config: Arc::new(AuthConfig {
                    ..Default::default()
                }),
                ws_config: Arc::new(WsConfig {
                    ..Default::default()
                }),
            };
            (Some(container), state)
        }
        _ => {
            let container = None;
            let database_url = "sqlite::memory:";
            let conn: sea_orm::DatabaseConnection =
                framework::database::establish_connection(database_url)
                    .await
                    .unwrap();
            migration::Migrator::up(&conn, None).await.unwrap();
            let state = AppState {
                conn,
                session_config: Arc::new(SessionConfig {
                    ..Default::default()
                }),
                mcp_config: Arc::new(McpConfig {
                    ..Default::default()
                }),
                vad_config: Arc::new(VadConfig {
                    ..Default::default()
                }),
                audio_config: Arc::new(AudioConfig {
                    ..Default::default()
                }),
                auth_config: Arc::new(AuthConfig {
                    ..Default::default()
                }),
                ws_config: Arc::new(WsConfig {
                    ..Default::default()
                }),
            };
            (container, state)
        }
    }
}

#[allow(dead_code)]
pub async fn tear_down(container: Option<ContainerAsync<Postgres>>) {
    if let Some(container) = container {
        container.rm().await.unwrap();
    }
}

#[allow(dead_code)]
pub async fn response_to_json(response: Response<Body>) -> Value {
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&body_bytes).unwrap();
    value
}

#[allow(dead_code)]
pub fn get_json_paging_result_items(value: &Value) -> Vec<Value> {
    value["data"]
        .as_object()
        .unwrap()
        .get("items")
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

#[allow(dead_code)]
pub fn get_json_result(value: &Value) -> Value {
    value["data"].clone()
}

#[allow(dead_code)]
pub fn get_from_value<T: FromStr>(value: &Value, name: &str) -> Result<T, T::Err> {
    if value.get(name).unwrap().is_string() {
        value
            .get(name)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string()
            .parse::<T>()
    } else {
        value.get(name).unwrap().to_string().parse::<T>()
    }
}

#[allow(dead_code)]
pub fn get_string_from_value(value: &Value, name: &str) -> String {
    value
        .get(name)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
        .parse()
        .unwrap()
}

#[allow(dead_code)]
pub async fn post_json(app: Router, uri: &str, json: &Value) -> Response<Body> {
    post_json_with_token(app, uri, json, None).await
}

#[allow(dead_code)]
pub async fn post_json_without_body(app: Router, uri: &str) -> Response<Body> {
    let builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref());
    let request = builder.body(Body::from(())).unwrap();
    app.oneshot(request).await.unwrap()
}

#[allow(dead_code)]
pub async fn post_json_with_token(
    app: Router,
    uri: &str,
    json: &Value,
    token: Option<String>,
) -> Response<Body> {
    let builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref());
    let builder = match token {
        Some(token) => builder.header(http::header::AUTHORIZATION, format!("Bearer {token}")),
        None => builder,
    };
    let request = builder
        .body(Body::from(serde_json::to_string(json).unwrap()))
        .unwrap();
    app.oneshot(request).await.unwrap()
}

#[allow(dead_code)]
pub async fn get_json(app: Router, uri: &str) -> Response<Body> {
    get_json_with_token(app, uri, None).await
}

#[allow(dead_code)]
pub async fn get_json_with_token(app: Router, uri: &str, token: Option<String>) -> Response<Body> {
    let builder = Request::builder()
        .method("GET")
        .uri(uri)
        .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref());
    let builder = match token {
        Some(token) => builder.header(http::header::AUTHORIZATION, format!("Bearer {token}")),
        None => builder,
    };
    let request = builder.body(Body::from(())).unwrap();
    app.oneshot(request).await.unwrap()
}

#[allow(dead_code)]
pub fn str_to_datetime(value: String) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(&value).ok()
}

#[allow(dead_code)]
pub fn datetime_to_str(datetime: Option<DateTime<FixedOffset>>) -> String {
    match datetime {
        Some(item) => item.to_rfc3339(),
        None => "".to_owned(),
    }
}
