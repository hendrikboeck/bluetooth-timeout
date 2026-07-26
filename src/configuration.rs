// -- std imports
use std::{fs, path::PathBuf, time::Duration};

// -- crate imports (conditional)
#[cfg(not(debug_assertions))]
use anyhow::Context;

// -- crate imports
use anyhow::Result;
use tracing::{info, warn};

// -- module imports
use crate::lua_config;

/// Major version of the config schema.
///
/// Bumped when the config format changes in a breaking way that requires migration.
/// This is stored as the `M.version` field in the generated Lua config.
pub const VERSION_MAJOR: u32 = 2;

/// Minor version of the config schema.
///
/// Bumped for non-breaking additions to the config template (new comments, new
/// default fields, LSP support files). Does not trigger migration.
pub const VERSION_MINOR: u32 = 0;

/// Returns the path to the configuration directory.
///
/// In debug builds this is `./contrib`. In release builds this uses the XDG base directory and
/// resolves to `~/.config/bluetooth-timeout`.
///
/// # Errors
/// - [`anyhow::Error`] if the config directory cannot be determined (release builds only).
pub fn conf_dirpath() -> Result<PathBuf> {
    let filepath = PathBuf::from(conf_filepath()?);
    Ok(filepath
        .parent()
        .unwrap_or(PathBuf::new().as_path())
        .to_path_buf())
}

/// Returns the path to the configuration file.
///
/// In debug builds this is `./contrib/config.lua` in the current working directory. In release
/// builds this uses the XDG base directory and resolves to a path like
/// `~/.config/bluetooth-timeout/config.lua`.
///
/// # Errors
/// - [`anyhow::Error`] if the config file path cannot be determined (release builds only).
#[allow(clippy::unnecessary_wraps)]
pub fn conf_filepath() -> Result<String> {
    #[cfg(debug_assertions)]
    {
        Ok("./contrib/config.lua".into())
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
    /// Config schema version.
    ///
    /// Default: `2`.
    pub version: u32,

    /// Number of seconds before a timeout is triggered.
    ///
    /// Default: `5m`.
    pub timeout: Duration,

    /// Notification configuration.
    pub notifications: NotificationConf,

    /// Runtime configuration.
    pub runtime: RuntimeConf,

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

/// Runtime configuration.
#[derive(Debug, Default, PartialEq, Eq, Clone)]
pub struct RuntimeConf {
    /// Whether to use a multi-threaded runtime.
    ///
    /// Default: `false`.
    pub multithreaded: bool,
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
            version: VERSION_MAJOR,
            timeout: Duration::from_mins(5),
            notifications: NotificationConf::default(),
            runtime: RuntimeConf::default(),
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
    pub async fn load() -> Self {
        let filepath = match conf_filepath() {
            Ok(p) => p,
            Err(e) => {
                warn!(
                    "Could not determine config file path: {}. Falling back to defaults.",
                    e
                );
                return Self::default();
            }
        };

        let contents = match fs::read_to_string(&filepath) {
            Ok(c) => c,
            Err(e) => {
                warn!(
                    "Could not read config file '{}': {}. Falling back to defaults.",
                    filepath, e
                );
                return Self::default();
            }
        };

        let adapters = match lua_config::discover_adapters().await {
            Ok(a) => a,
            Err(e) => {
                warn!(
                    "Could not discover Bluetooth adapters: {}. Continuing with empty adapter list.",
                    e
                );
                vec![]
            }
        };

        match lua_config::load_config(&contents, adapters) {
            Ok(conf) => {
                info!("Successfully loaded configuration from '{}'.", filepath);
                conf
            }
            Err(e) => {
                warn!(
                    "Could not parse config file '{}': {}. Falling back to defaults.",
                    filepath, e
                );
                Self::default()
            }
        }
    }
}
