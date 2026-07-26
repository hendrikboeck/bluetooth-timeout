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

install *args: build
    bash contrib/scripts/install.sh {{args}}

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

uninstall *args:
    bash contrib/scripts/install.sh --uninstall {{args}}
