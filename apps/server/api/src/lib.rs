pub use framework::error;

pub mod activation_pool;
pub mod asr;
pub mod auth;
pub mod common;
pub mod config;
pub mod device;
pub mod index;
pub mod ling_core;
pub mod llm;
pub mod matrix;
pub mod mcp;
pub mod ota;
pub mod record;
pub mod server;
pub mod stats;
pub mod tts;
pub mod util;
pub mod vad;
pub mod ws;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

use axum::Router;
use axum::ServiceExt;
use axum::extract::DefaultBodyLimit;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::routing::get;
use axum::serve::ListenerExt;
use bytesize::ByteSize;
use either::Either;
use framework::config::auth::AuthConfig;
use framework::error::{
    AppError, AppResult, critical_code::CriticalErrorCode, framework_code::FrameworkErrorCode,
};
use framework::trace::LatencyOnResponse;
use futures::future::join_all;
use migration::MigratorTrait;
use sea_orm::ColumnTrait;
use sea_orm::DatabaseConnection;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;
use sea_orm::QuerySelect;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::compression::CompressionLayer;
use tower_http::cors;
use tower_http::cors::CorsLayer;
use tower_http::normalize_path::NormalizePathLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tower_layer::Layer;
use utoipa::OpenApi;
use utoipa::openapi::security::Http;
use utoipa::openapi::security::HttpAuthScheme;
use utoipa::openapi::security::SecurityScheme;
use utoipa_axum::router::OpenApiRouter;
use utoipa_scalar::{Scalar, Servable as ScalarServable};

use framework::auth::Jwt;

use crate::asr::AsrManager;
use crate::config::Config;
use crate::config::asr::AsrConfig;
use crate::config::audio::AudioConfig;
use crate::config::database::DatabaseConfig;
use crate::config::llm::LlmConfig;
use crate::config::matrix::MatrixConfig;
use crate::config::mcp::McpConfig;
use crate::config::server::ServerConfig;
use crate::config::session::SessionConfig;
use crate::config::tts::TtsConfig;
use crate::config::vad::VadConfig;
use crate::config::ws::WsConfig;
use crate::llm::LlmManager;
use crate::tts::TtsManager;
use crate::vad::VadManager;

pub struct StartParams {
    pub server_config: Arc<ServerConfig>,
    pub database_config: Arc<DatabaseConfig>,
    pub session_config: Arc<SessionConfig>,
    pub mcp_config: Arc<McpConfig>,
    pub vad_config: Arc<VadConfig>,
    pub audio_config: Arc<AudioConfig>,
    pub auth_config: Arc<AuthConfig>,
    pub ws_config: Arc<WsConfig>,
    pub tts_config: Arc<TtsConfig>,
    pub asr_config: Arc<AsrConfig>,
    pub llm_config: Arc<LlmConfig>,
    pub matrix_config: Arc<MatrixConfig>,
    pub shutdown_token: CancellationToken,
}

pub async fn start(params: StartParams) -> anyhow::Result<()> {
    let StartParams {
        server_config,
        database_config,
        session_config,
        mcp_config,
        vad_config,
        audio_config,
        auth_config,
        ws_config,
        tts_config,
        asr_config,
        llm_config,
        matrix_config,
        shutdown_token,
    } = params;
    Jwt::init(auth_config.clone());
    let database_url = database_config.url.as_ref().expect("database url is empty");
    let conn: sea_orm::DatabaseConnection =
        framework::database::establish_connection(database_url).await?;
    conn.ping().await?;
    tracing::info!("Database connected successfully");
    migration::Migrator::up(&conn, None).await?;
    let used_codes = load_used_codes(&conn).await?;
    activation_pool::ActivationPool::init(&used_codes);
    tracing::info!("init tts manager");
    TtsManager::init(tts_config, audio_config.clone()).await?;
    tracing::info!("init tts manager successfully");
    tracing::info!("init vad manager");
    VadManager::init(vad_config.clone()).await;
    tracing::info!("init vad manager successfully");
    tracing::info!("init asr manager");
    AsrManager::init(asr_config).await;
    tracing::info!("init asr manager successfully");
    tracing::info!("init llm manager");
    LlmManager::init(llm_config).await;
    tracing::info!("init llm manager successfully");
    let ct_for_app = shutdown_token.clone();
    let ct_for_matrix = shutdown_token.clone();
    let mut handles = Vec::new();
    let state = AppState {
        conn,
        session_config: session_config.clone(),
        mcp_config: mcp_config.clone(),
        vad_config: vad_config.clone(),
        audio_config: audio_config.clone(),
        auth_config: auth_config.clone(),
        ws_config: ws_config.clone(),
        cancellation_token: shutdown_token,
    };
    handles.push(tokio::spawn(async move {
        if let Err(error) = start_app(server_config, state, ct_for_app).await {
            tracing::error!("{:?}", error);
        }
    }));
    if matrix_config.enable.expect("matrix enable is empty") {
        handles.push(tokio::spawn(async move {
            if let Err(error) = start_matrix_client(
                matrix_config,
                session_config,
                mcp_config,
                vad_config,
                audio_config,
                ct_for_matrix,
            )
            .await
            {
                tracing::error!("{:?}", error);
            }
        }));
    }
    let join_results = join_all(handles).await;
    tracing::info!("all joinhandle({}) end", join_results.len());
    Ok(())
}

