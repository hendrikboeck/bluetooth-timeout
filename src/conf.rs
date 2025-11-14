use std::sync::OnceLock;

use std::fs;
use tracing::{Level, info, warn};

pub fn init_tracing() {
    #[cfg(debug_assertions)]
    let fmt = tracing_subscriber::fmt()
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_max_level(Level::DEBUG)
        .with_target(false);

    #[cfg(not(debug_assertions))]
    let fmt = tracing_subscriber::fmt()
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_ansi(false)
        .with_max_level(Level::INFO);

    tracing::subscriber::set_global_default(fmt.finish())
        .expect("Failed to initialize global tracing subscriber");
}

pub fn conf_filepath() -> Option<String> {
    xdg::BaseDirectories::with_prefix("bluetooth-timeout")
        .place_config_file("config.yml")
        .map(|path| path.to_string_lossy().to_string())
        .ok()
}

static CONF: OnceLock<Conf> = OnceLock::new();

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
            Some(p) => Self::from_file(&p),
            None => Self::instance(),
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
