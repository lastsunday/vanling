use std::collections::HashMap;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};

use framework::rate_limit::{FixedWindowConfig, UsageConfig};

/// Keying strategy for a rate-limit resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RateLimitKeyBy {
    /// Bucket keyed by peer IP address.
    Ip,
    /// Bucket keyed by the authenticated principal id.
    Principal,
}

/// A named rate-limit resource matched by regex route patterns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RateLimitResourceConfig {
    /// Unique resource name surfaced via `x-ratelimit-resource`.
    pub name: String,
    /// Request quota within `window_secs`.
    pub limit: u32,
    /// Fixed window length in seconds.
    pub window_secs: u64,
    /// Whether the bucket is keyed per peer IP or per principal.
    pub key_by: RateLimitKeyBy,
    /// Regex patterns against the request path. The first resource with a
    /// matching pattern owns the request, so config order is priority.
    pub paths: Vec<String>,
    /// `false` never records requests for this resource.
    #[serde(default = "default_count", skip_serializing_if = "count_is_default")]
    pub count: bool,
}

const fn default_count() -> bool {
    true
}

const fn count_is_default(count: &bool) -> bool {
    *count
}

impl RateLimitResourceConfig {
    pub fn new(
        name: &str,
        limit: u32,
        window_secs: u64,
        key_by: RateLimitKeyBy,
        paths: Vec<String>,
        count: bool,
    ) -> Self {
        Self {
            name: name.to_string(),
            limit,
            window_secs,
            key_by,
            paths,
            count,
        }
    }
}

/// A compiled rate-limit resource ready for request-time matching.
#[derive(Debug, Clone)]
pub struct RateLimitMatcher {
    pub name: String,
    pub limit: u32,
    pub window_secs: u64,
    pub key_by: RateLimitKeyBy,
    pub count: bool,
    regexes: Vec<Regex>,
}

impl RateLimitMatcher {
    /// True when any configured pattern matches `path`.
    pub fn matches(&self, path: &str) -> bool {
        self.regexes.iter().any(|re| re.is_match(path))
    }
}

fn default_rate_limit_resources() -> Vec<RateLimitResourceConfig> {
    vec![
        RateLimitResourceConfig::new(
            "auth",
            20,
            15 * 60,
            RateLimitKeyBy::Ip,
            vec!["^/api/auth/".to_string()],
            true,
        ),
        RateLimitResourceConfig::new(
            "ota",
            30,
            60,
            RateLimitKeyBy::Ip,
            vec!["^/api/ota".to_string()],
            true,
        ),
        RateLimitResourceConfig::new(
            "mcp",
            5000,
            60 * 60,
            RateLimitKeyBy::Principal,
            vec!["^/mcp".to_string()],
            true,
        ),
        RateLimitResourceConfig::new(
            "core",
            5000,
            60 * 60,
            RateLimitKeyBy::Principal,
            vec!["^/api/".to_string()],
            true,
        ),
    ]
}

/// Security subsystem configuration: config-driven per-resource rate limits,
/// the per-account login failure lockout and security event retention.
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub rate_limit_resources: Vec<RateLimitResourceConfig>,
    pub login_fail_limit: u32,
    pub login_fail_window_secs: u64,
    pub event_retention_days: i64,
    pub cleanup_interval_secs: u64,
    pub api_access_log_enabled: bool,
    pub api_access_log_retention_days: i64,
    pub access_log_path_prefixes: Vec<String>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            rate_limit_resources: default_rate_limit_resources(),
            login_fail_limit: 5,
            login_fail_window_secs: 15 * 60,
            event_retention_days: 30,
            cleanup_interval_secs: 6 * 60 * 60,
            api_access_log_enabled: true,
            api_access_log_retention_days: 30,
            access_log_path_prefixes: vec!["/api/".to_string(), "/mcp".to_string()],
        }
    }
}

