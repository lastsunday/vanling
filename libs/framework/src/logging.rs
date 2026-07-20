use std::{
    io,
    sync::{
        Arc, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use tracing_appender::rolling;
use tracing_subscriber::{
    EnvFilter, Layer, Registry,
    fmt::{self, format::FmtSpan, writer::BoxMakeWriter},
    layer::SubscriberExt,
    reload,
};

use crate::config::logging::{LogConfig, LogFormat};
use crate::log::ConsoleFormat;
use crate::log::capture;

#[cfg(feature = "perf")]
use tracing_flame::FlameLayer;

#[cfg(feature = "perf")]
type TracingFlameGuard = Option<tracing_flame::FlushGuard<std::fs::File>>;
#[cfg(not(feature = "perf"))]
type TracingFlameGuard = ();

const LEVELS: &[tracing_subscriber::filter::LevelFilter] = &[
    tracing_subscriber::filter::LevelFilter::ERROR,
    tracing_subscriber::filter::LevelFilter::WARN,
    tracing_subscriber::filter::LevelFilter::INFO,
    tracing_subscriber::filter::LevelFilter::DEBUG,
    tracing_subscriber::filter::LevelFilter::TRACE,
];

struct ReloadHandle {
    set_level: Box<dyn Fn(tracing_subscriber::filter::LevelFilter) + Send + Sync>,
}

impl ReloadHandle {
    fn new<S: tracing::Subscriber + 'static>(handle: reload::Handle<EnvFilter, S>) -> Self {
        Self {
            set_level: Box::new(move |level: tracing_subscriber::filter::LevelFilter| {
                let filter = EnvFilter::builder()
                    .with_default_directive(level.into())
                    .from_env_lossy();
                let _ = handle.modify(|f| *f = filter);
            }),
        }
    }

    fn set(&self, level: tracing_subscriber::filter::LevelFilter) {
        (self.set_level)(level);
    }
}

pub struct LoggingHandle {
    console_ctl: ReloadHandle,
    file_ctl: Option<ReloadHandle>,
    pub config: Arc<RwLock<LogConfig>>,
    pub level_index: Arc<AtomicUsize>,
    _console_guard: tracing_appender::non_blocking::WorkerGuard,
    _file_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
    _flame_guard: TracingFlameGuard,
}

impl LoggingHandle {
    pub fn cycle_console_level(&self) {
        let idx = self.level_index.fetch_add(1, Ordering::Relaxed);
        let level = LEVELS[idx % LEVELS.len()];
        self.console_ctl.set(level);
    }

    pub fn reload_from_config(&self) {
        if let Ok(cfg) = self.config.read() {
            let level = cfg
                .console_level
                .parse::<tracing_subscriber::filter::LevelFilter>()
                .unwrap_or(tracing_subscriber::filter::LevelFilter::INFO);
            self.console_ctl.set(level);

            if let Some(ref file_ctl) = self.file_ctl {
                let file_level = cfg
                    .file_level
                    .parse::<tracing_subscriber::filter::LevelFilter>()
                    .unwrap_or(tracing_subscriber::filter::LevelFilter::INFO);
                file_ctl.set(file_level);
            }
        }
    }
}

fn build_console_layer<S, W>(
    format: LogFormat,
    log_thread_ids: bool,
    log_colors: bool,
    writer: W,
) -> Box<dyn Layer<S> + Send + Sync + 'static>
where
    S: tracing::Subscriber
        + for<'a> tracing_subscriber::registry::LookupSpan<'a>
        + Send
        + Sync
        + 'static,
    W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    match format {
        LogFormat::Json => Box::new(
            fmt::layer()
                .with_thread_ids(log_thread_ids)
                .with_ansi(log_colors)
                .with_target(false)
                .json()
                .with_current_span(true)
                .with_writer(writer),
        ),
        LogFormat::Compact => Box::new(
            fmt::layer()
                .compact()
                .with_thread_ids(log_thread_ids)
                .with_ansi(log_colors)
                .with_target(false)
                .fmt_fields(ConsoleFormat::new(log_thread_ids, log_colors))
                .with_writer(writer),
        ),
        LogFormat::Pretty => Box::new(
            fmt::layer()
                .with_thread_ids(log_thread_ids)
                .with_ansi(log_colors)
                .with_target(false)
                .pretty()
                .with_writer(writer),
        ),
        LogFormat::Text => Box::new(
            fmt::layer()
                .with_thread_ids(log_thread_ids)
                .with_ansi(log_colors)
                .with_target(false)
                .fmt_fields(ConsoleFormat::new(log_thread_ids, log_colors))
                .with_writer(writer),
        ),
    }
}

