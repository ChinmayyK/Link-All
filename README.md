# Link All

<div align="center">

  <p><strong>A local-first, zero-cloud desktop and mobile continuity engine for resumable file transfers, clipboard synchronization, and hardware interaction across mixed operating systems.</strong></p>

  <p><a href="#test-the-google-play-version">Help test the Android app on Google Play</a></p>

  <div>
    <img src="https://img.shields.io/badge/Platform-macOS-lightgrey?style=for-the-badge&logo=apple" alt="macOS" />
    <img src="https://img.shields.io/badge/Platform-Android-brightgreen?style=for-the-badge&logo=android" alt="Android" />
    <img src="https://img.shields.io/badge/Platform-Linux-orange?style=for-the-badge&logo=linux" alt="Linux" />
    <img src="https://img.shields.io/badge/Platform-Windows%20(WinUI%203)-blue?style=for-the-badge&logo=windows" alt="Windows" />
    <img src="https://img.shields.io/badge/Core-Rust-B7410E?style=for-the-badge&logo=rust" alt="Rust Core" />
  </div>

  <br />

  <picture><source media="(prefers-color-scheme: dark)" srcset="assets/screenshots/dark/mac-devices.png" /><img src="assets/screenshots/light/mac-devices.png" width="820" alt="Link All on macOS: this Mac's devices, with a phone's battery and storage, and quick actions" /></picture>

  <table>
    <tr>
      <td align="center"><b>Windows</b></td>
      <td align="center"><b>Android</b></td>
    </tr>
    <tr>
      <td align="center"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/screenshots/dark/win-clipboard.png" /><img src="assets/screenshots/light/win-clipboard.png" width="420" alt="Link All on Windows: shared clipboard history" /></picture><br /><br /><picture><source media="(prefers-color-scheme: dark)" srcset="assets/screenshots/dark/win-devices.png" /><img src="assets/screenshots/light/win-devices.png" width="420" alt="Link All on Windows: home with three connected devices" /></picture></td>
      <td align="center"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/screenshots/dark/android-home.png" /><img src="assets/screenshots/light/android-home.png" width="180" alt="Link All on Android: home, with this phone linked to three devices" /></picture> &nbsp; <picture><source media="(prefers-color-scheme: dark)" srcset="assets/screenshots/dark/android-devices.png" /><img src="assets/screenshots/light/android-devices.png" width="180" alt="Link All on Android: devices" /></picture></td>
    </tr>
    <tr>
      <td align="center" colspan="2"><b>Linux</b></td>
    </tr>
    <tr>
      <td align="center" colspan="2"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/screenshots/dark/linux-devices.png" /><img src="assets/screenshots/light/linux-devices.png" width="640" alt="Link All on Linux: this computer, send actions, paired devices and recent transfers" /></picture></td>
    </tr>
    <tr>
      <td align="center"><b>macOS menu bar</b></td>
      <td align="center"><b>Transfers on macOS</b></td>
    </tr>
    <tr>
      <td align="center"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/screenshots/dark/mac-menubar-card.png" /><img src="assets/screenshots/light/mac-menubar-card.png" width="300" alt="Link All in the macOS menu bar" /></picture></td>
      <td align="center"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/screenshots/dark/mac-transfers-active.png" /><img src="assets/screenshots/light/mac-transfers-active.png" width="420" alt="A transfer in progress on macOS" /></picture></td>
    </tr>
  </table>

</div>

---

## Overview

Link All bridges disparate operating systems (macOS, Windows, Android, Linux) without relying on intermediate cloud infrastructure, proprietary ecosystem lock-in, or external routing servers. All communication takes place point-to-point over local wireless networks (Wi-Fi, LAN, or standalone Wi-Fi Direct / Mobile Hotspot connections).

At its core sits an event-driven asynchronous **Rust daemon (`linkall-core`)**, designed for zero idle resource consumption and low-latency local IPC communication with platform-native graphical frontends. Whether transferring multi-gigabyte payloads or synchronizing high-frequency clipboard updates, Link All relies strictly on local system primitives and explicit end-to-end encryption.

---

## Core Capabilities

Link All focuses primarily on two battle-tested primitives: resilient local file transport and high-speed universal clipboard synchronization.