impl SecurityConfig {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        rate_limit_resources: Vec<RateLimitResourceConfig>,
        login_fail_limit: u32,
        login_fail_window_secs: u64,
        event_retention_days: i64,
        cleanup_interval_secs: u64,
        api_access_log_enabled: bool,
        api_access_log_retention_days: i64,
        access_log_path_prefixes: Vec<String>,
    ) -> Self {
        Self {
            rate_limit_resources,
            login_fail_limit,
            login_fail_window_secs,
            event_retention_days,
            cleanup_interval_secs,
            api_access_log_enabled,
            api_access_log_retention_days,
            access_log_path_prefixes,
        }
    }

    /// Maps the resource quotas onto the [`UsageRegistry`] configuration,
    /// preserving config order.
    pub fn to_usage_config(&self) -> UsageConfig {
        UsageConfig::new(
            self.rate_limit_resources
                .iter()
                .map(|r| {
                    (
                        r.name.clone(),
                        FixedWindowConfig::new(r.limit, Duration::from_secs(r.window_secs)),
                    )
                })
                .collect(),
        )
    }

    /// Compiles every resource's route patterns, failing fast on invalid
    /// names, duplicated names/patterns and unparseable regexes so that
    /// misconfigurations surface at startup instead of runtime.
    pub fn compile_matchers(&self) -> Result<Vec<RateLimitMatcher>, anyhow::Error> {
        let mut names = std::collections::HashSet::new();
        let mut pattern_owners: HashMap<&str, &str> = HashMap::new();
        let mut matchers = Vec::with_capacity(self.rate_limit_resources.len());

        for resource in &self.rate_limit_resources {
            if resource.name.is_empty() {
                return Err(anyhow::anyhow!(
                    "rate limit resource name must not be empty"
                ));
            }
            if !names.insert(resource.name.as_str()) {
                return Err(anyhow::anyhow!(
                    "rate limit resource name '{}' is duplicated",
                    resource.name
                ));
            }
            if resource.limit == 0 {
                return Err(anyhow::anyhow!(
                    "rate limit resource '{}' must have a limit greater than 0",
                    resource.name
                ));
            }
            if resource.window_secs == 0 {
                return Err(anyhow::anyhow!(
                    "rate limit resource '{}' must have a window_secs greater than 0",
                    resource.name
                ));
            }
            if resource.paths.is_empty() {
                return Err(anyhow::anyhow!(
                    "rate limit resource '{}' must have at least one path pattern",
                    resource.name
                ));
            }

            let mut regexes = Vec::with_capacity(resource.paths.len());
            for pattern in &resource.paths {
                let re = Regex::new(pattern).map_err(|e| {
                    anyhow::anyhow!(
                        "rate limit resource '{}' has invalid path pattern '{}': {e}",
                        resource.name,
                        pattern
                    )
                })?;
                if regexes
                    .iter()
                    .any(|existing: &Regex| existing.as_str() == pattern)
                {
                    continue;
                }
                if let Some(owner) = pattern_owners.insert(pattern.as_str(), &resource.name) {
                    return Err(anyhow::anyhow!(
                        "path pattern '{}' is used by both '{}' and '{}'; \
                         only the first resource is reachable",
                        pattern,
                        owner,
                        resource.name
                    ));
                }
                regexes.push(re);
            }

            matchers.push(RateLimitMatcher {
                name: resource.name.clone(),
                limit: resource.limit,
                window_secs: resource.window_secs,
                key_by: resource.key_by,
                count: resource.count,
                regexes,
            });
        }

        Ok(matchers)
    }
}

#[cfg(test)]
mod tests {
    use figment::{
        Figment,
        providers::{Format, Toml},
    };

    use super::*;

    fn resource(
        name: &str,
        limit: u32,
        window_secs: u64,
        key_by: RateLimitKeyBy,
        paths: &[&str],
    ) -> RateLimitResourceConfig {
        RateLimitResourceConfig::new(
            name,
            limit,
            window_secs,
            key_by,
            paths.iter().map(|p| p.to_string()).collect(),
            true,
        )
    }

