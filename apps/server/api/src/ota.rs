use axum::{
    debug_handler,
    extract::{ConnectInfo, Json, State},
    http::{HeaderMap, StatusCode},
};
use axum_extra::{TypedHeader, headers};
use entity::{device, prelude::*};
use framework::{
    auth::{Jwt, Principal},
    data::valid::ValidJson,
    err,
    error::AppResult,
    prelude::error,
};
use sea_orm::{ActiveValue::Set, prelude::*};
use serde::{Deserialize, Serialize};
use serde_aux::prelude::*;
use std::net::SocketAddr;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use validator::Validate;

use crate::AppState;
use crate::activation_pool::ActivationPool;

use anyhow::Context;
use chrono::Duration;
use chrono::Local;
use jiff::tz::TimeZone;

pub const KEY_DEVICE_ID: &str = "Device-Id";
pub const KEY_CLIENT_ID: &str = "Client-Id";
pub const KEY_USER_AGENT: &str = "User-Agent";

#[derive(Debug, Deserialize, Validate, ToSchema)]
#[schema(example = json!({}))]
pub struct OtaParam {
    pub version: Option<u32>,
    pub language: Option<String>,
    pub flash_size: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_option_number_from_string")]
    pub minimum_free_heap_size: Option<u64>,
    pub mac_address: Option<String>,
    pub chip_model_name: Option<String>,
    pub chip_info: Option<ChipInfo>,
    pub psram_size: Option<u64>,
    pub uuid: Option<String>,
    pub application: Application,
    pub partition_table: Option<Vec<Partition>>,
    pub ota: Option<Ota>,
    pub board: Board,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
#[schema(description = "包含设备当前固件版本信息的对象")]
pub struct Application {
    pub name: Option<String>,
    pub version: String,
    pub compile_time: Option<String>,
    pub idf_version: Option<String>,
    pub elf_sha256: String,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct Partition {
    pub label: String,
    #[serde(rename = "type")]
    pub mtype: u32,
    pub subtype: u32,
    pub address: u64,
    pub size: u64,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ChipInfo {
    pub model: u64,
    pub cores: u64,
    pub revision: u64,
    pub features: u64,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct Ota {
    pub label: String,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
#[schema(description = "开发板类型与版本，以及所运行的环境")]
pub struct Board {
    #[serde(rename = "type")]
    pub mtype: String,
    pub name: Option<String>,
    pub ssid: Option<String>,
    pub rssi: Option<i32>,
    pub channel: Option<i32>,
    pub ip: Option<String>,
    pub mac: Option<String>,
}

#[derive(Debug, Serialize, ToSchema, Default)]
pub struct OtaResult {
    pub activation: Option<Activation>,
    pub mqtt: Option<Mqtt>,
    pub websocket: Websocket,
    pub server_time: ServerTime,
    pub firmware: Option<Firmware>,
}

#[derive(Debug, Serialize, ToSchema, Default)]
#[schema(description = "设备需要激活")]
pub struct Activation {
    pub code: String,
    pub message: String,
    pub challenge: String,
}

#[derive(Debug, Serialize, ToSchema, Default)]
#[schema(description = "MQTT协议服务器配置信息")]
pub struct Mqtt {
    pub endpoint: String,
    pub client_id: String,
    pub username: String,
    pub password: String,
    pub publish_topic: String,
}

#[derive(Debug, Serialize, ToSchema, Default)]
#[schema(description = "Websocket协议服务器配置信息")]
pub struct Websocket {
    pub url: String,
    pub token: String,
}

#[derive(Debug, Serialize, ToSchema, Default)]
#[schema(description = "服务器时间信息（用于同步设备时间）")]
pub struct ServerTime {
    pub timestamp: i64,
    pub timezone: String,
    pub timezone_offset: i32,
}

#[derive(Debug, Serialize, ToSchema, Default)]
#[schema(description = "最新版本固件信息")]
pub struct Firmware {
    pub version: String,
    pub url: Option<String>,
}

const TAG: &str = "ota";

pub fn create_routes(state: AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(ota))
        .routes(routes!(activate))
        .with_state(state)
}

#[debug_handler]
#[utoipa::path(post, path = "/ota",tag=TAG,security(()),
    params(
        ("Device-Id" = String,Header,description="设备的唯一标识符（使用MAC地址或由硬件ID生成的伪MAC地址）",example="11:22:33:44:55:66"),
        ("Client-Id" = String,Header,description="客户端的唯一标识符，由软件自动生成的UUID v4（擦除FLASH或重装后会变化）",example="7b94d69a-9808-4c59-9c9b-704333b38aff"),
        ("User-Agent" = String,Header,description="客户端的名字和版本号（例如 esp-box-3/1.5.6）",example="xingzhi-cube-1.54tft-wifi/1.0.1"),
        ("Accept-Language" = Option<String>,Header,description="客户端的当前语言（可选，例如 zh-CN）",example="zh-CN"),
    ),
    request_body(content=OtaParam,examples(
    ("ESP32 完整请求示例" = (value=json!(
         {
          "version": 2,
          "language": "zh-CN",
          "flash_size": 16777216,
          "minimum_free_heap_size": 8457848,
          "mac_address": "11:22:33:44:55:66",
          "chip_model_name": "esp32s3",
          "uuid": "7b94d69a-9808-4c59-9c9b-704333b38aff",
          "application": {
            "name": "xiaozhi",
            "version": "1.0.1",
            "compile_time": "Feb  1 2025T23:02:27Z",
            "idf_version": "v5.4-dirty",
            "elf_sha256": "c8a8ecb6d6fbcda682494d9675cd1ead240ecf38bdde75282a42365a0e396033"
          },
          "partition_table": [
            {
              "label": "nvs",
              "type": 1,
              "subtype": 2,
              "address": 36864,
              "size": 16384
            },
            {
              "label": "otadata",
              "type": 1,
              "subtype": 0,
              "address": 53248,
              "size": 8192
            },
            {
              "label": "phy_init",
              "type": 1,
              "subtype": 1,
              "address": 61440,
              "size": 4096
            },
            {
              "label": "model",
              "type": 1,
              "subtype": 130,
              "address": 65536,
              "size": 983040
            },
            {
              "label": "ota_0",
              "type": 0,
              "subtype": 16,
              "address": 1048576,
              "size": 6291456
            },
            {
              "label": "ota_1",
              "type": 0,
              "subtype": 17,
              "address": 7340032,
              "size": 6291456
            }
          ],
          "ota": {
            "label": "ota_0"
          },
          "board": {
            "type": "xingzhi-cube-1.54tft-wifi",
            "name": "xingzhi-cube-1.54tft-wifi",
            "ssid": "卧室",
            "rssi": -55,
            "channel": 1,
            "ip": "192.168.1.11",
            "mac": "11:22:33:44:55:66"
          }
    }
    ))),
    ("非ESP32最小请求示例 Wi-Fi" = (value=json!(
       {
          "application": {
            "version": "1.0.1",
            "elf_sha256": "c8a8ecb6d6fbcda682494d9675cd1ead240ecf38bdde75282a42365a0e396033"
          },
          "board": {
            "type": "bread-compact-wifi",
            "name": "bread-compact-wifi-128x64",
            "ssid": "卧室",
            "rssi": -55,
            "channel": 1,
            "ip": "192.168.1.11",
            "mac": "11:22:33:44:55:66"
          }
        }
    ))),
    ("非ESP32最小请求示例 4G" = (value=json!(
        {
          "application": {
            "version": "1.0.1",
            "elf_sha256": "c8a8ecb6d6fbcda682494d9675cd1ead240ecf38bdde75282a42365a0e396033"
          },
          "board": {
            "type": "kevin-box",
            "name": "kevin-box-2",
            "revision": "ML307R-DL-MBRH0S00",
            "carrier": "CHINA MOBILE",
            "csq": "22",
            "imei": "****",
            "iccid": "****"
          }
        }
     ))),
    )),
    responses(
    (status=OK,body=OtaResult,example=json!(
        {
          "mqtt": {
            "endpoint": "mqtt.example.com",
            "client_id": "GID_test@@@device-id@@@uuid",
            "username": "device_12345",
            "password": "password",
            "publish_topic": "device-server"
          },
          "websocket": {
            "url": "wss://api.tenclass.net/xiaozhi/v1/",
            "token": "test-token"
          },
          "server_time": {
            "timestamp": 1633024800000i64,
            "timezone": "Asia/Shanghai",
            "timezone_offset": -480
          },
          "firmware": {
            "version": "1.0.0",
            "url": "https://example.com/firmware/1.0.0.bin"
          }
        }
     ))
))]
async fn ota(
    State(AppState {
        conn, ws_config, ..
    }): State<AppState>,
    ConnectInfo(_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    host: TypedHeader<headers::Host>,
    ValidJson(param): ValidJson<OtaParam>,
) -> AppResult<Json<OtaResult>> {
    let device_id = headers
        .get(KEY_DEVICE_ID)
        .ok_or_else(|| err!(OtaErrorCode::LackDeviceId))?
        .to_str()
        .map_err(|_| err!(OtaErrorCode::LackDeviceId))?;
    let client_id = headers
        .get(KEY_CLIENT_ID)
        .ok_or_else(|| err!(OtaErrorCode::LackClientId))?
        .to_str()
        .map_err(|_| err!(OtaErrorCode::LackClientId))?;
    let user_agent = headers
        .get(KEY_USER_AGENT)
        .ok_or_else(|| err!(OtaErrorCode::LackUserAgent))?
        .to_str()
        .map_err(|_| err!(OtaErrorCode::LackUserAgent))?;

    let now = Local::now().fixed_offset();

    let device_mac = param
        .board
        .mac
        .clone()
        .unwrap_or_else(|| device_id.to_owned());

    let existing = Device::find()
        .filter(device::Column::Uid.eq(device_id))
        .one(&conn)
        .await?;

    let (device_id_owned, board_type, activated, activation_code) = if let Some(device) = existing {
        if device.disabled {
            tracing::warn!(component = "OTA", event = "device_disabled", device_id = %device_id, "ota request from disabled device");
            return Err(err!(OtaErrorCode::DeviceDisabled));
        }

        let device_id_owned = device.id.clone();
        let board_type = device.board_type.clone();
        let activated = device.activated;
        let mut activation_code = device.activation_code.clone();
        let code_expires_at = device.activation_code_expires_at;

        let mut active: device::ActiveModel = device.into();
        active.client_id = Set(Some(client_id.to_owned()));
        active.user_agent = Set(Some(user_agent.to_owned()));
        active.mac_address = Set(Some(device_mac.clone()));
        active.chip_model_name = Set(param.chip_model_name.clone());
        active.application_name = Set(param.application.name.clone());
        active.application_version = Set(param.application.version.clone());
        active.board_type = Set(board_type.clone());
        active.board_name = Set(param.board.name.clone());
        active.last_online_datetime = Set(Some(now));

        if !activated {
            let is_expired = match code_expires_at {
                Some(exp) => exp <= now,
                None => true,
            };
            if is_expired {
                let code_num = ActivationPool::global().lock().unwrap().draw();
                let code_str = format!("{:06}", code_num);
                active.activation_code = Set(Some(code_str.clone()));
                active.activation_code_expires_at = Set(Some(now + Duration::minutes(15)));
                activation_code = Some(code_str);
            }
        }

        active.update(&conn).await?;

        (device_id_owned, board_type, activated, activation_code)
    } else {
        let board_type = param.board.mtype.clone();
        let code_num = ActivationPool::global().lock().unwrap().draw();
        let code = format!("{:06}", code_num);

        let new_device = device::ActiveModel {
            uid: Set(device_id.to_owned()),
            client_id: Set(Some(client_id.to_owned())),
            user_agent: Set(Some(user_agent.to_owned())),
            mac_address: Set(Some(device_mac.clone())),
            chip_model_name: Set(param.chip_model_name.clone()),
            application_name: Set(param.application.name.clone()),
            application_version: Set(param.application.version.clone()),
            board_type: Set(board_type.clone()),
            board_name: Set(param.board.name.clone()),
            activated: Set(false),
            disabled: Set(false),
            activation_code: Set(Some(code.clone())),
            activation_code_expires_at: Set(Some(now + Duration::minutes(15))),
            last_online_datetime: Set(Some(now)),
            ..Default::default()
        }
        .insert(&conn)
        .await?;

        (new_device.id.clone(), board_type, false, Some(code))
    };

    let address = match host.port() {
        Some(port) => format!("{}:{}", host.hostname(), port),
        None => host.hostname().to_owned(),
    };

    let tz = TimeZone::system();
    let iana_identifier = tz.iana_name().context("get iana name failure")?;

    let ws_url = format!(
        "{}://{}/vanling/v1",
        ws_config.schema.as_ref().expect("ws schema is empty"),
        address
    );

    let websocket = if activated {
        let token = Jwt::global().access_token_encode(&Principal {
            id: device_id_owned.clone(),
            name: Some(board_type.clone()),
            token_type: String::from("device"),
        })?;
        Websocket { url: ws_url, token }
    } else {
        Websocket {
            url: ws_url,
            token: String::new(),
        }
    };

    Ok(Json(OtaResult {
        mqtt: None,
        websocket,
        server_time: ServerTime {
            timestamp: now.timestamp_millis(),
            timezone: String::from(iana_identifier),
            timezone_offset: -(now.offset().utc_minus_local() / 60),
        },
        firmware: Some(Firmware {
            version: String::from("0.0.1"),
            url: None,
        }),
        activation: match activation_code {
            Some(code) if !activated => Some(Activation {
                message: format!("请在后台管理输入激活码绑定设备，激活码为：{}", code),
                code,
                challenge: String::new(),
            }),
            _ => None,
        },
    }))
}

#[debug_handler]
#[utoipa::path(post, path = "/ota/activate",tag=TAG,security(()),
    params(
        ("Device-Id" = String,Header,description="设备的唯一标识符（使用MAC地址或由硬件ID生成的伪MAC地址）",example="11:22:33:44:55:66"),
        ("Client-Id" = Option<String>,Header,description="客户端的唯一标识符，由软件自动生成的UUID v4（擦除FLASH或重装后会变化）",example="7b94d69a-9808-4c59-9c9b-704333b38aff"),
        ("Accept-Language" = Option<String>,Header,description="客户端的当前语言（可选，例如 zh-CN）",example="zh-CN"),
    ),
)]
async fn activate(
    State(AppState { conn, .. }): State<AppState>,
    ConnectInfo(_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> AppResult<StatusCode> {
    let device_id = headers
        .get(KEY_DEVICE_ID)
        .ok_or_else(|| err!(OtaErrorCode::LackDeviceId))?
        .to_str()
        .map_err(|_| err!(OtaErrorCode::LackDeviceId))?;

    let device = Device::find()
        .filter(device::Column::Uid.eq(device_id))
        .one(&conn)
        .await?
        .ok_or_else(|| {
            tracing::warn!(component = "OTA", event = "device_not_found", device_id = %device_id, "activate failed: device not found");
            err!(OtaErrorCode::DeviceNotFound)
        })?;

    if device.activated {
        Ok(StatusCode::OK)
    } else {
        Ok(StatusCode::ACCEPTED)
    }
}

#[error]
pub enum OtaErrorCode {
    LackDeviceId = 502001,
    LackClientId = 502002,
    LackUserAgent = 502003,
    DeviceNotFound = 502004,
    ActivationCodeNotFound = 502005,
    DeviceAlreadyActivated = 502006,
    DeviceDisabled = 502007,
    DeviceAlreadyDisabled = 502008,
    DeviceNotDisabled = 502009,
    ActivationCodeExpired = 502010,
}
