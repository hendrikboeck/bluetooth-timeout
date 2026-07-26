/// YAML config parsing and conversion (version 1 → 2).
mod yaml;

use std::path::Path;

use anyhow::{Context, Result};
use tracing::info;

use crate::{
    configuration::{Conf, VERSION_MAJOR, VERSION_MINOR},
    lua_config,
};

/// Outcome of a [`migrate`] operation.
pub enum MigrationResult {
    /// Configuration is already at the latest version.
    UpToDate,
    /// Configuration was migrated from one version to another.
    Migrated { from_version: u32, to_version: u32 },
    /// No configuration existed; a fresh one was created from the template.
    Created,
}

// -----------------------------------------------------------------------------------------------
//  Public API
// -----------------------------------------------------------------------------------------------

/// Detect the version of the configuration found in `config_dir`.
///
/// Returns `None` if no config file exists at all. If a `config.lua` is present, its embedded
/// version field is used; if absent (pre-version Lua configs) version 2 is assumed. If only
/// `config.yml` exists, version 1 is reported.
pub fn detect_config_version(config_dir: &Path) -> Option<u32> {
    let yml_path = config_dir.join("config.yml");
    let lua_path = config_dir.join("config.lua");

    if lua_path.exists() {
        if let Ok(contents) = std::fs::read_to_string(&lua_path)
            && let Some(ver) = extract_lua_version(&contents)
        {
            return Some(ver);
        }
        return Some(2);
    }

    if yml_path.exists() {
        return Some(1);
    }

    None
}

/// Run any pending migrations to bring the configuration in `config_dir` up to
/// [`VERSION_MAJOR`].
///
/// - No config files → creates a fresh `config.lua` from the template.
/// - YAML config (version 1) → parses the YAML, converts to [`Conf`], generates Lua via
///   [`lua_config::generate_config`], and removes the old YAML file.
/// - Lua config at an older version → applies incremental migrations.
/// - Lua config at the current version → ensures the version field and LSP files are present,
///   removes stale YAML.
pub fn migrate(config_dir: &Path) -> Result<MigrationResult> {
    let version = detect_config_version(config_dir);

    match version {
        None => {
            fresh_install(config_dir)?;
            Ok(MigrationResult::Created)
        }
        Some(1) => {
            migrate_v1_to_v2(config_dir)?;
            info!("Migrated config from version 1 (YAML) to version 2 (Lua).");
            Ok(MigrationResult::Migrated {
                from_version: 1,
                to_version: VERSION_MAJOR,
            })
        }
        Some(2) => {
            let yml_path = config_dir.join("config.yml");
            if yml_path.exists() {
                std::fs::remove_file(&yml_path).ok();
                info!("Removed stale config.yml (config.lua is authoritative).");
            }

            let lua_path = config_dir.join("config.lua");
            let contents =
                std::fs::read_to_string(&lua_path).context("Failed to read config.lua")?;

            if extract_lua_version(&contents).is_none() {
                add_version_to_lua(&lua_path, &contents)?;
                info!("Added version field to existing Lua config.");
            }

            install_lsp_files(config_dir)?;

            Ok(MigrationResult::UpToDate)
        }
        Some(v) if v < VERSION_MAJOR => {
            anyhow::bail!(
                "Config version {v} is older than current version {VERSION_MAJOR}.{VERSION_MINOR} but no migration path \
                 exists. This is a bug — please file an issue.",
            );
        }
        Some(v) => {
            anyhow::bail!(
                "Config version {v} is newer than this binary (version {VERSION_MAJOR}.{VERSION_MINOR}). Upgrade bluetooth-timeout.",
            );
        }
    }
}

// -----------------------------------------------------------------------------------------------
//  Version detection
// -----------------------------------------------------------------------------------------------

/// Extract the config schema version from a Lua source string.
///
/// Looks for a line matching `M.version = "N"` (with optional whitespace and quoting).
/// Returns `None` if the version field is not present or cannot be parsed.
fn extract_lua_version(source: &str) -> Option<u32> {
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("M.version") {
            let rest = rest.trim_start().strip_prefix('=')?;
            let ver_str = rest
                .trim()
                .trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace());
            return ver_str.parse::<u32>().ok();
        }
    }
    None
}

// -----------------------------------------------------------------------------------------------
//  Migration implementations
// -----------------------------------------------------------------------------------------------

/// Read a legacy YAML config, convert it to a [`Conf`], generate the equivalent Lua via
/// [`lua_config::generate_config`], validate the result, and remove the old YAML file.
fn migrate_v1_to_v2(config_dir: &Path) -> Result<()> {
    let yml_path = config_dir.join("config.yml");
    let yaml_content = std::fs::read_to_string(&yml_path).context("Failed to read config.yml")?;

    let yaml: yaml::YamlConfig =
        serde_yaml::from_str(&yaml_content).context("Failed to parse config.yml")?;

    let conf = yaml.into_conf();
    let lua_source = lua_config::generate_config(&conf);

    lua_config::load_config(&lua_source, vec![]).context("Generated config failed validation")?;

    let lua_path = config_dir.join("config.lua");
    std::fs::write(&lua_path, lua_source).context("Failed to write config.lua")?;
    std::fs::remove_file(&yml_path).ok();

    install_lsp_files(config_dir)?;

    Ok(())
}

/// Create the config directory, write a fresh `config.lua` generated from default settings,
/// and install LSP support files.
fn fresh_install(config_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(config_dir).context("Failed to create config directory")?;

    let lua_path = config_dir.join("config.lua");
    if !lua_path.exists() {
        let conf = Conf::default();
        let lua_source = lua_config::generate_config(&conf);

        lua_config::load_config(&lua_source, vec![])
            .context("Generated config failed validation")?;

        std::fs::write(&lua_path, lua_source).context("Failed to write config.lua")?;
    }

    install_lsp_files(config_dir)?;

    Ok(())
}

/// Insert `M.version = "N"` into an existing Lua config that lacks a version field.
/// The statement is placed right after the `local M = {}` declaration.
fn add_version_to_lua(lua_path: &Path, contents: &str) -> Result<()> {
    let target = "local M = {}\n";
    let version_stmt = format!("\nM.version = \"{VERSION_MAJOR}\"\n");

    if let Some(pos) = contents.find(target) {
        let insertion_point = pos + target.len();
        let mut new_contents = String::with_capacity(contents.len() + version_stmt.len());
        new_contents.push_str(&contents[..insertion_point]);
        new_contents.push_str(&version_stmt);
        new_contents.push_str(&contents[insertion_point..]);

        std::fs::write(lua_path, new_contents).context("Failed to write config.lua")?;
    }

    Ok(())
}

// -----------------------------------------------------------------------------------------------
//  File installation
// -----------------------------------------------------------------------------------------------

/// LSP type definitions for editor autocompletion.
const TYPES_LUA: &str = include_str!("../../contrib/config/v2.0/types.lua");

/// LSP configuration for the Lua language server.
const LUARC_JSON: &str = include_str!("../../contrib/config/v2.0/.luarc.json");

/// Install LSP support files (types.lua, .luarc.json) if they do not already exist.
fn install_lsp_files(config_dir: &Path) -> Result<()> {
    let types_path = config_dir.join("types.lua");
    if !types_path.exists() {
        std::fs::write(&types_path, TYPES_LUA).context("Failed to write types.lua")?;
    }

    let luarc_path = config_dir.join(".luarc.json");
    if !luarc_path.exists() {
        std::fs::write(&luarc_path, LUARC_JSON).context("Failed to write .luarc.json")?;
    }

    Ok(())
}