    #[test]
    fn default_maps_to_usage_config_in_order() {
        let usage = SecurityConfig::default().to_usage_config();
        let names: Vec<&str> = usage.resources.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["auth", "ota", "mcp", "core"]);
        let auth = &usage.resources[0].1;
        assert_eq!(auth.limit, 20);
        assert_eq!(auth.window.as_secs(), 900);
        let ota = &usage.resources[1].1;
        assert_eq!(ota.limit, 30);
        assert_eq!(ota.window.as_secs(), 60);
        let mcp = &usage.resources[2].1;
        assert_eq!(mcp.limit, 5000);
        let core = &usage.resources[3].1;
        assert_eq!(core.limit, 5000);
        assert_eq!(core.window.as_secs(), 3600);
    }

    #[test]
    fn custom_resources_map_to_usage_config() {
        let config = SecurityConfig::new(
            vec![
                resource("login", 10, 60, RateLimitKeyBy::Ip, &["^/api/auth/"]),
                resource("mcp", 20, 120, RateLimitKeyBy::Principal, &["^/mcp"]),
            ],
            3,
            60,
            7,
            3600,
            false,
            7,
            vec!["/mcp".to_string()],
        );
        let usage = config.to_usage_config();
        let names: Vec<&str> = usage.resources.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["login", "mcp"]);
        assert_eq!(usage.resources[0].1.limit, 10);
        assert_eq!(usage.resources[0].1.window.as_secs(), 60);
        assert_eq!(usage.resources[1].1.limit, 20);
        assert!(!config.api_access_log_enabled);
        assert_eq!(config.access_log_path_prefixes, vec!["/mcp"]);
    }

    #[test]
    fn default_access_log_path_prefixes() {
        let config = SecurityConfig::default();
        assert_eq!(
            config.access_log_path_prefixes,
            vec!["/api/".to_string(), "/mcp".to_string()]
        );
    }

    #[test]
    fn default_resources_compile() {
        let matchers = SecurityConfig::default()
            .compile_matchers()
            .expect("defaults must compile");
        let names: Vec<&str> = matchers.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["auth", "ota", "mcp", "core"]);
        assert!(matchers[0].matches("/api/auth/login"));
        assert!(matchers[0].matches("/api/auth/access_token"));
        assert!(matchers[1].matches("/api/ota"));
        assert!(matchers[1].matches("/api/ota/activate"));
        assert!(matchers[2].matches("/mcp"));
        assert!(matchers[3].matches("/api/session"));
        assert!(!matchers[3].matches("/mcp"));
    }

    #[test]
    fn duplicate_resource_name_fails() {
        let config = SecurityConfig::default();
        let mut resources = config.rate_limit_resources.clone();
        resources.push(resource("auth", 1, 60, RateLimitKeyBy::Ip, &["^/x"]));
        let err = SecurityConfig {
            rate_limit_resources: resources,
            ..config
        }
        .compile_matchers()
        .expect_err("duplicate name must fail");
        assert!(err.to_string().contains("duplicated"));
    }

    #[test]
    fn empty_paths_fails() {
        let config = SecurityConfig::default();
        let resources = vec![resource("lonely", 1, 60, RateLimitKeyBy::Ip, &[])];
        let err = SecurityConfig {
            rate_limit_resources: resources,
            ..config
        }
        .compile_matchers()
        .expect_err("empty paths must fail");
        assert!(err.to_string().contains("at least one path pattern"));
    }

    #[test]
    fn invalid_regex_fails_with_resource_context() {
        let config = SecurityConfig::default();
        let resources = vec![resource(
            "broken",
            1,
            60,
            RateLimitKeyBy::Ip,
            &["^/api/[unclosed"],
        )];
        let err = SecurityConfig {
            rate_limit_resources: resources,
            ..config
        }
        .compile_matchers()
        .expect_err("bad regex must fail");
        assert!(err.to_string().contains("broken"));
    }

    #[test]
    fn duplicate_pattern_across_resources_fails() {
        let config = SecurityConfig::default();
        let resources = vec![
            resource("first", 1, 60, RateLimitKeyBy::Ip, &["^/api/auth/"]),
            resource("second", 1, 60, RateLimitKeyBy::Ip, &["^/api/auth/"]),
        ];
        let err = SecurityConfig {
            rate_limit_resources: resources,
            ..config
        }
        .compile_matchers()
        .expect_err("duplicate pattern must fail");
        assert!(err.to_string().contains("used by both"));
    }

    #[test]
    fn resource_config_parses_from_toml() {
        let raw = r#"
            name = "auth"
            limit = 10
            window_secs = 60
            key_by = "ip"
            paths = ["^/api/auth/"]
        "#;
        let config: RateLimitResourceConfig =
            Figment::new().merge(Toml::string(raw)).extract().unwrap();
        assert_eq!(config.name, "auth");
        assert_eq!(config.limit, 10);
        assert_eq!(config.key_by, RateLimitKeyBy::Ip);
        assert!(config.count);
    }

    #[test]
    fn resource_config_defaults_count_to_true() {
        let raw = r#"
            name = "mcp"
            limit = 100
            window_secs = 60
            key_by = "principal"
            paths = ["^/mcp"]
        "#;
        let config: RateLimitResourceConfig =
            Figment::new().merge(Toml::string(raw)).extract().unwrap();
        assert_eq!(config.key_by, RateLimitKeyBy::Principal);
        assert!(config.count);
    }

    #[test]
    fn resource_config_rejects_unknown_key_by() {
        let raw = r#"
            name = "auth"
            limit = 10
            window_secs = 60
            key_by = "user"
            paths = ["^/api/auth/"]
        "#;
        let result: Result<RateLimitResourceConfig, _> =
            Figment::new().merge(Toml::string(raw)).extract();
        assert!(result.is_err());
    }
}
