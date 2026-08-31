use std::net::SocketAddr;
use std::sync::Arc;

use api::AppState;
use api::component::asr::AsrManager;
use api::component::llm::LlmManager;
use api::component::tts::TtsManager;
use api::component::vad::VadManager;
use api::config::AsrModel;
use api::config::LlmProvider;
use api::config::TtsModel;
use api::config::VadModel;
use api::config::asr::AsrConfig;
use api::config::audio::AudioConfig;
use api::config::ling::LingConfig;
use api::config::llm::LlmConfig;
use api::config::mcp::McpConfig;
use api::config::security::SecurityConfig;
use api::config::session::SessionConfig;
use api::config::tts::TtsConfig;
use api::config::vad::VadConfig;
use api::config::ws::WsConfig;
use api::setup_ws;
use axum::extract::connect_info::MockConnectInfo;
use framework::auth::{Jwt, Principal};
use framework::config::auth::AuthConfig;
use migration::MigratorTrait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;
use utoipa_axum::router::OpenApiRouter;

fn jwt_config() -> AuthConfig {
    AuthConfig {
        access_token_secret: Some(String::from("test-secret")),
        access_token_expires_in: Some(28800),
        refresh_token_secret: Some(String::from("test-refresh-secret")),
        refresh_token_expires_in: Some(15897600),
        audience: Some(String::from("test-aud")),
        issuer: Some(String::from("test-iss")),
        ..Default::default()
    }
}

async fn init_test_managers() {
    TtsManager::init(
        Arc::new(TtsConfig {
            model: Some(TtsModel::Mute),
            ..Default::default()
        }),
        Arc::new(AudioConfig::default()),
    )
    .await
    .ok();
    VadManager::init(Arc::new(VadConfig {
        model: Some(VadModel::Void),
        ..Default::default()
    }))
    .await;
    AsrManager::init(Arc::new(AsrConfig {
        model: Some(AsrModel::Void),
        ..Default::default()
    }))
    .await;
    LlmManager::init(Arc::new(LlmConfig {
        provider: Some(LlmProvider::LocalEcho),
        ..Default::default()
    }))
    .await;
}

/// 构造带 Void VAD 配置的 AppState。`common::setup_database()` 返回的
/// `vad_config` 为默认（model=None），而会话管线的 VAD node 使用 `ctx.vad_config`
/// 而非 `VadManager` 全局，故这里需显式注入 `Some(Void)`。
async fn setup_state() -> AppState {
    let database_url = "sqlite::memory:";
    let conn: sea_orm::DatabaseConnection = framework::database::establish_connection(database_url)
        .await
        .unwrap();
    migration::Migrator::up(&conn, None).await.unwrap();
    AppState {
        conn,
        session_config: Arc::new(SessionConfig::default()),
        ling_config: Arc::new(LingConfig::default()),
        mcp_config: Arc::new(McpConfig::default()),
        vad_config: Arc::new(VadConfig {
            model: Some(VadModel::Void),
            ..Default::default()
        }),
        audio_config: Arc::new(AudioConfig::default()),
        auth_config: Arc::new(AuthConfig::default()),
        security_config: Arc::new(SecurityConfig::default()),
        ws_config: Arc::new(WsConfig::default()),
        usage_registry: Arc::new(framework::rate_limit::UsageRegistry::default()),
        rate_limit_matchers: Arc::new(
            SecurityConfig::default()
                .compile_matchers()
                .expect("default matchers compile"),
        ),
        cancellation_token: CancellationToken::new(),
    }
}