### 1. Resumable & Zero-Copy File Transport
Sending files across a local network shouldn't stall due to transient packet loss or intermittent interface drops. Link All implements a streaming transport layer engineered for high saturation and resiliency:
- **Zero-Copy Memory Pipelines (`bytes::Bytes`)**: The networking core operates on reference-counted memory slices. Buffer slicing ($O(1)$ complexity) across asynchronous reading channels prevents redundant heap allocation and RAM duplication during massive file relays.
- **Adaptive Chunk Batching (`adaptive_batch_size`)**: Outbound file transmission dynamically adjusts read-ahead queue depth based on live acknowledgment feedback (`next_chunk - last_acked_chunk`). High-bandwidth links scale up concurrent operations, while congested links throttle down automatically to prevent memory spikes or timeout drops.
- **Geometric Socket Auto-Tuning**: Sockets apply geometric buffer fallback progressions (`16 MB` down to `256 KB`), securing the largest OS kernel buffer (`SO_SNDBUF` / `SO_RCVBUF`) supported by the local network interface.
- **Resumable Transfers & Fast Entropy Sampling**: Mid-transfer interruptions resume seamlessly without starting over. Prior to compression stages, the engine performs a brief 4 KB entropy check (`should_try_compress`); incompressible binaries (`.mp4`, `.zip`, `.jpg`) immediately bypass CPU compression cycles.

### 2. Real-Time Universal Clipboard & Timeline
- **Cross-Platform Activity Feed**: Copying text, URLs, or image payloads on one device transmits structured payload metadata across trusted peer sockets in real time. Items appear inside a searchable local history ring buffer with support for pinning and tagging.
- **Configurable Content Filtering (`filter.rs`)**: To safeguard passwords, One-Time Passwords (OTPs), API keys, and credit cards from persisting on unattended hardware, Link All evaluates clipboard contents against local pre-flight heuristic rules and regular expressions before initiating network broadcasts.
- **Deduplicated FFI String Caching**: Across C-FFI boundaries (`PbEvent::cache_str`), common heap strings are recycled during high-frequency IPC updates, avoiding unneeded allocations within UI view loops.

---

## Extended Modules

In addition to core file and clipboard synchronization, Link All includes several secondary modules built atop the secure peer connection layer:
- **Remote File Browsing**: Search, preview and fetch files on a paired device. A phone can browse a computer's Downloads, Documents, Pictures, Videos and Music once that computer turns on **Let devices browse files** (off by default). A computer can browse a phone with the Android build from GitHub Releases; the Google Play build leaves this out, because Play limits all-files access to file managers.
- **Notification & Call Handoff**: Mirror mobile SMS messages, phone call alerts, and application notifications directly to desktop notification centers.
- **Power & Battery Monitoring**: Surface remote hardware battery status (`0–100%`) and charging states directly within desktop system trays.
- **OS-Level Sleep Immunity**: Leverages platform power management primitives (`ProcessInfo.beginActivity` on macOS, `SetThreadExecutionState` on Windows, and selective wake locks on Android) during ongoing bulk transfers to prevent unintended system suspension.

---

## Cryptography & Session Security

Link All operates under a zero-trust model for local wireless broadcast networks. Device discovery over mDNS does not imply trust.

- **End-to-End Encryption**: Session handshakes utilize ephemeral **Curve25519 (X25519) ECDH** key exchanges, **HKDF-SHA256** key derivation, and **ChaCha20-Poly1305** authenticated encryption (`AEAD`).
- **Replay Protection**: All network frames integrate strictly increasing 64-bit counter nonces, rejecting unordered or replayed packet transmission at the parsing layer.
- **Memory Zeroization**: Secret Diffie-Hellman keys and session material are zeroed directly from physical RAM immediately upon derivation or termination via the `zeroize` crate to mitigate memory exposure.
- **mDNS Privacy Enforcer**: Friendly device names are obfuscated during general network broadcasting; only unprivileged UUID identifiers are visible over open mDNS until a cryptographic pairing verification concludes.
- **Code-Verified Pairing**: Pairing shows the same security code on both devices, derived from the encrypted session, so a Man-in-the-Middle shows a different one. A request stays open for 60 seconds on both sides and always ends the same way on each (accepted, declined, withdrawn or expired). Declining means "not now", never a silent block. Pairing can also be done by scanning a QR code, valid for 10 minutes.
- **Reinstall-Aware Trust**: Reinstalling Link All gives a device a new identity. Pairing the new install replaces its old entries instead of listing the device twice.

---

## Platform Feature Parity & Architecture

Link All combines a unified high-performance Rust core with platform-native interface runtimes:

| Platform | Frontend Stack | IPC / Binding Mechanism | Current Status & Notes |
| :--- | :--- | :--- | :--- |
| **macOS** | Swift & SwiftUI | Direct C-FFI / Unix Sockets | 🟢 **Production Ready** (Menu bar integration, native notifications) |
| **Android** | Kotlin & Jetpack Compose | JNI Bridge | 🟢 **Production Ready** (Background service runtime, native share-sheet target, QR/PIN pairing) |
| **Linux** | Rust, GTK 4 & libadwaita | Unix Sockets | 🟢 **Production Ready** (tray icon, desktop notifications, "Send with Link All" in Files, Dolphin and Nemo, `systemd` user service, `.deb` and tarball) |
| **Windows** | WinUI 3 / .NET 10 | In-process C-FFI / Named Pipes | 🟢 **Production Ready** (Native AOT, tray helper, per-user MSI and portable zip) |

