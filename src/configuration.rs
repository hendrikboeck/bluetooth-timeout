// -- std imports
use std::{fs, time::Duration};

// -- crate imports (conditional)
#[cfg(not(debug_assertions))]
use anyhow::Context;

// -- crate imports
use anyhow::Result;
use tap::TapFallible;
use tracing::{info, warn};

// -- module imports
use crate::lua_config;

/// Returns the path to the configuration file.
///
/// In debug builds this is `.local/config/config.lua`. In release builds this uses the XDG base
/// directory and resolves to a path like `~/.config/bluetooth-timeout/config.lua`.
///
/// # Errors
/// - [`anyhow::Error`] if the config file path cannot be determined (release builds only).
#[allow(clippy::unnecessary_wraps)]
pub fn conf_filepath() -> Result<String> {
    #[cfg(debug_assertions)]
    {
        Ok(".local/config/config.lua".into())
    }

    #[cfg(not(debug_assertions))]
    {
        const APP_ID: &str = env!("CARGO_PKG_NAME");

        xdg::BaseDirectories::with_prefix(APP_ID)
            .get_config_file("config.lua")
            .map(|path| path.to_string_lossy().to_string())
            .context("Could not determine config file path")
    }
}

/// Application configuration.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Conf {
    /// Number of seconds before a timeout is triggered.
    ///
    /// Default: `5m`.
    pub timeout: Duration,

    /// Notification configuration.
    pub notifications: NotificationConf,

    /// D-Bus object paths of the Bluetooth adapters to manage.
    ///
    /// Default: `["/org/bluez/hci0"]`.
    pub adapter_paths: Vec<String>,
}

/// Notification configuration.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct NotificationConf {
    /// Whether notifications are enabled.
    ///
    /// Default: `true`.
    pub enabled: bool,

    /// Notifications to be sent at specified durations before the timeout ends.
    ///
    /// Default: `[5m, 1m, 30s, 10s]`.
    pub at: Vec<Duration>,
}

/// Default notification configuration: enabled with standard warning intervals.
impl Default for NotificationConf {
    fn default() -> Self {
        Self {
            enabled: true,
            at: vec![
                Duration::from_mins(5),
                Duration::from_mins(1),
                Duration::from_secs(30),
                Duration::from_secs(10),
            ],
        }
    }
}

/// Default configuration: 5m timeout, hci0 adapter, notifications enabled.
impl Default for Conf {
    fn default() -> Self {
        Self {
            timeout: Duration::from_mins(5),
            notifications: NotificationConf::default(),
            adapter_paths: vec!["/org/bluez/hci0".to_string()],
        }
    }
}

/// Configuration loading and lifecycle.
impl Conf {
    /// Loads the configuration from the Lua config file.
    ///
    /// If the path cannot be determined or the file cannot be read or parsed, falls back to
    /// [`Conf::default`].
    ///
    /// # Errors
    ///
    /// Returns an error when no Bluetooth adapters can be discovered, so the caller can exit and
    /// let the systemd service restart the daemon (`Restart=on-failure`).
    pub async fn load() -> Result<Self> {
        let filepath = conf_filepath()
            .tap_err(|e| {
                warn!("Could not determine config file path: {e}. Falling back to defaults.");
            })
            .unwrap_or_default();

        let contents = fs::read_to_string(&filepath)
            .tap_err(|e| {
                warn!("Could not read config file '{filepath}': {e}. Falling back to defaults.");
            })
            .unwrap_or_default();

        let adapters = lua_config::discover_adapters().await?;

        let conf = lua_config::load_config(&contents, adapters)
            .tap_ok(|_| info!("Successfully loaded configuration from '{filepath}'."))
            .tap_err(|e| {
                warn!("Could not load config file '{filepath}': {e}. Falling back to defaults.");
            })
            .unwrap_or_default();

        Ok(conf)
    }
}