fn build_file_layer<S>(
    format: LogFormat,
    writer: BoxMakeWriter,
) -> Box<dyn Layer<S> + Send + Sync + 'static>
where
    S: tracing::Subscriber
        + for<'a> tracing_subscriber::registry::LookupSpan<'a>
        + Send
        + Sync
        + 'static,
{
    let span_events = FmtSpan::CLOSE;
    match format {
        LogFormat::Json => Box::new(
            fmt::layer()
                .with_span_events(span_events)
                .json()
                .with_writer(writer),
        ),
        LogFormat::Compact => Box::new(
            fmt::layer()
                .with_span_events(span_events)
                .compact()
                .with_writer(writer),
        ),
        LogFormat::Pretty => Box::new(
            fmt::layer()
                .with_span_events(span_events)
                .pretty()
                .with_writer(writer),
        ),
        LogFormat::Text => Box::new(
            fmt::layer()
                .with_span_events(span_events)
                .with_writer(writer),
        ),
    }
}

fn mk_filter(enabled: bool, level: &str) -> EnvFilter {
    if !enabled {
        return EnvFilter::new("off");
    }
    let level_filter = level
        .parse::<tracing_subscriber::filter::LevelFilter>()
        .unwrap_or(tracing_subscriber::filter::LevelFilter::INFO);
    EnvFilter::builder()
        .with_default_directive(level_filter.into())
        .from_env_lossy()
        .add_directive(
            "sea_orm::database::transaction=warn"
                .parse()
                .expect("invalid filter directive"),
        )
        .add_directive(
            "hyper_util::client::legacy::connect::http=warn"
                .parse()
                .expect("invalid filter directive"),
        )
        .add_directive(
            "tower_http::trace=warn"
                .parse()
                .expect("invalid filter directive"),
        )
        .add_directive("rmcp=warn".parse().expect("invalid filter directive"))
}

#[allow(unused_variables)]
pub fn init(config: LogConfig) -> anyhow::Result<LoggingHandle> {
    let _console_level = config
        .console_level
        .parse::<tracing_subscriber::filter::LevelFilter>()
        .map_err(|e| anyhow::anyhow!("invalid console log level: {e}"))?;
    let _file_level = config
        .file_level
        .parse::<tracing_subscriber::filter::LevelFilter>()
        .map_err(|e| anyhow::anyhow!("invalid file log level: {e}"))?;

    let console_filter = mk_filter(config.console_enabled, &config.console_level);
    let (console_filter_layer, console_reload) = reload::Layer::new(console_filter);

    let file_filter = mk_filter(config.file_enabled, &config.file_level);
    let (file_filter_layer, file_reload) = reload::Layer::new(file_filter);

    let (file_writer, file_guard) = if config.file_enabled {
        let rotation = match config.file_rotation {
            crate::config::logging::LogRotation::Daily => rolling::Rotation::DAILY,
            crate::config::logging::LogRotation::Hourly => rolling::Rotation::HOURLY,
            crate::config::logging::LogRotation::Never => rolling::Rotation::NEVER,
        };
        let appender = rolling::RollingFileAppender::builder()
            .rotation(rotation)
            .filename_prefix(&config.file_name)
            .max_log_files(config.file_max_files)
            .build(&config.file_directory)
            .map_err(|e| anyhow::anyhow!("failed to create file appender: {e}"))?;
        let (non_blocking, guard) = tracing_appender::non_blocking(appender);
        (BoxMakeWriter::new(non_blocking), Some(guard))
    } else {
        let (w, g) = tracing_appender::non_blocking(io::sink());
        (BoxMakeWriter::new(w), Some(g))
    };

    let (console_writer, console_guard) = tracing_appender::non_blocking(io::stdout());

    let console_layer = build_console_layer(
        config.console_format,
        config.log_thread_ids,
        config.log_colors,
        console_writer,
    );

    let file_layer = build_file_layer(config.file_format, file_writer);

    let cap_state = Arc::new(capture::State::new());
    let cap_layer = capture::Layer::new(&cap_state);

    let subscriber = Registry::default()
        .with(console_layer.with_filter(console_filter_layer))
        .with(file_layer.with_filter(file_filter_layer))
        .with(cap_layer);

    #[cfg(all(target_family = "unix", feature = "journald"))]
    if config.log_to_journald {
        println!("Initialising journald logging");
        if let Err(e) = init_journald_logging(&config) {
            eprintln!("Failed to initialize journald logging: {e}");
        }
    }

    let console_reload_handle = ReloadHandle::new(console_reload);
    let file_reload_handle = if config.file_enabled {
        Some(ReloadHandle::new(file_reload))
    } else {
        None
    };

    #[cfg(feature = "sentry")]
    let subscriber = {
        let sentry_filter = EnvFilter::try_new(&config.sentry_filter)
            .map_err(|e| anyhow::anyhow!("Config(sentry_filter, \"{e}.\")"))?;

        let sentry_layer = sentry_tracing::layer();
        let (sentry_reload_filter, _sentry_reload_handle) = reload::Layer::new(sentry_filter);

        subscriber.with(sentry_layer.with_filter(sentry_reload_filter))
    };

    #[cfg(feature = "otlp")]
    let subscriber = {
        let otlp_filter = EnvFilter::try_new(&config.otlp_filter)
            .map_err(|e| anyhow::anyhow!("Config(otlp_filter, \"{e}.\")"))?;

        let otlp_layer = config.allow_otlp.then(|| {
			opentelemetry::global::set_text_map_propagator(
				opentelemetry_sdk::propagation::TraceContextPropagator::new(),
			);

			let exporter = match config.otlp_protocol.as_str() {
				| "grpc" => opentelemetry_otlp::SpanExporter::builder()
					.with_tonic()
					.with_protocol(opentelemetry_otlp::Protocol::Grpc)
					.build()
					.expect("Failed to create OTLP gRPC exporter"),
				| "http" => opentelemetry_otlp::SpanExporter::builder()
					.with_http()
					.build()
					.expect("Failed to create OTLP HTTP exporter"),
				| protocol => {
					tracing::warn!(
						"Invalid OTLP protocol '{protocol}', falling back to HTTP. Valid options are \
						 'http' or 'grpc'."
					);
					opentelemetry_otlp::SpanExporter::builder()
						.with_http()
						.build()
						.expect("Failed to create OTLP HTTP exporter")
				},
			};

			let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
				.with_batch_exporter(exporter)
				.build();

			let tracer = provider.tracer(crate::info::name());

			let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

			let (otlp_reload_filter, _otlp_reload_handle) =
				reload::Layer::new(otlp_filter.clone());

			Some(telemetry.with_filter(otlp_reload_filter))
		});

        subscriber.with(otlp_layer)
    };

    #[cfg(feature = "perf")]
    let (subscriber, flame_guard) = {
        let (flame_layer, flame_guard) = if config.tracing_flame {
            let flame_filter = EnvFilter::try_new(&config.tracing_flame_filter)
                .map_err(|e| anyhow::anyhow!("Config(tracing_flame_filter, \"{e}.\")"))?;

            let (flame_layer, flame_guard) =
                FlameLayer::with_file(&config.tracing_flame_output_path)
                    .map_err(|e| anyhow::anyhow!("Config(tracing_flame_output_path, \"{e}.\")"))?;

            let flame_layer = flame_layer
                .with_empty_samples(false)
                .with_filter(flame_filter);

            (Some(flame_layer), Some(flame_guard))
        } else {
            (None, None)
        };

        let subscriber = subscriber.with(flame_layer);
        (subscriber, flame_guard)
    };

    #[cfg(not(feature = "perf"))]
    #[cfg_attr(not(feature = "perf"), allow(clippy::let_unit_value))]
    let flame_guard = ();

    let handle = LoggingHandle {
        console_ctl: console_reload_handle,
        file_ctl: file_reload_handle,
        config: Arc::new(RwLock::new(config)),
        level_index: Arc::new(AtomicUsize::new(0)),
        _console_guard: console_guard,
        _file_guard: file_guard,
        _flame_guard: flame_guard,
    };

    let (console_enabled, console_disabled_reason) =
        tokio_console_enabled(&handle.config.read().unwrap());
    #[cfg(all(feature = "console", feature = "tokio_unstable"))]
    if console_enabled {
        let console_layer = console_subscriber::ConsoleLayer::builder()
            .with_default_env()
            .spawn();

        set_global_default(subscriber.with(console_layer));
        return Ok(handle);
    }

    set_global_default(subscriber);

    if !console_enabled && !console_disabled_reason.is_empty() {
        tracing::warn!(%console_disabled_reason, "console layer disabled");
    }

    Ok(handle)
}

