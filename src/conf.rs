use std::sync::OnceLock;

use std::fs;
use tracing::{info, warn};

static CONF: OnceLock<Conf> = OnceLock::new();

pub fn conf_filepath() -> anyhow::Result<String> {
    #[cfg(debug_assertions)]
    {
        Ok("./config.yml".into())
    }

    #[cfg(not(debug_assertions))]
    {
        xdg::BaseDirectories::with_prefix("bluetooth-timeout")
            .get_config_file("config.yml")
            .map(|path| path.to_string_lossy().to_string())
            .context("Could not determine config file path")
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct Conf {
    timeout_seconds: u64,
    adapter_path: String,
}

impl Default for Conf {
    fn default() -> Self {
        Self {
            timeout_seconds: 300, // 5 minutes
            adapter_path: "/org/bluez/hci0".to_string(),
        }
    }
}

impl Conf {
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