pub async fn start_matrix_client(
    matrix_config: Arc<MatrixConfig>,
    session_config: Arc<SessionConfig>,
    mcp_config: Arc<McpConfig>,
    vad_config: Arc<VadConfig>,
    audio_config: Arc<AudioConfig>,
    shutdown_token: CancellationToken,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("matrix client start");
    matrix::client::start(
        matrix_config,
        session_config,
        mcp_config,
        vad_config,
        audio_config,
        shutdown_token,
    )
    .await?;
    tracing::info!("matrix client end");
    Ok(())
}

pub async fn start_app(
    server_config: Arc<ServerConfig>,
    state: AppState,
    ct: CancellationToken,
) -> anyhow::Result<()> {
    let addrs = server_config
        .address
        .as_ref()
        .expect("server address is empty")
        .addrs
        .clone();
    let port = match &server_config
        .port
        .as_ref()
        .expect("server port is empty")
        .ports
    {
        Either::Left(value) => value,
        Either::Right(values) => values.first().expect("port is empty"),
    };
    let (app, ct) = create_router(state, ct);
    tracing::info!("app start");
    let addr = match addrs {
        Either::Left(value) => value.to_string(),
        Either::Right(values) => values.first().expect("addrs is empty").to_string(),
    };
    let listener = TcpListener::bind(format!("{addr}:{port}")).await?.tap_io(|tcp_stream| {
        if let Err(e) = tcp_stream.set_nodelay(true) {
            tracing::warn!(component = "WS", event = "tcp_nodelay_failed", error = %e, "failed to set TCP_NODELAY");
        }
    });
    tracing::info!("listening on {addr}:{port}");
    let app = NormalizePathLayer::trim_trailing_slash().layer(app);
    axum::serve(
        listener,
        ServiceExt::<Request>::into_make_service_with_connect_info::<SocketAddr>(app),
    )
    .with_graceful_shutdown(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = ct.cancelled() => {}
        }
        tracing::info!("shutting down...");
        ct.cancel();
    })
    .await?;
    tracing::info!("shutdown complete");
    Ok(())
}

#[derive(OpenApi)]
#[openapi()]
struct ApiDoc;

pub fn create_router(
    state: AppState,
    cancellation_token: CancellationToken,
) -> (Router, CancellationToken) {
    let mut api = ApiDoc::openapi();
    api.components.as_mut().unwrap().add_security_scheme(
        "AccessToken",
        SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
    );
    let mut api_router = OpenApiRouter::with_openapi(api);
    api_router = setup_index(api_router);
    api_router = setup_auth(api_router, state.clone());
    api_router = setup_ota(api_router, state.clone());
    api_router = setup_device(api_router, state.clone());
    api_router = setup_record(api_router, state.clone());
    api_router = setup_stats(api_router, state.clone());
    api_router = setup_ws(api_router, state.clone());
    api_router = setup_mcp(api_router, state.clone(), cancellation_token.child_token());
    let (mut app, api) = api_router.split_for_parts();
    app = setup_web(app);
    app = setup_api_fallback(app);
    app = setup_default(app);
    app = app.merge(Scalar::with_url("/docs", api));
    (app, cancellation_token)
}

