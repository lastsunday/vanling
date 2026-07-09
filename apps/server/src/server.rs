use std::sync::Arc;

use api::config::Config;
use async_trait::async_trait;
use framework::config::logging::{LogConfig, LogFormat, LogRotation};
use framework::signal::SignalHandler;
use tokio::runtime;
use tracing::info;

use crate::clap::{ServeArgs, update};

pub(crate) struct Server {
    pub(crate) server: Arc<api::server::Server>,
    pub(crate) logging_handle: framework::log::LoggingHandle,
}

impl Server {
    pub(crate) fn new(
        args: &ServeArgs,
        runtime: Option<&runtime::Handle>,
    ) -> Result<Arc<Self>, anyhow::Error> {
        let _runtime_guard = runtime.map(runtime::Handle::enter);

        let config_paths = args.config.clone().unwrap_or_default();

        let config = Config::load(&config_paths)
            .and_then(|raw| update(raw, args))
            .and_then(|raw| Config::new(&raw))?;

        let log_config = LogConfig {
            console_enabled: config
                .log_console_enabled
                .expect("log_console_enabled has a default"),
            console_level: config
                .log_console_level
                .clone()
                .expect("log_console_level has a default"),
            console_format: config
                .log_console_format
                .as_deref()
                .expect("log_console_format has a default")
                .parse::<LogFormat>()
                .expect("invalid log_console_format"),
            file_enabled: config
                .log_file_enabled
                .expect("log_file_enabled has a default"),
            file_level: config
                .log_file_level
                .clone()
                .expect("log_file_level has a default"),
            file_format: config
                .log_file_format
                .as_deref()
                .expect("log_file_format has a default")
                .parse::<LogFormat>()
                .expect("invalid log_file_format"),
            file_directory: config
                .log_file_directory
                .clone()
                .expect("log_file_directory has a default"),
            file_name: config
                .log_file_name
                .clone()
                .expect("log_file_name has a default"),
            file_max_files: config
                .log_file_max_files
                .expect("log_file_max_files has a default"),
            file_rotation: config
                .log_file_rotation
                .as_deref()
                .expect("log_file_rotation has a default")
                .parse::<LogRotation>()
                .expect("invalid log_file_rotation"),
            flame_enabled: config
                .log_flame_enabled
                .expect("log_flame_enabled has a default"),
            flame_directory: config
                .log_flame_directory
                .clone()
                .expect("log_flame_directory has a default"),
            tokio_console_enabled: config
                .log_tokio_console_enabled
                .expect("log_tokio_console_enabled has a default"),
            ..Default::default()
        };

        config.check()?;

        let logging_handle = framework::logging::init(log_config)?;

        info!(
            server_name = %config.server_name,
            "{}",
            framework::version(),
        );

        Ok(Arc::new(Self {
            server: Arc::new(api::server::Server::new(config, runtime.cloned())),
            logging_handle,
        }))
    }
}

#[async_trait]
impl SignalHandler for Server {
    async fn reload(&self) -> Result<(), anyhow::Error> {
        self.server.reload()
    }

    async fn shutdown(&self) -> Result<(), anyhow::Error> {
        self.server.shutdown()
    }

    async fn signal(&self, sig: &'static str) -> Result<(), anyhow::Error> {
        match sig {
            "SIGUSR1" => {
                self.logging_handle.reload_from_config();
                Ok(())
            }
            "SIGUSR2" => {
                self.logging_handle.cycle_console_level();
                Ok(())
            }
            _ => self.server.signal(sig),
        }
    }
}
