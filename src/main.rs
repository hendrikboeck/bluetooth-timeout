use std::time::Duration;

use tracing::debug;

mod bluetooth;
mod configuration;
mod log;
mod timeout;

use crate::bluetooth::{observer::BluetoothEventObserver, service::BluetoothService};
use crate::configuration::Conf;

#[tokio::main]
async fn main() {
    log::init_tracing().expect("Could not initialize tracing");
    debug!("Tracing initialized");

    let conf = Conf::load();
    debug!("Configuration:\n{:#?}", conf);

    let observer = BluetoothEventObserver::new(conf.dbus.adapter_path.clone())
        .await
        .expect("Could not create Bluetooth observer");

    let rx = observer.subscribe();
    observer.listen();

    let mut bt_service = BluetoothService::new(
        conf.dbus.adapter_path.clone(),
        Duration::from_secs(conf.timeout_s),
    )
    .await
    .expect("Could not create Bluetooth service");

    bt_service
        .subscribe_to(rx)
        .start()
        .await
        .expect("Bluetooth service failed");
}
