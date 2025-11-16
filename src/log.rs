use std::fs;
use std::{path::PathBuf, sync::OnceLock};

use anyhow::{Context, Result};

use tracing::warn;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::{filter::LevelFilter, fmt, prelude::*, registry::Registry};

/// Guard that keeps the non-blocking file writer alive for the entire process lifetime.
///
/// This is stored in a static to prevent the worker thread from being dropped,
/// which would cause logs to be lost.
static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// Name of the log file created by the application.
const LOG_FILE_NAME: &str = "bluetooth-timeout.log";

/// Log level used in debug builds.
#[cfg(debug_assertions)]
const LOG_LEVEL: LevelFilter = LevelFilter::DEBUG;

/// Log level used in release builds.
#[cfg(not(debug_assertions))]
const LOG_LEVEL: LevelFilter = LevelFilter::INFO;

/// Return the path to the log file.
///
/// - debug:  ./bluetooth-timeout.log (current directory)
/// - release: XDG data dir + "bluetooth-timeout/bluetooth-timeout.log"
pub fn log_filepath() -> Result<PathBuf> {
    #[cfg(debug_assertions)]
    {
        let path = PathBuf::from(LOG_FILE_NAME);
        if fs::exists(&path).unwrap_or(false) {
            let _ = fs::remove_file(&path).ok();
        }
        Ok(path)
    }

    #[cfg(not(debug_assertions))]
    {
        xdg::BaseDirectories::with_prefix("bluetooth-timeout").place_data_file(LOG_FILE_NAME)?;
    }
}

/// Build a non-blocking file writer for tracing logs.
///
/// Creates a file appender that writes to the log file path determined by `log_filepath()`.
/// The writer guard is stored in a static `OnceLock` to keep it alive for the process lifetime.
///
/// # Errors
///
/// Returns an error if:
/// - The log file path cannot be determined
/// - The log file directory or name cannot be extracted
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

/// Initialize the tracing subscriber with both stdout and file logging.
///
/// Sets up a dual-output logging system:
/// - Stdout layer: For console output (with ANSI colors in debug mode)
/// - File layer: For persistent logging to disk
///
/// If file logging cannot be initialized, falls back to stdout-only logging
/// and emits a warning.
///
/// # Debug vs Release
///
/// Debug builds include additional metadata (file names, line numbers) and use DEBUG level.
/// Release builds omit extra metadata and use INFO level.
///
/// # Errors
///
/// Returns an error if the global tracing subscriber cannot be set.
pub fn init_tracing() -> Result<()> {
    #[cfg(debug_assertions)]
    let stdout_layer = fmt::layer()
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_target(false)
        .with_filter(LOG_LEVEL);

    #[cfg(not(debug_assertions))]
    let stdout_layer = fmt::layer()
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_ansi(false)
        .with_filter(LOG_LEVEL);

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
                .with_filter(LOG_LEVEL);

            #[cfg(not(debug_assertions))]
            let file_layer = fmt::layer()
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_ansi(false)
                .with_writer(writer)
                .with_filter(LOG_LEVEL);

            let subscriber = Registry::default().with(stdout_layer).with(file_layer);
            tracing::subscriber::set_global_default(subscriber)?;
        }
        Err(e) => {
            let subscriber = Registry::default().with(stdout_layer);
            tracing::subscriber::set_global_default(subscriber)?;

            warn!(
                "File logging could not be initialized. Falling back to stdout only: {}",
                e
            );
        }
    }

    Ok(())
}
