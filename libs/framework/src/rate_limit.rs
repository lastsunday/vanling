use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Maximum number of tracked keys before eviction kicks in.
const MAX_ENTRIES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitError {
    pub retry_after: Duration,
}

#[derive(Debug, Clone, Copy)]
pub struct FixedWindowConfig {
    pub limit: u32,
    pub window: Duration,
}

impl FixedWindowConfig {
    pub const fn new(limit: u32, window: Duration) -> Self {
        Self { limit, window }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum RateLimitDecision {
    Allowed {
        limit: u32,
        remaining: u32,
        reset_after: Duration,
    },
    Limited {
        limit: u32,
        retry_after: Duration,
    },
}

/// Snapshot of a rate-limit bucket without recording a request.
#[derive(Debug, Clone, Copy)]
pub struct BucketSnapshot {
    pub limit: u32,
    pub used: u32,
    pub remaining: u32,
    pub reset_after: Duration,
}

/// Rate-limit quota configuration for the [`UsageRegistry`].
///
/// Resource order is significant: matching, introspection and usage stats all
/// follow the order of `resources`.
#[derive(Debug, Clone)]
pub struct UsageConfig {
    pub resources: Vec<(String, FixedWindowConfig)>,
}

impl Default for UsageConfig {
    fn default() -> Self {
        Self::new(vec![
            (
                "auth".to_string(),
                FixedWindowConfig::new(20, Duration::from_secs(15 * 60)),
            ),
            (
                "ota".to_string(),
                FixedWindowConfig::new(30, Duration::from_secs(60)),
            ),
            (
                "mcp".to_string(),
                FixedWindowConfig::new(1000, Duration::from_secs(60 * 60)),
            ),
            (
                "core".to_string(),
                FixedWindowConfig::new(1000, Duration::from_secs(60 * 60)),
            ),
        ])
    }
}

impl UsageConfig {
    pub fn new(resources: Vec<(String, FixedWindowConfig)>) -> Self {
        Self { resources }
    }
}

/// Per-resource runtime usage snapshot for the [`UsageRegistry`].
#[derive(Debug, Clone)]
pub struct ResourceUsage {
    pub limit: u32,
    pub window_secs: u64,
    pub active_keys: usize,
    pub allowed: u64,
    pub limited: u64,
    pub top_keys: Vec<(String, BucketSnapshot)>,
}

/// In-memory rate-limit buckets for a set of named resources.
#[derive(Debug)]
pub struct UsageRegistry {
    limiters: HashMap<String, FixedWindowLimiter>,
    config: UsageConfig,
}

impl Default for UsageRegistry {
    fn default() -> Self {
        Self::new(UsageConfig::default())
    }
}

impl UsageRegistry {
    pub fn new(config: UsageConfig) -> Self {
        let limiters = config
            .resources
            .iter()
            .map(|(name, cfg)| (name.clone(), FixedWindowLimiter::new(*cfg)))
            .collect();
        Self { limiters, config }
    }

    /// Records one request for `name`/`key`. Returns `None` when `name` is not
    /// a configured resource.
    pub fn check(&self, name: &str, key: &str) -> Option<RateLimitDecision> {
        self.limiter(name).map(|limiter| limiter.check(key))
    }

    /// Returns the current usage snapshot for `name`/`key` without recording a
    /// request. `None` when `name` is unknown or `key` has no active bucket.
    pub fn peek(&self, name: &str, key: &str) -> Option<BucketSnapshot> {
        self.limiter(name)?.peek(key)
    }

    pub fn limit(&self, name: &str) -> Option<u32> {
        self.config_for(name).map(|cfg| cfg.limit)
    }

    pub fn window_secs(&self, name: &str) -> Option<u64> {
        self.config_for(name).map(|cfg| cfg.window.as_secs())
    }

    /// Returns runtime usage snapshots for all configured resources in config
    /// order, with up to `top_n` keys per resource.
    pub fn usage_stats(&self, top_n: usize) -> Vec<(String, ResourceUsage)> {
        self.config
            .resources
            .iter()
            .filter_map(|(name, _)| {
                let usage = self.resource_usage(name, top_n)?;
                Some((name.clone(), usage))
            })
            .collect()
    }

    fn resource_usage(&self, name: &str, top_n: usize) -> Option<ResourceUsage> {
        let limiter = self.limiters.get(name)?;
        let (_, cfg) = self.config.resources.iter().find(|(n, _)| n == name)?;
        Some(ResourceUsage {
            limit: cfg.limit,
            window_secs: cfg.window.as_secs(),
            active_keys: limiter.active_keys(),
            allowed: limiter.allowed(),
            limited: limiter.limited(),
            top_keys: limiter.top_keys(top_n),
        })
    }

    fn limiter(&self, name: &str) -> Option<&FixedWindowLimiter> {
        self.limiters.get(name)
    }

    fn config_for(&self, name: &str) -> Option<FixedWindowConfig> {
        self.config
            .resources
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, cfg)| *cfg)
    }
}

