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
/// Lua-based config parsing.
mod lua_config;
/// Desktop notification sending via D-Bus.
mod notification;
/// Inactivity timeout task with warning notifications.
mod timeout;

// -- crate imports
use tracing::{debug, error};

// -- module imports
use crate::{
    bluetooth::{observer::BluetoothEventObserver, service::BluetoothService},
    configuration::Conf,
};

/// Entry point: parses CLI args, loads configuration, and runs the daemon
/// on a single-threaded tokio runtime.
#[tokio::main(flavor = "current_thread")]
async fn main() {
    log::init_tracing().expect("Could not initialize tracing");
    debug!("Tracing initialized");

    let conf = Conf::load().await;
    debug!("Configuration:\n{:#?}", conf);

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
