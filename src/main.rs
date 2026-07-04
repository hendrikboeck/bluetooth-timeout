// -- module definitions
mod bluetooth;
mod configuration;
mod log;
mod lua_config;
mod notification;
mod timeout;

// -- crate imports
use tracing::{debug, error};

// -- module imports
use crate::{
    bluetooth::{observer::BluetoothEventObserver, service::BluetoothService},
    configuration::Conf,
};

fn main() {
    log::init_tracing().expect("Could not initialize tracing");
    debug!("Tracing initialized");

    let bootstrap_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Could not create bootstrap runtime");

    let conf = bootstrap_rt.block_on(Conf::load());
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

async fn async_main(conf: &'static Conf) {
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

        let mut bt_service =
            match BluetoothService::new(adapter_path.clone(), conf.timeout.clone()).await {
                Ok(s) => s,
                Err(e) => {
                    error!(
                        "Could not create Bluetooth service for {}: {}",
                        adapter_path, e
                    );
                    continue;
                }
            };

        tokio::spawn(async move {
            if let Err(e) = bt_service.subscribe_to(rx).start().await {
                error!("Bluetooth service for {} failed: {}", adapter_path, e);
            }
        });
    }

    // Wait forever (signal handler or similar would go here)
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");
}
