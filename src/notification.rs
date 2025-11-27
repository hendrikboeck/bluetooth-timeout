// -- crate imports
use anyhow::Result;
use zbus::Connection;

/// The application name used when sending notifications to the desktop environment.
pub const NOTIFICATION_APP_NAME: &str = "bluetooth-timeout";

/// A builder-pattern struct for constructing and sending desktop notifications via D-Bus.
#[derive(Debug, Clone)]
pub struct Notification {
    app_name: String,
    title: String,
    body: String,
    icon: String,
    replaces_id: u32,
    timeout: i32, // milliseconds; -1 = server default
}

impl Notification {
    /// Create a new builder with some sane defaults.
    pub fn new() -> Self {
        Self {
            app_name: NOTIFICATION_APP_NAME.to_string(),
            title: String::new(),
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
    pub fn title(mut self, summary: impl Into<String>) -> Self {
        self.title = summary.into();
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
                    self.title,
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
