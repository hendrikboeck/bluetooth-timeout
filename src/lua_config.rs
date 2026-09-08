// -- std imports
use std::time::Duration;

// -- crate imports
use anyhow::{Context, Result};
use mlua::{Lua, Table};
use tracing::warn;
use zbus::{Connection, fdo::ObjectManagerProxy};

// -- module imports
use crate::bluetooth::constants::{BLUEZ_ADAPTER_IFACE, BLUEZ_SERVICE};

/// Represents a Bluetooth adapter discovered via D-Bus.
#[derive(Debug, Clone)]
pub struct LuaAdapter {
    /// D-Bus object path of the adapter (e.g., `/org/bluez/hci0`).
    pub path: String,
    /// Hardware (MAC) address of the adapter.
    pub address: String,
    /// Human-readable name (alias) of the adapter.
    pub name: String,
    /// Whether the adapter is currently powered on.
    pub powered: bool,
    /// Whether the adapter is currently discoverable.
    pub discoverable: bool,
}

/// Discover all Bluetooth adapters via D-Bus.
pub async fn discover_adapters() -> Result<Vec<LuaAdapter>> {
    let conn = Connection::system().await?;
    let proxy = ObjectManagerProxy::builder(&conn)
        .destination(BLUEZ_SERVICE)?
        .path("/")?
        .build()
        .await?;

    let objects = proxy.get_managed_objects().await?;
    let mut adapters = vec![];

    for (path, ifaces) in objects {
        let Some(props) = ifaces.get(BLUEZ_ADAPTER_IFACE) else {
            continue;
        };

        let address = props
            .get("Address")
            .and_then(|v| {
                v.downcast_ref::<zbus::zvariant::Str>()
                    .ok()
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();
        let name = props
            .get("Alias")
            .and_then(|v| {
                v.downcast_ref::<zbus::zvariant::Str>()
                    .ok()
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();
        let powered = props
            .get("Powered")
            .and_then(|v| v.downcast_ref::<bool>().ok())
            .unwrap_or(false);
        let discoverable = props
            .get("Discoverable")
            .and_then(|v| v.downcast_ref::<bool>().ok())
            .unwrap_or(false);

        adapters.push(LuaAdapter {
            path: path.to_string(),
            address,
            name,
            powered,
            discoverable,
        });
    }

    Ok(adapters)
}

/// Converts a [`LuaAdapter`] into a Lua table for injection into the config environment.
fn adapter_to_lua_table(lua: &Lua, adapter: LuaAdapter) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("path", adapter.path)?;
    table.set("address", adapter.address)?;
    table.set("name", adapter.name)?;
    table.set("powered", adapter.powered)?;
    table.set("discoverable", adapter.discoverable)?;
    Ok(table)
}

/// Injects discovered adapters into the Lua global `__ALL_ADAPTERS` as a 1-indexed table.
fn inject_adapters(lua: &Lua, adapters: Vec<LuaAdapter>) -> mlua::Result<()> {
    let adapters_table = lua.create_table()?;
    for (i, adapter) in adapters.into_iter().enumerate() {
        adapters_table.set(
            i64::try_from(i + 1).expect("too many adapters for Lua index"),
            adapter_to_lua_table(lua, adapter)?,
        )?;
    }
    lua.globals().set("__ALL_ADAPTERS", adapters_table)?;
    Ok(())
}

/// Creates the `find_adapters(filter)` Lua function for config-driven adapter discovery.
fn create_find_adapters(lua: &Lua) -> mlua::Result<mlua::Function> {
    lua.create_function(|lua, filter: Option<Table>| {
        let adapters: Table = lua.globals().get("__ALL_ADAPTERS")?;

        let Some(filter) = filter else {
            return Ok(adapters);
        };

        let filter_name: Option<String> = filter.get("name").ok();
        let filter_name_pattern: Option<String> = filter.get("name_pattern").ok();
        let filter_address: Option<String> = filter.get("address").ok();
        let filter_address_prefix: Option<String> = filter.get("address_prefix").ok();
        // `.get::<bool>` maps a missing key to `false` (mlua coerces nil -> false),
        // so read as `Option<bool>` and flatten to treat absence as "no filter".
        let filter_powered: Option<bool> = filter.get("powered").ok().flatten();
        let filter_discoverable: Option<bool> = filter.get("discoverable").ok().flatten();

        let result = lua.create_table()?;
        let mut idx: i64 = 1;

        for pair in adapters.pairs::<mlua::Value, Table>() {
            let (_, adapter): (mlua::Value, Table) = pair?;

            if let Some(ref name) = filter_name {
                let a_name: String = adapter.get("name")?;
                if a_name != *name {
                    continue;
                }
            }
            if let Some(ref pattern) = filter_name_pattern {
                let a_name: String = adapter.get("name")?;
                if !lua_match(lua, &a_name, pattern)? {
                    continue;
                }
            }
            if let Some(ref addr) = filter_address {
                let a_addr: String = adapter.get("address")?;
                if a_addr != *addr {
                    continue;
                }
            }
            if let Some(ref prefix) = filter_address_prefix {
                let a_addr: String = adapter.get("address")?;
                if !a_addr.starts_with(prefix.as_str()) {
                    continue;
                }
            }
            if let Some(p) = filter_powered {
                let a_p: bool = adapter.get("powered")?;
                if a_p != p {
                    continue;
                }
            }
            if let Some(d) = filter_discoverable {
                let a_d: bool = adapter.get("discoverable")?;
                if a_d != d {
                    continue;
                }
            }

            result.set(idx, adapter)?;
            idx += 1;
        }

        Ok(result)
    })
}

/// Calls Lua's `string.find` to test whether `s` matches `pattern`.
fn lua_match(lua: &Lua, s: &str, pattern: &str) -> mlua::Result<bool> {
    let string_find: mlua::Function = lua.globals().get::<Table>("string")?.get("find")?;
    let result: mlua::Value = string_find.call((s, pattern))?;
    Ok(!result.is_nil())
}

/// Load the Lua config source and return a Conf.
///
/// `lua_source` is the contents of the config.lua file.
/// `adapters` are the pre-discovered Bluetooth adapters from D-Bus.
pub fn load_config(
    lua_source: &str,
    adapters: Vec<LuaAdapter>,
) -> Result<super::configuration::Conf> {
    let lua = Lua::new();
    inject_adapters(&lua, adapters)
        .map_err(|e| anyhow::anyhow!("Failed to inject adapters into Lua: {e}"))?;

    lua.globals()
        .set("find_adapters", create_find_adapters(&lua)?)
        .map_err(|e| anyhow::anyhow!("Failed to set find_adapters: {e}"))?;

    let result: Table = lua
        .load(lua_source)
        .eval()
        .map_err(|e| anyhow::anyhow!("Failed to evaluate Lua config: {e}"))?;

    let adapter_paths: Vec<String> = if let Ok(adapters) = result.get::<Table>("adapters") {
        let mut paths = vec![];
        for pair in adapters.pairs::<mlua::Value, Table>() {
            let (_, adapter): (mlua::Value, Table) =
                pair.map_err(|e| anyhow::anyhow!("Failed to iterate adapters: {e}"))?;
            let path: String = adapter
                .get("path")
                .map_err(|e| anyhow::anyhow!("adapter missing 'path' field: {e}"))?;
            paths.push(path);
        }
        paths
    } else {
        warn!("No 'adapters' field in config, falling back to default adapter.");
        vec!["/org/bluez/hci0".to_string()]
    };

    let timeout_str: String = result
        .get("timeout")
        .map_err(|e| anyhow::anyhow!("missing 'timeout' field: {e}"))?;
    let timeout = humantime::parse_duration(&timeout_str).context("invalid timeout format")?;

    let notifications = match result.get::<Table>("notifications") {
        Ok(nt) => {
            let enabled: bool = nt.get::<bool>("enabled").unwrap_or(true);
            let at: Vec<String> = nt.get::<Vec<String>>("at").unwrap_or_default();
            let at_durations: Vec<Duration> = at
                .into_iter()
                .map(|s| {
                    humantime::parse_duration(&s)
                        .with_context(|| format!("invalid notification duration: {s}"))
                })
                .collect::<Result<Vec<_>>>()?;
            super::configuration::NotificationConf {
                enabled,
                at: at_durations,
            }
        }
        Err(_) => super::configuration::NotificationConf::default(),
    };

    Ok(super::configuration::Conf {
        timeout,
        notifications,
        adapter_paths,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::NotificationConf;

    fn adapter(
        path: &str,
        address: &str,
        name: &str,
        powered: bool,
        discoverable: bool,
    ) -> LuaAdapter {
        LuaAdapter {
            path: path.to_string(),
            address: address.to_string(),
            name: name.to_string(),
            powered,
            discoverable,
        }
    }

    fn two_adapters() -> Vec<LuaAdapter> {
        vec![
            adapter(
                "/org/bluez/hci0",
                "00:1A:7D:DA:71:13",
                "My Dongle",
                true,
                true,
            ),
            adapter(
                "/org/bluez/hci1",
                "00:1B:2C:3D:4E:5F",
                "Other Adapter",
                false,
                false,
            ),
        ]
    }

    #[test]
    fn parses_full_config() {
        let src = r#"
            local M = {}
            M.timeout = "5m"
            M.adapters = find_adapters { powered = true }
            M.notifications = { enabled = true, at = { "5m", "1m", "30s", "10s" } }
            return M
        "#;
        let conf = load_config(src, two_adapters()).unwrap();
        assert_eq!(conf.timeout, Duration::from_mins(5));
        assert_eq!(conf.adapter_paths, vec!["/org/bluez/hci0".to_string()]);
        assert_eq!(
            conf.notifications,
            NotificationConf {
                enabled: true,
                at: vec![
                    Duration::from_mins(5),
                    Duration::from_mins(1),
                    Duration::from_secs(30),
                    Duration::from_secs(10),
                ],
            }
        );
    }

    #[test]
    fn parses_compound_timeout() {
        let src = r#"return { timeout = "1m30s", adapters = find_adapters() }"#;
        let conf = load_config(src, two_adapters()).unwrap();
        assert_eq!(conf.timeout, Duration::from_secs(90));
    }

    #[test]
    fn missing_timeout_errors() {
        let src = r"return { adapters = find_adapters() }";
        assert!(load_config(src, two_adapters()).is_err());
    }

    #[test]
    fn invalid_timeout_errors() {
        let src = r#"return { timeout = "not-a-duration", adapters = find_adapters() }"#;
        assert!(load_config(src, two_adapters()).is_err());
    }

    #[test]
    fn missing_adapters_falls_back_to_hci0() {
        let src = r#"return { timeout = "5m" }"#;
        let conf = load_config(src, two_adapters()).unwrap();
        assert_eq!(conf.adapter_paths, vec!["/org/bluez/hci0".to_string()]);
    }

    #[test]
    fn hardcoded_adapters_are_used() {
        let src = r#"
            return {
                timeout = "5m",
                adapters = { { path = "/org/bluez/hci0" }, { path = "/org/bluez/hci1" } },
            }
        "#;
        let conf = load_config(src, vec![]).unwrap();
        assert_eq!(
            conf.adapter_paths,
            vec!["/org/bluez/hci0".to_string(), "/org/bluez/hci1".to_string()]
        );
    }

    #[test]
    fn missing_notifications_uses_default() {
        let src = r#"return { timeout = "5m", adapters = find_adapters() }"#;
        let conf = load_config(src, two_adapters()).unwrap();
        assert_eq!(conf.notifications, NotificationConf::default());
    }

    #[test]
    fn notifications_can_be_disabled() {
        let src = r#"return { timeout = "5m", adapters = find_adapters(), notifications = { enabled = false } }"#;
        let conf = load_config(src, two_adapters()).unwrap();
        assert!(!conf.notifications.enabled);
    }

    #[test]
    fn notifications_custom_intervals() {
        let src = r#"return { timeout = "5m", adapters = find_adapters(), notifications = { enabled = true, at = { "1m", "10s" } } }"#;
        let conf = load_config(src, two_adapters()).unwrap();
        assert_eq!(
            conf.notifications.at,
            vec![Duration::from_mins(1), Duration::from_secs(10)]
        );
    }

    #[test]
    fn invalid_notification_duration_errors() {
        let src = r#"return { timeout = "5m", adapters = find_adapters(), notifications = { at = { "nope" } } }"#;
        assert!(load_config(src, two_adapters()).is_err());
    }

    #[test]
    fn find_adapters_no_filter_returns_all() {
        let src = r#"return { timeout = "5m", adapters = find_adapters() }"#;
        let conf = load_config(src, two_adapters()).unwrap();
        assert_eq!(
            conf.adapter_paths,
            vec!["/org/bluez/hci0".to_string(), "/org/bluez/hci1".to_string()]
        );
    }

    #[test]
    fn find_adapters_filter_by_name() {
        let src =
            r#"return { timeout = "5m", adapters = find_adapters { name = "Other Adapter" } }"#;
        let conf = load_config(src, two_adapters()).unwrap();
        assert_eq!(conf.adapter_paths, vec!["/org/bluez/hci1".to_string()]);
    }

    #[test]
    fn find_adapters_filter_by_name_pattern() {
        let src =
            r#"return { timeout = "5m", adapters = find_adapters { name_pattern = "Dongle" } }"#;
        let conf = load_config(src, two_adapters()).unwrap();
        assert_eq!(conf.adapter_paths, vec!["/org/bluez/hci0".to_string()]);
    }

    #[test]
    fn find_adapters_filter_by_address() {
        let src = r#"return { timeout = "5m", adapters = find_adapters { address = "00:1B:2C:3D:4E:5F" } }"#;
        let conf = load_config(src, two_adapters()).unwrap();
        assert_eq!(conf.adapter_paths, vec!["/org/bluez/hci1".to_string()]);
    }

    #[test]
    fn find_adapters_filter_by_address_prefix() {
        let src =
            r#"return { timeout = "5m", adapters = find_adapters { address_prefix = "00:1A" } }"#;
        let conf = load_config(src, two_adapters()).unwrap();
        assert_eq!(conf.adapter_paths, vec!["/org/bluez/hci0".to_string()]);
    }

    #[test]
    fn find_adapters_filter_by_powered() {
        let src = r#"return { timeout = "5m", adapters = find_adapters { powered = true } }"#;
        let conf = load_config(src, two_adapters()).unwrap();
        assert_eq!(conf.adapter_paths, vec!["/org/bluez/hci0".to_string()]);
    }

    #[test]
    fn find_adapters_filter_by_discoverable() {
        let src = r#"return { timeout = "5m", adapters = find_adapters { discoverable = true } }"#;
        let conf = load_config(src, two_adapters()).unwrap();
        assert_eq!(conf.adapter_paths, vec!["/org/bluez/hci0".to_string()]);
    }

    #[test]
    fn find_adapters_filter_no_match_is_empty() {
        let src = r#"return { timeout = "5m", adapters = find_adapters { name = "nope" } }"#;
        let conf = load_config(src, two_adapters()).unwrap();
        assert!(conf.adapter_paths.is_empty());
    }

    #[test]
    fn find_adapters_combined_filters() {
        let src = r#"return { timeout = "5m", adapters = find_adapters { powered = true, address_prefix = "00:1A" } }"#;
        let conf = load_config(src, two_adapters()).unwrap();
        assert_eq!(conf.adapter_paths, vec!["/org/bluez/hci0".to_string()]);
    }

    #[test]
    fn parses_shipped_example_config() {
        let src = include_str!("../contrib/config/config.example.lua");
        let conf = load_config(src, two_adapters()).unwrap();
        assert_eq!(conf.timeout, Duration::from_mins(5));
        assert_eq!(
            conf.adapter_paths,
            vec!["/org/bluez/hci0".to_string(), "/org/bluez/hci1".to_string()]
        );
        assert_eq!(conf.notifications, NotificationConf::default());
    }
}
