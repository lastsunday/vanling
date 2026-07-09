#[cfg(feature = "perf")]
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogFormat {
    Text,
    Json,
    Compact,
    Pretty,
}

impl std::str::FromStr for LogFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            "compact" => Ok(Self::Compact),
            "pretty" => Ok(Self::Pretty),
            _ => Err(format!("unknown log format: {s}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogRotation {
    Daily,
    Hourly,
    Never,
}

impl std::str::FromStr for LogRotation {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "daily" => Ok(Self::Daily),
            "hourly" => Ok(Self::Hourly),
            "never" => Ok(Self::Never),
            _ => Err(format!("unknown log rotation: {s}")),
        }
    }
}

#[derive(Clone)]
pub struct LogConfig {
    // Console
    pub console_enabled: bool,
    pub console_level: String,
    pub console_format: LogFormat,
    // File
    pub file_enabled: bool,
    pub file_level: String,
    pub file_format: LogFormat,
    pub file_directory: String,
    pub file_name: String,
    pub file_max_files: usize,
    pub file_rotation: LogRotation,
    // Flame
    pub flame_enabled: bool,
    pub flame_directory: String,
    // Tokio console
    pub tokio_console_enabled: bool,
    // Legacy
    pub log_thread_ids: bool,
    pub log_colors: bool,
    pub log_filter_regex: bool,
    pub log_span_events: String,
    pub log_to_journald: bool,
    pub journald_identifier: Option<String>,
    #[cfg(feature = "sentry")]
    pub sentry_filter: String,
    #[cfg(feature = "otlp")]
    pub otlp_filter: String,
    #[cfg(feature = "otlp")]
    pub allow_otlp: bool,
    #[cfg(feature = "otlp")]
    pub otlp_protocol: String,
    #[cfg(feature = "perf")]
    pub tracing_flame: bool,
    #[cfg(feature = "perf")]
    pub tracing_flame_filter: String,
    #[cfg(feature = "perf")]
    pub tracing_flame_output_path: PathBuf,
    #[cfg(feature = "console")]
    pub tokio_console: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            console_enabled: true,
            console_level: "info".into(),
            console_format: LogFormat::Text,
            file_enabled: false,
            file_level: "info".into(),
            file_format: LogFormat::Json,
            file_directory: "./logs".into(),
            file_name: "server".into(),
            file_max_files: 10,
            file_rotation: LogRotation::Daily,
            flame_enabled: false,
            flame_directory: "./flame".into(),
            tokio_console_enabled: false,
            log_thread_ids: false,
            log_colors: true,
            log_filter_regex: false,
            log_span_events: "CLOSE".into(),
            log_to_journald: false,
            journald_identifier: None,
            #[cfg(feature = "sentry")]
            sentry_filter: String::new(),
            #[cfg(feature = "otlp")]
            otlp_filter: String::new(),
            #[cfg(feature = "otlp")]
            allow_otlp: false,
            #[cfg(feature = "otlp")]
            otlp_protocol: "http".into(),
            #[cfg(feature = "perf")]
            tracing_flame: false,
            #[cfg(feature = "perf")]
            tracing_flame_filter: String::new(),
            #[cfg(feature = "perf")]
            tracing_flame_output_path: PathBuf::from("tracing.flame"),
            #[cfg(feature = "console")]
            tokio_console: false,
        }
    }
}
