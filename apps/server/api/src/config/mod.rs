pub mod asr;
pub mod audio;
pub mod check;
pub mod database;
pub mod llm;
pub mod manager;
pub mod matrix;
pub mod mcp;
pub mod server;
pub mod session;
pub mod tts;
pub mod vad;
pub mod ws;

use anyhow::Error;
use either::Either::{self, Left, Right};
use figment::{
    Figment,
    providers::{Env, Format, Toml},
};
use serde::{Deserialize, Serialize, de::IgnoredAny};
use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    result::Result,
};
use vanling_macros::config_example_generator;

pub use self::{check::check, manager::Manager};

const DEPRECATED_KEYS: &[&str] = &[];

/// All the config options for vanling.
#[allow(clippy::struct_excessive_bools)]
#[allow(rustdoc::broken_intra_doc_links, rustdoc::bare_urls)]
#[derive(Clone, Debug, Deserialize)]
#[config_example_generator(
    filename = "application-example.toml",
    section = "global",
    undocumented = "# This item is undocumented. Please contribute documentation for it.",
    header = r#"### vanling Configuration
###
### THIS FILE IS GENERATED. CHANGES/CONTRIBUTIONS IN THE REPO WILL BE
### OVERWRITTEN!
###
### You should rename this file before configuring your server. Changes to
### documentation and defaults can be contributed in source code at
### src/config/mod.rs. This file is generated when building.
###
### Any values pre-populated are the default values for said config option.
###
### At the minimum, you MUST edit all the config options to your environment
### that say "YOU NEED TO EDIT THIS".
###
"#,
    ignore = "config_paths catchall",
    source = "src/config/mod.rs"
)]
pub struct Config {
    /// Server hostname displayed to clients.
    ///
    /// default: "localhost.localdomain"
    #[serde(default = "default_server_name")]
    pub server_name: String,

    /// Base data directory for model files. All model paths are relative to this.
    ///
    /// default: "data"
    #[serde(default = "default_data_dir")]
    pub data_dir: Option<String>,

    /// The default address (IPv4 or IPv6)  will listen on.
    ///
    /// If you are using Docker or a container NAT networking setup, this must
    /// be "0.0.0.0".
    ///
    ///
    /// default: "127.0.0.1"
    #[serde(default = "default_address")]
    pub address: ListeningAddr,

    /// The port(s) will listen on.
    ///
    /// If you are using Docker, don't change this, you'll need to map an
    /// external port to this.
    ///
    /// default: 3000
    #[serde(default = "default_port")]
    pub port: ListeningPort,

    /// Database connection URL.
    ///
    /// default: "sqlite://db.sqlite?mode=rwc"
    #[serde(default = "default_database_url")]
    pub database_url: Option<String>,

    /// Secret key for signing JWT access tokens.
    ///
    /// default: "QLjJTeVblAlM47de"
    #[serde(default = "default_auth_access_token_secret")]
    pub auth_access_token_secret: Option<String>,

    /// Access token expiry duration in seconds.
    ///
    /// default: 28800
    #[serde(default = "default_auth_access_token_expires_in")]
    pub auth_access_token_expires_in: Option<u64>,

    /// Secret key for signing JWT refresh tokens.
    ///
    /// default: "N8lI0uitNzJl6vYK"
    #[serde(default = "default_auth_refresh_token_secret")]
    pub auth_refresh_token_secret: Option<String>,

    /// Refresh token expiry duration in seconds.
    ///
    /// default: 15897600
    #[serde(default = "default_auth_refresh_token_expires_in")]
    pub auth_refresh_token_expires_in: Option<u64>,

    /// JWT audience claim.
    ///
    /// default: "audience"
    #[serde(default = "default_auth_audience")]
    pub auth_audience: Option<String>,

    /// JWT issuer claim.
    ///
    /// default: "issuer"
    #[serde(default = "default_auth_issuer")]
    pub auth_issuer: Option<String>,

