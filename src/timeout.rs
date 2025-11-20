use anyhow::Result;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};
use zbus::Connection;

use crate::bluetooth::service_proxy::BluetoothServiceProxy;

pub const NOTIFICATION_APP_NAME: &str = "bluetooth-timeout";

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

    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }

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

pub struct TimeoutTask {
    pub timeout: Duration,
    pub service_proxy: BluetoothServiceProxy,
    last_notification_id: u32,
}

impl TimeoutTask {
    pub fn new(timeout: Duration, service_proxy: BluetoothServiceProxy) -> Self {
        Self {
            timeout,
            service_proxy,
            last_notification_id: 0,
        }
    }

    async fn run(mut self) {
        info!(
            "Starting timeout task: will turn off adapter after {:?} of inactivity.",
            self.timeout
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
            .replaces_id(self.last_notification_id)
            .show()
            .await
            .inspect_err(|e| error!("Failed to show notification: {}", e))
            .ok();
    }

    async fn notification_at(&mut self, time: Duration) {
        if self.timeout <= time {
            return;
        }

        tokio::time::sleep(self.timeout - time).await;
        self.timeout = time;
        self.send_notification(&time).await;
    }

    async fn send_notification(&mut self, duration: &Duration) {
        self.last_notification_id = Notification::builder()
            .summary("Bluetooth Timeout Warning")
            .body(&format!(
                "Bluetooth adapter will turn off in {:?} due to inactivity.",
                duration
            ))
            .icon("bluetooth-symbolic")
            .replaces_id(self.last_notification_id)
            .show()
            .await
            .inspect_err(|e| error!("Failed to show notification: {}", e))
            .ok()
            .unwrap_or(0);
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move { self.run().await })
    }
}
