use tracing::{debug, info};

mod bluetooth;
mod configuration;
mod log;

use crate::bluetooth::observer::BluetoothEventObserver;
use crate::configuration::Conf;

#[tokio::main]
async fn main() {
    log::init_tracing().expect("Could not initialize tracing");
    debug!("Tracing initialized");

    let conf = Conf::load();
    debug!("Configuration:\n{:#?}", conf);

    let observer = BluetoothEventObserver::new(conf.adapter_path.clone())
        .await
        .expect("Could not create Bluetooth observer");

    let mut rx = observer.subscribe();
    observer.listen();

    loop {
        let event = rx.recv().await.expect("Bluetooth observer channel closed");
        info!("Received Bluetooth event: {:#?}", event);
    }
}
