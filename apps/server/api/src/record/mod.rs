pub mod collector;
pub mod observer;

use std::collections::HashMap;

use axum::{
    debug_handler,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use chrono::{DateTime, Utc};
use framework::{data::ApiResponse, error::AppResult, middleware::get_auth_layer};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QueryTrait};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use entity::prelude::*;
use entity::{frame, round, round_data, session};

const TAG: &str = "record";

pub fn create_routes(state: AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(list_sessions))
        .routes(routes!(get_session_rounds))
        .routes(routes!(list_round_data))
        .routes(routes!(get_round_data_blob))
        .routes(routes!(list_frames))
        .route_layer(get_auth_layer())
        .with_state(state)
}

fn to_utc(dt: Option<chrono::DateTime<chrono::FixedOffset>>) -> Option<DateTime<Utc>> {
    dt.map(|d| d.naive_utc().and_utc())
}

async fn batch_load_round_data(
    conn: &sea_orm::DatabaseConnection,
    round_ids: &[String],
) -> Result<HashMap<String, Vec<round_data::Model>>, sea_orm::DbErr> {
    if round_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let items = round_data::Entity::find()
        .filter(round_data::Column::RoundId.is_in(round_ids.iter().cloned()))
        .all(conn)
        .await?;
    let mut map: HashMap<String, Vec<round_data::Model>> = HashMap::new();
    for d in items {
        map.entry(d.round_id.clone()).or_default().push(d);
    }
    Ok(map)
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListSessionsParams {
    pub search: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub sort_order: Option<String>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TurnStep {
    pub step: String,
    pub has_data: bool,
    pub text: Option<String>,
    pub duration_ms: Option<i64>,
    pub audio_duration_ms: Option<i64>,
}

fn extract_field(d: &&round_data::Model, field: &str) -> Option<i64> {
    d.metadata
        .as_ref()
        .and_then(|m| m.get(field))
        .and_then(|v| v.as_i64())
}

fn build_turn_steps(rd: &[round_data::Model]) -> Vec<TurnStep> {
    rd.iter()
        .map(|d| {
            let step = d.data_type.as_str();
            TurnStep {
                step: step.to_string(),
                has_data: true,
                text: match step {
                    "input_audio" | "tts" => None,
                    _ => d.text.clone(),
                },
                duration_ms: extract_field(&d, "duration_ms"),
                audio_duration_ms: match step {
                    "input_audio" | "tts" => extract_field(&d, "audio_duration_ms"),
                    _ => None,
                },
            }
        })
        .collect()
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TurnSummary {
    pub turn_index: i32,
    pub round_id: String,
    pub mode: String,
    pub create_datetime: Option<DateTime<Utc>>,
    pub steps: Vec<TurnStep>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SessionListItem {
    pub session_id: String,
    pub create_datetime: Option<DateTime<Utc>>,
    pub update_datetime: Option<DateTime<Utc>>,
    pub turn_count: i64,
    pub turns: Vec<TurnSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SessionListResponse {
    pub items: Vec<SessionListItem>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SessionRound {
    pub round_id: String,
    pub mode: String,
    pub create_datetime: Option<DateTime<Utc>>,
    pub steps: Vec<TurnStep>,
}

#[debug_handler]
#[utoipa::path(get, path = "/record/sessions", tag = TAG, params(ListSessionsParams), responses(
    (status = OK, body = ApiResponse<SessionListResponse>)
))]
async fn list_sessions(
    State(AppState { conn, .. }): State<AppState>,
    Query(params): Query<ListSessionsParams>,
) -> AppResult<ApiResponse<SessionListResponse>> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).min(100);
    let order = match params.sort_order.as_deref() {
        Some("asc") => sea_orm::Order::Asc,
        _ => sea_orm::Order::Desc,
    };

    let query = Session::find()
        .apply_if(params.search, |query, q| {
            query.filter(session::Column::Id.contains(&q))
        })
        .apply_if(params.date_from, |query, dt| {
            if let Ok(d) = dt.parse::<DateTime<Utc>>() {
                query.filter(session::Column::CreateDatetime.gte(d))
            } else {
                query
            }
        })
        .apply_if(params.date_to, |query, dt| {
            if let Ok(d) = dt.parse::<DateTime<Utc>>() {
                query.filter(session::Column::CreateDatetime.lte(d))
            } else {
                query
            }
        });

    let query = query.order_by(session::Column::CreateDatetime, order);
    let paginator = query.paginate(&conn, page_size);
    let items = paginator.fetch_page(page - 1).await?;
    let total = paginator.num_items().await?;

    let session_ids: Vec<String> = items.iter().map(|s| s.id.clone()).collect();
    let rounds = if !session_ids.is_empty() {
        round::Entity::find()
            .filter(round::Column::SessionId.is_in(session_ids.clone()))
            .order_by_asc(round::Column::CreateDatetime)
            .all(&conn)
            .await?
    } else {
        vec![]
    };

    let round_ids: Vec<String> = rounds.iter().map(|r| r.id.clone()).collect();
    let data_by_round = batch_load_round_data(&conn, &round_ids).await?;

    let mut rounds_by_session: HashMap<String, Vec<&round::Model>> = HashMap::new();
    for r in &rounds {
        rounds_by_session
            .entry(r.session_id.clone())
            .or_default()
            .push(r);
    }

    let response_items: Vec<SessionListItem> = items
        .into_iter()
        .map(|s| {
            let session_rounds = rounds_by_session.remove(&s.id).unwrap_or_default();
            let turn_count = session_rounds.len() as i64;
            let turns: Vec<TurnSummary> = session_rounds
                .into_iter()
                .enumerate()
                .map(|(i, r)| {
                    let rd = data_by_round.get(&r.id).cloned().unwrap_or_default();
                    let steps = build_turn_steps(&rd);
                    TurnSummary {
                        turn_index: (i + 1) as i32,
                        round_id: r.id.clone(),
                        mode: r.mode.clone(),
                        create_datetime: to_utc(r.create_datetime),
                        steps,
                    }
                })
                .collect();

            SessionListItem {
                session_id: s.id,
                create_datetime: to_utc(s.create_datetime),
                update_datetime: to_utc(s.update_datetime),
                turn_count,
                turns,
            }
        })
        .collect();

    Ok(ApiResponse::success(Some(SessionListResponse {
        items: response_items,
        total,
        page,
        page_size,
    })))
}

#[debug_handler]
#[utoipa::path(get, path = "/record/sessions/{session_id}/rounds", tag = TAG, responses(
    (status = OK, body = ApiResponse<Vec<SessionRound>>)
))]
async fn get_session_rounds(
    State(AppState { conn, .. }): State<AppState>,
    Path(session_id): Path<String>,
) -> AppResult<ApiResponse<Vec<SessionRound>>> {
    let rounds = round::Entity::find()
        .filter(round::Column::SessionId.eq(&session_id))
        .order_by_asc(round::Column::CreateDatetime)
        .all(&conn)
        .await?;

    let round_ids: Vec<String> = rounds.iter().map(|r| r.id.clone()).collect();
    let data_by_round = batch_load_round_data(&conn, &round_ids).await?;

    let result: Vec<SessionRound> = rounds
        .into_iter()
        .map(|r| {
            let rd = data_by_round.get(&r.id).cloned().unwrap_or_default();
            let steps = build_turn_steps(&rd);
            SessionRound {
                round_id: r.id,
                mode: r.mode,
                create_datetime: to_utc(r.create_datetime),
                steps,
            }
        })
        .collect();

    Ok(ApiResponse::success(Some(result)))
}

