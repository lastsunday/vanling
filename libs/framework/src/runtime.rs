use std::{
    iter::once,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use tokio::runtime::Builder;
use tracing::debug;

pub struct RuntimeConfig {
    pub worker_threads: usize,
    pub global_event_interval: u32,
    pub kernel_event_interval: u32,
    pub kernel_events_per_tick: usize,
    pub worker_affinity: bool,
    pub gc_on_park: Option<bool>,
    pub gc_muzzy: Option<bool>,
    #[cfg(feature = "tokio_unstable")]
    pub worker_histogram_interval: u64,
    #[cfg(feature = "tokio_unstable")]
    pub worker_histogram_buckets: usize,
    pub worker_name: &'static str,
    pub worker_min: usize,
    pub worker_keepalive: u64,
    pub max_blocking_threads: usize,
    pub shutdown_timeout: Duration,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            worker_threads: 4,
            global_event_interval: 192,
            kernel_event_interval: 512,
            kernel_events_per_tick: 512,
            worker_affinity: true,
            gc_on_park: None,
            gc_muzzy: None,
            #[cfg(feature = "tokio_unstable")]
            worker_histogram_interval: 100,
            #[cfg(feature = "tokio_unstable")]
            worker_histogram_buckets: 10,
            worker_name: "worker",
            worker_min: 2,
            worker_keepalive: 36,
            max_blocking_threads: 1024,
            shutdown_timeout: Duration::from_secs(10),
        }
    }
}

static WORKER_AFFINITY: OnceLock<bool> = OnceLock::new();
static GC_ON_PARK: OnceLock<Option<bool>> = OnceLock::new();
static GC_MUZZY: OnceLock<Option<bool>> = OnceLock::new();

pub fn build(config: &RuntimeConfig) -> Result<tokio::runtime::Runtime, anyhow::Error> {
    WORKER_AFFINITY
        .set(config.worker_affinity)
        .expect("set WORKER_AFFINITY from program argument");

    GC_ON_PARK
        .set(config.gc_on_park)
        .expect("set GC_ON_PARK from program argument");

    GC_MUZZY
        .set(config.gc_muzzy)
        .expect("set GC_MUZZY from program argument");

    let mut builder = Builder::new_multi_thread();
    builder
        .enable_io()
        .enable_time()
        .thread_name(config.worker_name)
        .worker_threads(config.worker_threads.max(config.worker_min))
        .max_blocking_threads(config.max_blocking_threads)
        .thread_keep_alive(Duration::from_secs(config.worker_keepalive))
        .global_queue_interval(config.global_event_interval)
        .event_interval(config.kernel_event_interval)
        .max_io_events_per_tick(config.kernel_events_per_tick)
        .on_thread_start(thread_start)
        .on_thread_stop(thread_stop)
        .on_thread_unpark(thread_unpark)
        .on_thread_park(thread_park);

    #[cfg(feature = "tokio_unstable")]
    builder
        .on_task_spawn(task_spawn)
        .on_before_task_poll(task_enter)
        .on_after_task_poll(task_leave)
        .on_task_terminate(task_terminate);

    #[cfg(feature = "tokio_unstable")]
    enable_histogram(&mut builder, config);

    builder.build().map_err(Into::into)
}

#[cfg(feature = "tokio_unstable")]
fn enable_histogram(builder: &mut Builder, config: &RuntimeConfig) {
    use tokio::runtime::HistogramConfiguration;

    let buckets = config.worker_histogram_buckets;
    let interval = Duration::from_micros(config.worker_histogram_interval);
    let linear = HistogramConfiguration::linear(interval, buckets);
    builder
        .enable_metrics_poll_time_histogram()
        .metrics_poll_time_histogram_configuration(linear);
}

#[tracing::instrument(name = "stop", level = "info", skip_all)]
pub fn shutdown(runtime: tokio::runtime::Runtime, timeout: Duration) {
    debug!(?timeout, "Waiting for runtime...");
    runtime.shutdown_timeout(timeout);
}

#[tracing::instrument(
	name = "fork",
	level = "debug",
	skip_all,
	fields(
		id = ?thread::current().id(),
		name = %thread::current().name().unwrap_or("None"),
	),
)]
fn thread_start() {
    if WORKER_AFFINITY.get().is_some_and(|&x| x) {
        set_worker_affinity();
    }
}

fn set_worker_affinity() {
    static CORES_OCCUPIED: AtomicUsize = AtomicUsize::new(0);

    let handle = tokio::runtime::Handle::current();
    let num_workers = handle.metrics().num_workers();
    let i = CORES_OCCUPIED.fetch_add(1, Ordering::Relaxed);
    if i >= num_workers {
        return;
    }

    let Some(id) = crate::utils::sys::compute::nth_core_available(i) else {
        return;
    };

    crate::utils::sys::compute::set_affinity(once(id));
    set_worker_mallctl(id);
}

#[cfg(all(not(target_env = "msvc"), feature = "jemalloc"))]
fn set_worker_mallctl(id: usize) {
    // jemalloc arena / muzzy decay configuration
    // (implemented per-project, framework provides the hook point)
    let _ = id;
}

#[cfg(any(not(feature = "jemalloc"), target_env = "msvc"))]
fn set_worker_mallctl(_: usize) {}

#[tracing::instrument(
	name = "join",
	level = "debug",
	skip_all,
	fields(
		id = ?thread::current().id(),
		name = %thread::current().name().unwrap_or("None"),
	),
)]
fn thread_stop() {}

#[tracing::instrument(
	name = "work",
	level = "trace",
	skip_all,
	fields(
		id = ?thread::current().id(),
		name = %thread::current().name().unwrap_or("None"),
	),
)]
fn thread_unpark() {}

#[tracing::instrument(
	name = "park",
	level = "trace",
	skip_all,
	fields(
		id = ?thread::current().id(),
		name = %thread::current().name().unwrap_or("None"),
	),
)]
fn thread_park() {
    match GC_ON_PARK
        .get()
        .as_ref()
        .expect("GC_ON_PARK initialized by runtime::new()")
    {
        Some(true) | None if cfg!(feature = "jemalloc") => gc_on_park(),
        _ => (),
    }
}

fn gc_on_park() {
    // jemalloc decay on park
}

#[cfg(feature = "tokio_unstable")]
#[tracing::instrument(
	name = "spawn",
	level = "trace",
	skip_all,
	fields(
		id = %meta.id(),
	),
)]
fn task_spawn(meta: &tokio::runtime::TaskMeta<'_>) {}

#[cfg(feature = "tokio_unstable")]
#[tracing::instrument(
	name = "finish",
	level = "trace",
	skip_all,
	fields(
		id = %meta.id()
	),
)]
fn task_terminate(meta: &tokio::runtime::TaskMeta<'_>) {}

#[cfg(feature = "tokio_unstable")]
#[tracing::instrument(
	name = "enter",
	level = "trace",
	skip_all,
	fields(
		id = %meta.id()
	),
)]
fn task_enter(meta: &tokio::runtime::TaskMeta<'_>) {}

#[cfg(feature = "tokio_unstable")]
#[tracing::instrument(
	name = "leave",
	level = "trace",
	skip_all,
	fields(
		id = %meta.id()
	),
)]
fn task_leave(meta: &tokio::runtime::TaskMeta<'_>) {}
