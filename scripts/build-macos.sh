#!/usr/bin/env bash
# build-macos.sh — Build the Link All.app bundle for macOS
#
# Requirements:
#   - Rust toolchain (cargo)
#   - Xcode command-line tools (xcode-select --install)
#   - Apple Developer ID (for notarization — optional for local builds)
#
# Usage:
#   ./build-macos.sh [--release] [--notarize]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CORE_DIR="${REPO_ROOT}/linkall-core"
MACOS_DIR="${REPO_ROOT}/platforms/macos"
SOURCE_DIR_NAME="LinkAll"
PRODUCT_NAME="Link All"
BUILD_TYPE="${1:---release}"
APP_BUNDLE="${MACOS_DIR}/build/${PRODUCT_NAME}.app"
TARGET_DIR="${REPO_ROOT}/target/release"
ICON_SRC="${REPO_ROOT}/platforms/macos/${SOURCE_DIR_NAME}/Resources/AppIconSource.png"
STATUS_ICON_SRC="${MACOS_DIR}/${SOURCE_DIR_NAME}/Resources/StatusBarSource.png"

log() { echo "▶ $*"; }

# ── 1. Build Rust library + daemon ────────────────────────────────────────────

log "Building Rust core (${BUILD_TYPE})..."
cd "${REPO_ROOT}"
cargo build --release -p linkall-core --features compress --lib --bin linkall-daemon

DYLIB_SRC="${TARGET_DIR}/liblinkall_core.dylib"
DAEMON_SRC="${TARGET_DIR}/linkall-daemon"

# ── 2. Create .app bundle skeleton ───────────────────────────────────────────

log "Creating ${PRODUCT_NAME}.app bundle..."
rm -rf "${APP_BUNDLE}"
mkdir -p "${APP_BUNDLE}/Contents/"{MacOS,Frameworks,Resources}

# Copy dylib.
cp "${DYLIB_SRC}" "${APP_BUNDLE}/Contents/Frameworks/liblinkall_core.dylib"
cp "${DAEMON_SRC}" "${APP_BUNDLE}/Contents/MacOS/linkall-daemon"
chmod +x "${APP_BUNDLE}/Contents/MacOS/linkall-daemon"

# Fix dylib install name.
install_name_tool \
    -id "@rpath/liblinkall_core.dylib" \
    "${APP_BUNDLE}/Contents/Frameworks/liblinkall_core.dylib"

# ── 3. Compile Swift app ─────────────────────────────────────────────────────

log "Compiling Swift sources..."
SWIFT_FILES=()
while IFS= read -r file; do
    # Include all files found in the source directory
    SWIFT_FILES+=("${file}")
done < <(find "${MACOS_DIR}/${SOURCE_DIR_NAME}" -name '*.swift' | sort)

SDK_PATH="$(xcrun --sdk macosx --show-sdk-path)"
MACOS_TARGET="arm64-apple-macos13.0"
MACRO_PLUGIN_DIR="${REPO_ROOT}/.build-tools/swift-plugins"

swiftc \
    "${SWIFT_FILES[@]}" \
    -import-objc-header "${MACOS_DIR}/${SOURCE_DIR_NAME}/LinkAllBridge.h" \
    -sdk "${SDK_PATH}" \
    -target "${MACOS_TARGET}" \
    $( [[ -d "${MACRO_PLUGIN_DIR}" ]] && echo "-plugin-path ${MACRO_PLUGIN_DIR}" ) \
    -framework AppKit \
    -framework SwiftUI \
    -framework Carbon \
    -framework UserNotifications \
    -F "${APP_BUNDLE}/Contents/Frameworks" \
    -L "${APP_BUNDLE}/Contents/Frameworks" \
    -llinkall_core \
    -Xlinker -rpath -Xlinker @executable_path/../Frameworks \
    -o "${APP_BUNDLE}/Contents/MacOS/${PRODUCT_NAME}"

# ── 4. Copy resources ─────────────────────────────────────────────────────────

cp "${MACOS_DIR}/${SOURCE_DIR_NAME}/Info.plist" "${APP_BUNDLE}/Contents/Info.plist"
cp "${STATUS_ICON_SRC}" "${APP_BUNDLE}/Contents/Resources/StatusBarIcon.png"
if [[ -f "${MACOS_DIR}/${SOURCE_DIR_NAME}/Resources/AndroidLogo.png" ]]; then
    cp "${MACOS_DIR}/${SOURCE_DIR_NAME}/Resources/AndroidLogo.png" "${APP_BUNDLE}/Contents/Resources/AndroidLogo.png"
fi

# The in-app logo (CRAppIconMark) in both themes.
for logo in AppIconSource AppIconSourceDark; do
    if [[ -f "${MACOS_DIR}/${SOURCE_DIR_NAME}/Resources/${logo}.png" ]]; then
        cp "${MACOS_DIR}/${SOURCE_DIR_NAME}/Resources/${logo}.png" "${APP_BUNDLE}/Contents/Resources/${logo}.png"
    fi
done

