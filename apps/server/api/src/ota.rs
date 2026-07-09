use anyhow::Context;
use axum::{
    debug_handler,
    extract::{ConnectInfo, Json, State},
    http::HeaderMap,
};
use axum_extra::{TypedHeader, headers};
use framework::{data::valid::ValidJson, err, error::AppResult, id::gen_id};
use std::net::SocketAddr;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::ota_data::*;

use chrono::Local;
use jiff::tz::TimeZone;

const TAG: &str = "ota";

pub fn create_routes(state: AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(ota))
        .routes(routes!(activate))
        .with_state(state)
}

//from https://ccnphfhqs21z.feishu.cn/wiki/FjW6wZmisimNBBkov6OcmfvknVd
#[debug_handler]
#[tracing::instrument(name="ota",skip_all,fields(ip = %addr))]
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
    State(AppState { ws_config, .. }): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    host: TypedHeader<headers::Host>,
    //TODO: param not use
    ValidJson(_param): ValidJson<OtaParam>,
) -> AppResult<Json<OtaResult>> {
    if headers.get(KEY_DEVICE_ID).is_none() {
        return Err(err!(OtaErrorCode::LackDeviceId));
    }
    if headers.get(KEY_CLIENT_ID).is_none() {
        return Err(err!(OtaErrorCode::LackClientId));
    }
    if headers.get(KEY_USER_AGENT).is_none() {
        return Err(err!(OtaErrorCode::LackUserAgent));
    }
    // TODO: save device info to database
    let _device_id = headers.get(KEY_DEVICE_ID).unwrap().to_str().unwrap();
    // TODO: save to database and fill logic
    let _activation_code = gen_id();
    let now = Local::now();
    let tz = TimeZone::system();
    let iana_identifier = tz.iana_name().context("get iana name failure")?;
    let address = match host.port() {
        Some(port) => &format!("{}:{}", host.hostname(), port),
        None => host.hostname(),
    };
    Ok(Json(OtaResult {
        mqtt: None,
        websocket: Websocket {
            url: format!(
                "{}://{}/chobits/v1",
                ws_config.schema.as_ref().expect("ws schema is empty"),
                address
            ),
            token: String::from(""),
        },
        server_time: ServerTime {
            timestamp: now.timestamp_millis(),
            timezone: String::from(iana_identifier),
            timezone_offset: -(now.offset().utc_minus_local() / 60),
        },
        firmware: Some(Firmware {
            version: String::from("0.0.1"),
            url: None,
        }),
        activation: None,
    })) //
}

#[debug_handler]
#[tracing::instrument(name="ota",skip_all,fields(ip = %addr))]
#[utoipa::path(post, path = "/ota/activate",tag=TAG,security(()),
    params(
        ("Device-Id" = String,Header,description="设备的唯一标识符（使用MAC地址或由硬件ID生成的伪MAC地址）",example="11:22:33:44:55:66"),
        ("Client-Id" = Option<String>,Header,description="客户端的唯一标识符，由软件自动生成的UUID v4（擦除FLASH或重装后会变化）",example="7b94d69a-9808-4c59-9c9b-704333b38aff"),
        ("Accept-Language" = Option<String>,Header,description="客户端的当前语言（可选，例如 zh-CN）",example="zh-CN"),
    ),
)]
async fn activate(
    //TODO: conn not use
    State(AppState { .. }): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> AppResult<String> {
    if headers.get(KEY_DEVICE_ID).is_none() {
        return Err(err!(OtaErrorCode::LackDeviceId));
    }
    // TODO:check device id in database
    // TODO:logic failure need return status code 202
    Ok(String::from("success"))
}

use framework::prelude::error;

#[error]
pub enum OtaErrorCode {
    LackDeviceId = 502001,
    LackClientId = 502002,
    LackUserAgent = 502003,
}
