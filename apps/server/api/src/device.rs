use axum::{
    Extension, debug_handler,
    extract::{Path, Query, State},
};
use entity::{device, prelude::*};
use framework::{
    auth::{Jwt, Principal},
    data::{ApiResponse, valid::ValidJson},
    err,
    error::AppResult,
    middleware::get_auth_layer,
};
use sea_orm::{ActiveValue::Set, QueryOrder as _, QuerySelect as _, prelude::*};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use validator::Validate;

use crate::AppState;
use crate::activation_pool::ActivationPool;
use crate::ota::OtaErrorCode;

use chrono::Local;
use serde::{Deserialize, Serialize};

const TAG: &str = "device";

pub fn create_routes(state: AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(devices))
        .routes(routes!(activate))
        .routes(routes!(activate_by_id))
        .routes(routes!(disable_device))
        .routes(routes!(enable_device))
        .routes(routes!(delete_device))
        .route_layer(get_auth_layer())
        .with_state(state)
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ActivateParam {
    /// 设备屏幕上显示的激活码
    pub activation_code: String,
}

#[derive(Debug, Serialize, ToSchema, Default)]
pub struct ActivateResult {
    pub device_id: String,
    pub board_type: String,
    pub board_name: Option<String>,
    pub activated: bool,
    /// 设备 JWT token
    pub token: String,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DeviceListParam {
    #[param(example = "1")]
    pub page: Option<u64>,
    #[param(example = "20")]
    pub page_size: Option<u64>,
    /// 搜索关键词（匹配 device_id、board_type、board_name）
    pub search: Option<String>,
    /// 状态筛选：all / pending / activated / disabled
    #[param(example = "all")]
    pub status: Option<String>,
}

#[derive(Debug, Serialize, ToSchema, Default)]
pub struct DeviceListResult {
    pub items: Vec<device::Model>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
}

#[debug_handler]
#[utoipa::path(get, path = "/devices", tag = TAG, params(DeviceListParam))]
async fn devices(
    State(AppState { conn, .. }): State<AppState>,
    Query(params): Query<DeviceListParam>,
) -> AppResult<ApiResponse<DeviceListResult>> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;

    let mut query = Device::find();

    if let Some(ref status) = params.status {
        match status.as_str() {
            "pending" => {
                query = query.filter(device::Column::Activated.eq(false));
            }
            "activated" => {
                query = query.filter(device::Column::Activated.eq(true));
            }
            "disabled" => {
                query = query.filter(device::Column::Disabled.eq(true));
            }
            _ => {}
        }
    }

    if let Some(ref search) = params.search {
        let pattern = format!("%{}%", search);
        query = query.filter(
            sea_orm::Condition::any()
                .add(device::Column::DeviceId.like(&pattern))
                .add(device::Column::BoardType.like(&pattern))
                .add(device::Column::BoardName.like(&pattern)),
        );
    }

    let total = query.clone().count(&conn).await? as u64;
    let items = query
        .order_by_desc(device::Column::CreateDatetime)
        .offset(offset)
        .limit(page_size)
        .all(&conn)
        .await?;

    Ok(ApiResponse::success(Some(DeviceListResult {
        items,
        total,
        page,
        page_size,
    })))
}

#[debug_handler]
#[utoipa::path(post, path = "/devices/activate", tag = TAG)]
async fn activate(
    State(AppState { conn, .. }): State<AppState>,
    Extension(principal): Extension<Principal>,
    ValidJson(param): ValidJson<ActivateParam>,
) -> AppResult<ApiResponse<ActivateResult>> {
    let now = Local::now().fixed_offset();

    let device = Device::find()
        .filter(device::Column::ActivationCode.eq(Some(param.activation_code.clone())))
        .one(&conn)
        .await?
        .ok_or_else(|| {
            tracing::warn!(component = "OTA", event = "activation_code_not_found", code = %param.activation_code, "admin activate failed: code not found");
            err!(OtaErrorCode::ActivationCodeNotFound)
        })?;

    if device.activated {
        tracing::warn!(component = "OTA", event = "device_already_activated", device_id = %device.device_id, "admin activate failed: already activated");
        return Err(err!(OtaErrorCode::DeviceAlreadyActivated));
    }

    if let Some(exp) = device.activation_code_expires_at
        && exp <= now
    {
        tracing::warn!(component = "OTA", event = "activation_code_expired", device_id = %device.device_id, "admin activate failed: code expired");
        return Err(err!(OtaErrorCode::ActivationCodeExpired));
    }

    if let Some(ref code) = device.activation_code
        && let Ok(code_num) = code.parse::<u32>()
    {
        ActivationPool::global().lock().unwrap().discard(code_num);
    }

    let device_mac = device
        .mac_address
        .clone()
        .unwrap_or_else(|| device.device_id.clone());

    let mut active: device::ActiveModel = device.clone().into();
    active.activated = Set(true);
    active.activation_code = Set(None);
    active.activation_code_expires_at = Set(None);
    active.user_id = Set(Some(principal.id.clone()));
    active.update(&conn).await?;

    let token = Jwt::global().access_token_encode(&Principal {
        id: device.device_id.clone(),
        name: Some(device.board_type.clone()),
        device_id: Some(device_mac),
        token_type: String::from("device"),
    })?;

    tracing::info!(component = "OTA", event = "device_activated", device_id = %device.device_id, user_id = %principal.id, "device activated by admin");

    Ok(ApiResponse::success(Some(ActivateResult {
        device_id: device.device_id,
        board_type: device.board_type,
        board_name: device.board_name,
        activated: true,
        token,
    })))
}

