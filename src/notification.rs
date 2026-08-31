use std::collections::HashMap;

use anyhow::Result;
use zbus::{Connection, zvariant::Value};

/// Send a desktop notification via org.freedesktop.Notifications D-Bus.
///
/// `replaces_id` is the ID of the notification to replace (0 for a new one).
/// Returns the new notification ID on success.
pub async fn notify(replaces_id: u32, title: &str, body: &str, icon: &str) -> Result<u32> {
    let conn = Connection::session().await?;
    let reply = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                env!("CARGO_PKG_NAME"),
                replaces_id,
                icon,
                title,
                body,
                Vec::<String>::new(),
                HashMap::<String, Value>::new(),
                -1i32,
            ),
        )
        .await?;
    Ok(reply.body().deserialize()?)
}