/// 启动一个真实 `/vanling/v1` WS 服务（Void ASR + Mute TTS + Echo LLM），返回监听地址。
async fn start_ws_server() -> SocketAddr {
    Jwt::init(Arc::new(jwt_config()));
    init_test_managers().await;
    let state = setup_state().await;
    let app = OpenApiRouter::new();
    let app = setup_ws(app, state);
    let (app, _) = app.split_for_parts();
    let app = app.layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

fn device_token() -> String {
    let principal = Principal {
        id: String::from("test-device"),
        name: Some(String::from("test-board")),
        token_type: String::from("device"),
    };
    Jwt::global().access_token_encode(&principal).unwrap()
}

/// 最小 RFC6455 客户端：完成握手，并提供 send_text / read_text_until。
struct WsClient {
    stream: TcpStream,
    buf: [u8; 65536],
    buf_len: usize,
    buf_pos: usize,
}

impl WsClient {
    async fn connect(addr: SocketAddr) -> Self {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let key = "dGhlIHNhbWBsZSBub25jZQ==";
        let request = format!(
            "GET /vanling/v1 HTTP/1.1\r\n\
             Host: localhost\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {key}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             Authorization: Bearer {}\r\n\
             \r\n",
            device_token()
        );
        stream.write_all(request.as_bytes()).await.unwrap();

        // 读取 HTTP 响应头，直到出现空行（\r\n\r\n）。
        let mut total = Vec::new();
        let mut tmp = [0u8; 1];
        loop {
            stream.read_exact(&mut tmp).await.unwrap();
            total.push(tmp[0]);
            if total.len() >= 4 && &total[total.len() - 4..] == b"\r\n\r\n" {
                break;
            }
        }
        let head = String::from_utf8_lossy(&total).to_string();
        assert!(
            head.contains("101 Switching Protocols"),
            "expected 101 Switching Protocols, got: {head}"
        );
        Self {
            stream,
            buf: [0u8; 65536],
            buf_len: 0,
            buf_pos: 0,
        }
    }

    /// 从流中读取一个原始字节（带内部缓冲）。
    async fn read_byte(&mut self) -> u8 {
        if self.buf_pos >= self.buf_len {
            self.buf_len = self
                .stream
                .read(&mut self.buf)
                .await
                .expect("ws eof while reading frame");
            assert!(self.buf_len > 0, "ws stream closed");
            self.buf_pos = 0;
        }
        let b = self.buf[self.buf_pos];
        self.buf_pos += 1;
        b
    }

    async fn send_text(&mut self, text: &str) {
        let payload = text.as_bytes();
        let len = payload.len();
        let mut header = Vec::new();
        header.push(0x81); // FIN + text
        if len < 126 {
            header.push(0x80 | len as u8); // client→server masked
        } else {
            header.push(0x80 | 126);
            header.extend_from_slice(&(len as u16).to_be_bytes());
        }
        let mask: [u8; 4] = [0x12, 0x34, 0x56, 0x78];
        header.extend_from_slice(&mask);
        let mut masked = Vec::with_capacity(len);
        for (i, b) in payload.iter().enumerate() {
            masked.push(b ^ mask[i % 4]);
        }
        self.stream.write_all(&header).await.unwrap();
        self.stream.write_all(&masked).await.unwrap();
    }

    /// 读取下一帧。返回 (opcode, 负载)。自动应答 ping。
    async fn read_frame(&mut self) -> std::io::Result<(u8, Vec<u8>)> {
        let b0 = self.read_byte().await;
        let opcode = b0 & 0x0f;
        let b1 = self.read_byte().await;
        let masked = b1 & 0x80 != 0;
        let mut len = (b1 & 0x7f) as u64;
        if len == 126 {
            let mut e = [0u8; 2];
            for x in e.iter_mut() {
                *x = self.read_byte().await;
            }
            len = u16::from_be_bytes(e) as u64;
        } else if len == 127 {
            let mut e = [0u8; 8];
            for x in e.iter_mut() {
                *x = self.read_byte().await;
            }
            len = u64::from_be_bytes(e);
        }
        let mut mask = [0u8; 4];
        if masked {
            for x in mask.iter_mut() {
                *x = self.read_byte().await;
            }
        }
        let mut payload = vec![0u8; len as usize];
        for b in &mut payload {
            *b = self.read_byte().await;
        }
        if masked {
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
        }
        if opcode == 0x9 {
            // ping → pong
            let mut pong = vec![0x8A, 0x80];
            let m = [0x12, 0x34, 0x56, 0x78];
            pong.extend_from_slice(&m);
            let p: Vec<u8> = payload
                .iter()
                .enumerate()
                .map(|(i, b)| b ^ m[i % 4])
                .collect();
            self.stream.write_all(&pong).await.unwrap();
            self.stream.write_all(&p).await.unwrap();
        }
        Ok((opcode, payload))
    }

    /// 依次读取文本帧直至出现满足 predicate 的帧（忽略其余帧），Timeout 后 panic。
    async fn read_text_until(
        &mut self,
        desc: &str,
        timeout: Duration,
        mut predicate: impl FnMut(&serde_json::Value) -> bool,
    ) -> serde_json::Value {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline - tokio::time::Instant::now();
            if remaining.is_zero() {
                panic!("timeout waiting for {desc}");
            }
            let (opcode, payload) =
                tokio::time::timeout(remaining, async { self.read_frame().await })
                    .await
                    .unwrap_or_else(|_| panic!("timeout waiting for {desc}"))
                    .expect("read_frame failed");
            if opcode == 0x1 {
                let v: serde_json::Value = serde_json::from_slice(&payload).unwrap_or_else(
                    |_| serde_json::json!({ "raw": String::from_utf8_lossy(&payload).to_string() }),
                );
                if predicate(&v) {
                    return v;
                }
            }
        }
    }
}

use std::time::Duration;

#[tokio::test]
async fn test_ws_e2e_text_input_twice_gets_two_stt_frames() {
    let addr = start_ws_server().await;
    let mut client = WsClient::connect(addr).await;

    // 发送 hello，期望收到 hello 结果。
    client.send_text(r#"{"type":"hello"}"#).await;
    let _hello = client
        .read_text_until("hello result", Duration::from_secs(30), |v| {
            v["type"] == "hello"
        })
        .await;

    // 第一次文本输入 → stt
    client
        .send_text(r#"{"type":"listen","state":"text","text":"第一段"}"#)
        .await;
    let stt1 = client
        .read_text_until("first stt", Duration::from_secs(30), |v| v["type"] == "stt")
        .await;
    assert_eq!(stt1["text"], "第一段");

    // 第二次文本输入 → stt（第二轮）
    client
        .send_text(r#"{"type":"listen","state":"text","text":"第二段"}"#)
        .await;
    let stt2 = client
        .read_text_until("second stt", Duration::from_secs(30), |v| {
            v["type"] == "stt"
        })
        .await;
    assert_eq!(stt2["text"], "第二段");
}