#[derive(Debug)]
struct FixedWindowInner {
    buckets: HashMap<String, (Instant, u32)>,
}

/// In-memory fixed-window rate limiter keyed by arbitrary string (e.g. peer IP).
/// Single-process only; suitable for the single-instance deployment of this project.
#[derive(Debug)]
pub struct FixedWindowLimiter {
    inner: Mutex<FixedWindowInner>,
    config: FixedWindowConfig,
    allowed: AtomicU64,
    limited: AtomicU64,
}

impl FixedWindowLimiter {
    pub fn new(config: FixedWindowConfig) -> Self {
        Self {
            inner: Mutex::new(FixedWindowInner {
                buckets: HashMap::new(),
            }),
            config,
            allowed: AtomicU64::new(0),
            limited: AtomicU64::new(0),
        }
    }

    /// Records one request for `key`. Returns `Allowed` with remaining quota or
    /// `Limited` once the window quota is exhausted.
    pub fn check(&self, key: &str) -> RateLimitDecision {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner
            .buckets
            .retain(|_, (start, _)| now.duration_since(*start) < self.config.window);
        let (remaining, reset_after) = {
            let entry = inner
                .buckets
                .entry(key.to_string())
                .or_insert_with(|| (now, 0));
            let reset_after = self
                .config
                .window
                .saturating_sub(now.duration_since(entry.0));
            if entry.1 >= self.config.limit {
                self.limited.fetch_add(1, Ordering::Relaxed);
                return RateLimitDecision::Limited {
                    limit: self.config.limit,
                    retry_after: reset_after.max(Duration::from_secs(1)),
                };
            }
            entry.1 += 1;
            (self.config.limit - entry.1, reset_after)
        };
        self.allowed.fetch_add(1, Ordering::Relaxed);
        self.evict_if_needed(&mut inner, now);
        RateLimitDecision::Allowed {
            limit: self.config.limit,
            remaining,
            reset_after,
        }
    }

    /// Total number of allowed requests since the limiter was created.
    pub fn allowed(&self) -> u64 {
        self.allowed.load(Ordering::Relaxed)
    }

    /// Total number of rejected (rate-limited) requests since creation.
    pub fn limited(&self) -> u64 {
        self.limited.load(Ordering::Relaxed)
    }

    /// Number of distinct keys with an active bucket in the current window.
    pub fn active_keys(&self) -> usize {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner
            .buckets
            .retain(|_, (start, _)| now.duration_since(*start) < self.config.window);
        inner.buckets.len()
    }

    /// Up to `n` highest-usage keys sorted by consumed quota descending.
    pub fn top_keys(&self, n: usize) -> Vec<(String, BucketSnapshot)> {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner
            .buckets
            .retain(|_, (start, _)| now.duration_since(*start) < self.config.window);
        let mut snapshots: Vec<(String, BucketSnapshot)> = inner
            .buckets
            .iter()
            .map(|(key, (start, used))| {
                let remaining = self.config.limit.saturating_sub(*used);
                let reset_after = self
                    .config
                    .window
                    .saturating_sub(now.duration_since(*start))
                    .max(Duration::from_secs(1));
                (
                    key.clone(),
                    BucketSnapshot {
                        limit: self.config.limit,
                        used: *used,
                        remaining,
                        reset_after,
                    },
                )
            })
            .collect();
        snapshots.sort_by_key(|(_, s)| std::cmp::Reverse(s.used));
        snapshots.truncate(n);
        snapshots
    }

    /// Returns the current usage snapshot for `key` without recording a
    /// request. Returns `None` when `key` has no active bucket in the window.
    pub fn peek(&self, key: &str) -> Option<BucketSnapshot> {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner
            .buckets
            .retain(|_, (start, _)| now.duration_since(*start) < self.config.window);
        let (start, used) = inner.buckets.get(key)?;
        let remaining = self.config.limit.saturating_sub(*used);
        let reset_after = self
            .config
            .window
            .saturating_sub(now.duration_since(*start))
            .max(Duration::from_secs(1));
        Some(BucketSnapshot {
            limit: self.config.limit,
            used: *used,
            remaining,
            reset_after,
        })
    }

