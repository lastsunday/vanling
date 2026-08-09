use std::time::Duration;

use framework::rate_limit::{FixedWindowConfig, UsageConfig};

/// Security subsystem configuration: GitHub-style per-resource rate limits,
/// the per-account login failure lockout and security event retention.
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub auth_limit: u32,
    pub auth_window_secs: u64,
    pub ota_limit: u32,
    pub ota_window_secs: u64,
    pub core_limit: u32,
    pub core_window_secs: u64,
    pub login_fail_limit: u32,
    pub login_fail_window_secs: u64,
    pub event_retention_days: i64,
    pub cleanup_interval_secs: u64,
    pub api_access_log_enabled: bool,
    pub api_access_log_retention_days: i64,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            auth_limit: 20,
            auth_window_secs: 15 * 60,
            ota_limit: 30,
            ota_window_secs: 60,
            core_limit: 5000,
            core_window_secs: 60 * 60,
            login_fail_limit: 5,
            login_fail_window_secs: 15 * 60,
            event_retention_days: 30,
            cleanup_interval_secs: 6 * 60 * 60,
            api_access_log_enabled: true,
            api_access_log_retention_days: 30,
        }
    }
}

impl SecurityConfig {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        auth_limit: u32,
        auth_window_secs: u64,
        ota_limit: u32,
        ota_window_secs: u64,
        core_limit: u32,
        core_window_secs: u64,
        login_fail_limit: u32,
        login_fail_window_secs: u64,
        event_retention_days: i64,
        cleanup_interval_secs: u64,
        api_access_log_enabled: bool,
        api_access_log_retention_days: i64,
    ) -> Self {
        Self {
            auth_limit,
            auth_window_secs,
            ota_limit,
            ota_window_secs,
            core_limit,
            core_window_secs,
            login_fail_limit,
            login_fail_window_secs,
            event_retention_days,
            cleanup_interval_secs,
            api_access_log_enabled,
            api_access_log_retention_days,
        }
    }

    /// Maps the resource quotas onto the [`UsageRegistry`] configuration.
    pub fn to_usage_config(&self) -> UsageConfig {
        UsageConfig::new(
            FixedWindowConfig::new(self.auth_limit, Duration::from_secs(self.auth_window_secs)),
            FixedWindowConfig::new(self.ota_limit, Duration::from_secs(self.ota_window_secs)),
            FixedWindowConfig::new(self.core_limit, Duration::from_secs(self.core_window_secs)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_maps_to_current_usage_config() {
        let usage = SecurityConfig::default().to_usage_config();
        assert_eq!(usage.auth.limit, 20);
        assert_eq!(usage.auth.window.as_secs(), 900);
        assert_eq!(usage.ota.limit, 30);
        assert_eq!(usage.ota.window.as_secs(), 60);
        assert_eq!(usage.core.limit, 5000);
        assert_eq!(usage.core.window.as_secs(), 3600);
    }

    #[test]
    fn new_maps_custom_limits() {
        let config = SecurityConfig::new(10, 60, 20, 120, 30, 180, 3, 60, 7, 3600, false, 7);
        let usage = config.to_usage_config();
        assert_eq!(usage.auth.limit, 10);
        assert_eq!(usage.auth.window.as_secs(), 60);
        assert_eq!(usage.ota.limit, 20);
        assert_eq!(usage.core.limit, 30);
        assert!(!config.api_access_log_enabled);
    }
}
