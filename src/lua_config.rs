// -- std imports
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

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

/// Discovers all Bluetooth adapters via D-Bus, retrying until at least one is found.
///
/// At boot or after system wake, the Bluetooth adapter may not be enumerated by `BlueZ` yet. A
/// single discovery attempt would return an empty adapter list, leaving the daemon with nothing
/// to manage (and the event observer with no subscribers, causing "channel closed" errors).
/// Discovery is therefore retried according to `conf` until an adapter appears.
pub async fn discover_adapters(conf: &super::configuration::DiscoveryConf) -> Result<Vec<LuaAdapter>> {
    let mut last_error = None;

    for attempt in 1..=conf.attempts {
        match discover_adapters_once().await {
            Ok(adapters) if !adapters.is_empty() => return Ok(adapters),
            Ok(_) => {
                warn!(
                    "No Bluetooth adapters discovered (attempt {attempt}/{}); retrying...",
                    conf.attempts
                );
                last_error = None;
            }
            Err(e) => {
                warn!(
                    "Could not discover Bluetooth adapters (attempt {attempt}/{}): {e}",
                    conf.attempts
                );
                last_error = Some(e);
            }
        }

        if attempt < conf.attempts {
            tokio::time::sleep(conf.delay).await;
        }
    }

    last_error.map_or_else(|| Ok(vec![]), Err)
}

/// Performs a single adapter discovery attempt via D-Bus.
async fn discover_adapters_once() -> Result<Vec<LuaAdapter>> {
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
///
/// If `filter` contains a `retries` table (`{ attempts = N, delay = "2s" }`), its discovery
/// settings are captured into `retry_probe` so the caller can read them before performing the
/// actual discovery.
fn create_find_adapters(
    lua: &Lua,
    retry_probe: Arc<Mutex<Option<super::configuration::DiscoveryConf>>>,
) -> mlua::Result<mlua::Function> {
    lua.create_function(move |lua, filter: Option<Table>| {
        let adapters: Table = lua.globals().get("__ALL_ADAPTERS")?;

        let Some(filter) = filter else {
            return Ok(adapters);
        };

        // Capture discovery retry settings when provided.
        if let Ok(retries) = filter.get::<Table>("retries") {
            let defaults = super::configuration::DiscoveryConf::default();
            let attempts = retries
                .get::<usize>("attempts")
                .unwrap_or(defaults.attempts);
            let delay = retries
                .get::<String>("delay")
                .ok()
                .and_then(|s| humantime::parse_duration(&s).ok())
                .unwrap_or(defaults.delay);
            *retry_probe.lock().unwrap() = Some(super::configuration::DiscoveryConf {
                attempts,
                delay,
            });
        }

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

/// Evaluates a Lua config source and invokes `extract` with the resulting table.
///
/// Injects the discovered adapters (or an empty list, when only discovery settings are needed)
/// and the `find_adapters` helper before evaluating `lua_source`. The `Lua` instance is kept
/// alive for the duration of `extract`.
fn evaluate_config<F, R>(
    lua_source: &str,
    adapters: Vec<LuaAdapter>,
    retry_probe: Arc<Mutex<Option<super::configuration::DiscoveryConf>>>,
    extract: F,
) -> Result<R>
where
    F: FnOnce(Table) -> Result<R>,
{
    let lua = Lua::new();
    inject_adapters(&lua, adapters)
        .map_err(|e| anyhow::anyhow!("Failed to inject adapters into Lua: {e}"))?;

    lua.globals()
        .set("find_adapters", create_find_adapters(&lua, retry_probe)?)
        .map_err(|e| anyhow::anyhow!("Failed to set find_adapters: {e}"))?;

    let result: Table = lua
        .load(lua_source)
        .eval()
        .map_err(|e| anyhow::anyhow!("Failed to evaluate Lua config: {e}"))?;

    extract(result)
}

/// Extracts the adapter discovery settings from a Lua config source.
///
/// This runs before adapters are discovered, so no real adapters are injected yet. The settings
/// are read from the `retries` table passed to `find_adapters` (e.g.
/// `find_adapters { retries = { attempts = 6, delay = "2s" } }`).
pub fn load_discovery_conf(lua_source: &str) -> Result<super::configuration::DiscoveryConf> {
    let probe = Arc::new(Mutex::new(None));
    evaluate_config(lua_source, vec![], probe.clone(), |_| Ok(()))?;
    Ok(probe.lock().unwrap().take().unwrap_or_default())
}

/// Load the Lua config source and return a Conf.
///
/// `lua_source` is the contents of the config.lua file.
/// `adapters` are the pre-discovered Bluetooth adapters from D-Bus.
pub fn load_config(
    lua_source: &str,
    adapters: Vec<LuaAdapter>,
) -> Result<super::configuration::Conf> {
    evaluate_config(lua_source, adapters, Arc::new(Mutex::new(None)), |result| {
        extract_conf(&result)
    })
}

/// Extracts the full [`super::configuration::Conf`] from an evaluated config table.
fn extract_conf(result: &Table) -> Result<super::configuration::Conf> {
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
