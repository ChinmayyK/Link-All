#!/usr/bin/env bash
# Link All for Linux: installs into your home folder, no root needed.
#
#   ./install.sh               build from source (if needed) and install
#   ./install.sh --uninstall   remove everything this script installed
#
# Works from a source checkout (builds with cargo) or from the release tarball
# (binaries next to this script). Needs GTK 4.12+ and libadwaita 1.5+, e.g.
# Ubuntu 24.04, Fedora 40, Debian 13, Arch.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="$HOME/.local/bin"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}"
UNIT_DIR="$CONFIG_DIR/systemd/user"
APP_ID="app.linkall.LinkAll"

bold()  { printf '\033[1m%s\033[0m\n' "$*"; }
green() { printf '\033[32m%s\033[0m\n' "$*"; }
red()   { printf '\033[31m%s\033[0m\n' "$*" >&2; }

if [[ "${1:-}" == "--uninstall" ]]; then
    bold "Removing Link All…"
    systemctl --user disable --now linkall-daemon.service 2>/dev/null || true
    pkill -x linkall 2>/dev/null || true
    rm -f "$BIN_DIR/linkall" "$BIN_DIR/linkall-daemon" "$BIN_DIR/linkall-cli"
    rm -f "$UNIT_DIR/linkall-daemon.service"
    rm -f "$DATA_DIR/applications/$APP_ID.desktop" "$DATA_DIR/metainfo/$APP_ID.metainfo.xml"
    rm -f "$CONFIG_DIR/autostart/$APP_ID.desktop"
    rm -f "$DATA_DIR/nautilus/scripts/Send with Link All" "$DATA_DIR/kio/servicemenus/linkall-send.desktop" "$DATA_DIR/nemo/actions/linkall-send.nemo_action"
    find "$DATA_DIR/icons/hicolor" -name "$APP_ID.png" -delete 2>/dev/null || true
    systemctl --user daemon-reload 2>/dev/null || true
    update-desktop-database "$DATA_DIR/applications" 2>/dev/null || true
    green "Link All is removed. Your pairings and history stay in ~/.local/share/linkall; delete that folder to forget them too."
    exit 0
fi

# ── Binaries: next to this script (release tarball) or built from source ──
if [[ -x "$SCRIPT_DIR/linkall" && -x "$SCRIPT_DIR/linkall-daemon" ]]; then
    SRC_BIN="$SCRIPT_DIR"
else
    command -v cargo >/dev/null || { red "Rust isn't installed. Get it from https://rustup.rs, or use the release tarball."; exit 1; }
    for lib in gtk4 libadwaita-1; do
        pkg-config --exists "$lib" 2>/dev/null || {
            red "Missing $lib development files. Install them first:"
            red "  Ubuntu/Debian: sudo apt install libgtk-4-dev libadwaita-1-dev"
            red "  Fedora:        sudo dnf install gtk4-devel libadwaita-devel"
            red "  Arch:          sudo pacman -S gtk4 libadwaita"
            exit 1
        }
    done
    ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
    bold "Building Link All (this takes a few minutes the first time)…"
    (cd "$ROOT" && cargo build --release -p linkall-linux -p linkall-core -p linkall-cli --bin linkall --bin linkall-daemon --bin linkall-cli)
    SRC_BIN="$ROOT/target/release"
fi
DATA_SRC="$SCRIPT_DIR/data"
ICON_SRC="$SCRIPT_DIR/icons/hicolor"

bold "Installing…"
mkdir -p "$BIN_DIR" "$DATA_DIR/applications" "$DATA_DIR/metainfo" "$UNIT_DIR" "$CONFIG_DIR/autostart"
install -Dm755 "$SRC_BIN/linkall" "$BIN_DIR/linkall"
install -Dm755 "$SRC_BIN/linkall-daemon" "$BIN_DIR/linkall-daemon"
[[ -x "$SRC_BIN/linkall-cli" ]] && install -Dm755 "$SRC_BIN/linkall-cli" "$BIN_DIR/linkall-cli"

# Icons, launcher entry and app-store metadata.
for icon in "$ICON_SRC"/*/apps/"$APP_ID.png"; do
    size="$(basename "$(dirname "$(dirname "$icon")")")"
    install -Dm644 "$icon" "$DATA_DIR/icons/hicolor/$size/apps/$APP_ID.png"
done
sed "s|^Exec=linkall|Exec=$BIN_DIR/linkall|" "$DATA_SRC/$APP_ID.desktop" > "$DATA_DIR/applications/$APP_ID.desktop"
install -Dm644 "$DATA_SRC/$APP_ID.metainfo.xml" "$DATA_DIR/metainfo/$APP_ID.metainfo.xml"

# "Send with Link All" in the file manager's right-click menu.
install -Dm755 "$DATA_SRC/file-managers/send-with-link-all" "$DATA_DIR/nautilus/scripts/Send with Link All"
sed -i "s|exec linkall|exec $BIN_DIR/linkall|" "$DATA_DIR/nautilus/scripts/Send with Link All"
install -Dm755 "$DATA_SRC/file-managers/linkall-send.desktop" "$DATA_DIR/kio/servicemenus/linkall-send.desktop"
sed -i "s|^Exec=linkall|Exec=$BIN_DIR/linkall|" "$DATA_DIR/kio/servicemenus/linkall-send.desktop"
install -Dm644 "$DATA_SRC/file-managers/linkall-send.nemo_action" "$DATA_DIR/nemo/actions/linkall-send.nemo_action"
sed -i "s|^Exec=linkall|Exec=$BIN_DIR/linkall|" "$DATA_DIR/nemo/actions/linkall-send.nemo_action"

# Background service from login, and the tray icon at login.
if command -v systemctl >/dev/null && systemctl --user show-environment >/dev/null 2>&1; then
    sed "s|^ExecStart=/usr/bin/linkall-daemon|ExecStart=$BIN_DIR/linkall-daemon|" "$DATA_SRC/linkall-daemon.service" > "$UNIT_DIR/linkall-daemon.service.tmp"
    mkdir -p "$UNIT_DIR" && mv "$UNIT_DIR/linkall-daemon.service.tmp" "$UNIT_DIR/linkall-daemon.service"
    systemctl --user daemon-reload
    systemctl --user enable --now linkall-daemon.service
    systemctl --user restart linkall-daemon.service
fi
mkdir -p "$CONFIG_DIR/autostart"
cat > "$CONFIG_DIR/autostart/$APP_ID.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Link All
Comment=Keeps your devices in sync
Exec=$BIN_DIR/linkall --background
Icon=$APP_ID
X-GNOME-Autostart-enabled=true
NoDisplay=true
EOF

update-desktop-database "$DATA_DIR/applications" 2>/dev/null || true
gtk-update-icon-cache -qtf "$DATA_DIR/icons/hicolor" 2>/dev/null || true

green "Link All is installed. Open it from your apps, or run: linkall"
case ":$PATH:" in *":$BIN_DIR:"*) ;; *) echo "Add $BIN_DIR to your PATH to run linkall-cli from a terminal." ;; esac
echo "Devices connect on TCP 47823 and find each other on UDP 47824 and mDNS. If a firewall is on, allow them:"
echo "  ufw:       sudo ufw allow 47823/tcp && sudo ufw allow 47824/udp && sudo ufw allow mdns"
echo "  firewalld: sudo firewall-cmd --permanent --add-port=47823/tcp --add-port=47824/udp --add-service=mdns && sudo firewall-cmd --reload"