    /// OAuth 2.0 client ID.
    ///
    /// default: "d1aicsr57dijo7h963ig"
    #[serde(default = "default_auth_client_id")]
    pub auth_client_id: Option<String>,

    /// OAuth 2.0 client secret.
    ///
    /// default: "ujTgh2lEQYy0PXhK"
    #[serde(default = "default_auth_client_secret")]
    pub auth_client_secret: Option<String>,

    /// WebSocket URL scheme (ws or wss).
    ///
    /// default: "ws"
    #[serde(default = "default_ws_schema")]
    pub ws_schema: Option<String>,

    /// Voice Activity Detection model to use.
    ///
    /// default: "earshot"
    #[serde(default = "default_vad_model")]
    pub vad_model: Option<VadModel>,

    /// Path to the VAD model file or directory.
    ///
    /// default: "auto-derived from model+variant"
    #[serde(default = "default_vad_path")]
    pub vad_path: Option<String>,

    /// Variant of the VAD model to load.
    ///
    /// default: "auto-detected"
    #[serde(default)]
    pub vad_variant: Option<String>,

    /// Number of threads for VAD inference.
    ///
    /// default: 4
    #[serde(default = "default_vad_num_threads")]
    pub vad_num_threads: Option<i32>,

    /// VAD detection threshold (0.0–1.0). Higher = fewer false positives, more false negatives.
    ///
    /// display: VAD Threshold
    /// default: 0.5
    #[serde(default = "default_vad_threshold")]
    pub vad_threshold: Option<f32>,

    /// VAD deactivation threshold for hysteresis. Must be < vad_threshold.
    /// Speech stops when score drops below this value during active speech.
    ///
    /// display: VAD Deactivation Threshold
    /// default: 0.35
    #[serde(default = "default_vad_deactivation_threshold")]
    pub vad_deactivation_threshold: Option<f32>,

    /// Minimum silence duration in ms before VAD considers speech ended.
    ///
    /// display: Min Silence Duration
    /// default: 550
    #[serde(default = "default_vad_min_silence_duration")]
    pub vad_min_silence_duration: Option<f32>,

    /// Minimum continuous speech duration in ms before VAD triggers is_speech.
    /// Prevents short noise bursts from being classified as speech.
    ///
    /// display: Min Speech Duration
    /// default: 300
    #[serde(default = "default_vad_min_speech_duration")]
    pub vad_min_speech_duration: Option<f32>,

    /// Text-to-Speech model to use.
    ///
    /// default: "matcha_tts"
    #[serde(default = "default_tts_model")]
    pub tts_model: Option<TtsModel>,

    /// Path to the TTS model file or directory.
    ///
    /// default: "auto-derived from model manifest"
    #[serde(default = "default_tts_path")]
    pub tts_path: Option<String>,

    /// Variant override for the active TTS model.
    ///
    /// When not set, the default variant is read from the embedded model manifest.
    /// This is useful for switching between different model variants without
    /// changing the model type.
    ///
    /// default: "auto-detected from model manifest"
    #[serde(default)]
    pub tts_variant: Option<String>,

    /// Variant override for the TTS reference audio.
    ///
    /// default: "auto-detected from model manifest"
    #[serde(default)]
    pub tts_reference_variant: Option<String>,

    /// Override the auto-derived prompt text from manifest.
    ///
    /// default: "auto-derived"
    #[serde(default)]
    pub tts_reference_prompt_text: Option<String>,

    /// Override the auto-derived prompt wav path from manifest.
    ///
    /// default: "auto-derived"
    #[serde(default)]
    pub tts_reference_prompt_wav_path: Option<String>,

    /// TTS model-specific options as a JSON object.
    ///
    /// For MatchaTTS:
    ///   - num_threads:     inference threads (default 2)
    ///   - noise_scale:     generation noise parameter (default 0.667)
    ///   - length_scale:    speed scaling (default auto-detected)
    ///   - speed:           playback speed (default 1.0)
    ///   - debug:           debug output (default false)
    ///   - dict_dir:        pronunciation dictionary directory
    ///   - data_dir:        espeak-ng data directory
    ///   - acoustic_model:  onnx path (auto-discovered)
    ///   - vocoder:         onnx path (auto-discovered)
    ///
    /// default: {}
    #[serde(default)]
    pub tts_options: Option<serde_json::Value>,

