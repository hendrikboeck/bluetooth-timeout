use std::time::Duration;

use serde::Deserialize;

use crate::configuration::{Conf, NotificationConf, RuntimeConf, VERSION_MAJOR};

/// Deserialized form of the legacy YAML config file.
///
/// All fields are optional to handle the evolution of the YAML format across pre-2.0 releases:
///   - Early versions used `timeout_s` (bare seconds), later switched to `timeout` (humantime).
///   - The `notifications` section was added in a later release.
///   - `dbus` contained `BlueZ` interface strings that are now hard-coded; only `adapter_path` is kept.
#[derive(Debug, Deserialize)]
pub struct YamlConfig {
    #[serde(alias = "timeout_s")]
    /// Inactivity timeout as a humantime string or raw seconds.
    timeout: Option<String>,
    /// Notification settings (optional, defaults apply).
    notifications: Option<YamlNotifications>,
    /// D-Bus adapter path configuration (optional, hci0 by default).
    dbus: Option<YamlDbus>,
}

/// Notification section of the legacy YAML config.
#[derive(Debug, Deserialize)]
struct YamlNotifications {
    /// Whether desktop notifications are enabled.
    enabled: Option<bool>,
    /// Warning intervals as a list of humantime strings.
    at: Option<Vec<String>>,
}

/// D-Bus adapter section of the legacy YAML config.
#[derive(Debug, Deserialize)]
struct YamlDbus {
    /// Object path of the Bluetooth adapter (e.g., `/org/bluez/hci0`).
    adapter_path: Option<String>,
}

/// Accessors for extracting values from the parsed YAML with sensible defaults.
impl YamlConfig {
    /// Extract the timeout value and normalise it to a humantime string.
    ///
    /// Raw-second values (legacy `timeout_s`) are converted: multiples of 60 become `"Nm"`,
    /// other values stay as `"Ns"`. Already-humantime strings pass through unchanged.
    fn timeout_str(&self) -> String {
        if let Some(ref t) = self.timeout {
            if let Ok(secs) = t.parse::<u64>() {
                if secs >= 60 && secs % 60 == 0 {
                    return format!("{}m", secs / 60);
                }
                return format!("{secs}s");
            }
            return t.clone();
        }
        "5m".into()
    }

    /// Extract the adapter D-Bus path, falling back to the default hci0 path.
    fn adapter_path(&self) -> &str {
        self.dbus
            .as_ref()
            .and_then(|d| d.adapter_path.as_deref())
            .unwrap_or("/org/bluez/hci0")
    }

    /// Whether notifications were enabled in the YAML config (default true).
    fn notifications_enabled(&self) -> bool {
        self.notifications
            .as_ref()
            .is_none_or(|n| n.enabled.unwrap_or(true))
    }

    /// Warning intervals from the YAML config, or sensible defaults.
    fn notifications_at(&self) -> Vec<String> {
        self.notifications
            .as_ref()
            .and_then(|n| n.at.clone())
            .unwrap_or_else(|| vec!["5m".into(), "1m".into(), "30s".into(), "10s".into()])
    }
}

impl YamlConfig {
    /// Convert this YAML config into a [`Conf`].
    ///
    /// Missing fields are filled with sensible defaults.
    pub fn into_conf(self) -> Conf {
        let timeout_str = self.timeout_str();
        let timeout = humantime::parse_duration(&timeout_str).unwrap_or(Duration::from_secs(300));

        let at_strs = self.notifications_at();
        let at = at_strs
            .iter()
            .map(|s| humantime::parse_duration(s).unwrap_or(Duration::from_secs(60)))
            .collect();

        Conf {
            version: VERSION_MAJOR,
            timeout,
            notifications: NotificationConf {
                enabled: self.notifications_enabled(),
                at,
            },
            runtime: RuntimeConf::default(),
            adapter_paths: vec![self.adapter_path().to_string()],
        }
    }
}
