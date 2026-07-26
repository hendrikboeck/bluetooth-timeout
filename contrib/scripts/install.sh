#!/usr/bin/env bash
# install.sh — install bluetooth-timeout binary, config, and systemd service
set -euo pipefail

# ------------------------------------------------------------------------------
# Constants
# ------------------------------------------------------------------------------
readonly BIN_NAME="bluetooth-timeout"
readonly SERVICE_NAME="bluetooth-timeout.service"
readonly BIN="$HOME/.local/bin/$BIN_NAME"
readonly UNIT_DIR="$HOME/.config/systemd/user"
readonly CONFIG_DIR="$HOME/.config/$BIN_NAME"
readonly CONFIG_FILE="$CONFIG_DIR/config.lua"
readonly LOCAL_CONFIG_FILE=".local/config/config.lua"

# ANSI escapes
readonly BOLD=$'\033[1m'
readonly DIM=$'\033[2m'
readonly GREEN=$'\033[32m'
readonly YELLOW=$'\033[33m'
readonly BLUE=$'\033[34m'
readonly RESET=$'\033[0m'

# ------------------------------------------------------------------------------
# Output helpers — cargo style: right-aligned verb column (12 chars)
# ------------------------------------------------------------------------------
readonly VW=12

say()   { printf "${GREEN}${BOLD}%${VW}s${RESET} %s\n" "$1" "${2:-}"; }
warn()  { printf "${YELLOW}${BOLD}%${VW}s${RESET} %s\n" "$1" "${2:-}"; }
note()  { printf "%${VW}s ${DIM}%s${RESET}\n" "" "$1"; }
ask()   { printf "${BLUE}${BOLD}%${VW}s${RESET} %s " "$1" "${2:-}"; }
finish(){ printf "${GREEN}${BOLD}%${VW}s${RESET} %s\n" "Finished" "$1"; }

# Run a command and print its output in dim with tree-drawing prefixes.
run() {
    local sh="${SHELL##*/}"; sh="${sh^^}"
    printf "             ${DIM}\033[3m[${sh}]\033[23m %s${RESET}\n" "$*"
    local tmp rc
    tmp="$(mktemp)"
    set +e
    "$@" >"$tmp" 2>&1
    rc=$?
    set -e
    while IFS= read -r line || [ -n "$line" ]; do
        printf "             ${DIM}│${RESET} %s\n" "$line"
    done <"$tmp"
    printf "             ${DIM}└ \033[3m(exit: %d)${RESET}\n" "$rc"
    rm -f "$tmp"
    return "$rc"
}

# ------------------------------------------------------------------------------
# CLI argument parsing
# ------------------------------------------------------------------------------
MIGRATE=false
CONFIG_ACT=""
UNINSTALL=false
REMOVE_CONFIG=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --uninstall)        UNINSTALL=true;           shift ;;
        --remove-config)    REMOVE_CONFIG=true;       shift ;;
        --migrate)          MIGRATE=true;             shift ;;
        --skip-config)      CONFIG_ACT="skip";        shift ;;
        --keep-config)      CONFIG_ACT="keep";        shift ;;
        --overwrite-config) CONFIG_ACT="overwrite";   shift ;;
        -h|--help)
            echo "Usage: install.sh [--migrate] [--skip-config|--keep-config|--overwrite-config]"
            echo "       install.sh --uninstall [--remove-config]"
            exit 0 ;;
        *)
            echo "Unknown option: $1"
            exit 1 ;;
    esac
done

# ------------------------------------------------------------------------------
# Uninstall mode
# ------------------------------------------------------------------------------
if $UNINSTALL; then
    say "Uninstalling" "${BIN_NAME}..."
    systemctl --user disable --now "$SERVICE_NAME" 2>/dev/null || true
    say "Stopped" "service"
    rm -f "$BIN" "$UNIT_DIR/$SERVICE_NAME"
    run systemctl --user daemon-reload
    say "Removed" "${BIN}"

    if [ -d "$CONFIG_DIR" ]; then
        if $REMOVE_CONFIG; then
            rm -rf "$CONFIG_DIR"
            say "Removed" "${CONFIG_DIR}"
        else
            ask "Remove" "config directory? ${DIM}[y/N]${RESET}"
            read -r ans
            if [[ "$ans" =~ ^[Yy] ]]; then
                rm -rf "$CONFIG_DIR"
                say "Removed" "${CONFIG_DIR}"
            else
                warn "Kept" "${CONFIG_DIR}"
            fi
        fi
    fi

    finish "uninstall [${BIN_NAME}]"
    exit 0
fi

# ------------------------------------------------------------------------------
# Install mode
# ------------------------------------------------------------------------------
say "Installing" "${BIN_NAME}..."

systemctl --user disable --now "$SERVICE_NAME" 2>/dev/null || true
say "Stopped" "existing service"

mkdir -p "$(dirname "$BIN")"
cp "target/release/$BIN_NAME" "$BIN"
say "Installed" "${BIN}"

# Config migration
# Local config: choose skip, keep, or overwrite.
if [ -f "$LOCAL_CONFIG_FILE" ] && [ -z "$CONFIG_ACT" ]; then
    local_ver="$(sed -n 's/.*M\.version\s*=\s*"\([0-9]*\)".*/\1/p' "$LOCAL_CONFIG_FILE" | head -1)"
    [ -n "$local_ver" ] && note "Found local config (v${local_ver}) at ${LOCAL_CONFIG_FILE}"
    ask "Choose" "skip, keep, or overwrite? ${DIM}[S/k/o]${RESET}"
    read -r ans
    case "${ans:-}" in
        o*|O*)
            rm -f "$CONFIG_FILE"
            mkdir -p "$CONFIG_DIR"
            cp "$LOCAL_CONFIG_FILE" "$CONFIG_FILE"
            say "Copied" "local config to ${CONFIG_FILE} (overwritten)"
            ;;
        k*|K*)
            mkdir -p "$CONFIG_DIR"
            cp "$LOCAL_CONFIG_FILE" "$CONFIG_FILE"
            say "Copied" "local config to ${CONFIG_FILE}"
            ;;
        *)
            warn "Skipped" "local config"
            ;;
    esac
fi

# Migration prompt (keep mode).
if ! $MIGRATE && [ -z "$CONFIG_ACT" ]; then
    ask "Run" "migration? ${DIM}[y/N]${RESET}"
    read -r ans
    [[ "$ans" =~ ^[Yy] ]] && MIGRATE=true
fi

if $MIGRATE; then
    run "$BIN" migrate
    say "Migrated" "config"
fi

mkdir -p "$UNIT_DIR"
cp "contrib/systemd/$SERVICE_NAME" "$UNIT_DIR/$SERVICE_NAME"
say "Installed" "${UNIT_DIR}/${SERVICE_NAME}"

run systemctl --user daemon-reload
run systemctl --user enable --now "$SERVICE_NAME"
say "Enabled" "and started service"

finish "install [${BIN_NAME}]"