    /// Automatic Speech Recognition model to use.
    ///
    /// default: "x_asr"
    #[serde(default = "default_asr_model")]
    pub asr_model: Option<AsrModel>,

    /// Path to the ASR model file or directory.
    ///
    /// default: "auto-derived from model+variant"
    #[serde(default = "default_asr_path")]
    pub asr_path: Option<String>,

    /// Variant of the ASR model to load.
    ///
    /// default: "auto-detected from model manifest"
    #[serde(default)]
    pub asr_variant: Option<String>,

    /// LLM provider to use.
    ///
    /// default: "local_qwen3"
    #[serde(default = "default_llm_provider")]
    pub llm_provider: Option<LlmProvider>,

    /// Path to the local LLM model file or directory.
    /// Only used with local providers (local_qwen3, local_echo).
    ///
    /// default: "auto-derived from model+variant"
    #[serde(default = "default_llm_path")]
    pub llm_path: Option<String>,

    /// Model variant for the local LLM.
    /// Only used with local providers (local_qwen3, local_echo).
    #[serde(default)]
    pub llm_variant: Option<String>,

    /// Base URL for the remote OpenAI-compatible API.
    /// Only used with remote_open_ai_compatible provider.
    ///
    /// default: "http://localhost:11434/v1/"
    #[serde(default = "default_llm_api_url")]
    pub llm_api_url: Option<String>,

    /// API key for remote OpenAI-compatible providers.
    /// Leave empty for local-only setups.
    ///
    /// display: sensitive
    /// default:
    #[serde(default = "default_llm_api_key")]
    pub llm_api_key: Option<String>,

    /// Remote model identifier sent to the API.
    /// Only used with remote_open_ai_compatible provider.
    ///
    /// default: "qwen3.5:4b"
    #[serde(default = "default_llm_model")]
    pub llm_model: Option<String>,

    /// Output audio sample rate in Hz.
    ///
    /// default: 16000
    #[serde(default = "default_audio_output_sample_rate")]
    pub audio_output_sample_rate: Option<u32>,

    /// Number of output audio channels.
    ///
    /// default: 1
    #[serde(default = "default_audio_output_channel")]
    pub audio_output_channel: Option<u32>,

    /// Output audio frame duration in milliseconds.
    ///
    /// default: 20
    #[serde(default = "default_audio_output_frame_duration")]
    pub audio_output_frame_duration: Option<u64>,

    /// Time in ms before closing connection when no activity detected.
    ///
    /// default: 30000
    #[serde(default = "default_session_close_connection_no_activity_time")]
    pub session_close_connection_no_activity_time: Option<i64>,

    /// Silence timeout in ms during active voice session.
    ///
    /// default: 1200
    #[serde(default = "default_session_silence_voice_timeout")]
    pub session_silence_voice_timeout: Option<i64>,

    /// System prompt for the LLM.
    ///
    /// default: "你是一个知识丰富的语音对话助手，用亲切自然的语气与用户交流。回答必须有实质内容，直接提供有价值的信息。避免只问不答——先给出信息，再视情况补充。必须使用中文口述。禁止使用 Markdown、emoji、特殊符号、HTML 标签、英文标点符号。必须使用中文标点符号（。！？，；：）分隔句子，每句话末尾必须带句号、问号或感叹号。禁止使用换行符代替标点来分隔内容。所有数字必须用中文书写（二十三而不是23），日期用中文（七月二十四日）。如果用户输入为空，请求用户描述清楚。",
    #[serde(default = "default_session_system_prompt")]
    pub session_system_prompt: Option<String>,

    /// Maximum number of prompt tokens.
    ///
    /// default: 3000
    #[serde(default = "default_session_max_prompt_len")]
    pub session_max_prompt_len: Option<u64>,

