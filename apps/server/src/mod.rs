#[cfg(unix)]
use std::sync::atomic::Ordering;
use std::{error::Error, sync::Arc, time::Duration};

use anyhow::anyhow;
use api::config::{
    AsrModel, asr::AsrConfig, audio::AudioConfig, database::DatabaseConfig, llm::LlmConfig,
    matrix::MatrixConfig, mcp::McpConfig, server::ServerConfig, session::SessionConfig,
    tts::TtsConfig, vad::VadConfig, ws::WsConfig,
};
use framework::config::auth::AuthConfig;
use tracing::info;

use crate::{
    clap::{Commands, DownloaderAction, ServeArgs},
    server::Server,
};
mod clap;
mod downloader;
mod server;

pub fn run() -> Result<(), Box<dyn Error>> {
    let cli = clap::parse();
    match &cli.command {
        Some(Commands::Downloader {
            action:
                DownloaderAction::Install {
                    category,
                    model,
                    variant,
                    data_dir,
                    quiet,
                    mirror,
                    overrides,
                    config,
                    all,
                },
        }) => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(downloader::run(
                category.as_deref(),
                model.as_deref(),
                variant.as_deref(),
                data_dir,
                *quiet,
                mirror,
                overrides.as_deref(),
                config.as_ref(),
                *all,
            ))
        }
        Some(Commands::Downloader {
            action: DownloaderAction::Wizard { data_dir, quiet },
        }) => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(downloader::run_wizard(data_dir, *quiet))
        }
        Some(Commands::Downloader {
            action: DownloaderAction::List { category, json },
        }) => {
            downloader::list(category.as_deref(), *json);
            Ok(())
        }
        Some(Commands::Downloader {
            action: DownloaderAction::UpdateChecksums { data_dir, quiet },
        }) => downloader::update_checksums(data_dir, *quiet),
        None => run_with_args(&cli.serve),
    }
}

pub fn run_with_args(args: &ServeArgs) -> Result<(), Box<dyn Error>> {
    framework::panic::init();
    framework::deadlock::spawn();

    let runtime = framework::runtime::build(&args.runtime_config())?;
    let server = Server::new(args, Some(runtime.handle()))?;

    runtime.spawn(framework::signal::handle_signals(server.clone()));
    runtime.block_on(async_main(&server))?;
    framework::runtime::shutdown(runtime, Duration::from_millis(10000));

    #[cfg(unix)]
    if server.server.restarting.load(Ordering::Acquire) {
        framework::utils::restart::restart_process();
    }

    info!("Exit");
    Ok(())
}

