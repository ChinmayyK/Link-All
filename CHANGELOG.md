# Changelog

All notable changes to Link All (called Deskdrop until 1.4.0) are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning follows [Semantic Versioning](https://semver.org/).

---

## [Unreleased]
### Changed
- **All platforms:** Deskdrop is now Link All. Existing installs keep their pairings, settings and history; the data folder moves from `deskdrop` to `linkall` on first launch. The Android app has a new ID (`app.linkall`) and installs beside the old one, so pair it once more and remove the old app. Received files now go to `Downloads/Link All`. Release downloads are named `LinkAll-*`.
### Removed
- **All platforms:** The phone camera stream to the computer (Continuity Camera), including the macOS virtual camera extension. Android no longer asks for camera access.
### Fixed
- **All platforms:** A phone with its screen off no longer drops off the computer after 30 seconds of quiet.

## [1.4.0] - 2026-10-02
### Added
- **All platforms:** Send whole folders. A folder keeps its tree on the other device, a second copy lands in "Name (2)", and each side shows one progress row and one notification per folder. Accepting, declining or cancelling one file answers for the whole folder.
- **All platforms:** A health banner says what stops sync right now (no network, listener down, sync paused, devices not found, connection blocked) and the one thing to do about it.
- **All platforms:** Each device shows its operating system's icon.
- **Windows:** Clipboard images sync both ways, including Win+Shift+S screenshots. Images in history copy again when clicked.
- **Windows:** Phone notifications and incoming calls show as Windows notifications.
- **Windows:** The welcome guide opens on first run until a device is paired, and Settings can show it again. The installer adds a desktop shortcut.
- **Android:** Pin activity items to the top; pinned items are never trimmed or cleared and survive restarts.
- **Home screens:** Each app's home leads with this device's identity ("Visible as"), then Send (Files & folders, Clipboard, and Camera or Browse device), Transferring, Your devices and Recent.
- **Modals:** Android, macOS and Windows each use one custom modal style for sending, pairing, device actions and questions. Sending defaults to all connected devices.

### Changed
- **Older installs:** A device reinstalled or renamed no longer shows up as several paired devices; old copies of it are retired automatically.
- **Pairing:** A device on an older version is told it needs an update instead of failing silently.
- **Windows:** The system tray helper is now a native Rust exe (`platforms/windows/tray`, ~0.3 MB) instead of a WinForms app with its own self-contained .NET runtime (~110 MB). Same process name, pipes and menu.
- **Windows:** Only English language resources ship; WinUI's built-in control text shows in English on other display languages.
- **Windows:** The app targets .NET 10 and publishes with Native AOT: `Deskdrop.exe` is native code and no .NET runtime ships with it. Daemon requests are built as `JsonObject`s and all JSON reading goes through a source-generated `DeskdropJsonContext`, so nothing relies on reflection. The unused `System.Drawing.Common` package is removed.
- **Windows:** The Rust engine DLL and tray helper link the C runtime statically, so they no longer need `vcruntime140.dll` (missing on some clean installs).
- **Build:** Windows releases ship as a per-user MSI and a portable zip of the whole app folder instead of a single-file `Deskdrop.exe`. The MSI now installs every published file; before it listed only `Deskdrop.exe` and `deskdrop_core.dll`.

- **Android (battery):** No Wi-Fi lock is held while idle; only a transfer that is moving bytes takes one. The clipboard is polled at full rate only when it can be read (app in the foreground, an enabled accessibility service, or Android 9 and below), otherwise every 2 s instead of 2–5 times a second. Notification mirroring skips group summaries, media/progress/navigation updates and re-posts with unchanged text. Battery level is reported in 5% steps while the screen is off.
- **Core (battery):** LAN discovery sweeps (~250 TCP connects each) pause while the device sleeps, and the steady UDP discovery beacon slows from every 15 s to every 60 s.

### Fixed
- **Core:** Sending many files at once no longer loses about a third of them (a race accepted the same transfer twice).
- **Core:** Pausing a large transfer near the end and resuming it no longer fails, and resuming no longer rehashes the whole file.
- **macOS:** The transfer pill no longer stays on screen after a transfer finishes, and it counts a folder's files instead of the few in flight.
- **Android:** Folder progress keeps in step with the other device instead of sitting still during big files.
- **Android (Play build):** No longer asks for permissions Play restricts (all-files media, contacts, full-screen intents, battery exemption, background clipboard, accessibility); data shared with calls and notification mirroring is explained before it is turned on.
- **Android:** The benchmark module failed to configure (`missingDimensionStrategy` outside `defaultConfig`), which broke every Android build after the `full`/`play` flavors were added.
- **Windows:** The tray menu's "Rescan Network" now rescans. It used to post to a local HTTP port nothing listened on.
- **Build:** A plain `dotnet build` no longer writes a stale 113 MB copy of the tray helper into `publish\`.
- **Core:** Transfers interrupted by a reconnect now resume reliably; about half of them used to end with "missing chunks". Three races are closed: the old connection's teardown no longer pauses a transfer the new connection already resumed, a stale send loop can no longer skip chunks after a resume, and chunks still arriving on the old connection are dropped instead of being queued on a disk writer that is about to be torn down (which left the resumed copies skipped as duplicates).
- **Windows:** The taskbar progress bar uses source-generated COM interop; the old `[ComImport]` code is not supported under Native AOT and would have thrown on the first transfer. Value converters are `partial` for the same reason.
- **Windows:** The MSI now builds with WiX 5 (it had an invalid XML comment, an unsupported `Condition` attribute, and a `<Files>` path that produced an empty installer), and installing a rebuilt MSI of the same version replaces the installed app instead of registering a second Deskdrop entry. The native `Deskdrop.pdb` (~52 MB) no longer ships.

## [1.3.3] - 2026-09-05
### Fixed
- **Android:** Fix release build workflow by targeting `:app:assembleRelease` directly to avoid building the broken benchmark module.

## [1.3.2] - 2026-09-05
### Fixed
- **Android:** Added `-dontwarn` rules for the `benchmark` module's R8 pass (`androidx.profileinstaller.ProfileInstallReceiver`, `androidx.startup.Initializer`, `com.google.errorprone.annotations.MustBeClosed`) — optional classes referenced by `androidx.benchmark:benchmark-macro-junit4` that aren't on the classpath and aren't needed at runtime, which made `minifyReleaseWithR8` fail once shrinking was enabled there in 1.3.1.

## [1.3.1] - 2026-09-05
### Fixed
- **Android:** Release builds are now signed with the release keystore instead of the debug key; the build also fails fast if the keystore is configured but any of the store password, key alias, or key password is missing.
- **Android:** Enabled R8 shrinking in the `benchmark` module's release build type so it matches `app:release`, fixing a `checkTestedAppObfuscationRelease` failure in the release build.

## [1.3.0] - 2026-09-04
### Added
- **Core:** Known devices now display their trust fingerprint and first-seen date.

### Fixed
- **Core:** Closed a race in `PeerManager::replace_live_session` where two concurrent sessions for the same peer could each read the same stale state, both pass the tie-break check, and both insert — letting whichever insert landed last win the live slot regardless of which one the tie-break rule selected.
- **Core:** Closed a race where a stale failure report from a losing multi-address connect attempt could mark a peer `Failed` immediately after it had successfully connected.
- **Core:** Reconnect logging no longer floods during transient network interface flaps.

### Changed
- **Windows:** Diagnostic tracing consolidated into a single `TraceLog` writer instead of ad-hoc `File.AppendAllText` calls scattered across the UI layer; daemon calls that previously ran fire-and-forget now report failures through `DaemonActions`.
- **Build:** Every shippable artifact now carries a single shared version. The Rust crates inherit it from `[workspace.package]`, and `scripts/bump-version.sh --check` gates CI so the components cannot drift apart again.

### Removed
- Stale build artifacts (prebuilt `.zip`/`.dll`/`.exe`/`.pdb`) are no longer tracked in the repository, along with one-shot migration scripts and dated internal audit documents.

## [1.2.8] - 2026-08-24
### Fixed
- **Core:** Fixed a bug where untrusted discovery beacons (mDNS/UDP) were incorrectly saved as "remembered" peers, causing phantom devices to appear in the Known Devices list.

## [1.2.7] - 2026-08-24
### Fixed
- **Core:** Fix intermittent e2e test failure (test_tier4_scenario_device_reconnect_retry).

## [1.2.6] - 2026-08-24
### Fixed
- **Core:** Fix formatting errors that broke CI pipeline (rustfmt).

## [1.2.5] - 2026-08-24
### Added
- **Core:** Added camera stream request API to FFI.
- **Android:** Redesigned Ecosystem header with a sleek segmented control.
- **Android:** Upgraded "JUST COPIED" card styling and contrast.
- **Android:** Disabled mesh gradient in pure dark mode for a true black background.
- **Windows:** Comprehensive UI overhaul for the WinUI dashboard and dialogs.

## Pre-1.0 history

The entries below predate the 1.0 release and were never assigned version
headings. They accumulated under an `[Unreleased]` marker that went stale as
releases shipped around it. Kept for reference; `git log` is authoritative for
which release first contained a given change.

### Added (v4)
- **Android:** Added "Finish Onboarding" re-entry flow to empty dashboard.
- **Android:** Added OEM-specific battery restriction diagnostics (Xiaomi, Samsung) to help prevent background service termination.
- **macOS:** Implemented `ProcessInfo.beginActivity` to prevent App Nap from throttling the background daemon.
- **Windows:** Implemented `SetThreadExecutionState` to prevent Modern Standby sleep during active file transfers.
- **Core:** Received files are now saved directly to the root `Downloads` directory across all platforms.

### Added (v3)
- `ClipboardContent::is_empty()` — single guard method, eliminating per-call-site duplication (#16)
- `CompressionStats: Display` — consistent log format, usable in IPC responses (#12)
- `QualityProbe::degraded_from()` — detect link quality regressions between probe cycles with `quality_severity` ordering (#13)
- `PeerManager::sync_eligible_count()` — O(1) count of connected+trusted+sync-enabled peers (#14)
- TCP connect timeout (5 s) via `tokio::time::timeout` on all outbound connects (#1)
- TCP keepalive (`SO_KEEPALIVE`, idle 30 s / interval 5 s / 3 probes) on both accept and connect paths (#9)
- Unit tests: `network.rs` (framing, nonce XOR, nonce-echo verification, handshake integration), `discovery.rs` (address preference, version validation), `peer_manager.rs` (`connected_count`, `sync_eligible_count`), `probe.rs` (`degraded_from`, quality ordering), `compress.rs` (Display), `chunked.rs` (size cap rejection), `identity.rs` (fingerprint format), `protocol.rs` (is_empty)

### Fixed (v3)
- **Critical** — Handshake nonce verification was a stub; replay protection now enforced on the initiator side (#2)
- **Critical** — `set_nodelay(true)` was absent on the outbound (initiator) TCP path, adding ~40 ms Nagle delay (#3)
- **Critical** — `pairing.rs` `expire_stale()` silently dropped oneshot channels; now sends explicit `false` so waiters receive `Ok(false)` instead of a channel error (#5)
- **Critical** — `fuzz_sanity_test.rs` used `ExtensionFilter` before it existed; `bincode` added to `[dev-dependencies]` so integration tests compile without a separate crate override (#4)
- **Security** — mDNS version TXT record `v` was advertised but never validated on the browsing side; incompatible peers are now skipped at mDNS time (#6)
- **Security** — mDNS address selection used `HashSet::iter().next()` (arbitrary order); now prefers IPv4 → IPv6 global → IPv6 link-local to avoid silent connect failures from fe80:: addresses without a scope_id (#7)
- **Security** — `Reassembler::feed(Start)` had no total-payload size cap; malicious peers could announce `u64::MAX` bytes and tie up in-flight state indefinitely; capped at 512 MB / 8 192 chunks (#8)
- **Correctness** — `fingerprint_display()` doc comment showed wrong format (16 pairs of 2 chars) vs actual output (8 groups of 4 chars); corrected and test added (#10)
- **Correctness** — `crypto_vectors_test.rs` session key vector was a non-asserting stub; real HKDF-SHA256 vector computed and asserted (#11)
- **Correctness** — CLI unknown command printed bare `"Unknown command: foo"` with no quoting and wrong help hint; now prints `"Unknown command: 'foo'\n\nRun \`deskdrop-cli help\` to see all available commands."` (#15)

- Rust core engine (`deskdrop-core`) with:
  - X25519 ephemeral ECDH key exchange
  - HKDF-SHA256 session key derivation
  - ChaCha20-Poly1305 AEAD encryption with monotonic nonce counter
  - Replay attack protection
  - mDNS-SD service discovery (`_deskdrop._tcp.local.`)
  - Framed TCP transport with sub-500 ms LAN propagation
  - TOFU (Trust On First Use) device trust model
  - PIN-based pairing as alternative to TOFU
  - Chunked transfer for payloads > 128 KB (streaming, pipeline-friendly)
  - Echo suppression (deduplication) with per-peer rate limiting
  - Clipboard history ring buffer (100 entries, persisted as NDJSON)
  - Per-peer metrics: latency p50/p95, throughput, session duration
  - Bandwidth throttle (token bucket, default 4 MB/s for large payloads)
  - Content filter chain: size limits, type allow-list, extension block-list,
    optional sensitive-text heuristics
  - Connection retry with exponential back-off and ±25 % jitter
  - Settings system with atomic JSON persistence and hot-patch API
  - Unix domain socket IPC server (CLI ↔ daemon)
  - C FFI exports for macOS / Windows platform wrappers
  - JNI bridge for Android

- **macOS** platform (`platforms/macos/`):
  - Menu-bar–only app (LSUIElement)
  - NSPasteboard watcher at 100 ms poll
  - TOFU sheet with fingerprint display
  - SwiftUI clipboard history popover (last 100 entries, searchable)
  - SwiftUI preferences panel with live settings editing
  - Hardened runtime + App Sandbox entitlements
  - Universal dylib (arm64 + x86_64) build script
  - Code-signing and optional DMG packaging

- **Windows** platform (`platforms/windows/`):
  - System-tray WinForms application
  - Win32 clipboard sequence-number watcher
  - P/Invoke bridge to Rust DLL
  - TOFU MessageBox with fingerprint
  - Clipboard history floating panel (searchable, re-push)
  - Preferences dialog with Registry persistence
  - Auto-start via `HKCU\...\Run`
  - Named-pipe IPC client (`DaemonClient`)
  - Daemon status poller (`DaemonPoller`)
  - WiX v4 MSI installer with firewall exception

- **Android** platform (`platforms/android/`):
  - Foreground service with `foregroundServiceType=dataSync`
  - JNI calls into libdeskdrop_core.so
  - Clipboard monitoring via `ClipboardManager`
  - PIN-based pairing full-screen activity
  - Settings activity
  - Auto-start `BootReceiver`
  - Notification channel (low-importance, persistent)

- **Linux** platform (`platforms/linux/`):
  - Headless daemon mode (arboard for X11 + Wayland)
  - `notify-send` desktop notifications
  - `.desktop` file for XDG autostart
  - systemd user service unit with security hardening

- **CLI** (`deskdrop-cli`):
  - `status`, `ping`, `push`, `peers`
  - `devices list`, `devices revoke`
  - `history`, `history --last N`, `history --search`, `history clear`
  - `settings get/set/reset`
  - `sync on/off`, `stop`
  - Live IPC when daemon running; offline file-read fallback

- **CI / CD** (`.github/workflows/`):
  - `ci.yml`: Rust fmt/clippy/test on Linux, macOS, Windows;
    Android cross-compile for arm64/armv7/x86_64;
    macOS Swift type-check; Windows dotnet build;
    `cargo audit` security scan; SBOM generation
  - `release.yml`: Produces universal macOS dylib, Windows zip + MSI,
    Android APK, Linux tarball; creates GitHub Release with all artifacts

- **Tests**:
  - Unit tests in every module (crypto, trust, dedup, chunked, settings,
    history, metrics, filter, retry, throttle, pairing, sim)
  - In-process `SimNetwork` harness for deterministic two-node tests
  - Criterion benchmarks: X25519 handshake, ChaCha20 encrypt/decrypt
    (1 KB – 4 MB), chunk reassembly, content hashing, dedup
  - Integration test: two real engine instances exchanging clipboard text

---

## Links

- [Security Policy](SECURITY.md)
- [Contributing](CONTRIBUTING.md)
- [GitHub](https://github.com/deskdrop/deskdrop)
