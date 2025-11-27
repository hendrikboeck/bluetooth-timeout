// -- std imports
use std::time::Duration;

// -- crate imports
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

// -- module imports
use crate::{
    bluetooth::service_proxy::BluetoothServiceProxy, configuration::Conf,
    notification::Notification,
};

/// A task that monitors inactivity and turns off the Bluetooth adapter after a specified duration.
///
/// It sends warning notifications at specific intervals (5m, 1m, 30s, 10s) before the timeout occurs.
#[derive(Debug, Clone)]

pub struct TimeoutTask {
    pub timeout: Duration,
    pub service_proxy: BluetoothServiceProxy,
    last_notification_id: u32,
}

impl TimeoutTask {
    /// Creates a new `TimeoutTask`.
    ///
    /// # Arguments
    ///
    /// * `timeout` - The total duration to wait before turning off the adapter.
    /// * `service_proxy` - The proxy to communicate with the Bluetooth service.
    pub fn new(timeout: Duration, service_proxy: BluetoothServiceProxy) -> Self {
        Self {
            timeout,
            service_proxy,
            last_notification_id: 0,
        }
    }

    /// Runs the timeout logic.
    ///
    /// This method sleeps for calculated intervals to send notifications at
    /// 5 minutes, 60 seconds, 30 seconds, and 10 seconds remaining.
    /// Finally, it turns off the adapter and sends a final notification.
    async fn run(mut self) {
        info!(
            "Starting timeout task: will turn off adapter after {} of inactivity.",
            humantime::format_duration(self.timeout)
        );
        let conf = Conf::instance();

        if conf.notifications_enabled {
            for &time in &conf.notifications_at {
                self.notification_at(time.clone()).await;
            }
        }

        tokio::time::sleep(self.timeout).await;
        match self.service_proxy.turn_off_adapter().await {
            Ok(_) => info!("Adapter turned off."),
            Err(e) => warn!("Failed to turn off adapter: {}", e),
        }

        if conf.notifications_enabled {
            let _ = Notification::new()
                .title("Bluetooth Adapter Turned Off")
                .body("Bluetooth adapter has been turned off due to inactivity.")
                .icon("bluetooth-disabled-symbolic")
                // .replaces_id(self.last_notification_id)
                .show()
                .await
                .inspect_err(|e| error!("Failed to show notification: {}", e));
        }
        info!("Timeout task completed.");
    }

    /// Waits until the remaining time matches the specified `time`, then sends a warning n
    /// otification.
    ///
    /// If the current timeout duration is already less than or equal to `time`, this method does
    /// nothing immediately.
    async fn notification_at(&mut self, time: Duration) {
        if self.timeout < time {
            return;
        }

        if self.timeout != time {
            tokio::time::sleep(self.timeout - time).await;
            self.timeout = time;
        }

        self.send_notification(&time).await;
    }

    /// Helper method to construct and send a warning notification.
    ///
    /// Updates `last_notification_id` to allow future notifications to replace this one (if implemented).
    async fn send_notification(&mut self, duration: &Duration) {
        self.last_notification_id = Notification::new()
            .title("Bluetooth Timeout Warning")
            .body(&format!(
                "Bluetooth adapter will turn off in {} due to inactivity.",
                humantime::format_duration(*duration)
            ))
            .icon("bluetooth-symbolic")
            // .replaces_id(self.last_notification_id)
            .show()
            .await
            .inspect_err(|e| error!("Failed to show notification: {}", e))
            .unwrap_or(0);
    }

    /// Spawns the `TimeoutTask` onto the Tokio runtime.
    ///
    /// Returns a `JoinHandle` that can be used to await the task's completion or abort it.
    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move { self.run().await })
    }
}