#[debug_handler]
#[utoipa::path(post, path = "/devices/{device_id}/activate", tag = TAG,
    params(
        ("device_id" = String, Path, description="设备的唯一标识符"),
    ),
)]
async fn activate_by_id(
    State(AppState { conn, .. }): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(device_id): Path<String>,
) -> AppResult<ApiResponse<ActivateResult>> {
    let device = Device::find()
        .filter(device::Column::DeviceId.eq(&device_id))
        .one(&conn)
        .await?
        .ok_or_else(|| {
            tracing::warn!(component = "OTA", event = "device_not_found", device_id = %device_id, "admin activate-by-id failed: device not found");
            err!(OtaErrorCode::DeviceNotFound)
        })?;

    if device.activated {
        tracing::warn!(component = "OTA", event = "device_already_activated", device_id = %device_id, "admin activate-by-id failed: already activated");
        return Err(err!(OtaErrorCode::DeviceAlreadyActivated));
    }

    if let Some(ref code) = device.activation_code
        && let Ok(code_num) = code.parse::<u32>()
    {
        ActivationPool::global().lock().unwrap().discard(code_num);
    }

    let device_mac = device
        .mac_address
        .clone()
        .unwrap_or_else(|| device.device_id.clone());

    let mut active: device::ActiveModel = device.clone().into();
    active.activated = Set(true);
    active.activation_code = Set(None);
    active.activation_code_expires_at = Set(None);
    active.user_id = Set(Some(principal.id.clone()));
    active.update(&conn).await?;

    let token = Jwt::global().access_token_encode(&Principal {
        id: device.device_id.clone(),
        name: Some(device.board_type.clone()),
        device_id: Some(device_mac),
        token_type: String::from("device"),
    })?;

    tracing::info!(component = "OTA", event = "device_activated_by_id", device_id = %device_id, user_id = %principal.id, "device activated by admin via device_id");

    Ok(ApiResponse::success(Some(ActivateResult {
        device_id: device.device_id,
        board_type: device.board_type,
        board_name: device.board_name,
        activated: true,
        token,
    })))
}

#[debug_handler]
#[utoipa::path(post, path = "/devices/{device_id}/disable", tag = TAG,
    params(
        ("device_id" = String, Path, description="设备的唯一标识符"),
    ),
)]
async fn disable_device(
    State(AppState { conn, .. }): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(device_id): Path<String>,
) -> AppResult<ApiResponse<()>> {
    let device = Device::find()
        .filter(device::Column::DeviceId.eq(&device_id))
        .one(&conn)
        .await?
        .ok_or_else(|| {
            tracing::warn!(component = "OTA", event = "device_not_found", device_id = %device_id, "admin disable failed: device not found");
            err!(OtaErrorCode::DeviceNotFound)
        })?;

    if device.disabled {
        tracing::warn!(component = "OTA", event = "device_already_disabled", device_id = %device_id, "admin disable failed: already disabled");
        return Err(err!(OtaErrorCode::DeviceAlreadyDisabled));
    }

    let mut active: device::ActiveModel = device.into();
    active.disabled = Set(true);
    active.update(&conn).await?;

    tracing::info!(component = "OTA", event = "device_disabled_by_admin", device_id = %device_id, user_id = %principal.id, "device disabled by admin");

    Ok(ApiResponse::success(None))
}

#[debug_handler]
#[utoipa::path(post, path = "/devices/{device_id}/enable", tag = TAG,
    params(
        ("device_id" = String, Path, description="设备的唯一标识符"),
    ),
)]
async fn enable_device(
    State(AppState { conn, .. }): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(device_id): Path<String>,
) -> AppResult<ApiResponse<()>> {
    let device = Device::find()
        .filter(device::Column::DeviceId.eq(&device_id))
        .one(&conn)
        .await?
        .ok_or_else(|| {
            tracing::warn!(component = "OTA", event = "device_not_found", device_id = %device_id, "admin enable failed: device not found");
            err!(OtaErrorCode::DeviceNotFound)
        })?;

    if !device.disabled {
        tracing::warn!(component = "OTA", event = "device_not_disabled", device_id = %device_id, "admin enable failed: not disabled");
        return Err(err!(OtaErrorCode::DeviceNotDisabled));
    }

    let mut active: device::ActiveModel = device.into();
    active.disabled = Set(false);
    active.update(&conn).await?;

    tracing::info!(component = "OTA", event = "device_enabled_by_admin", device_id = %device_id, user_id = %principal.id, "device enabled by admin");

    Ok(ApiResponse::success(None))
}

#[debug_handler]
#[utoipa::path(delete, path = "/devices/{device_id}", tag = TAG,
    params(
        ("device_id" = String, Path, description="设备的唯一标识符"),
    ),
)]
async fn delete_device(
    State(AppState { conn, .. }): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(device_id): Path<String>,
) -> AppResult<ApiResponse<()>> {
    let device = Device::find()
        .filter(device::Column::DeviceId.eq(&device_id))
        .one(&conn)
        .await?
        .ok_or_else(|| {
            tracing::warn!(component = "OTA", event = "device_not_found", device_id = %device_id, "admin delete failed: device not found");
            err!(OtaErrorCode::DeviceNotFound)
        })?;

    if let Some(ref code) = device.activation_code
        && !device.activated
        && let Ok(code_num) = code.parse::<u32>()
    {
        ActivationPool::global().lock().unwrap().discard(code_num);
    }

    let id_to_delete = device.id.clone();
    Device::delete_by_id(id_to_delete).exec(&conn).await?;

    tracing::info!(component = "OTA", event = "device_deleted_by_admin", device_id = %device_id, user_id = %principal.id, "device deleted by admin");

    Ok(ApiResponse::success(None))
}