pub fn setup_default(router: Router) -> Router {
    let app = router
        .fallback(web::index_handler)
        .method_not_allowed_fallback(async || -> AppResult<()> {
            tracing::warn!("Method not allowed");
            Err(AppError::from_code(FrameworkErrorCode::MethodNotAllowed))
        });
    let timeout = TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, HTTP_REQUEST_TIMEOUT);
    let body_limit = DefaultBodyLimit::max(ByteSize::mib(10).as_u64() as usize);
    let cors = CorsLayer::new()
        .allow_origin(cors::Any)
        .allow_methods(cors::Any)
        .allow_headers(cors::Any)
        .allow_credentials(false)
        .max_age(Duration::from_secs(3600 * 12));
    let tracing = TraceLayer::new_for_http()
        .make_span_with(|request: &Request| {
            let method = request.method();
            let path = request.uri().path();
            let headers = request.headers();
            let id = xid::new();
            tracing::trace!("headers = {:?}", headers);
            tracing::debug_span!("Api Request",id = %id,method = %method,path = %path)
        })
        .on_request(())
        .on_failure(())
        .on_response(LatencyOnResponse);
    app.layer(timeout)
        .layer(body_limit)
        .layer(tracing)
        .layer(cors)
}

pub fn setup_index(router: OpenApiRouter) -> OpenApiRouter {
    router.merge(index::create_routes())
}

pub fn setup_ws(router: OpenApiRouter, state: AppState) -> OpenApiRouter {
    router.merge(ws::create_routes(state))
}

pub fn setup_mcp(
    router: OpenApiRouter,
    state: AppState,
    cancellation_token: CancellationToken,
) -> OpenApiRouter {
    router.merge(mcp::create_routes(state, cancellation_token))
}

pub fn setup_auth(router: OpenApiRouter, state: AppState) -> OpenApiRouter {
    api_setup(router, auth::create_routes(state))
}

pub fn setup_ota(router: OpenApiRouter, state: AppState) -> OpenApiRouter {
    api_setup(router, ota::create_routes(state))
}

pub fn setup_device(router: OpenApiRouter, state: AppState) -> OpenApiRouter {
    api_setup(router, device::create_routes(state))
}

pub fn setup_record(router: OpenApiRouter, state: AppState) -> OpenApiRouter {
    api_setup(router, record::create_routes(state))
}

pub fn setup_stats(router: OpenApiRouter, state: AppState) -> OpenApiRouter {
    api_setup(router, stats::create_routes(state))
}

fn api_setup(router: OpenApiRouter, api_router: OpenApiRouter) -> OpenApiRouter {
    router.nest("/api", api_router)
}

fn setup_api_fallback(router: Router) -> Router {
    router.nest(
        "/api",
        Router::new().fallback(async || -> AppResult<()> {
            tracing::warn!("Not found");
            Err(AppError::from_code(CriticalErrorCode::ResourceNotFound))
        }),
    )
}

pub fn setup_web(router: Router) -> Router {
    router
        .nest(
            "/assets",
            Router::new()
                .route("/{*file}", get(web::assets_handler))
                .route_layer(CompressionLayer::new()),
        )
        .nest(
            "/locales",
            Router::new()
                .route("/{*file}", get(web::locales_handler))
                .route_layer(CompressionLayer::new()),
        )
        .nest(
            "/device/assets",
            Router::new()
                .route("/{*file}", get(web::device_assets_handler))
                .route_layer(CompressionLayer::new()),
        )
        .nest(
            "/test",
            Router::new()
                .route("/{*file}", get(web::test_handler))
                .route_layer(CompressionLayer::new()),
        )
}

async fn load_used_codes(conn: &DatabaseConnection) -> Result<Vec<u32>, sea_orm::DbErr> {
    entity::device::Entity::find()
        .select_only()
        .column(entity::device::Column::ActivationCode)
        .filter(entity::device::Column::Activated.eq(false))
        .filter(entity::device::Column::ActivationCode.is_not_null())
        .filter(
            entity::device::Column::ActivationCodeExpiresAt.gt(chrono::Local::now().fixed_offset()),
        )
        .all(conn)
        .await
        .map(|rows| {
            rows.into_iter()
                .filter_map(|d: entity::device::Model| d.activation_code?.parse::<u32>().ok())
                .collect()
        })
}

#[derive(Clone, Debug, Default)]
pub struct AppState {
    pub conn: DatabaseConnection,
    pub session_config: Arc<SessionConfig>,
    pub mcp_config: Arc<McpConfig>,
    pub vad_config: Arc<VadConfig>,
    pub audio_config: Arc<AudioConfig>,
    pub auth_config: Arc<AuthConfig>,
    pub ws_config: Arc<WsConfig>,
    pub cancellation_token: CancellationToken,
}
