use std::{path::PathBuf, sync::OnceLock};

use std::fs;
use tracing::{Level, info, warn};

use anyhow::{Context, Result};
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{filter::LevelFilter, fmt, prelude::*, registry::Registry};

static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// Return the path to the log file.
///
/// - debug:  ./bluetooth-timeout.log (current directory)
/// - release: XDG data dir + "bluetooth-timeout/bluetooth-timeout.log"
pub fn log_filepath() -> Result<PathBuf> {
    #[cfg(debug_assertions)]
    {
        Ok(PathBuf::from("./bluetooth-timeout.log"))
    }

    #[cfg(not(debug_assertions))]
    {
        xdg::BaseDirectories::with_prefix("bluetooth-timeout")
            .place_data_file("bluetooth-timeout.log")
            .map_err(|e| anyhow::anyhow!("Could not determine log file path: {}", e))
    }
}

fn build_file_writer() -> anyhow::Result<NonBlocking> {
    let path = log_filepath()?;
    let file_appender = tracing_appender::rolling::never(
        path.parent()
            .context("Could not determine log file directory")?,
        path.file_name()
            .context("Could not determine log file name")?,
    );
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    let _ = LOG_GUARD.set(guard);

    Ok(file_writer)
}

pub fn init_tracing() {
    #[cfg(debug_assertions)]
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_target(false);

    #[cfg(not(debug_assertions))]
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_ansi(false);

    let registry = tracing_subscriber::registry().with(stdout_layer);

    #[cfg(debug_assertions)]
    match build_file_writer() {
        Ok(writer) => {
            let subscriber = registry.with(
                fmt::layer()
                    .with_thread_ids(true)
                    .with_thread_names(true)
                    .with_file(true)
                    .with_line_number(true)
                    .with_target(false)
                    .with_ansi(false)
                    .with_writer(writer)
                    .with_filter(LevelFilter::DEBUG),
            );
            tracing::subscriber::set_global_default(subscriber)
                .expect("Failed to initialize global tracing subscriber");
        }
        Err(e) => {
            tracing::subscriber::set_global_default(registry)
                .expect("Failed to initialize global tracing subscriber");
            warn!(
                "Could not initialize file logging: {}. Logging only to stdout/stderr.",
                e
            );
        }
    }

    #[cfg(not(debug_assertions))]
    {
        match build_file_writer() {
            Ok(writer) => {
                let subscriber = registry.with(
                    fmt::layer()
                        .with_thread_ids(true)
                        .with_thread_names(true)
                        .with_ansi(false)
                        .with_writer(writer)
                        .with_filter(LevelFilter::INFO),
                );
                tracing::subscriber::set_global_default(subscriber)
                    .expect("Failed to initialize global tracing subscriber");
            }
            Err(e) => {
                tracing::subscriber::set_global_default(registry)
                    .expect("Failed to initialize global tracing subscriber");
                warn!(
                    "Could not initialize file logging: {}. Logging only to stdout/stderr.",
                    e
                );
            }
        }
    }
}

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
