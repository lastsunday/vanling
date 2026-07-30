use std::collections::HashMap;

use axum::debug_handler;
use axum::extract::State;
use chrono::{DateTime, Days, Local, NaiveDate, Utc};
use entity::prelude::*;
use entity::{device, round_data, session};
use framework::{data::ApiResponse, error::AppResult, middleware::get_auth_layer};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;

const TAG: &str = "stats";

pub fn create_routes(state: AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(summary))
        .routes(routes!(trends))
        .routes(routes!(latency))
        .route_layer(get_auth_layer())
        .with_state(state)
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DashboardSummary {
    pub total_devices: u64,
    pub activated_devices: u64,
    pub pending_devices: u64,
    pub disabled_devices: u64,
    pub online_devices: u64,
    pub total_sessions: u64,
    pub sessions_today: u64,
    pub total_rounds: u64,
    pub server_version: String,
    pub server_time: String,
    pub recent_sessions: Vec<RecentSession>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RecentSession {
    pub session_id: String,
    pub device_id: Option<String>,
    pub uid: Option<String>,
    pub board_type: Option<String>,
    pub board_name: Option<String>,
    pub chip_model_name: Option<String>,
    pub create_datetime: Option<DateTime<Utc>>,
    pub turn_count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DailyTrend {
    pub date: String,
    pub sessions: i64,
    pub rounds: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DashboardTrends {
    pub daily: Vec<DailyTrend>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StepLatency {
    pub data_type: String,
    pub avg_ms: f64,
    pub max_ms: i64,
    pub min_ms: i64,
    pub count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DashboardLatency {
    pub steps: Vec<StepLatency>,
}

fn to_utc(dt: Option<DateTime<chrono::FixedOffset>>) -> Option<DateTime<Utc>> {
    dt.map(|d| d.with_timezone(&Utc))
}

#[debug_handler]
#[utoipa::path(get, path = "/stats/summary", tag = TAG)]
async fn summary(
    State(AppState { conn, .. }): State<AppState>,
) -> AppResult<ApiResponse<DashboardSummary>> {
    let total_devices = Device::find().count(&conn).await?;
    let activated_devices = Device::find()
        .filter(device::Column::Activated.eq(true))
        .filter(device::Column::Disabled.eq(false))
        .count(&conn)
        .await?;
    let pending_devices = Device::find()
        .filter(device::Column::Activated.eq(false))
        .filter(device::Column::Disabled.eq(false))
        .count(&conn)
        .await?;
    let disabled_devices = Device::find()
        .filter(device::Column::Disabled.eq(true))
        .count(&conn)
        .await?;

    let now_local = Local::now().fixed_offset();
    let cutoff = now_local - chrono::Duration::minutes(5);
    let online_devices = Device::find()
        .filter(device::Column::Activated.eq(true))
        .filter(device::Column::Disabled.eq(false))
        .filter(device::Column::LastOnlineDatetime.gte(cutoff))
        .count(&conn)
        .await?;

    let total_sessions = Session::find().count(&conn).await?;

    let today_start = now_local
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|d| d.and_local_timezone(Local).unwrap())
        .map(|d| d.fixed_offset());
    let sessions_today = match today_start {
        Some(ts) => {
            Session::find()
                .filter(session::Column::CreateDatetime.gte(ts))
                .count(&conn)
                .await?
        }
        None => 0,
    };

    let total_rounds = Round::find().count(&conn).await?;

    let recent_sessions = load_recent_sessions(&conn).await?;

    Ok(ApiResponse::success(Some(DashboardSummary {
        total_devices,
        activated_devices,
        pending_devices,
        disabled_devices,
        online_devices,
        total_sessions,
        sessions_today,
        total_rounds,
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        server_time: Local::now().to_rfc3339(),
        recent_sessions,
    })))
}

async fn load_recent_sessions(conn: &sea_orm::DatabaseConnection) -> AppResult<Vec<RecentSession>> {
    let sessions = Session::find()
        .order_by_desc(session::Column::CreateDatetime)
        .limit(5)
        .all(conn)
        .await?;

    let mut result = Vec::with_capacity(sessions.len());
    for s in sessions {
        let turn_count = Round::find()
            .filter(entity::round::Column::SessionId.eq(&s.id))
            .count(conn)
            .await? as i64;

        let device = match &s.device_id {
            Some(did) => {
                Device::find()
                    .filter(device::Column::Id.eq(did))
                    .one(conn)
                    .await?
            }
            None => None,
        };

        result.push(RecentSession {
            session_id: s.id,
            device_id: s.device_id,
            uid: device.as_ref().map(|d| d.uid.clone()),
            board_type: device.as_ref().map(|d| d.board_type.clone()),
            board_name: device.as_ref().and_then(|d| d.board_name.clone()),
            chip_model_name: device.as_ref().and_then(|d| d.chip_model_name.clone()),
            create_datetime: to_utc(s.create_datetime),
            turn_count,
        });
    }
    Ok(result)
}

#[debug_handler]
#[utoipa::path(get, path = "/stats/trends", tag = TAG)]
async fn trends(
    State(AppState { conn, .. }): State<AppState>,
) -> AppResult<ApiResponse<DashboardTrends>> {
    let today = Local::now().date_naive();
    let start_date = today - Days::new(13);

    let date_range: Vec<NaiveDate> = (0..14)
        .filter_map(|i| start_date.checked_add_days(Days::new(i)))
        .collect();

    let start_dt = start_date
        .and_hms_opt(0, 0, 0)
        .map(|d| d.and_local_timezone(Local).unwrap())
        .map(|d| d.fixed_offset());

    let daily = match start_dt {
        Some(threshold) => {
            let sessions = Session::find()
                .filter(session::Column::CreateDatetime.gte(threshold))
                .all(&conn)
                .await?;

            let rounds = Round::find()
                .filter(entity::round::Column::CreateDatetime.gte(threshold))
                .all(&conn)
                .await?;

            let mut sessions_by_day: HashMap<NaiveDate, i64> = HashMap::new();
            for s in &sessions {
                if let Some(dt) = s.create_datetime {
                    *sessions_by_day.entry(dt.date_naive()).or_default() += 1;
                }
            }

            let mut rounds_by_day: HashMap<NaiveDate, i64> = HashMap::new();
            for r in &rounds {
                if let Some(dt) = r.create_datetime {
                    *rounds_by_day.entry(dt.date_naive()).or_default() += 1;
                }
            }

            date_range
                .into_iter()
                .map(|d| DailyTrend {
                    date: d.format("%Y-%m-%d").to_string(),
                    sessions: sessions_by_day.get(&d).copied().unwrap_or(0),
                    rounds: rounds_by_day.get(&d).copied().unwrap_or(0),
                })
                .collect()
        }
        None => vec![],
    };

    Ok(ApiResponse::success(Some(DashboardTrends { daily })))
}

#[debug_handler]
#[utoipa::path(get, path = "/stats/latency", tag = TAG)]
async fn latency(
    State(AppState { conn, .. }): State<AppState>,
) -> AppResult<ApiResponse<DashboardLatency>> {
    let seven_days_ago = (Local::now() - chrono::Duration::days(7)).fixed_offset();

    let items = RoundData::find()
        .filter(round_data::Column::CreateDatetime.gte(seven_days_ago))
        .all(&conn)
        .await?;

    let mut elapsed_by_type: HashMap<String, Vec<i64>> = HashMap::new();
    for item in &items {
        if let Some(ref meta) = item.metadata
            && let Some(elapsed) = meta.get("elapsed_ms").and_then(|v| v.as_i64())
        {
            elapsed_by_type
                .entry(item.data_type.clone())
                .or_default()
                .push(elapsed);
        }
    }

    let mut steps: Vec<StepLatency> = elapsed_by_type
        .into_iter()
        .map(|(data_type, vals)| {
            let count = vals.len() as i64;
            let sum: i64 = vals.iter().sum();
            let max = *vals.iter().max().unwrap_or(&0);
            let min = *vals.iter().min().unwrap_or(&0);
            let avg = if count > 0 {
                sum as f64 / count as f64
            } else {
                0.0
            };
            StepLatency {
                data_type,
                avg_ms: (avg * 100.0).round() / 100.0,
                max_ms: max,
                min_ms: min,
                count,
            }
        })
        .collect();

    steps.sort_by(|a, b| {
        b.avg_ms
            .partial_cmp(&a.avg_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(ApiResponse::success(Some(DashboardLatency { steps })))
}
