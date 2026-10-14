#!/usr/bin/env bash
# package.sh <arch>: turns release builds of linkall, linkall-daemon and
# linkall-cli into LinkAll-linux-<arch>.tar.gz and linkall_<version>_<debarch>.deb.
# <arch> is x86_64 or aarch64. Run from the repository root after
#   cargo build --release -p linkall-linux -p linkall-core -p linkall-cli
set -euo pipefail
ARCH="$1"
case "$ARCH" in x86_64) DEB_ARCH=amd64 ;; aarch64) DEB_ARCH=arm64 ;; *) echo "unknown arch $ARCH" >&2; exit 1 ;; esac
VERSION="$(bash scripts/bump-version.sh --current)"
BIN=target/release
L=platforms/linux
APP_ID=app.linkall.LinkAll
rm -rf dist && mkdir -p dist

# ── Tarball: binaries plus the per-user installer ──
T=dist/linkall
mkdir -p "$T"
install -m755 "$BIN/linkall" "$BIN/linkall-daemon" "$BIN/linkall-cli" "$T/"
install -m755 "$L/install.sh" "$T/"
cp -r "$L/data" "$T/data"
mkdir -p "$T/icons" && cp -r "$L/icons/hicolor" "$T/icons/"
cp README.md LICENSE "$T/"
tar czf "LinkAll-linux-$ARCH.tar.gz" -C dist linkall

# ── Debian package: system-wide ──
D=dist/deb
install -Dm755 "$BIN/linkall" "$D/usr/bin/linkall"
install -Dm755 "$BIN/linkall-daemon" "$D/usr/bin/linkall-daemon"
install -Dm755 "$BIN/linkall-cli" "$D/usr/bin/linkall-cli"
install -Dm644 "$L/data/$APP_ID.desktop" "$D/usr/share/applications/$APP_ID.desktop"
install -Dm644 "$L/data/$APP_ID.metainfo.xml" "$D/usr/share/metainfo/$APP_ID.metainfo.xml"
install -Dm644 "$L/data/linkall-daemon.service" "$D/usr/lib/systemd/user/linkall-daemon.service"
install -Dm755 "$L/data/file-managers/linkall-send.desktop" "$D/usr/share/kio/servicemenus/linkall-send.desktop"
install -Dm644 "$L/data/file-managers/linkall-send.nemo_action" "$D/usr/share/nemo/actions/linkall-send.nemo_action"
install -Dm755 "$L/data/file-managers/send-with-link-all" "$D/usr/share/linkall/nautilus-script"
for icon in "$L"/icons/hicolor/*/apps/"$APP_ID.png"; do
    size="$(basename "$(dirname "$(dirname "$icon")")")"
    install -Dm644 "$icon" "$D/usr/share/icons/hicolor/$size/apps/$APP_ID.png"
done
install -Dm644 LICENSE "$D/usr/share/doc/linkall/copyright"
mkdir -p "$D/DEBIAN"
cat > "$D/DEBIAN/control" <<CONTROL
Package: linkall
Version: $VERSION
Section: net
Priority: optional
Architecture: $DEB_ARCH
Maintainer: Chinmay Kudalkar <noreply@linkall.chinmaykudalkar.com>
Homepage: https://linkall.chinmaykudalkar.com
Depends: libgtk-4-1 (>= 4.12), libadwaita-1-0 (>= 1.5), libc6 (>= 2.35)
Recommends: avahi-daemon
Description: One clipboard, files and notifications across your devices
 Link All links your Linux computer with your Android phone, Windows PC and
 Mac over your own network, encrypted end to end: a shared clipboard with
 history, file and folder transfers, phone notifications and calls on the
 desktop, and browsing files on your other devices. No cloud, no account.
CONTROL
cat > "$D/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e
command -v update-desktop-database >/dev/null && update-desktop-database -q /usr/share/applications || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -qtf /usr/share/icons/hicolor || true
# Start the background service at login for every user; the app also starts it on demand.
command -v systemctl >/dev/null && systemctl --global enable linkall-daemon.service >/dev/null 2>&1 || true
POSTINST
cat > "$D/DEBIAN/prerm" <<'PRERM'
#!/bin/sh
set -e
command -v systemctl >/dev/null && systemctl --global disable linkall-daemon.service >/dev/null 2>&1 || true
PRERM
chmod 755 "$D/DEBIAN/postinst" "$D/DEBIAN/prerm"
dpkg-deb --root-owner-group --build "$D" "linkall_${VERSION}_${DEB_ARCH}.deb"
ls -la "LinkAll-linux-$ARCH.tar.gz" "linkall_${VERSION}_${DEB_ARCH}.deb"