#[cfg(all(target_family = "unix", feature = "journald"))]
fn init_journald_logging(config: &LogConfig) -> Result<(), anyhow::Error> {
    use tracing_journald::Layer as JournaldLayer;

    let journald_filter = EnvFilter::try_new(&config.console_level)
        .map_err(|e| anyhow::anyhow!("Config(log, \"{e}.\")"))?;

    let mut journald_layer = JournaldLayer::new().map_err(|e| {
        anyhow::anyhow!("Config(journald, \"Failed to initialize journald layer: {e}.\")")
    })?;

    if let Some(ref identifier) = config.journald_identifier {
        journald_layer = journald_layer.with_syslog_identifier(identifier.to_owned());
    }

    let journald_subscriber = Registry::default().with(journald_layer.with_filter(journald_filter));

    let _guard = tracing::subscriber::set_default(journald_subscriber);

    Ok(())
}

fn tokio_console_enabled(config: &LogConfig) -> (bool, &'static str) {
    #[cfg(all(feature = "console", feature = "tokio_unstable"))]
    {
        if !config.tokio_console_enabled {
            return (
                false,
                "tokio console is available but disabled by the configuration.",
            );
        }

        (true, "")
    }

    #[cfg(not(all(feature = "console", feature = "tokio_unstable")))]
    {
        let _ = config;
        (false, "")
    }
}

fn set_global_default<S>(subscriber: S)
where
    S: tracing::Subscriber + Send + Sync + 'static,
{
    tracing::subscriber::set_global_default(subscriber)
        .expect("the global default tracing subscriber failed to be initialized");
}
