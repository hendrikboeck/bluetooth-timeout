// -- std imports (conditional)
#[cfg(debug_assertions)]
use std::fs;

// -- std imports
use std::{path::PathBuf, sync::OnceLock};

// -- crate imports (conditional)
#[cfg(all(debug_assertions, feature = "tokio-console"))]
use console_subscriber::ConsoleLayer;

// -- crate imports
use anyhow::{Context, Result};
use tracing::warn;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::{filter::LevelFilter, fmt, prelude::*, registry::Registry};

/// Global guard that keeps the non-blocking file writer alive.
///
/// The guard is stored in a [`OnceLock`] so the background worker thread used by the non-blocking
/// logger is not dropped prematurely, which would otherwise cause log records to be lost.
static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// Name of the log file created by the application.
const LOG_FILE_NAME: &str = "bluetooth-timeout.log";

/// Log level used in debug builds.
#[cfg(debug_assertions)]
const LOG_LEVEL: LevelFilter = LevelFilter::DEBUG;

/// Log level used in release builds.
#[cfg(not(debug_assertions))]
const LOG_LEVEL: LevelFilter = LevelFilter::INFO;

/// Returns the path to the log file used by the application.
///
/// In debug builds this is `./bluetooth-timeout.log`. In release builds this uses the XDG data
/// directory.
///
/// # Errors
/// - [`anyhow::Error`] if the XDG data directory cannot be used or created. (!release builds only)
pub fn log_filepath() -> Result<PathBuf> {
    #[cfg(debug_assertions)]
    {
        let path = PathBuf::from(LOG_FILE_NAME);
        let _ = fs::remove_file(&path);
        Ok(path)
    }

    #[cfg(not(debug_assertions))]
    {
        xdg::BaseDirectories::with_prefix("bluetooth-timeout")
            .place_data_file(LOG_FILE_NAME)
            .with_context(|| "Could not determine log file path")
    }
}

/// Builds a non-blocking file writer for tracing logs.
///
/// The returned writer is backed by a file appender that writes to the path returned by
/// [`log_filepath`]. The associated [`WorkerGuard`] is stored in [`LOG_GUARD`] to ensure the
/// background worker thread lives for the entire lifetime of the process.
///
/// # Errors
/// - [`anyhow::Error`] if the log file path cannot be determined or the file appender cannot be
///   created.
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

/// Initializes global tracing with stdout and file logging.
///
/// # Errors
/// - [`anyhow::Error`] if the global tracing subscriber cannot be installed.
pub fn init_tracing() -> Result<()> {
    #[cfg(debug_assertions)]
    let stdout_layer = fmt::layer()
        // .pretty()
        .with_thread_ids(true)
        // .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_target(false)
        .with_filter(LOG_LEVEL);

    #[cfg(not(debug_assertions))]
    let stdout_layer = fmt::layer()
        .with_thread_ids(true)
        // .with_thread_names(true)
        .with_target(false)
        .with_filter(LOG_LEVEL);

    match build_file_writer() {
        Ok(writer) => {
            #[cfg(debug_assertions)]
            let file_layer = fmt::layer()
                .pretty()
                .with_thread_ids(true)
                // .with_thread_names(true)
                .with_file(true)
                .with_line_number(true)
                .with_target(false)
                .with_ansi(false)
                .with_writer(writer)
                .with_filter(LOG_LEVEL);

            #[cfg(not(debug_assertions))]
            let file_layer = fmt::layer()
                .with_thread_ids(true)
                // .with_thread_names(true)
                .with_ansi(false)
                .with_writer(writer)
                .with_target(false)
                .with_filter(LOG_LEVEL);

            #[cfg(all(debug_assertions, feature = "tokio-console"))]
            let subscriber = Registry::default()
                .with(stdout_layer)
                .with(file_layer)
                .with(ConsoleLayer::builder().spawn());

            #[cfg(not(all(debug_assertions, feature = "tokio-console")))]
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
