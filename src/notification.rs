use std::collections::HashMap;

use anyhow::Result;
use zbus::{Connection, zvariant::Value};

/// Send a desktop notification via org.freedesktop.Notifications D-Bus.
///
/// Returns the notification ID on success.
pub async fn notify(title: &str, body: &str, icon: &str) -> Result<u32> {
    let conn = Connection::session().await?;
    let reply = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                env!("CARGO_PKG_NAME"),
                0u32,
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
