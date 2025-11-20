use anyhow::Result;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};
use zbus::Connection;

use crate::bluetooth::service_proxy::BluetoothServiceProxy;

/// The application name used when sending notifications to the desktop environment.
pub const NOTIFICATION_APP_NAME: &str = "bluetooth-timeout";

/// A builder-pattern struct for constructing and sending desktop notifications via D-Bus.
#[derive(Debug, Clone)]
pub struct Notification {
    app_name: String,
    summary: String,
    body: String,
    icon: String,
    replaces_id: u32,
    timeout: i32, // milliseconds; -1 = server default
}

impl Notification {
    /// Create a new builder with some sane defaults.
    pub fn builder() -> Self {
        Self {
            app_name: NOTIFICATION_APP_NAME.to_string(),
            summary: String::new(),
            body: String::new(),
            icon: String::new(),
            replaces_id: 0,
            timeout: -1,
        }
    }

    /// (Optional) override the app name shown in the notification.
    #[allow(dead_code)]
    pub fn app_name(mut self, app_name: impl Into<String>) -> Self {
        self.app_name = app_name.into();
        self
    }

    /// Set the summary (title) of the notification.
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }

    /// Set the body text of the notification.
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    /// Icon name from your icon theme (e.g. "dialog-information"), or "" for none.
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = icon.into();
        self
    }

    /// Notification ID to replace (0 = none).
    ///
    /// This allows updating an existing notification instead of creating a new one.
    #[allow(dead_code)]
    pub fn replaces_id(mut self, replaces_id: u32) -> Self {
        self.replaces_id = replaces_id;
        self
    }

    /// Timeout in milliseconds, -1 = server default, 0 = persistent (depends on daemon).
    #[allow(dead_code)]
    pub fn timeout(mut self, timeout_ms: i32) -> Self {
        self.timeout = timeout_ms;
        self
    }

    /// Send the notification via org.freedesktop.Notifications.
    ///
    /// Returns the ID of the sent notification on success.
    pub async fn show(self) -> Result<u32> {
        // Connect to session bus
        let connection = Connection::session().await?;

        // Call Notify
        let reply = connection
            .call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "Notify",
                &(
                    self.app_name,
                    self.replaces_id,
                    self.icon,
                    self.summary,
                    self.body,
                    Vec::<String>::new(), // actions
                    std::collections::HashMap::<String, zbus::zvariant::Value>::new(), // hints
                    self.timeout,
                ),
            )
            .await?;

        Ok(reply.body().deserialize()?)
    }
}

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

        self.notification_at(Duration::from_mins(5)).await;
        self.notification_at(Duration::from_secs(60)).await;
        self.notification_at(Duration::from_secs(30)).await;
        self.notification_at(Duration::from_secs(10)).await;

        tokio::time::sleep(self.timeout).await;
        match self.service_proxy.turn_off_adapter().await {
            Ok(_) => info!("Adapter turned off."),
            Err(e) => warn!("Failed to turn off adapter: {}", e),
        }

        Notification::builder()
            .summary("Bluetooth Adapter Turned Off")
            .body("Bluetooth adapter has been turned off due to inactivity.")
            .icon("bluetooth-disabled-symbolic")
            // .replaces_id(self.last_notification_id)
            .show()
            .await
            .inspect_err(|e| error!("Failed to show notification: {}", e))
            .ok();
        info!("Timeout task completed.");
    }

    /// Waits until the remaining time matches the specified `time`, then sends a warning n
    /// otification.
    ///
    /// If the current timeout duration is already less than or equal to `time`, this method does
    /// nothing immediately.
    async fn notification_at(&mut self, time: Duration) {
        if self.timeout <= time {
            return;
        }

        tokio::time::sleep(self.timeout - time).await;
        self.timeout = time;
        self.send_notification(&time).await;
    }

    /// Helper method to construct and send a warning notification.
    ///
    /// Updates `last_notification_id` to allow future notifications to replace this one (if implemented).
    async fn send_notification(&mut self, duration: &Duration) {
        self.last_notification_id = Notification::builder()
            .summary("Bluetooth Timeout Warning")
            .body(&format!(
                "Bluetooth adapter will turn off in {} due to inactivity.",
                humantime::format_duration(*duration)
            ))
            .icon("bluetooth-symbolic")
            // .replaces_id(self.last_notification_id)
            .show()
            .await
            .inspect_err(|e| error!("Failed to show notification: {}", e))
            .ok()
            .unwrap_or(0);
    }

    /// Spawns the `TimeoutTask` onto the Tokio runtime.
    ///
    /// Returns a `JoinHandle` that can be used to await the task's completion or abort it.
    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move { self.run().await })
    }
}