# Generate AppIcon.icns from the bundled source PNG.
if [[ -f "${ICON_SRC}" ]]; then
    log "Generating app icon..."
    ICON_TMP_DIR="$(mktemp -d /tmp/linkall-icon.XXXXXX)"
    ICONSET_DIR="${ICON_TMP_DIR}/AppIcon.iconset"
    mkdir -p "${ICONSET_DIR}"
    for size in 16 32 128 256 512; do
        sips -s format png -z "${size}" "${size}" "${ICON_SRC}" --out "${ICONSET_DIR}/icon_${size}x${size}.png" >/dev/null
        retina_size=$((size * 2))
        sips -s format png -z "${retina_size}" "${retina_size}" "${ICON_SRC}" --out "${ICONSET_DIR}/icon_${size}x${size}@2x.png" >/dev/null
    done
    iconutil -c icns "${ICONSET_DIR}" -o "${APP_BUNDLE}/Contents/Resources/AppIcon.icns"
    rm -rf "${ICON_TMP_DIR}"
fi

# macOS 26 and later: an icon with a light and a dark appearance (Icon
# Composer's .icon, compiled by actool into Assets.car). Older systems keep
# the AppIcon.icns above. Skipped, not fatal, where actool is missing.
ICON_BUNDLE="${MACOS_DIR}/${SOURCE_DIR_NAME}/Resources/AppIcon.icon"
ACTOOL=""
if xcrun --find actool >/dev/null 2>&1; then
    ACTOOL="xcrun actool"
elif [[ -x /Applications/Xcode.app/Contents/Developer/usr/bin/actool ]]; then
    ACTOOL="env DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun actool"
fi
if [[ -d "${ICON_BUNDLE}" && -n "${ACTOOL}" ]]; then
    log "Compiling themed app icon..."
    ICON_OUT="$(mktemp -d /tmp/linkall-appicon.XXXXXX)"
    if ${ACTOOL} "${ICON_BUNDLE}" --compile "${ICON_OUT}" --platform macosx --target-device mac \
           --minimum-deployment-target 26.0 --app-icon AppIcon --include-all-app-icons \
           --output-partial-info-plist "${ICON_OUT}/partial.plist" >/dev/null 2>&1 \
       && [[ -f "${ICON_OUT}/Assets.car" ]]; then
        cp "${ICON_OUT}/Assets.car" "${APP_BUNDLE}/Contents/Resources/Assets.car"
    else
        log "actool could not build the themed icon; using AppIcon.icns only"
    fi
    rm -rf "${ICON_OUT}"
fi

# ── 5. Code sign ─────────────────────────────────────────────────────────────

IDENTITY="${CODESIGN_IDENTITY:-"-"}"   # "-" = ad-hoc for local builds
log "Code signing with identity: ${IDENTITY}"

codesign \
    --force \
    --sign "${IDENTITY}" \
    "${APP_BUNDLE}/Contents/Frameworks/liblinkall_core.dylib"

codesign \
    --force \
    --sign "${IDENTITY}" \
    "${APP_BUNDLE}/Contents/MacOS/linkall-daemon"

codesign \
    --force \
    --sign "${IDENTITY}" \
    --entitlements "${MACOS_DIR}/${SOURCE_DIR_NAME}/LinkAll.entitlements" \
    --options runtime \
    "${APP_BUNDLE}"

# ── 6. Verify ────────────────────────────────────────────────────────────────

log "Verifying bundle..."
codesign --verify --deep --strict "${APP_BUNDLE}"
spctl --assess --type exec "${APP_BUNDLE}" 2>/dev/null || \
    log "(spctl: unsigned build — expected for ad-hoc signing)"

log "✅ Built: ${APP_BUNDLE}"

# ── 7. Optional: create DMG ──────────────────────────────────────────────────

if command -v create-dmg &>/dev/null && [[ "${SKIP_DMG:-}" != "true" ]]; then
    log "Creating DMG..."
    # No --app-drop-link: that symlink points at the shared /Applications,
    # which non-admin (e.g. managed corporate) users can't write to without
    # an administrator password. Link All relocates itself to the per-user
    # ~/Applications on first launch instead (see AppDelegate.
    # relocateToUserApplicationsIfNeeded), so users just double-click it
    # straight from the mounted DMG.
    create-dmg \
        --volname "Link All" \
        --window-size 600 400 \
        --icon-size 128 \
        "${MACOS_DIR}/build/LinkAll.dmg" \
        "${APP_BUNDLE}" || {
            log "create-dmg failed (headless CI), falling back to zip..."
            (cd "${MACOS_DIR}/build" && zip -rq "LinkAll-macOS.zip" "${PRODUCT_NAME}.app")
        }
    log "✅ DMG (or zip fallback) created."
else
    log "Creating zip archive..."
    (cd "${MACOS_DIR}/build" && zip -rq "LinkAll-macOS.zip" "${PRODUCT_NAME}.app")
    log "✅ ZIP: ${MACOS_DIR}/build/LinkAll-macOS.zip"
fi