---

## Capability Comparison Matrix

Below is a technical feature comparison between Link All and existing cross-platform or proprietary continuity utilities:

| Capability / Domain | Link All (Ours) | LocalSend | KDE Connect | Apple Handoff / AirDrop | Microsoft Phone Link |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Supported Operating Systems** | macOS, Windows, Android, Linux | macOS, Windows, Android, Linux, iOS | macOS, Windows, Android, Linux | macOS, iOS, iPadOS only | Windows, Android, iOS (Limited) |
| **Primary Architecture** | Event-driven Rust daemon + Native UIs | Flutter (Dart) single-process app | Qt / C++ background daemon | Native OS kernel & system daemons | Native Windows OS / Background service |
| **File Transfer Transport** | Zero-copy TCP/IP & Wi-Fi Direct | TCP/IP HTTP network serving | TCP/IP custom socket payloads | Wi-Fi Direct / Apple Custom Wireless | Wi-Fi Direct / Cloud-relayed hybrid |
| **Resumable Pipeline Architecture** | Yes (Chunk-level ack verification) | No (Restart required on drop) | Partial / Protocol dependent | No (Fail on signal loss) | Partial (Depends on file target) |
| **Universal Clipboard Timeline** | Yes (With searchable local ring buffer)| No (Manual text sending only) | Yes (Basic real-time string copy) | Yes (Real-time paste, no history feed)| Yes (Select Android hardware only) |
| **Sensitive Data Filtering (OTP Guard)**| Yes (Configurable regex / heuristics)| N/A | No | No | No |
| **Remote Filesystem Access** | Yes (Zero-trust secure directory browsing)| No | Yes (SFTP filesystem mounting) | No | No (Gallery sync only) |
| **Notification & Call Mirroring** | Yes | No | Yes | Yes (Apple hardware only) | Yes |

---

## Download

