#!/usr/bin/env bash
# Deskdrop Linux installer
# Usage: ./install.sh [--uninstall]
#
# Installs the daemon binary, systemd user service, and .desktop file.
# Does NOT require root — everything goes into ~/.local.

set -euo pipefail

BIN_NAME="deskdrop-gtk"
CLI_NAME="deskdrop-cli"
INSTALL_DIR="$HOME/.local/bin"
SERVICE_DIR="$HOME/.config/systemd/user"
DESKTOP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ── Colour helpers ────────────────────────────────────────────────────────────

green()  { echo -e "\033[32m$*\033[0m"; }
yellow() { echo -e "\033[33m$*\033[0m"; }
red()    { echo -e "\033[31m$*\033[0m"; }
bold()   { echo -e "\033[1m$*\033[0m"; }

# ── Uninstall ─────────────────────────────────────────────────────────────────

if [[ "${1:-}" == "--uninstall" ]]; then
    bold "Uninstalling Link All…"
    systemctl --user stop    deskdrop.service 2>/dev/null || true
    systemctl --user disable deskdrop.service 2>/dev/null || true
    rm -f "$SERVICE_DIR/deskdrop.service"
    rm -f "$DESKTOP_DIR/deskdrop.desktop"
    find "$ICON_DIR" -name deskdrop.png -path '*/apps/*' -delete 2>/dev/null || true
    rm -f "$INSTALL_DIR/$BIN_NAME"
    rm -f "$INSTALL_DIR/$CLI_NAME"
    systemctl --user daemon-reload 2>/dev/null || true
    update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
    green "Link All uninstalled."
    exit 0
fi

# ── Pre-flight checks ─────────────────────────────────────────────────────────

bold "Link All Linux Installer"
echo ""

# Check for required tools.
for cmd in systemctl notify-send; do
    if ! command -v "$cmd" &>/dev/null; then
        yellow "Warning: '$cmd' not found — some features may not work."
    fi
done

# Check if binary exists in the build output.
RELEASE_BIN="$SCRIPT_DIR/target/release/$BIN_NAME"
RELEASE_CLI="$SCRIPT_DIR/target/release/$CLI_NAME"

if [[ ! -f "$RELEASE_BIN" ]]; then
    yellow "Binary not found at $RELEASE_BIN"
    echo "Building release binary…"
    (cd "$SCRIPT_DIR" && cargo build --release 2>&1) || {
        red "Build failed. Run 'cargo build --release' manually and retry."
        exit 1
    }
fi

# ── Install ───────────────────────────────────────────────────────────────────

mkdir -p "$INSTALL_DIR" "$SERVICE_DIR" "$DESKTOP_DIR" "$ICON_DIR"

# Ensure ~/.local/bin is on PATH.
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    yellow "Note: $INSTALL_DIR is not on your PATH."
    echo "  Add this to ~/.bashrc or ~/.profile:"
    echo "    export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

# Daemon binary.
echo "Installing $BIN_NAME → $INSTALL_DIR/$BIN_NAME"
install -m755 "$RELEASE_BIN" "$INSTALL_DIR/$BIN_NAME"

# CLI binary (if built).
if [[ -f "$RELEASE_CLI" ]]; then
    echo "Installing $CLI_NAME → $INSTALL_DIR/$CLI_NAME"
    install -m755 "$RELEASE_CLI" "$INSTALL_DIR/$CLI_NAME"
fi

# Systemd user service.
echo "Installing systemd user service…"
# Substitute actual binary path.
sed "s|/usr/local/bin/deskdrop-gtk|$INSTALL_DIR/$BIN_NAME|g" \
    "$SCRIPT_DIR/deskdrop.service" > "$SERVICE_DIR/deskdrop.service"

# Icons: the Deskdrop logo at each size the desktop asks for.
echo "Installing icons…"
mkdir -p "$ICON_DIR"
cp -r "$SCRIPT_DIR/icons/hicolor/." "$ICON_DIR/"
gtk-update-icon-cache -q -t "$ICON_DIR" 2>/dev/null || true

# .desktop file.
echo "Installing desktop entry…"
sed "s|/usr/local/bin/deskdrop-gtk|$INSTALL_DIR/$BIN_NAME|g;s|/usr/local/bin/deskdrop-cli|$INSTALL_DIR/$CLI_NAME|g" \
    "$SCRIPT_DIR/deskdrop.desktop" > "$DESKTOP_DIR/deskdrop.desktop"

# ── Enable service ────────────────────────────────────────────────────────────

systemctl --user daemon-reload

if systemctl --user is-active deskdrop.service &>/dev/null; then
    echo "Restarting Link All service…"
    systemctl --user restart deskdrop.service
else
    echo "Enabling and starting Link All service…"
    systemctl --user enable --now deskdrop.service
fi

update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
xdg-mime default deskdrop.desktop x-scheme-handler/deskdrop 2>/dev/null || true

# ── Done ──────────────────────────────────────────────────────────────────────

echo ""
green "✅ Link All installed successfully."
echo ""
echo "  Status: systemctl --user status deskdrop"
echo "  Logs:   journalctl --user -u deskdrop -f"
echo "  Stop:   systemctl --user stop deskdrop"
echo "  Remove: $SCRIPT_DIR/install.sh --uninstall"
echo ""
echo "Link All is now running in the background."
echo "It will discover nearby devices automatically via mDNS."
