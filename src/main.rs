//! Automatically powers off idle Bluetooth adapters after a configurable inactivity timeout.
//!
//! Monitors Bluetooth adapter state via D-Bus, sends desktop notifications at configurable
//! intervals before the timeout expires, and powers off the adapter when the timer elapses.

#![allow(clippy::multiple_crate_versions)]

// -- module definitions
/// Bluetooth D-Bus integration (observer, service, device types).
mod bluetooth;
/// Application configuration (Conf struct, loading, defaults).
mod configuration;
/// Tracing and logging initialisation.
mod log;
/// Lua-based config parsing and code generation.
mod lua_config;
/// Config schema migration (version detection, YAML→Lua conversion).
mod migration;
/// Desktop notification sending via D-Bus.
mod notification;
/// Inactivity timeout task with warning notifications.
mod timeout;

// -- crate imports
use clap::{Parser, Subcommand};
#[cfg(debug_assertions)]
use clap::ArgAction;
use tracing::{debug, error, info};
#[cfg(debug_assertions)]
use tracing_subscriber::filter::LevelFilter;

// -- module imports
use crate::{
    bluetooth::{observer::BluetoothEventObserver, service::BluetoothService},
    configuration::{Conf, VERSION_MAJOR, VERSION_MINOR},
};

/// CLI argument parser for the bluetooth-timeout daemon.
#[derive(Parser)]
#[command(
    name = "bluetooth-timeout",
    about = "Bluetooth inactivity timeout daemon",
    version
)]
struct Cli {
    /// Increase logging verbosity: -v = DEBUG, -vv = TRACE.
    #[cfg(debug_assertions)]
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count, global = true)]
    verbose: u8,

    /// Optional subcommand (migrate, or none to run the daemon).
    #[command(subcommand)]
    command: Option<Commands>,
}

/// Available CLI subcommands.
#[derive(Subcommand)]
enum Commands {
    /// Migrate the configuration to the latest format version.
    Migrate,
}

/// Entry point: parses CLI args and dispatches to the appropriate subcommand
/// or runs the daemon.
fn main() {
    let cli = Cli::parse();
    #[cfg(debug_assertions)]
    let verbosity = match cli.verbose {
        0 => None,
        1 => Some(LevelFilter::DEBUG),
        _ => Some(LevelFilter::TRACE),
    };
    #[cfg(not(debug_assertions))]
    let verbosity = None;

    log::init_tracing(verbosity).expect("Could not initialize tracing");
    debug!("Tracing initialized");

    match cli.command {
        Some(Commands::Migrate) => {
            let result = run_migrate();
            if let Err(e) = result {
                eprintln!("Migration failed: {e}");
                std::process::exit(1);
            }
        }
        None => run_daemon(),
    }
}

/// Runs the config migration subcommand.
///
/// Detects the current config version, performs any necessary migrations,
/// installs LSP support files, and reports the result to stdout.
fn run_migrate() -> anyhow::Result<()> {
    let config_dir = configuration::conf_dirpath()?;
    let result = migration::migrate(&config_dir)?;

    match result {
        migration::MigrationResult::UpToDate => {
            info!("Configuration is already up to date (version {VERSION_MAJOR}.{VERSION_MINOR}).");
        }
        migration::MigrationResult::Migrated {
            from_version,
            to_version,
        } => {
            info!("Migrated configuration from version {from_version} to version {to_version}.");
        }
        migration::MigrationResult::Created => {
            info!(
                "Created new configuration at {}",
                config_dir.join("config.lua").display()
            );
        }
    }

    Ok(())
}

/// Initialises tracing, loads configuration, performs version compatibility
/// checks, and starts the async event loop.
fn run_daemon() {
    let bootstrap_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Could not create bootstrap runtime");

    let conf = bootstrap_rt.block_on(Conf::load());
    if conf.version > VERSION_MAJOR {
        error!(
            "Config version {} is newer than supported version {VERSION_MAJOR}.{VERSION_MINOR}. \
             Upgrade bluetooth-timeout.",
            conf.version,
        );
        std::process::exit(1);
    }
    if conf.version < VERSION_MAJOR {
        info!(
            "Config version {} is older than current version {VERSION_MAJOR}.{VERSION_MINOR}. \
             Run 'bluetooth-timeout migrate' to upgrade.",
            conf.version,
        );
    }
    debug!("Configuration:\n{:#?}", conf);
    drop(bootstrap_rt);

    let rt = if conf.runtime.multithreaded {
        tokio::runtime::Runtime::new().expect("Could not create multi-threaded runtime")
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Could not create single-threaded runtime")
    };
    rt.block_on(async_main(conf));
}

/// Main async event loop.
///
/// For each configured adapter, spawns a [`BluetoothEventObserver`] and a
/// [`BluetoothService`], then blocks on Ctrl+C.
async fn async_main(conf: Conf) {
    for adapter_path in &conf.adapter_paths {
        let observer = match BluetoothEventObserver::new(adapter_path.clone()).await {
            Ok(o) => o,
            Err(e) => {
                error!(
                    "Could not create Bluetooth observer for {}: {}",
                    adapter_path, e
                );
                continue;
            }
        };

        let rx = observer.subscribe();
        observer.listen();

        let mut bt_service = match BluetoothService::new(
            adapter_path.clone(),
            conf.timeout,
            conf.notifications.clone(),
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "Could not create Bluetooth service for {}: {}",
                    adapter_path, e
                );
                continue;
            }
        };

        let adapter_path = adapter_path.clone();
        tokio::spawn(async move {
            if let Err(e) = bt_service.subscribe_to(rx).start().await {
                error!("Bluetooth service for {} failed: {}", adapter_path, e);
            }
        });
    }

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");
}