Get the latest build for each platform from [GitHub Releases](https://github.com/ChinmayyK/Link-All/releases/latest):

| Platform | File |
| :--- | :--- |
| macOS 13+ (Apple Silicon) | `LinkAll.dmg` |
| Windows 10/11 x64 | `LinkAll-windows-x64.msi` (installer) or `LinkAll-windows-x64.zip` (portable) |
| Android 8.0+ | `LinkAll-android.apk` |
| Linux x86_64 / ARM64 (Ubuntu 24.04+, Debian 13, Fedora 40+, Arch) | `linkall_<version>_amd64.deb` / `_arm64.deb`, or `LinkAll-linux-<arch>.tar.gz` (unpack and run `./install.sh`, no root needed) |

**First launch on macOS:** Link All isn't notarized by Apple yet, so macOS asks once. Open Link All, click **Done** when macOS says it can't verify it, then go to **System Settings › Privacy & Security** and click **Open Anyway** next to Link All. On Windows, if SmartScreen warns about an unrecognised app, choose **More info › Run anyway**.

### Test the Google Play version

Link All for Android is in a closed test on Google Play, and it needs testers before it can be published for everyone. To join:

1. Join the [Link All testers group](https://groups.google.com/g/linkall-testers) with your Google account.
2. With the same account, open the [testing page](https://play.google.com/apps/testing/app.linkall) and tap **Become a tester**.
3. Install [Link All from Google Play](https://play.google.com/store/apps/details?id=app.linkall).

Please keep it installed for a couple of weeks and use it with your computer now and then. Report anything that goes wrong in [Issues](https://github.com/ChinmayyK/Link-All/issues).

The Play build leaves out the features Play reserves for default SMS, dialer and file-manager apps: SMS code forwarding, answering calls, and letting a computer browse the phone's files. Everything else, including browsing your computers from the phone, works the same.

---

## Getting Started & Usage

### Prerequisites for Compiling from Source
- **Rust Toolchain**: Latest stable (managed via [rustup](https://rustup.rs/)).
- **C Compiler & Build Tools**: Needed for native cryptography bindings (`build-essential` on Linux, `Xcode command-line tools` on macOS).

### 1. Unified Automated Build & Install
To clean, recompile the core Rust daemon, and install both the macOS application bundle and Android APK simultaneously:

```bash
# Clone repository
git clone https://github.com/ChinmayyK/Link-All.git
cd Link-All

# Compile and install targets in Debug mode
./scripts/reinstall-all.sh --debug

# Compile optimized release builds
./scripts/reinstall-all.sh --release
```

### 2. Platform-Specific Manual Builds

#### macOS App Bundle (`LinkAll.app`)
```bash
# Compile macOS universal target and stage to /Applications
./scripts/build-macos.sh --debug
cp -a platforms/macos/build/LinkAll.app /Applications/
open /Applications/LinkAll.app
```

#### Android APK (`app.linkall.debug`)
```bash
# Assemble debugging APK and deploy to a USB-connected Android device via ADB
./scripts/build-android.sh --debug --fast-abi
adb install -r platforms/android/app/build/outputs/apk/full/debug/app-full-debug.apk
adb shell monkey -p app.linkall.debug -c android.intent.category.LAUNCHER 1
```

#### Linux Desktop (`linkall-linux`, GTK 4)
```bash
# Needs GTK 4.12+ and libadwaita 1.5+:
#   Ubuntu/Debian: sudo apt install libgtk-4-dev libadwaita-1-dev
#   Fedora:        sudo dnf install gtk4-devel libadwaita-devel
# Build and install the app, background service and CLI into your home folder
platforms/linux/install.sh
# Or just run the app from source (it starts the background service itself)
cargo run -p linkall-linux
```

#### Windows Client (`LinkAll.WinUI`, WinUI 3)
```bash
# Compile native core dynamic library for Windows
cargo build --release -p linkall-core

# Build and register desktop client using PowerShell installer script
powershell -ExecutionPolicy Bypass -File platforms/windows/LinkAll.WinUI/install_and_run.ps1
```

---

## Command-Line Interface (CLI)

For headless operations, system administration, and custom terminal script automation, Link All offers a native command-line utility (`linkall-cli`) communicating directly with the background daemon over low-latency IPC sockets:

```bash
# Print daemon health, version, and active listening ports
cargo run -p linkall-cli -- status

# Benchmark IPC domain socket round-trip latency
cargo run -p linkall-cli -- ping
# Output: PONG (1.1ms)

# Display real-time throughput metrics, transfer counters, and latency percentiles
cargo run -p linkall-cli -- metrics

# Query recent local clipboard history records
cargo run -p linkall-cli -- history --last 15

# Lock a history record to prevent automatic buffer eviction
cargo run -p linkall-cli -- history pin <id>

# Enumerate active discovered and trusted peer nodes on local subnet
cargo run -p linkall-cli -- devices list

# Ask a device to pair, then answer or withdraw requests (handy on Linux)
cargo run -p linkall-cli -- pair <device-id>
cargo run -p linkall-cli -- pair accept <device-id>
cargo run -p linkall-cli -- pair decline <device-id>
cargo run -p linkall-cli -- pair cancel <device-id>

# Toggle sync permissions for specific hardware UUIDs
cargo run -p linkall-cli -- devices peer-settings <device-id> pause
cargo run -p linkall-cli -- devices peer-settings <device-id> resume
```

---

## Configuration & Local Storage

Link All maintains clean configuration boundaries conforming to OS standard conventions:

| Platform | Configuration & Storage Directory | Identity key |
| :--- | :--- | :--- |
| **macOS** | `~/Library/Application Support/linkall/` | same folder |
| **Linux** | `~/.config/linkall/` | `~/.local/share/linkall/` |
| **Windows** | `%APPDATA%\linkall\` | `%LOCALAPPDATA%\linkall\` |

- `settings.json`: User runtime configurations, interface bindings, and custom filtering heuristics.
- `peers.json`: Locally cached network peers, mDNS mappings, and device nicknames.
- `trust.json`: Persistent public keys of verified trusted nodes.
- `history.json`: Bounded NDJSON ring buffer logging historical payloads and activity events.
- `identity.key`: The device's private identity key (32 bytes, mode `0600` on macOS and Linux). Its hash is the device ID, so deleting it makes the device look new to every paired device.

> [!CAUTION]
> The `identity.key` file contains your device's private identity keys used for network authentication and DH key exchange. Do not expose, commit, or transfer this file across insecure channels.

---

## Contributing, Security & Licensing

- **Contributing**: Please review [CONTRIBUTING.md](CONTRIBUTING.md) for architectural documentation, code formatting rules, and pull request submission guidelines.
- **Security Policy**: To report potential cryptographic weaknesses, memory safety issues, or vulnerability exploits, refer to [SECURITY.md](SECURITY.md) for coordinated disclosure procedures.
- **Changelog**: Release history and API modifications are documented in [CHANGELOG.md](CHANGELOG.md).

### License & Commercial Notice
Link All is distributed under the **GNU Affero General Public License v3.0 (AGPL-3.0)**. 

*Note on AGPL-3.0*: This license explicitly enforces network copyleft. Any modifications, derived network daemons, or cloud-relayed SaaS implementations built upon this engine must make their complete source code available under the same terms. See [LICENSE](LICENSE) for full legal text.
