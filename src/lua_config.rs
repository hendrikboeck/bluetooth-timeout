// -- std imports
use std::time::Duration;

// -- crate imports
use anyhow::{Context, Result};
use mlua::{Lua, Table};
use tracing::warn;
use zbus::{Connection, fdo::ObjectManagerProxy};

// -- module imports
use crate::bluetooth::constants::{BLUEZ_ADAPTER_IFACE, BLUEZ_SERVICE};
use crate::configuration::VERSION_MAJOR;

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
        let filter_powered: Option<bool> = filter.get("powered").ok();
        let filter_discoverable: Option<bool> = filter.get("discoverable").ok();

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

    let runtime = result.get::<Table>("runtime").map_or_else(
        |_| super::configuration::RuntimeConf::default(),
        |rt| {
            let multithreaded: bool = rt.get::<bool>("multithreaded").unwrap_or(false);
            super::configuration::RuntimeConf { multithreaded }
        },
    );

    let version: u32 = result
        .get::<String>("version")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(VERSION_MAJOR);

    Ok(super::configuration::Conf {
        version,
        timeout,
        notifications,
        runtime,
        adapter_paths,
    })
}

// -----------------------------------------------------------------------------------------------
//  Lua code generation
// -----------------------------------------------------------------------------------------------

/// Embedded Lua config template for the current config schema version.
const CONFIG_TEMPLATE: &str = include_str!("../contrib/config/v2.0/config.lua");

/// Generate a `config.lua` source string from a [`super::configuration::Conf`].
///
/// The output is produced by substituting values into the embedded template
/// for the current config schema version.
pub fn generate_config(conf: &super::configuration::Conf) -> String {
    let at = conf
        .notifications
        .at
        .iter()
        .map(|d| format!("\"{}\"", humantime::format_duration(*d)))
        .collect::<Vec<_>>()
        .join(", ");

    let adapter_entries: Vec<String> = conf
        .adapter_paths
        .iter()
        .map(|p| format!("{{ path = \"{p}\" }}"))
        .collect();
    let adapters = if adapter_entries.is_empty() {
        "find_adapters()".to_string()
    } else {
        format!("{{ {} }}", adapter_entries.join(", "))
    };

    CONFIG_TEMPLATE
        .replace("@VERSION@", &conf.version.to_string())
        .replace(
            "@TIMEOUT@",
            &humantime::format_duration(conf.timeout).to_string(),
        )
        .replace("@ADAPTERS@", &adapters)
        .replace(
            "@NOTIFICATIONS_ENABLED@",
            if conf.notifications.enabled {
                "true"
            } else {
                "false"
            },
        )
        .replace("@NOTIFICATIONS_AT@", &at)
}
