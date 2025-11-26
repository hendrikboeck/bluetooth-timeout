// -- std imports
use std::sync::OnceLock;
use std::{fs, time::Duration};

// -- crate imports (conditional)
// for some reason, this is flagged as unused
#[cfg(not(debug_assertions))]
#[allow(unused_imports)]
use anyhow::Context;

// -- crate imports
use anyhow::Result;
use tracing::{info, warn};

// -- module imports
use crate::serde_ext::humantime_serde_duration;

/// Global singleton instance of [`Conf`].
static CONF: OnceLock<Conf> = OnceLock::new();

/// Returns the path to the configuration file.
///
/// In debug builds this is `./contrib/config.yml` in the current working directory. In release
/// builds this uses the XDG base directory and resolves to a path like
/// `~/.config/bluetooth-timeout/config.yml`.
///
/// # Errors
/// - [`anyhow::Error`] if the config file path cannot be determined (release builds only).
pub fn conf_filepath() -> Result<String> {
    #[cfg(debug_assertions)]
    {
        Ok("./contrib/config.yml".into())
    }

    #[cfg(not(debug_assertions))]
    {
        xdg::BaseDirectories::with_prefix("bluetooth-timeout")
            .get_config_file("config.yml")
            .map(|path| path.to_string_lossy().to_string())
            .context("Could not determine config file path")
    }
}

/// Application configuration.
///
/// This type is deserialized from a YAML config file and also provides built-in defaults.
#[derive(Debug, PartialEq, Eq, Clone, serde::Deserialize)]
pub struct Conf {
    /// Number of seconds before a timeout is triggered.
    ///
    /// Default: `5m1s`.
    #[serde(deserialize_with = "humantime_serde_duration::deserialize")]
    pub timeout: Duration,

    /// Whether notifications are enabled.
    ///
    /// Default: `true`.
    pub notifications_enabled: bool,

    /// Notifications to be sent at specified durations before the timeout ends.
    ///
    /// Default: `[5m, 1m, 30s, 10s]`.
    #[serde(deserialize_with = "humantime_serde_duration::deserialize_vec")]
    pub notifications_at: Vec<Duration>,

    /// D-Bus related configuration.
    pub dbus: DBusConf,
}

/// D-Bus related configuration.
///
/// This struct is part of the main [`Conf`] struct.
#[derive(Debug, PartialEq, Eq, Clone, serde::Deserialize)]
pub struct DBusConf {
    /// D-Bus service name (usually "org.bluez").
    ///
    /// Default: "org.bluez".
    pub service: String,

    /// D-Bus interface name for Bluetooth adapters.
    ///
    /// Default: "org.bluez.Adapter1".
    pub adapter_iface: String,

    /// D-Bus object path for the Bluetooth adapter to manage.
    ///
    /// Default: "/org/bluez/hci0".
    pub adapter_path: String,

    /// D-Bus interface name for Bluetooth devices.
    ///
    /// Default: "org.bluez.Device1".
    pub device_iface: String,
}

impl Default for Conf {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(301),
            notifications_enabled: true,
            notifications_at: vec![
                Duration::from_mins(5),
                Duration::from_mins(1),
                Duration::from_secs(30),
                Duration::from_secs(10),
            ],
            dbus: DBusConf {
                service: "org.bluez".to_string(),
                adapter_iface: "org.bluez.Adapter1".to_string(),
                device_iface: "org.bluez.Device1".to_string(),
                adapter_path: "/org/bluez/hci0".to_string(),
            },
        }
    }
}

impl Conf {
    /// Loads the configuration from [`conf_filepath`] into the global instance.
    ///
    /// If the path cannot be determined or the file cannot be read or parsed, falls back to
    /// [`Conf::instance`], which uses the default configuration.
    pub fn load() -> &'static Self {
        match conf_filepath() {
            Ok(p) => Self::from_file(&p),
            Err(e) => {
                warn!(
                    "Could not determine config file path: {}. Falling back to defaults.",
                    e
                );
                Self::instance()
            }
        }
    }

    /// Initializes the global configuration from the YAML file at `path`.
    ///
    /// If the configuration is already initialized, the existing instance is returned and the file
    /// is ignored. On any read or parse error, falls back to [`Conf::default`].
    pub fn from_file(path: &str) -> &'static Self {
        if let Some(conf) = CONF.get() {
            warn!(
                "Conf::from_file({}) called, but configuration is already initialized. Using \
                    existing configuration and ignoring the file.",
                path
            );
            return conf;
        }

        CONF.get_or_init(|| {
            fs::read_to_string(path)
                .map_err(|e| {
                    warn!(
                        "Could not read config file '{}': {}. Falling back to defaults.",
                        path, e
                    );
                })
                .and_then(|contents| {
                    serde_yaml::from_str::<Conf>(&contents).map_err(|e| {
                        warn!(
                            "Could not parse config file '{}': {}. Falling back to defaults.",
                            path, e
                        );
                    })
                })
                .map(|conf| {
                    info!("Successfully loaded configuration from '{}'.", path);
                    conf
                })
                .unwrap_or_else(|_| Conf::default())
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
