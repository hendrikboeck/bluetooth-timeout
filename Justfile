BIN_NAME := "bluetooth-timeout"

SERVICE_NAME := "bluetooth-timeout.service"
INSTALL_DIR := "$HOME/.local/bin"
SYSTEMD_USER_DIR := "$HOME/.config/systemd/user"
CONFIG_DIR := "$HOME/.config/" + BIN_NAME

default: build

# Build the Rust binary in release mode, temporarily disabling .cargo/config.toml
build:
    if [ -f .cargo/config.toml ]; then \
        echo "Temporarily disabling .cargo/config.toml"; \
        mv .cargo/config.toml .cargo/config.toml.bak; \
        cargo build --release; \
        mv .cargo/config.toml.bak .cargo/config.toml; \
    else \
        cargo build --release; \
    fi

install: build
    # Shutdown existing service if running
    systemctl --user disable --now {{SERVICE_NAME}} || true

    # Install binary
    mkdir -p {{INSTALL_DIR}}
    cp target/release/{{BIN_NAME}} {{INSTALL_DIR}}/{{BIN_NAME}}

    # Migrate config (handles YAML→Lua, version upgrades, fresh install)
    {{INSTALL_DIR}}/{{BIN_NAME}} migrate

    # Install systemd user unit
    mkdir -p {{SYSTEMD_USER_DIR}}
    cp contrib/systemd/{{SERVICE_NAME}} {{SYSTEMD_USER_DIR}}/{{SERVICE_NAME}}

    # Reload and enable service
    systemctl --user daemon-reload
    systemctl --user enable --now {{SERVICE_NAME}}

# Migrate the installed configuration to the latest format version
migrate:
    {{INSTALL_DIR}}/{{BIN_NAME}} migrate

start:
    systemctl --user start {{SERVICE_NAME}}

stop:
    systemctl --user stop {{SERVICE_NAME}}

restart:
    systemctl --user restart {{SERVICE_NAME}}

status:
    systemctl --user status {{SERVICE_NAME}}

logs:
    journalctl --user -u {{SERVICE_NAME}} -f

enable:
    systemctl --user enable {{SERVICE_NAME}}

disable:
    systemctl --user disable {{SERVICE_NAME}}

uninstall:
    systemctl --user disable --now {{SERVICE_NAME}} || true
    rm -f {{SYSTEMD_USER_DIR}}/{{SERVICE_NAME}}
    rm -f {{INSTALL_DIR}}/{{BIN_NAME}}
    systemctl --user daemon-reload