    /// Barge-in lockout duration in ms. After TTS starts or stops,
    /// barge-in is suppressed for this duration to prevent echo-triggered interruptions.
    ///
    /// default: 250
    #[serde(default = "default_session_barge_in_lockout_ms")]
    pub session_barge_in_lockout_ms: Option<u64>,

    /// List of MCP server URIs.
    ///
    /// default: ["http://127.0.0.1:3000/mcp"]
    #[serde(default = "default_mcp_uri_list")]
    pub mcp_uri_list: Option<Vec<String>>,

    /// Enable Matrix messaging integration.
    ///
    /// default: false
    #[serde(default = "default_matrix_enable")]
    pub matrix_enable: Option<bool>,

    /// Matrix client application name.
    ///
    /// default: "vanling"
    #[serde(default = "default_matrix_client_name")]
    pub matrix_client_name: Option<String>,

    /// Matrix homeserver URL.
    ///
    /// default: "http://127.0.0.1:8008"
    #[serde(default = "default_matrix_homeserver")]
    pub matrix_homeserver: Option<String>,

    /// Matrix client user ID.
    ///
    /// default: "@vanling:localhost.localdomain"
    #[serde(default = "default_matrix_client_username")]
    pub matrix_client_username: Option<String>,

    /// Matrix client password.
    ///
    /// default:
    #[serde(default = "default_matrix_client_password")]
    pub matrix_client_password: Option<String>,

    /// Enable console logging
    ///
    /// default: true
    #[serde(default = "default_log_console_enabled")]
    pub log_console_enabled: Option<bool>,

    /// Console log level (trace, debug, info, warn, error)
    ///
    /// default: "info"
    #[serde(default = "default_log_console_level")]
    pub log_console_level: Option<String>,

    /// Console log format (text, json, compact, pretty)
    ///
    /// default: "text"
    #[serde(default = "default_log_console_format")]
    pub log_console_format: Option<String>,

    /// Enable file logging
    ///
    /// default: false
    #[serde(default = "default_log_file_enabled")]
    pub log_file_enabled: Option<bool>,

    /// File log level (trace, debug, info, warn, error)
    ///
    /// default: "info"
    #[serde(default = "default_log_file_level")]
    pub log_file_level: Option<String>,

    /// File log format (text, json, compact, pretty)
    ///
    /// default: "json"
    #[serde(default = "default_log_file_format")]
    pub log_file_format: Option<String>,

    /// File log directory
    ///
    /// default: "./logs"
    #[serde(default = "default_log_file_directory")]
    pub log_file_directory: Option<String>,

    /// File log name prefix
    ///
    /// default: "server"
    #[serde(default = "default_log_file_name")]
    pub log_file_name: Option<String>,

    /// Max log files to retain
    ///
    /// default: 10
    #[serde(default = "default_log_file_max_files")]
    pub log_file_max_files: Option<usize>,

    /// Log rotation (daily, hourly, never)
    ///
    /// default: "daily"
    #[serde(default = "default_log_file_rotation")]
    pub log_file_rotation: Option<String>,

    /// Enable tracing-flame profiling output
    ///
    /// default: false
    #[serde(default = "default_log_flame_enabled")]
    pub log_flame_enabled: Option<bool>,

    /// Flame graph output directory
    ///
    /// default: "./flame"
    #[serde(default = "default_log_flame_directory")]
    pub log_flame_directory: Option<String>,

    /// Enable tokio-console
    ///
    /// default: false
    #[serde(default = "default_log_tokio_console_enabled")]
    pub log_tokio_console_enabled: Option<bool>,

    #[serde(flatten)]
    #[allow(clippy::zero_sized_map_values)]
    // this is a catchall, the map shouldn't be zero at runtime
    catchall: BTreeMap<String, IgnoredAny>,
}

fn default_server_name() -> String {
    String::from("localhost")
}

fn default_data_dir() -> Option<String> {
    Some("data".into())
}

