// -- std imports
use std::sync::OnceLock;
use std::{fs, time::Duration};

// -- crate imports (conditional)
#[cfg(not(debug_assertions))]
#[allow(unused_imports)]
use anyhow::Context;

// -- crate imports
use anyhow::Result;
use tracing::{info, warn};

// -- module imports
use crate::lua_config;

/// Global singleton instance of [`Conf`].
static CONF: OnceLock<Conf> = OnceLock::new();

/// Returns the path to the configuration file.
///
/// In debug builds this is `./contrib/config.lua` in the current working directory. In release
/// builds this uses the XDG base directory and resolves to a path like
/// `~/.config/bluetooth-timeout/config.lua`.
///
/// # Errors
/// - [`anyhow::Error`] if the config file path cannot be determined (release builds only).
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
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RuntimeConf {
    /// Whether to use a multi-threaded runtime.
    ///
    /// Default: `false`.
    pub multithreaded: bool,
}

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

impl Default for RuntimeConf {
    fn default() -> Self {
        Self {
            multithreaded: false,
        }
    }
}

impl Default for Conf {
    fn default() -> Self {
        Self {
            timeout: Duration::from_mins(5),
            notifications: NotificationConf::default(),
            runtime: RuntimeConf::default(),
            adapter_paths: vec!["/org/bluez/hci0".to_string()],
        }
    }
}

impl Conf {
    /// Loads the configuration from the Lua config file into the global instance.
    ///
    /// If the path cannot be determined or the file cannot be read or parsed, falls back to
    /// [`Conf::instance`], which uses the default configuration.
    pub async fn load() -> &'static Self {
        match conf_filepath() {
            Ok(p) => Self::from_file(&p).await,
            Err(e) => {
                warn!(
                    "Could not determine config file path: {}. Falling back to defaults.",
                    e
                );
                Self::instance()
            }
        }
    }

    /// Initializes the global configuration from the Lua file at `path`.
    ///
    /// If the configuration is already initialized, the existing instance is returned and the file
    /// is ignored. On any read or parse error, falls back to [`Conf::default`].
    pub async fn from_file(path: &str) -> &'static Self {
        if let Some(conf) = CONF.get() {
            warn!(
                "Conf::from_file({}) called, but configuration is already initialized. Using \
                    existing configuration and ignoring the file.",
                path
            );
            return conf;
        }

        let contents = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                warn!(
                    "Could not read config file '{}': {}. Falling back to defaults.",
                    path, e
                );
                return Self::instance();
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

        CONF.get_or_init(|| match lua_config::load_config(&contents, &adapters) {
            Ok(conf) => {
                info!("Successfully loaded configuration from '{}'.", path);
                conf
            }
            Err(e) => {
                warn!(
                    "Could not parse config file '{}': {}. Falling back to defaults.",
                    path, e
                );
                Conf::default()
            }
        })
    }

    /// Returns the global configuration instance.
    ///
    /// If the configuration has not been loaded yet, this initializes it with [`Conf::default`]
    /// and logs a warning.
    pub fn instance() -> &'static Self {
        CONF.get_or_init(|| {
            warn!(
                "Conf::instance() called before Conf::from_file(); initializing configuration with \
                default values."
            );
            Conf::default()
        })
    }
}
