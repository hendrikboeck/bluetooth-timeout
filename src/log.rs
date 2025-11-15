use std::{path::PathBuf, sync::OnceLock};

#[cfg(not(debug_assertions))]
use anyhow::anyhow;
use anyhow::{Context, Result};

use tracing::warn;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::{filter::LevelFilter, fmt, prelude::*, registry::Registry};

static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

const LOG_FILE_NAME: &str = "bluetooth-timeout.log";

/// Return the path to the log file.
///
/// - debug:  ./bluetooth-timeout.log (current directory)
/// - release: XDG data dir + "bluetooth-timeout/bluetooth-timeout.log"
pub fn log_filepath() -> Result<PathBuf> {
    #[cfg(debug_assertions)]
    {
        Ok(PathBuf::from(LOG_FILE_NAME))
    }

    #[cfg(not(debug_assertions))]
    {
        xdg::BaseDirectories::with_prefix("bluetooth-timeout")
            .and_then(|xdg| xdg.place_data_file(LOG_FILE_NAME))
            .map_err(|e| anyhow!("Could not determine log file path: {e}"))
    }
}

fn build_file_writer() -> Result<NonBlocking> {
    let path = log_filepath()?;

    let dir = path
        .parent()
        .context("Could not determine log file directory")?;
    let file_name = path
        .file_name()
        .context("Could not determine log file name")?;

    let file_appender = tracing_appender::rolling::never(dir, file_name);
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);

    // Keep guard alive for entire process
    let _ = LOG_GUARD.set(guard);

    Ok(file_writer)
}

pub fn init_tracing() {
    #[cfg(debug_assertions)]
    let log_level = LevelFilter::DEBUG;
    #[cfg(not(debug_assertions))]
    let log_level = LevelFilter::INFO;

    #[cfg(debug_assertions)]
    let stdout_layer = fmt::layer()
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_target(false)
        .with_filter(log_level);

    #[cfg(not(debug_assertions))]
    let stdout_layer = fmt::layer()
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_ansi(false)
        .with_filter(log_level);

    match build_file_writer() {
        Ok(writer) => {
            #[cfg(debug_assertions)]
            let file_layer = fmt::layer()
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_file(true)
                .with_line_number(true)
                .with_target(false)
                .with_ansi(false) // no ANSI in file
                .with_writer(writer)
                .with_filter(log_level);

            #[cfg(not(debug_assertions))]
            let file_layer = fmt::layer()
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_ansi(false)
                .with_writer(writer)
                .with_filter(log_level);

            let subscriber = Registry::default().with(stdout_layer).with(file_layer);

            tracing::subscriber::set_global_default(subscriber)
                .expect("Could not set global tracing subscriber with file logging");
        }
        Err(e) => {
            let subscriber = Registry::default().with(stdout_layer);

            tracing::subscriber::set_global_default(subscriber)
                .expect("Could not set global tracing subscriber without file logging");

            warn!(
                "File logging could not be initialized. Falling back to stdout only: {}",
                e
            );
        }
    }
}