#[debug_handler]
#[utoipa::path(get, path = "/record/rounds/{id}/data", tag = TAG, responses(
    (status = OK, body = ApiResponse<Vec<round_data::Model>>)
))]
async fn list_round_data(
    State(AppState { conn, .. }): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<ApiResponse<Vec<round_data::Model>>> {
    let items = RoundData::find()
        .filter(round_data::Column::RoundId.eq(&id))
        .all(&conn)
        .await?;
    Ok(ApiResponse::success(Some(items)))
}

#[debug_handler]
#[utoipa::path(get, path = "/record/rounds/{id}/data/{data_id}/blob", tag = TAG, responses(
    (status = 200, content_type = "audio/wav", body = Vec<u8>),
))]
async fn get_round_data_blob(
    State(AppState { conn, .. }): State<AppState>,
    Path((_id, data_id)): Path<(String, String)>,
) -> AppResult<axum::response::Response> {
    let item = RoundData::find_by_id(&data_id)
        .one(&conn)
        .await?
        .ok_or_else(|| {
            framework::err!(framework::error::critical_code::CriticalErrorCode::ResourceNotFound)
        })?;
    match item.data {
        Some(bytes) => Ok(([("Content-Type", "audio/wav")], bytes).into_response()),
        None => Err(framework::err!(
            framework::error::critical_code::CriticalErrorCode::ResourceNotFound
        )),
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListFramesParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FrameListResponse {
    pub items: Vec<frame::Model>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
}

#[debug_handler]
#[utoipa::path(get, path = "/record/rounds/{id}/frames", tag = TAG, params(ListFramesParams), responses(
    (status = OK, body = ApiResponse<FrameListResponse>)
))]
async fn list_frames(
    State(AppState { conn, .. }): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<ListFramesParams>,
) -> AppResult<ApiResponse<FrameListResponse>> {
    let page_size = params.page_size.unwrap_or(0);
    let page = if page_size == 0 {
        let items = Frame::find()
            .filter(frame::Column::RoundId.eq(&id))
            .order_by_asc(frame::Column::Seq)
            .all(&conn)
            .await?;
        let total = items.len() as u64;
        return Ok(ApiResponse::success(Some(FrameListResponse {
            items,
            total,
            page: 1,
            page_size: total,
        })));
    } else {
        params.page.unwrap_or(1).max(1)
    };

    let paginator = Frame::find()
        .filter(frame::Column::RoundId.eq(&id))
        .order_by_asc(frame::Column::Seq)
        .paginate(&conn, page_size);
    let items = paginator.fetch_page(page - 1).await?;
    let total = paginator.num_items().await?;
    Ok(ApiResponse::success(Some(FrameListResponse {
        items,
        total,
        page,
        page_size,
    })))
}