fn default_address() -> ListeningAddr {
    ListeningAddr {
        addrs: Right(vec![Ipv4Addr::LOCALHOST.into(), Ipv6Addr::LOCALHOST.into()]),
    }
}

fn default_port() -> ListeningPort {
    ListeningPort { ports: Left(3000) }
}

fn default_database_url() -> Option<String> {
    Some(String::from("sqlite://db.sqlite?mode=rwc"))
}

const DEFAULT_AUTH_ACCESS_TOKEN_SECRET: &str = "QLjJTeVblAlM47de";

fn default_auth_access_token_secret() -> Option<String> {
    Some(String::from(DEFAULT_AUTH_ACCESS_TOKEN_SECRET))
}

fn default_auth_access_token_expires_in() -> Option<u64> {
    Some(28800)
}

const DEFAULT_AUTH_REFRESH_TOKEN_SECRET: &str = "N8lI0uitNzJl6vYK";

fn default_auth_refresh_token_secret() -> Option<String> {
    Some(String::from(DEFAULT_AUTH_REFRESH_TOKEN_SECRET))
}

fn default_auth_refresh_token_expires_in() -> Option<u64> {
    Some(15897600)
}

fn default_auth_audience() -> Option<String> {
    Some(String::from("audience"))
}

fn default_auth_issuer() -> Option<String> {
    Some(String::from("issuer"))
}

fn default_auth_client_id() -> Option<String> {
    Some(String::from("d1aicsr57dijo7h963ig"))
}

const DEFAULT_AUTH_CLIENT_SECRET: &str = "ujTgh2lEQYy0PXhK";

fn default_auth_client_secret() -> Option<String> {
    Some(String::from(DEFAULT_AUTH_CLIENT_SECRET))
}

fn warn_on_default_auth_secrets(config: &Config) {
    if config
        .auth_access_token_secret
        .as_deref()
        .is_some_and(|v| v == DEFAULT_AUTH_ACCESS_TOKEN_SECRET)
    {
        tracing::warn!(
            component = "CONFIG",
            event = "default_auth_secret",
            secret = "access_token",
            "auth access token secret is using a built-in default; set `auth.access_token_secret` before going live"
        );
    }
    if config
        .auth_refresh_token_secret
        .as_deref()
        .is_some_and(|v| v == DEFAULT_AUTH_REFRESH_TOKEN_SECRET)
    {
        tracing::warn!(
            component = "CONFIG",
            event = "default_auth_secret",
            secret = "refresh_token",
            "auth refresh token secret is using a built-in default; set `auth.refresh_token_secret` before going live"
        );
    }
    if config
        .auth_client_secret
        .as_deref()
        .is_some_and(|v| v == DEFAULT_AUTH_CLIENT_SECRET)
    {
        tracing::warn!(
            component = "CONFIG",
            event = "default_auth_secret",
            secret = "client_secret",
            "auth client secret is using a built-in default; set `auth.client_secret` before going live"
        );
    }
}

fn default_tts_model() -> Option<TtsModel> {
    Some(TtsModel::MatchaTts)
}

fn default_tts_path() -> Option<String> {
    None
}

fn default_asr_model() -> Option<AsrModel> {
    Some(AsrModel::XAsr)
}

fn default_asr_path() -> Option<String> {
    None
}

fn default_llm_provider() -> Option<LlmProvider> {
    Some(LlmProvider::LocalQwen3)
}

fn default_llm_path() -> Option<String> {
    None
}

fn default_llm_api_url() -> Option<String> {
    Some("http://localhost:11434/v1".into())
}

fn default_llm_api_key() -> Option<String> {
    Some(String::new())
}

fn default_llm_model() -> Option<String> {
    Some("qwen3.5:4b".into())
}

fn default_audio_output_sample_rate() -> Option<u32> {
    Some(16000)
}

fn default_audio_output_channel() -> Option<u32> {
    Some(1)
}

fn default_audio_output_frame_duration() -> Option<u64> {
    Some(20_u64)
}

