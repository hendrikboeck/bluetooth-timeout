//! Automatically powers off idle Bluetooth adapters after a configurable inactivity timeout.
//!
//! Monitors Bluetooth adapter state via D-Bus, sends desktop notifications at configurable
//! intervals before the timeout expires, and powers off the adapter when the timer elapses.

// -- module definitions
/// Bluetooth D-Bus integration (observer, service, device types).
mod bluetooth;
/// Application configuration (Conf struct, loading, defaults).
mod configuration;
/// Tracing and logging initialisation.
mod log;
/// Lua-based config parsing.
mod lua_config;
/// Desktop notification sending via D-Bus.
mod notification;
/// Inactivity timeout task with warning notifications.
mod timeout;

// -- std imports
use core::panic;
use std::backtrace::Backtrace;

// -- crate imports
use tracing::{debug, error};

// -- module imports
use bluetooth::{observer::BluetoothEventObserver, service::BluetoothService};
use configuration::Conf;

/// Entry point: parses CLI args, loads configuration, and runs the daemon
/// on a single-threaded tokio runtime.
#[tokio::main(flavor = "current_thread")]
async fn main() {
    // Panics in spawned tasks are otherwise swallowed by Tokio. Install a hook that prints a
    // stack trace and exits non-zero so systemd (`Restart=on-failure`) restarts the daemon.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("{info}\nStack trace:\n{}", Backtrace::force_capture());
        std::process::exit(1);
    }));

    log::init_tracing().expect("Could not initialize tracing");
    debug!("Tracing initialized");

    let conf = Conf::load().await.unwrap_or_else(|e| {
        panic!("Failed to load configuration: {e}");
    });
    debug!("Configuration:\n{:#?}", conf);

    let observer = BluetoothEventObserver::new().await.unwrap_or_else(|e| {
        panic!("Could not create Bluetooth observer: {e}");
    });
    observer.listen();

    for adapter_path in &conf.adapter_paths {
        let rx = observer.subscribe();

        let mut bt_service = match BluetoothService::new(
            adapter_path.clone(),
            conf.timeout,
            conf.notifications.clone(),
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                error!("Could not create Bluetooth service for {adapter_path}: {e}");
                continue;
            }
        };

        let adapter_path = adapter_path.clone();
        tokio::spawn(async move {
            if let Err(e) = bt_service.start(rx).await {
                error!("Bluetooth service for {adapter_path} failed: {e}");
            }
        });
    }

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");
}
