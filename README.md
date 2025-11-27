<br />
<div align="center">
<a href="https://github.com/hendrikboeck/bluetooth-timeout">
    <img src="https://raw.githubusercontent.com/hendrikboeck/bluetooth-timeout/main/.github/md/icon_x1024.png" alt="Logo" width="128" height="128">
</a>

<h1 align="center">bluetooth-timeout <code>v0.1.3</code></h1>

<p align="center">
    Bluetooth Timeout Daemon for Linux <i>(written in Rust, btw.)</i>
</p>
</div>

## Table of Contents

- [Description](#description)
- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Configuration](#configuration)
- [Usage](#usage)
- [Logging](#logging)
- [Development](#development)

## Description

`bluetooth-timeout` is a lightweight Rust daemon for Linux that automatically turns off your Bluetooth adapter after a configurable period of inactivity (i.e., when the adapter is powered on but no devices are connected).

It integrates with the system D-Bus to monitor Bluetooth state (adapter on/off, device connect/disconnect) and uses that signal stream to reset or cancel the shutdown timer immediately. Before disabling the adapter, it sends desktop notifications (5m, 1m, 30s, etc.).

Internally, the service is built on `tokio`’s async runtime: when there are no relevant Bluetooth D-Bus events coming in, the async tasks simply park. Under the hood this means the threads are suspended by the OS event loop (`epoll`) until a matching D-Bus signal arrives, so the daemon is effectively idle, basically near-zero CPU/power usage, with only a small, steady RAM footprint (~12M).

It’s designed to run as a user-level `systemd` service and is configured via a simple YAML file (timeout duration, notification behavior, and D-Bus paths).

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
    This command builds the release binary, installs it to `~/.local/bin`, copies the service file to `~/.config/systemd/user`, and enables the service.

    ```sh
    just install
    ```

    _Note: The build process temporarily moves `.cargo/config.toml` to avoid conflicts with unstable Tokio flags during release builds._

## Configuration

The configuration file is located at `~/.config/bluetooth-timeout/config.yml` (created automatically during installation with `just install`).

You can modify the timeout duration (in seconds) in [contrib/config.yml](contrib/config.yml):

```yaml
timeout: 5m

notifications:
  enabled: true
  at:
    - 5m
    - 1m
    - 30s
    - 10s

dbus:
  service: org.bluez
  adapter_iface: org.bluez.Adapter1
  adapter_path: /org/bluez/hci0
  device_iface: org.bluez.Device1
```

`just install` copies this file to the appropriate XDG config directory if it doesn't already exist (does not check backwards compatibility). To manually overwrite the config file, you can copy it yourself (e.g.):

```sh
cp contrib/config.yml $XDG_CONFIG_HOME/bluetooth-timeout/config.yml
```

See [`src/configuration.rs`](src/configuration.rs) for implementation details.

## Usage

Once installed, the service runs automatically in the background. You can manage it using `just` commands or standard `systemctl` commands.

| Action           | Just Command     | Systemd Command                                      |
| :--------------- | :--------------- | :--------------------------------------------------- |
| **Install**      | `just install`   | _(See Installation steps above)_                     |
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

In debug mode, the configuration is read from [`contrib/config.yml`](contrib/config.yml) in the current directory instead of the XDG config path.

