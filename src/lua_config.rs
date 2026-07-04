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
    pub path: String,
    pub address: String,
    pub name: String,
    pub powered: bool,
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
        let props = match ifaces.get(BLUEZ_ADAPTER_IFACE) {
            Some(p) => p,
            None => continue,
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

fn adapter_to_lua_table(lua: &Lua, adapter: &LuaAdapter) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("path", adapter.path.clone())?;
    table.set("address", adapter.address.clone())?;
    table.set("name", adapter.name.clone())?;
    table.set("powered", adapter.powered)?;
    table.set("discoverable", adapter.discoverable)?;
    Ok(table)
}

fn inject_adapters(lua: &Lua, adapters: &[LuaAdapter]) -> mlua::Result<()> {
    let adapters_table = lua.create_table()?;
    for (i, adapter) in adapters.iter().enumerate() {
        adapters_table.set((i + 1) as i64, adapter_to_lua_table(lua, adapter)?)?;
    }
    lua.globals().set("__ALL_ADAPTERS", adapters_table)?;
    Ok(())
}

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
    adapters: &[LuaAdapter],
) -> Result<super::configuration::Conf> {
    let lua = Lua::new();
    inject_adapters(&lua, adapters)
        .map_err(|e| anyhow::anyhow!("Failed to inject adapters into Lua: {}", e))?;

    let find_adapters = lua
        .create_function(|lua, filter: Option<Table>| {
            let adapters: Table = lua.globals().get("__ALL_ADAPTERS")?;

            let filter = match filter {
                Some(f) => f,
                None => return Ok(adapters),
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
        .map_err(|e| anyhow::anyhow!("Failed to create find_adapters: {}", e))?;

    lua.globals()
        .set("find_adapters", find_adapters)
        .map_err(|e| anyhow::anyhow!("Failed to set find_adapters: {}", e))?;

    let result: Table = lua
        .load(lua_source)
        .eval()
        .map_err(|e| anyhow::anyhow!("Failed to evaluate Lua config: {}", e))?;

    let adapter_paths: Vec<String> = match result.get::<Table>("adapters") {
        Ok(adapters) => {
            let mut paths = vec![];
            for pair in adapters.pairs::<mlua::Value, Table>() {
                let (_, adapter): (mlua::Value, Table) =
                    pair.map_err(|e| anyhow::anyhow!("Failed to iterate adapters: {}", e))?;
                let path: String = adapter
                    .get("path")
                    .map_err(|e| anyhow::anyhow!("adapter missing 'path' field: {}", e))?;
                paths.push(path);
            }
            paths
        }
        Err(_) => {
            warn!("No 'adapters' field in config, falling back to default adapter.");
            vec!["/org/bluez/hci0".to_string()]
        }
    };

    let timeout_str: String = result
        .get("timeout")
        .map_err(|e| anyhow::anyhow!("missing 'timeout' field: {}", e))?;
    let timeout = humantime::parse_duration(&timeout_str).context("invalid timeout format")?;

    let notifications = match result.get::<Table>("notifications") {
        Ok(nt) => {
            let enabled: bool = nt.get::<bool>("enabled").unwrap_or(true);
            let at: Vec<String> = nt.get::<Vec<String>>("at").unwrap_or_default();
            let at_durations: Vec<Duration> = at
                .into_iter()
                .map(|s| {
                    humantime::parse_duration(&s)
                        .with_context(|| format!("invalid notification duration: {}", s))
                })
                .collect::<Result<Vec<_>>>()?;
            super::configuration::NotificationConf {
                enabled,
                at: at_durations,
            }
        }
        Err(_) => super::configuration::NotificationConf::default(),
    };

    let runtime = match result.get::<Table>("runtime") {
        Ok(rt) => {
            let multithreaded: bool = rt.get::<bool>("multithreaded").unwrap_or(false);
            super::configuration::RuntimeConf { multithreaded }
        }
        Err(_) => super::configuration::RuntimeConf::default(),
    };

    Ok(super::configuration::Conf {
        timeout,
        notifications,
        runtime,
        adapter_paths,
    })
}