fn default_session_close_connection_no_activity_time() -> Option<i64> {
    Some(30000)
}

fn default_session_silence_voice_timeout() -> Option<i64> {
    Some(1200)
}

fn default_session_system_prompt() -> Option<String> {
    Some(String::from(
        "你是一个知识丰富的语音对话助手，用亲切自然的语气与用户交流。\
回答必须有实质内容，直接提供有价值的信息。\
避免只问不答——先给出信息，再视情况补充。\
必须使用中文口述。禁止使用 Markdown、emoji、特殊符号、HTML 标签、英文标点符号。\
必须使用中文标点符号（。！？，；：）分隔句子，每句话末尾必须带句号、问号或感叹号。\
禁止使用换行符代替标点来分隔内容。\
所有数字必须用中文书写（二十三而不是23），日期用中文（七月二十四日）。\
如果用户输入为空，请求用户描述清楚。",
    ))
}

fn default_session_max_prompt_len() -> Option<u64> {
    Some(6000)
}

fn default_session_barge_in_lockout_ms() -> Option<u64> {
    Some(250)
}

fn default_mcp_uri_list() -> Option<Vec<String>> {
    Some(vec![String::from("http://127.0.0.1:3000/mcp")])
}

fn default_ws_schema() -> Option<String> {
    Some(String::from("ws"))
}

fn default_vad_model() -> Option<VadModel> {
    Some(VadModel::Earshot)
}

fn default_vad_path() -> Option<String> {
    None
}

fn default_vad_num_threads() -> Option<i32> {
    Some(4)
}

fn default_vad_threshold() -> Option<f32> {
    Some(0.6)
}

fn default_vad_deactivation_threshold() -> Option<f32> {
    Some(0.5)
}

fn default_vad_min_silence_duration() -> Option<f32> {
    Some(550.0)
}

fn default_vad_min_speech_duration() -> Option<f32> {
    Some(300.0)
}

fn default_matrix_enable() -> Option<bool> {
    Some(false)
}

fn default_matrix_client_name() -> Option<String> {
    Some(String::from("vanling"))
}

fn default_matrix_homeserver() -> Option<String> {
    Some(String::from("http://127.0.0.1:8008"))
}

fn default_matrix_client_username() -> Option<String> {
    Some(String::from("@vanling:localhost.localdomain"))
}

fn default_matrix_client_password() -> Option<String> {
    None
}

fn default_log_console_enabled() -> Option<bool> {
    Some(true)
}

fn default_log_console_level() -> Option<String> {
    Some("info".into())
}

fn default_log_console_format() -> Option<String> {
    Some("text".into())
}

fn default_log_file_enabled() -> Option<bool> {
    Some(false)
}

fn default_log_file_level() -> Option<String> {
    Some("info".into())
}

fn default_log_file_format() -> Option<String> {
    Some("json".into())
}

fn default_log_file_directory() -> Option<String> {
    Some("./logs".into())
}

fn default_log_file_name() -> Option<String> {
    Some("server".into())
}

fn default_log_file_max_files() -> Option<usize> {
    Some(10)
}

fn default_log_file_rotation() -> Option<String> {
    Some("daily".into())
}

fn default_log_flame_enabled() -> Option<bool> {
    Some(false)
}

fn default_log_flame_directory() -> Option<String> {
    Some("./flame".into())
}

fn default_log_tokio_console_enabled() -> Option<bool> {
    Some(false)
}

impl Config {
    /// Pre-initialize config
    pub fn load(paths: &[PathBuf]) -> std::result::Result<Figment, Error> {
        let envs = [Env::var("VANLING_CONFIG")];
        let mut config = envs
            .into_iter()
            .flatten()
            .map(Toml::file)
            .chain(paths.iter().cloned().map(Toml::file))
            .fold(Figment::new(), |config, file| config.merge(file.nested()))
            .merge(Env::prefixed("VANLING_").global().split("__"));

        config = config.join(("config_paths", paths));

        Ok(config)
    }

