use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::bluetooth::service_proxy::BluetoothServiceProxy;

pub struct TimeoutTask {
    pub timeout: Duration,
    pub service_proxy: BluetoothServiceProxy,
}

impl TimeoutTask {
    async fn run(self) {
        info!(
            "Starting timeout task: will turn off adapter after {:?} of inactivity.",
            self.timeout
        );
        tokio::time::sleep(self.timeout).await;
        match self.service_proxy.turn_off_adapter().await {
            Ok(_) => info!("Adapter turned off."),
            Err(e) => warn!("Failed to turn off adapter: {}", e),
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move { self.run().await })
    }
}