#[tracing::instrument(
	name = "main",
	parent = None,
	skip_all,
	level = "info"
)]
async fn async_main(server: &Arc<Server>) -> Result<(), anyhow::Error> {
    let config = server.server.config.clone();
    let server_config = Arc::new(ServerConfig {
        server_name: Some(config.server_name.to_owned()),
        address: Some(config.address.to_owned()),
        port: Some(config.port.to_owned()),
    });
    let database_config = Arc::new(DatabaseConfig {
        url: config.database_url.to_owned(),
    });
    let session_config = Arc::new(SessionConfig {
        close_connection_no_voice_time: config.session_close_connection_no_voice_time.to_owned(),
        silence_voice_timeout: config.session_silence_voice_timeout.to_owned(),
        system_prompt: config.session_system_prompt.to_owned(),
        max_prompt_len: config.session_max_prompt_len.to_owned(),
    });
    let mcp_config = Arc::new(McpConfig {
        uri_list: config.mcp_uri_list.to_owned(),
    });
    let vad_config = Arc::new(VadConfig {
        model: config.vad_model.to_owned(),
        variant: config.vad_variant.to_owned(),
        path: config
            .vad_path
            .to_owned()
            .or_else(|| config.derive_vad_path()),
        num_threads: config.vad_num_threads,
        threshold: config.vad_threshold,
        min_silence_duration: config.vad_min_silence_duration,
    });
    let audio_config = Arc::new(AudioConfig {
        output_sample_rate: config.audio_output_sample_rate,
        output_channel: config.audio_output_channel,
        output_frame_duration: config.audio_output_frame_duration,
    });
    let auth_config = Arc::new(AuthConfig {
        access_token_secret: config.auth_access_token_secret.to_owned(),
        access_token_expires_in: config.auth_access_token_expires_in,
        refresh_token_secret: config.auth_refresh_token_secret.to_owned(),
        refresh_token_expires_in: config.auth_refresh_token_expires_in,
        audience: config.auth_audience.to_owned(),
        issuer: config.auth_issuer.to_owned(),
        client_id: config.auth_client_id.to_owned(),
        client_secret: config.auth_client_secret.to_owned(),
    });
    let ws_config = Arc::new(WsConfig {
        schema: config.ws_schema.to_owned(),
    });
    let tts_config =
        {
            let data_dir = config.data_dir();
            let model = config
                .tts_model
                .clone()
                .expect("tts_model should have default");

            // Resolve variant: user config → manifest default
            let effective_variant = config
                .tts_variant
                .clone()
                .or_else(|| crate::downloader::default_tts_variant(&model))
                .ok_or_else(|| {
                    anyhow!("tts variant not configured and manifest missing default_variant")
                })?;

            // Resolve path: explicit config → manifest base + variant
            let tts_path = config.tts_path.clone().or_else(|| {
                let base = crate::downloader::tts_base_path(&model)?;
                config.derive_tts_path(&base, &effective_variant)
            });

            let ref_variant = config.tts_reference_variant.clone()
            .or_else(crate::downloader::default_reference_variant)
            .ok_or_else(|| anyhow!(
                "reference audio variant not configured and manifest missing default_variant"
            ))?;
            let (ref_path, ref_text) = crate::downloader::resolve_reference_audio(&ref_variant)
                .ok_or_else(|| {
                    anyhow!("reference audio variant '{ref_variant}' not found in manifest")
                })?;
            // The path from the embedded manifest is relative to data_dir
            let ref_path = if ref_path.starts_with('/') {
                ref_path
            } else {
                format!("{data_dir}/{ref_path}")
            };

            // Merge manifest defaults into tts_options, user values take precedence
            let tts_options = {
                let mut opts = config
                    .tts_options
                    .clone()
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                if let Some(ls) = crate::downloader::tts_length_scale(&model, &effective_variant)
                    && let Some(m) = opts.as_object_mut()
                {
                    m.entry("length_scale")
                        .or_insert_with(|| serde_json::json!(ls));
                }
                Some(opts)
            };

            Arc::new(TtsConfig {
                model: config.tts_model.to_owned(),
                variant: Some(effective_variant),
                path: tts_path,
                reference_prompt_text: config.tts_reference_prompt_text.to_owned().or(
                    if ref_text.is_empty() {
                        None
                    } else {
                        Some(ref_text)
                    },
                ),
                reference_prompt_wav_path: config
                    .tts_reference_prompt_wav_path
                    .to_owned()
                    .or(Some(ref_path)),
                options: tts_options,
            })
        };
    let asr_config = {
        let model = config
            .asr_model
            .clone()
            .expect("asr_model should have default");
        let (effective_variant, asr_path) = if model == AsrModel::Void {
            (None, None)
        } else {
            let v = config
                .asr_variant
                .clone()
                .or_else(|| crate::downloader::default_asr_variant(&model))
                .unwrap_or_else(|| "default".into());
            let p = config.asr_path.clone().or_else(|| {
                let base = crate::downloader::asr_base_path(&model)?;
                config.derive_asr_path(&base, &v)
            });
            (Some(v), p)
        };

        Arc::new(AsrConfig {
            model: config.asr_model.to_owned(),
            variant: effective_variant,
            path: asr_path,
        })
    };
    let llm_config = Arc::new(LlmConfig {
        model: config.llm_model.to_owned(),
        variant: config.llm_variant.to_owned(),
        path: config
            .llm_path
            .to_owned()
            .or_else(|| config.derive_llm_path()),
    });
    let matrix_config = Arc::new(MatrixConfig {
        enable: config.matrix_enable,
        client_name: config.matrix_client_name.to_owned(),
        homeserver: config.matrix_homeserver.to_owned(),
        client_username: config.matrix_client_username.to_owned(),
        client_password: config.matrix_client_password.to_owned(),
    });
    api::start(api::StartParams {
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
    })
    .await?;
    info!("Exit runtime");
    Ok(())
}
