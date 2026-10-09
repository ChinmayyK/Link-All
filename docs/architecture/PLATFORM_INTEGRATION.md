# Platform Integration Architecture

This document outlines how the Rust `linkall-core` interfaces with native OS APIs across macOS, Android, Windows, and Linux to provide premium ecosystem continuity features.

## 1. macOS (`platforms/macos`)

The macOS platform wrapper is a native Swift application residing in the Menu Bar (`LSUIElement`).

- **FFI Binding**: The Swift application communicates with `liblinkall_core.dylib` using standard C FFI bindings (`ffi.rs`).
- **Sleep Prevention (App Nap)**: To ensure background file transfers and continuous peer discovery, the macOS client utilizes `ProcessInfo.beginActivity(options: .background, reason: "Link All Sync")`. This blocks macOS from indiscriminately halting the daemon.
- **Continuity UI**: It features a frosted-glass (glassmorphic) Dashboard and uses `NSPasteboard` observers polling every 100ms. Remote phone calls on Android trigger interactive notification banners via Apple's Notification Center.

## 2. Android (`platforms/android`)

The Android implementation is a native Kotlin application focused on mobile constraints.

- **JNI Bridge**: The app communicates with `liblinkall_core.so` via Java Native Interface (JNI) definitions outlined in `jni_android.rs`.
- **Background Service**: Runs a Foreground Service with `foregroundServiceType=dataSync`. A low-importance, persistent notification is kept active to satisfy OS requirements without cluttering the user's shade.
- **OEM Battery Restrictions**: Added specific diagnostic logic to detect and circumvent aggressive battery optimization strategies used by OEMs like Xiaomi and Samsung.
- **Call Continuity**: The app binds to Android's `TelecomManager`, tracking phone call states (`ringing`, `offhook`, `idle`). This state telemetry is routed through the encrypted mesh to connected peers, allowing remote call management.

## 3. Windows (`platforms/windows`)

The Windows client is a C# WinUI/WPF application residing in the System Tray.

- **IPC Named Pipes**: Unlike macOS/Android which run the engine in-process, Windows runs the Rust core as a separate background daemon and communicates via Named Pipes (`ipc_windows.rs`).
- **Sleep Prevention (Modern Standby)**: To prevent Windows from suspending the PC during active synchronization, the client uses `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`.
- **Clipboard API**: Monitors the Win32 clipboard sequence number for changes, pushing updates safely across the tunnel.

## 4. Linux (`platforms/linux`)

The Linux build is a headless daemon coupled with optional GTK graphical tools.

- **Clipboard Integration**: Utilizes the `arboard` crate to natively interface with both X11 and Wayland clipboards.
- **Desktop Notifications**: Uses standard `notify-send` via the D-Bus desktop notification specification.
- **Systemd Integration**: Provided as a systemd user service unit with security hardening bounds.
