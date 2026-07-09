//! Integration with `clap`

use std::{path::PathBuf, time::Duration};

use clap::{ArgAction, Parser, Subcommand};
use figment::{Figment, value::Value};
use framework::utils::sys::available_parallelism;

/// Commandline arguments for the server
#[derive(Parser, Debug, Clone)]
pub struct ServeArgs {
    #[arg(short, long)]
    /// Path to the config TOML file (optional)
    pub config: Option<Vec<PathBuf>>,

    /// Override a configuration variable using TOML 'key=value' syntax
    #[arg(long, short('O'))]
    pub option: Vec<String>,

    /// Override the tokio worker_thread count.
    #[arg(
		long,
		hide(true),
		env = "TOKIO_WORKER_THREADS",
    default_value = available_parallelism().to_string()
	  )]
    pub worker_threads: usize,

    /// Override the tokio global_queue_interval.
    #[arg(
        long,
        hide(true),
        env = "TOKIO_GLOBAL_QUEUE_INTERVAL",
        default_value = "192"
    )]
    pub global_event_interval: u32,

    /// Override the tokio event_interval.
    #[arg(long, hide(true), env = "TOKIO_EVENT_INTERVAL", default_value = "512")]
    pub kernel_event_interval: u32,

    /// Override the tokio max_io_events_per_tick.
    #[arg(
        long,
        hide(true),
        env = "TOKIO_MAX_IO_EVENTS_PER_TICK",
        default_value = "512"
    )]
    pub kernel_events_per_tick: usize,

    /// Toggles worker affinity feature.
    #[arg(
		long,
		hide(true),
		env = "CHOBITS_RUNTIME_WORKER_AFFINITY",
		action = ArgAction::Set,
		num_args = 0..=1,
		require_equals(false),
		default_value = "true",
		default_missing_value = "true",
	)]
    pub worker_affinity: bool,

    /// Toggles feature to promote memory reclamation by the operating system
    /// when tokio worker runs out of work.
    #[arg(
		long,
		hide(true),
		env = "CHOBITS_RUNTIME_GC_ON_PARK",
		action = ArgAction::Set,
		num_args = 0..=1,
		require_equals(false),
	)]
    pub gc_on_park: Option<bool>,

    /// Toggles muzzy decay for jemalloc arenas associated with a tokio
    /// worker (when worker-affinity is enabled). Setting to false releases
    /// memory to the operating system using MADV_FREE without MADV_DONTNEED.
    /// Setting to false increases performance by reducing pagefaults, but
    /// resident memory usage appears high until there is memory pressure. The
    /// default is true unless the system has four or more cores.
    #[arg(
		long,
		hide(true),
		env = "CHOBITS_RUNTIME_GC_MUZZY",
		action = ArgAction::Set,
		num_args = 0..=1,
		require_equals(false),
	)]
    pub gc_muzzy: Option<bool>,
}

impl ServeArgs {
    pub(crate) fn runtime_config(&self) -> framework::runtime::RuntimeConfig {
        framework::runtime::RuntimeConfig {
            worker_threads: self.worker_threads,
            global_event_interval: self.global_event_interval,
            kernel_event_interval: self.kernel_event_interval,
            kernel_events_per_tick: self.kernel_events_per_tick,
            worker_affinity: self.worker_affinity,
            gc_on_park: self.gc_on_park,
            gc_muzzy: self.gc_muzzy,
            worker_name: "chobits:worker",
            worker_min: 2,
            worker_keepalive: 36,
            max_blocking_threads: 1024,
            shutdown_timeout: Duration::from_millis(10000),
        }
    }
}

/// Top-level CLI
#[derive(Parser, Debug)]
#[clap(
	about,
	long_about = None,
	name = framework::name(),
	version = framework::version(),
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[command(flatten)]
    pub serve: ServeArgs,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Download AI models
    Downloader {
        #[command(subcommand)]
        action: DownloaderAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum DownloaderAction {
    /// Download AI models to the local data directory
    Install {
        /// Category: tts, asr, llm, vad, reference (default: all)
        #[arg(conflicts_with = "all")]
        category: Option<String>,

        /// Model name (e.g., qwen3, earshot)
        #[arg(conflicts_with = "all")]
        model: Option<String>,

        /// Variant name (e.g., 0.5B, tiny, small, large-v3)
        #[arg(conflicts_with = "all")]
        variant: Option<String>,

        /// Base data directory
        #[arg(long, default_value = "data")]
        data_dir: PathBuf,

        /// Suppress progress output
        #[arg(long)]
        quiet: bool,

        /// Custom mirror domains (replaces default hf-mirror.com)
        #[arg(long)]
        mirror: Vec<String>,

        /// Path or URL to a JSON file overriding download URLs
        #[arg(long = "override")]
        overrides: Option<String>,

        /// Path to the application config TOML file.
        /// When set, only downloads models enabled in the config.
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Download all files from all manifests (ignores config)
        #[arg(long, conflicts_with_all = ["category", "model", "variant"])]
        all: bool,
    },
    /// Interactive download wizard
    Wizard {
        /// Base data directory
        #[arg(long, default_value = "data")]
        data_dir: PathBuf,

        /// Suppress progress output
        #[arg(long)]
        quiet: bool,
    },
    /// List available models and their variants
    List {
        /// Category filter: tts, asr, llm, vad, reference
        category: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Compute and write sha256 checksums for already-downloaded files
    UpdateChecksums {
        /// Base data directory
        #[arg(long, default_value = "data")]
        data_dir: PathBuf,

        /// Suppress progress output
        #[arg(long)]
        quiet: bool,
    },
}

/// Parse commandline arguments into structured data
#[must_use]
pub(crate) fn parse() -> Cli {
    Cli::parse()
}

/// Synthesize any command line options with configuration file options.
pub(crate) fn update(mut config: Figment, args: &ServeArgs) -> Result<Figment, anyhow::Error> {
    // All other individual overrides can go last in case we have options which
    // set multiple conf items at once and the user still needs granular overrides.
    for option in &args.option {
        let (key, val) = option
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("Missing '=' in -O/--option: {option:?}"))?;

        if key.is_empty() {
            return Err(anyhow::anyhow!("Missing key= in -O/--option: {option:?}"));
        }

        if val.is_empty() {
            return Err(anyhow::anyhow!("Missing =val in -O/--option: {option:?}"));
        }

        // The value has to pass for what would appear as a line in the TOML file.
        let val = toml::from_str::<Value>(option)?;
        let Value::Dict(_, val) = val else {
            panic!("Unexpected Figment Value: {val:#?}");
        };

        // Figment::merge() overrides existing
        config = config.merge((key, val[key].clone()));
    }

    Ok(config)
}