    fn evict_if_needed(&self, inner: &mut FixedWindowInner, now: Instant) {
        if inner.buckets.len() <= MAX_ENTRIES {
            return;
        }
        inner
            .buckets
            .retain(|_, (start, _)| now.duration_since(*start) < self.config.window);
        if inner.buckets.len() > MAX_ENTRIES {
            let oldest = inner
                .buckets
                .iter()
                .min_by_key(|(_, (start, _))| *start)
                .map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                inner.buckets.remove(&oldest);
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SlidingWindowConfig {
    pub limit: u32,
    pub window: Duration,
}

impl SlidingWindowConfig {
    pub const fn new(limit: u32, window: Duration) -> Self {
        Self { limit, window }
    }
}

#[derive(Debug)]
struct SlidingWindowInner {
    queues: HashMap<String, VecDeque<Instant>>,
}

/// In-memory sliding-window counter for per-account failure tracking.
#[derive(Debug)]
pub struct SlidingWindowCounter {
    inner: Mutex<SlidingWindowInner>,
    config: SlidingWindowConfig,
}

impl SlidingWindowCounter {
    pub fn new(config: SlidingWindowConfig) -> Self {
        Self {
            inner: Mutex::new(SlidingWindowInner {
                queues: HashMap::new(),
            }),
            config,
        }
    }

    /// Records an event for `key`, pruning timestamps outside the window.
    /// Returns `Err(RateLimitError)` once the window count exceeds the limit.
    pub fn record(&self, key: &str) -> Result<u32, RateLimitError> {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let queue = inner.queues.entry(key.to_string()).or_default();
        while let Some(&ts) = queue.front() {
            if now.duration_since(ts) < self.config.window {
                break;
            }
            queue.pop_front();
        }
        queue.push_back(now);
        let count = queue.len() as u32;
        if count > self.config.limit {
            let retry_after = self
                .config
                .window
                .saturating_sub(now.duration_since(*queue.front().expect("queue is non-empty")))
                .max(Duration::from_secs(1));
            return Err(RateLimitError { retry_after });
        }
        if inner.queues.len() > MAX_ENTRIES {
            inner.queues.retain(|_, q| !q.is_empty());
        }
        Ok(count)
    }

    /// Clears all recorded timestamps for `key` (e.g. after a successful login).
    pub fn clear(&self, key: &str) {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.queues.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(limit: u32, millis: u64) -> FixedWindowConfig {
        FixedWindowConfig::new(limit, Duration::from_millis(millis))
    }

    #[test]
    fn fixed_window_allows_within_limit() {
        let limiter = FixedWindowLimiter::new(cfg(3, 60_000));
        for i in 1..=3 {
            let decision = limiter.check("127.0.0.1");
            match decision {
                RateLimitDecision::Allowed { remaining, .. } => {
                    assert_eq!(remaining, 3 - i);
                }
                RateLimitDecision::Limited { .. } => panic!("should be allowed"),
            }
        }
        assert!(matches!(
            limiter.check("127.0.0.1"),
            RateLimitDecision::Limited { .. }
        ));
    }

    #[test]
    fn fixed_window_resets_after_window() {
        let limiter = FixedWindowLimiter::new(cfg(1, 30));
        assert!(matches!(
            limiter.check("key"),
            RateLimitDecision::Allowed { remaining: 0, .. }
        ));
        assert!(matches!(
            limiter.check("key"),
            RateLimitDecision::Limited { .. }
        ));
        std::thread::sleep(Duration::from_millis(40));
        assert!(matches!(
            limiter.check("key"),
            RateLimitDecision::Allowed { .. }
        ));
    }

    #[test]
    fn fixed_window_keys_are_independent() {
        let limiter = FixedWindowLimiter::new(cfg(1, 60_000));
        assert!(matches!(
            limiter.check("a"),
            RateLimitDecision::Allowed { .. }
        ));
        assert!(matches!(
            limiter.check("b"),
            RateLimitDecision::Allowed { .. }
        ));
        assert!(matches!(
            limiter.check("a"),
            RateLimitDecision::Limited { .. }
        ));
    }

    #[test]
    fn fixed_window_peek_does_not_record() {
        let limiter = FixedWindowLimiter::new(cfg(3, 60_000));
        assert!(limiter.peek("key").is_none());
        limiter.check("key");
        let snapshot = limiter.peek("key").expect("bucket exists");
        assert_eq!(snapshot.used, 1);
        assert_eq!(snapshot.remaining, 2);
        assert_eq!(limiter.peek("key").expect("bucket exists").used, 1);
    }

    #[test]
    fn default_usage_registry_has_configured_resources() {
        let registry = UsageRegistry::default();
        assert_eq!(registry.limit("auth"), Some(20));
        assert_eq!(registry.limit("ota"), Some(30));
        assert_eq!(registry.limit("core"), Some(1000));
        assert_eq!(registry.limit("mcp"), Some(1000));
        assert_eq!(registry.limit("unknown"), None);
        assert_eq!(registry.window_secs("unknown"), None);
        assert!(registry.check("unknown", "key").is_none());
    }

    #[test]
    fn usage_registry_tracks_resources_independently() {
        let registry = UsageRegistry::new(UsageConfig::new(vec![
            (
                "auth".to_string(),
                FixedWindowConfig::new(2, Duration::from_millis(60_000)),
            ),
            (
                "ota".to_string(),
                FixedWindowConfig::new(3, Duration::from_millis(60_000)),
            ),
            (
                "core".to_string(),
                FixedWindowConfig::new(4, Duration::from_millis(60_000)),
            ),
        ]));
        assert!(matches!(
            registry.check("auth", "ip:1"),
            Some(RateLimitDecision::Allowed { remaining: 1, .. })
        ));
        assert!(matches!(
            registry.check("auth", "ip:1"),
            Some(RateLimitDecision::Allowed { remaining: 0, .. })
        ));
        assert!(matches!(
            registry.check("auth", "ip:1"),
            Some(RateLimitDecision::Limited { .. })
        ));
        assert_eq!(registry.limit("ota"), Some(3));
        assert_eq!(registry.window_secs("core"), Some(60));
        assert!(registry.peek("core", "user:a").is_none());
        assert!(matches!(
            registry.check("core", "user:a"),
            Some(RateLimitDecision::Allowed { remaining: 3, .. })
        ));
        let snapshot = registry.peek("core", "user:a").expect("bucket exists");
        assert_eq!(snapshot.used, 1);
    }

    #[test]
    fn usage_registry_exposes_counters_and_top_keys() {
        let registry = UsageRegistry::new(UsageConfig::new(vec![
            (
                "auth".to_string(),
                FixedWindowConfig::new(2, Duration::from_millis(60_000)),
            ),
            (
                "ota".to_string(),
                FixedWindowConfig::new(3, Duration::from_millis(60_000)),
            ),
            (
                "core".to_string(),
                FixedWindowConfig::new(4, Duration::from_millis(60_000)),
            ),
        ]));
        registry.check("auth", "ip:1");
        registry.check("auth", "ip:1");
        registry.check("auth", "ip:1");
        registry.check("auth", "ip:2");
        registry.check("ota", "ip:9");
        registry.check("ota", "ip:9");
        registry.check("ota", "ip:9");
        registry.check("ota", "ip:9");

        let stats = registry.usage_stats(10);
        let auth = stats
            .iter()
            .find(|(name, _)| name == "auth")
            .expect("auth present");
        let ota = stats
            .iter()
            .find(|(name, _)| name == "ota")
            .expect("ota present");
        let core = stats
            .iter()
            .find(|(name, _)| name == "core")
            .expect("core present");
        assert_eq!(auth.1.limit, 2);
        assert_eq!(auth.1.allowed, 3);
        assert_eq!(auth.1.limited, 1);
        assert_eq!(auth.1.active_keys, 2);
        let top = &auth.1.top_keys;
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "ip:1");
        assert_eq!(top[0].1.used, 2);
        assert_eq!(ota.1.limited, 1);
        assert_eq!(ota.1.allowed, 3);
        assert_eq!(core.1.allowed, 0);
        assert_eq!(core.1.limited, 0);
    }

    #[test]
    fn usage_stats_respects_top_n() {
        let registry = UsageRegistry::new(UsageConfig::new(vec![(
            "auth".to_string(),
            FixedWindowConfig::new(100, Duration::from_millis(60_000)),
        )]));
        for i in 0..5 {
            registry.check("auth", &format!("ip:{i}"));
        }
        assert_eq!(
            registry
                .usage_stats(2)
                .first()
                .expect("auth present")
                .1
                .top_keys
                .len(),
            2
        );
        assert_eq!(
            registry
                .usage_stats(10)
                .first()
                .expect("auth present")
                .1
                .top_keys
                .len(),
            5
        );
    }

    #[test]
    fn sliding_window_blocks_after_limit() {
        let counter =
            SlidingWindowCounter::new(SlidingWindowConfig::new(2, Duration::from_millis(60_000)));
        assert_eq!(counter.record("alice").unwrap(), 1);
        assert_eq!(counter.record("alice").unwrap(), 2);
        assert!(counter.record("alice").is_err());
    }

    #[test]
    fn sliding_window_prunes_out_of_window() {
        let counter =
            SlidingWindowCounter::new(SlidingWindowConfig::new(2, Duration::from_millis(30)));
        assert_eq!(counter.record("alice").unwrap(), 1);
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(counter.record("alice").unwrap(), 1);
    }

    #[test]
    fn sliding_window_clear_resets_count() {
        let counter =
            SlidingWindowCounter::new(SlidingWindowConfig::new(2, Duration::from_millis(60_000)));
        assert_eq!(counter.record("alice").unwrap(), 1);
        counter.clear("alice");
        assert_eq!(counter.record("alice").unwrap(), 1);
    }
}
