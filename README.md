# Bluetooth Timeout Daemon

`bluetooth-timeout` is a lightweight Rust daemon for Linux that automatically turns off your Bluetooth adapter after a configurable period of inactivity (i.e., when the adapter is powered on but no devices are connected).

It integrates with the system D-Bus to monitor Bluetooth state and sends desktop notifications to warn you before the adapter is disabled.

## Features

- **Automatic Power Saving**: Turns off Bluetooth if no devices are connected for a set duration.
- **Smart Monitoring**: Listens for D-Bus signals (Adapter On/Off, Device Connect/Disconnect) to reset or cancel timers immediately.
- **Desktop Notifications**: Sends warnings at 5 minutes, 1 minute, 30 seconds, and 10 seconds before timeout.
- **Systemd Integration**: Runs as a user-level systemd service.
- **Configurable**: YAML-based configuration for timeout duration and D-Bus paths.

## Prerequisites

- **Linux** with **BlueZ** (standard Bluetooth stack).
- **Rust** (latest stable) and Cargo.
- **Just** (command runner) - Recommended for building and installing.
- A notification daemon (e.g., `dunst`, `mako`, or GNOME/KDE built-in) to see the warnings.

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

The configuration file is located at `~/.config/bluetooth-timeout/config.yml` (created automatically during installation).

You can modify the timeout duration (in seconds) in [config.yml](config.yml):

```yaml
# Number of seconds before timeout (default: 300s / 5 minutes)
timeout_s: 300

dbus:
  service: org.bluez
  adapter_iface: org.bluez.Adapter1
  adapter_path: /org/bluez/hci0
  device_iface: org.bluez.Device1
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

Logs are handled by [`src/log.rs`](src/log.rs).

- **Stdout**: Logs are printed to stdout, which `systemd` captures. View them with `just logs`.
- **File**:
  - **Release mode**: Logs are written to `~/.local/share/bluetooth-timeout/bluetooth-timeout.log`.
  - **Debug mode**: Logs are written to `bluetooth-timeout.log` in the project directory.

## Development

To run the project locally in debug mode:

```sh
cargo run
```

In debug mode, the configuration is read from [`config.yml`](config.yml) in the current directory instead of the XDG config path.

## Project Structure

- [`src/main.rs`](src/main.rs): Entry point.
- [`src/bluetooth/observer.rs`](src/bluetooth/observer.rs): Monitors D-Bus for signals (InterfaceAdded, PropertiesChanged).
- [`src/bluetooth/service.rs`](src/bluetooth/service.rs): State machine logic (Idle vs Running).
- [`src/timeout.rs`](src/timeout.rs): Handles the countdown timer and desktop notifications.
- [`contrib/bluetooth-timeout.service`](contrib/bluetooth-timeout.service): Systemd unit file.