    /// Finalize config
    pub fn new(raw_config: &Figment) -> Result<Self, Error> {
        let config = raw_config.extract::<Self>().map_err(|e| {
            anyhow::anyhow!("There was a problem with your configuration file: {e}")
        })?;

        // don't start if we're listening on both UNIX sockets and TCP at same time
        check::is_dual_listening(raw_config)?;

        warn_on_default_auth_secrets(&config);

        Ok(config)
    }

    #[must_use]
    pub fn get_bind_addrs(&self) -> Vec<SocketAddr> {
        let mut addrs = Vec::with_capacity(
            self.get_bind_hosts()
                .len()
                .saturating_mul(self.get_bind_ports().len()),
        );
        for host in &self.get_bind_hosts() {
            for port in &self.get_bind_ports() {
                addrs.push(SocketAddr::new(*host, *port));
            }
        }

        addrs
    }

    fn get_bind_hosts(&self) -> Vec<IpAddr> {
        match &self.address.addrs {
            Left(addr) => vec![*addr],
            Right(addrs) => addrs.clone(),
        }
    }

    fn get_bind_ports(&self) -> Vec<u16> {
        match &self.port.ports {
            Left(port) => vec![*port],
            Right(ports) => ports.clone(),
        }
    }

    pub fn check(&self) -> Result<(), Error> {
        check(self)
    }

    pub fn data_dir(&self) -> &str {
        self.data_dir
            .as_deref()
            .expect("data_dir should have default")
    }

    /// Derive the full TTS path by joining `data_dir`, `base_path`, and `variant`.
    ///
    /// `base_path` comes from the manifest (e.g. `"tts/model/matcha/"`).
    /// `variant` should already be resolved before calling this method.
    pub fn derive_tts_path(&self, base_path: &str, variant: &str) -> Option<String> {
        match self.tts_model.clone().unwrap_or_default() {
            TtsModel::Mute => None,
            _ => {
                let d = self.data_dir().trim_end_matches('/');
                Some(format!("{d}/{base_path}{variant}/"))
            }
        }
    }

    pub fn derive_asr_path(&self, base_path: &str, variant: &str) -> Option<String> {
        match self.asr_model.clone().unwrap_or_default() {
            AsrModel::Void => None,
            _ => {
                let d = self.data_dir().trim_end_matches('/');
                Some(format!("{d}/{base_path}{variant}/"))
            }
        }
    }

    pub fn derive_llm_path(&self) -> Option<String> {
        if self.llm_path.is_some() {
            return self.llm_path.clone();
        }
        if self.llm_provider.clone().unwrap_or_default() == LlmProvider::RemoteOpenAiCompatible {
            return None;
        }
        let variant = self.llm_variant.clone().unwrap_or_else(|| {
            match self.llm_provider.clone().unwrap_or_default() {
                LlmProvider::LocalQwen3 => "0.6b".into(),
                _ => String::new(),
            }
        });
        if variant.is_empty() {
            return None;
        }
        let d = self.data_dir();
        match self.llm_provider.clone().unwrap_or_default() {
            LlmProvider::LocalQwen3 => Some(format!("{d}/llm/model/qwen3/{variant}/")),
            _ => None,
        }
    }

    pub fn derive_vad_path(&self) -> Option<String> {
        if self.vad_path.is_some() {
            return self.vad_path.clone();
        }
        None
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(transparent)]
pub struct ListeningPort {
    #[serde(with = "either::serde_untagged")]
    pub ports: Either<u16, Vec<u16>>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(transparent)]
pub struct ListeningAddr {
    #[serde(with = "either::serde_untagged")]
    pub addrs: Either<IpAddr, Vec<IpAddr>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VadModel {
    Void,
    #[default]
    Earshot,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AsrModel {
    #[default]
    XAsr,
    Void,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TtsModel {
    Mute,
    #[default]
    MatchaTts,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LlmProvider {
    #[default]
    LocalQwen3,
    LocalEcho,
    RemoteOpenAiCompatible,
}
