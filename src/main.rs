use std::time::Duration;

use tokio::time::sleep;
use tracing::{debug, info};

mod bluetooth;
mod configuration;
mod log;

use crate::bluetooth::{
    observer::{BluetoothEvent, BluetoothEventObserver},
    service_proxy::BluetoothServiceProxy,
};
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

    let service_proxy = BluetoothServiceProxy::new(conf.adapter_path.clone())
        .await
        .expect("Could not create Bluetooth service proxy");
    let devices = service_proxy
        .get_devices()
        .await
        .expect("Could not get Bluetooth devices");
    info!("Discovered Bluetooth devices:\n{:#?}", devices);

    loop {
        let event = rx.recv().await.expect("Bluetooth observer channel closed");
        info!("Received Bluetooth event: {:#?}", event);

        match event {
            BluetoothEvent::AdapterOn => {
                info!("Shutting down Adapter in 10s...");
                tokio::spawn({
                    let service_proxy = service_proxy.clone();
                    async move {
                        sleep(Duration::from_secs(10)).await;
                        service_proxy
                            .turn_off_adapter()
                            .await
                            .expect("Could not turn off adapter");
                        info!("Adapter turned off.");
                    }
                });
            }
            _ => {}
        }
    }
}
