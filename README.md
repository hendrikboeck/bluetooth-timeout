<br />
<div align="center">
<a href="https://github.com/hendrikboeck/bluetooth-timeout">
    <img src="https://raw.githubusercontent.com/hendrikboeck/bluetooth-timeout/main/.github/md/icon_x1024.png" alt="Logo" width="128" height="128">
</a>

<h1 align="center">bluetooth-timeout <code>v0.2.2</code></h1>

<p align="center">
    Bluetooth Timeout Daemon for Linux <i>(written in Rust, btw.)</i>
</p>
</div>

## Table of Contents

- [Description](#description)
- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Configuration](#configuration)
- [Migration](#migration)
- [Usage](#usage)
- [Logging](#logging)
- [Development](#development)

## Description

`bluetooth-timeout` is a lightweight Rust daemon for Linux that automatically turns off your Bluetooth adapter after a configurable period of inactivity (i.e., when the adapter is powered on but no devices are connected).

It integrates with the system D-Bus to monitor Bluetooth state (adapter on/off, device connect/disconnect) and uses that signal stream to reset or cancel the shutdown timer immediately. Before disabling the adapter, it sends desktop notifications (5m, 1m, 30s, etc.).

Internally, the service is built on `tokio`'s async runtime: when there are no relevant Bluetooth D-Bus events coming in, the async tasks simply park. Under the hood this means the threads are suspended by the OS event loop (`epoll`) until a matching D-Bus signal arrives, so the daemon is effectively idle, basically near-zero CPU/power usage, with only a small, steady RAM footprint (~10M).

It manages all Bluetooth adapters by default (filters can be applied in config). Designed as a user-level `systemd` service, configured via a Lua script.

### Config Schema Versioning

Config files carry a schema version (`M.version = "2"`). Each binary supports one major schema version. The `migrate` subcommand handles upgrades:
- **v2.0** (current): Lua-based config with `find_adapters()` discovery.
- **v1**: Legacy YAML config (auto-migrated to v2 on `migrate`).

## Prerequisites

- **Linux** with **BlueZ** (standard Bluetooth stack).
- **Rust** (latest stable) and Cargo.
- **Just** (command runner) - Recommended for building and installing.
- A notification daemon (e.g., `dunst`, `mako`, `swaync` or GNOME/KDE built-in) to see the warnings.

## Installation

The project uses a [Justfile](Justfile) to automate building and installation.

1.  **Clone the repository:**

    ```sh
    git clone https://github.com/hendrikboeck/bluetooth-timeout.git
    cd bluetooth-timeout
    ```

2.  **Install using Just:**
    This command builds the release binary, installs it to `~/.local/bin`, runs the migration (creates config + LSP files), and enables the systemd service.

    ```sh
    just install
    ```

    _Note: The build process temporarily moves `.cargo/config.toml` to avoid conflicts with unstable Tokio flags during release builds._

## Configuration

Configuration is written in **Lua** at `~/.config/bluetooth-timeout/config.lua`. LSP type annotations are provided alongside (`types.lua`, `.luarc.json`).

The `migrate` command creates these files automatically on first run, or you can upgrade an existing config with `bluetooth-timeout migrate`.

### Example

```lua
---@type BluetoothTimeoutConfig
local M = {}

M.version = "2"

M.timeout = "5m"

-- Auto-discover all Bluetooth adapters
M.adapters = find_adapters()

-- Or filter: only powered adapters
-- M.adapters = find_adapters { powered = true }

-- Or hardcode specific adapters
-- M.adapters = { { path = "/org/bluez/hci0" } }

M.notifications = {
  enabled = true,
  at = { "5m", "1m", "30s", "10s" },
}

M.runtime = {
  multithreaded = false,
}

return M
```

### Adapter Discovery

The `find_adapters()` function discovers Bluetooth adapters via D-Bus at config-load time. It returns an array of adapter objects with the following fields:

| Field          | Type    | Description                             |
| :------------- | :------ | :-------------------------------------- |
| `path`         | string  | D-Bus object path (`/org/bluez/hci0`)   |
| `address`      | string  | MAC address (`00:1A:7D:DA:71:13`)       |
| `name`         | string  | Adapter alias / display name            |
| `powered`      | boolean | Whether the adapter is currently on     |
| `discoverable` | boolean | Whether the adapter is discoverable     |

An optional filter table can be passed to narrow results:

| Filter             | Type    | Description                     |
| :----------------- | :------ | :------------------------------ |
| `name`             | string  | Exact match on adapter name     |
| `name_pattern`     | string  | Lua pattern match on name       |
| `address`          | string  | Exact MAC address match         |
| `address_prefix`   | string  | MAC address prefix match        |
| `powered`          | boolean | Filter by powered state         |
| `discoverable`     | boolean | Filter by discoverable state    |

### Runtime

| Setting          | Default | Description                                                                 |
| :--------------- | :------ | :-------------------------------------------------------------------------- |
| `multithreaded`  | `false` | `false`: single-threaded (ideal for 1-2 adapters). `true`: multi-threaded (3+ adapters). |

See [`contrib/config/v2.0/config.lua`](contrib/config/v2.0/config.lua) for the full template and [`src/configuration.rs`](src/configuration.rs) for implementation details.

## Migration

Config schema versions are tracked independently of the crate version. Run `migrate` to:

- Create a fresh config on new installs.
- Upgrade from v1 (YAML) to v2 (Lua), preserving settings.
- Install/update LSP support files (`types.lua`, `.luarc.json`).

```sh
bluetooth-timeout migrate               # from the installed binary
just migrate                            # or via Justfile
```

The daemon also emits a warning at startup if the config is outdated.

## Usage

Once installed, the service runs automatically in the background. You can manage it using `just` commands or standard `systemctl` commands.

| Action           | Just Command     | Systemd Command                                      |
| :--------------- | :--------------- | :--------------------------------------------------- |
| **Install**      | `just install`   | _(See Installation steps above)_                     |
| **Migrate**      | `just migrate`   | `bluetooth-timeout migrate`                          |
| **Check Status** | `just status`    | `systemctl --user status bluetooth-timeout.service`  |
| **View Logs**    | `just logs`      | `journalctl --user -u bluetooth-timeout.service -f`  |
| **Restart**      | `just restart`   | `systemctl --user restart bluetooth-timeout.service` |
| **Stop**         | `just stop`      | `systemctl --user stop bluetooth-timeout.service`    |
| **Uninstall**    | `just uninstall` | _(See Justfile for cleanup steps)_                   |

## Logging

- **Stdout**: Logs are printed to stdout, which `systemd` captures. View them with `just logs`.
- **File**:
  - **Release mode**: Logs are written to `~/.local/share/bluetooth-timeout/bluetooth-timeout.log`.
  - **Debug mode**: Logs are written to `bluetooth-timeout.log` in the project directory.

## Development

To run the project locally in debug mode:

```sh
cargo run
```

In debug mode, the configuration is read from [`contrib/config/v2.0/config.lua`](contrib/config/v2.0/config.lua) in the current directory instead of the XDG config path.
