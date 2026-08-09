use figment::Figment;
use std::path::Path;
use tracing::{debug, error, info, warn};

use super::DEPRECATED_KEYS;
use super::security::SecurityConfig;
use crate::Config;

#[allow(clippy::cognitive_complexity)]
pub fn check(config: &Config) -> Result<(), anyhow::Error> {
    if cfg!(debug_assertions) {
        warn!("Note: vanling was built without optimisations (i.e. debug build)");
    }

    warn_deprecated(config);
    warn_unknown_key(config);

    if config.get_bind_ports().is_empty() {
        return Err(anyhow::anyhow!(
            "config port : No ports were specified to listen on"
        ));
    }

    check_security_config(config)?;

    config.get_bind_addrs().iter().for_each(|addr| {
        if addr.ip().is_loopback() {
            info!(
                "Found loopback listening address {addr}, running checks if we're in a \
					 container."
            );

            if Path::new("/proc/vz").exists() /* Guest */ && !Path::new("/proc/bz").exists()
            /* Host */
            {
                error!(
                    "You are detected using OpenVZ with a loopback/localhost listening \
						 address of {addr}. If you are using OpenVZ for containers and you use \
						 NAT-based networking to communicate with the host and guest, this will \
						 NOT work. Please change this to \"0.0.0.0\". If this is expected, you \
						 can ignore.",
                );
            } else if Path::new("/.dockerenv").exists() {
                error!(
                    "You are detected using Docker with a loopback/localhost listening \
						 address of {addr}. If you are using a reverse proxy on the host and \
						 require communication to vanling in the Docker container via \
						 NAT-based networking, this will NOT work. Please change this to \
						 \"0.0.0.0\". If this is expected, you can ignore.",
                );
            } else if Path::new("/run/.containerenv").exists() {
                error!(
                    "You are detected using Podman with a loopback/localhost listening \
						 address of {addr}. If you are using a reverse proxy on the host and \
						 require communication to vanling in the Podman container via \
						 NAT-based networking, this will NOT work. Please change this to \
						 \"0.0.0.0\". If this is expected, you can ignore.",
                );
            }
        }
    });
    Ok(())
}

/// Validates that configured security limits and windows are positive and that
/// every rate-limit resource compiles.
fn check_security_config(config: &Config) -> Result<(), anyhow::Error> {
    let limits: [(&str, Option<u32>); 1] = [(
        "security_login_fail_limit",
        config.security_login_fail_limit,
    )];
    for (name, value) in limits {
        if value.is_some_and(|v| v == 0) {
            return Err(anyhow::anyhow!("config {name} must be greater than 0"));
        }
    }

    let windows: [(&str, Option<u64>); 2] = [
        (
            "security_login_fail_window_secs",
            config.security_login_fail_window_secs,
        ),
        (
            "security_cleanup_interval_secs",
            config.security_cleanup_interval_secs,
        ),
    ];
    for (name, value) in windows {
        if value.is_some_and(|v| v == 0) {
            return Err(anyhow::anyhow!("config {name} must be greater than 0"));
        }
    }

    let mut security = SecurityConfig::default();
    if let Some(resources) = config.security_rate_limit_resources.clone() {
        security.rate_limit_resources = resources;
    }
    security.compile_matchers()?;

    Ok(())
}

/// Iterates over all the keys in the config file and warns if there is a
/// deprecated key specified
fn warn_deprecated(config: &Config) {
    debug!("Checking for deprecated config keys");
    let mut was_deprecated = false;
    for key in config
        .catchall
        .keys()
        .filter(|key| DEPRECATED_KEYS.iter().any(|s| s == key))
    {
        warn!("Config parameter \"{}\" is deprecated, ignoring.", key);
        was_deprecated = true;
    }

    if was_deprecated {
        warn!(
            "Read vanling config documentation and check your \
			 configuration if any new configuration parameters should be adjusted"
        );
    }
}

/// iterates over all the catchall keys (unknown config options) and warns
/// if there are any.
fn warn_unknown_key(config: &Config) {
    debug!("Checking for unknown config keys");
    for key in config.catchall.keys().filter(
        |key| "config".to_owned().ne(key.to_owned()), /* "config" is expected */
    ) {
        warn!(
            "Config parameter \"{}\" is unknown to vanling, ignoring.",
            key
        );
    }
}

/// Checks the presence of the `address` and `unix_socket_path` keys in the
/// raw_config, exiting the process if both keys were detected.
pub(super) fn is_dual_listening(raw_config: &Figment) -> Result<(), anyhow::Error> {
    let contains_address = raw_config.contains("address");
    let contains_unix_socket = raw_config.contains("unix_socket_path");
    if contains_address && contains_unix_socket {
        return Err(anyhow::anyhow!(
            "TOML keys \"address\" and \"unix_socket_path\" were both defined. Please specify \
			 only one option."
        ));
    }

    Ok(())
}
