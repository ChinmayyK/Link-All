#![allow(
    dead_code,
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::manual_checked_ops
)]
//! # linkall-core
//!
//! Proximity-Based Shared Clipboard — Rust engine library.
//!
//! ## Architecture
//!
//! ```text
//!  ┌───────────────────────────────────────────────────────────────────┐
//!  │                        Platform Layer                              │
//!  │  macOS (Swift)   Windows (C#)   Android (Kotlin/JNI)  Linux (GTK) │
//!  └────────────────────────────┬──────────────────────────────────────┘
//!                               │  C FFI / JNI / Unix IPC
//!  ┌────────────────────────────▼──────────────────────────────────────┐
//!  │                       linkall-core                              │
//!  │                                                                    │
//!  │  engine.rs ──► network.rs ──► crypto.rs                           │
//!  │      │               │                                             │
//!  │      │           chunked.rs  (streaming large payloads)           │
//!  │      │               │                                             │
//!  │  discovery.rs     dedup.rs   (echo suppression, rate limiting)    │
//!  │  (mDNS-SD)        pairing.rs (PIN-based secure pairing)           │
//!  │      │                                                             │
//!  │  trust.rs       settings.rs    history.rs    metrics.rs           │
//!  │  (TOFU/PIN)     (user config)  (ring buffer)  (stats)             │
//!  │                                                                    │
//!  │  ipc.rs    (Unix socket / named pipe daemon <-> CLI bridge)        │
//!  │  protocol.rs  (wire format)                                        │
//!  └────────────────────────────────────────────────────────────────────┘
//! ```

pub mod activity;
pub mod chunked;
pub mod compress;
pub mod crypto;
pub mod dedup;
pub mod discovery;
pub mod discovery_manager;
pub mod engine;
pub mod engine_support;
pub mod ffi;
pub mod file_transfer;
pub mod filter;
pub mod history;
pub mod identity;
pub mod ipc;
pub mod lan_probe;
pub mod mesh;
pub mod metrics;
pub mod network;
pub mod network_manager;
pub mod pairing;
pub mod peer_manager;
pub mod probe;
pub mod protocol;
pub mod retry;
pub mod settings;
pub mod sim;
pub mod speed_test;
pub mod sync_controller;
pub mod transformer;
pub mod trust;

#[cfg(target_os = "android")]
pub mod jni_android;

#[cfg(windows)]
pub mod ipc_windows;

#[cfg(not(target_os = "android"))]
pub mod local_files;

pub use engine::{Engine, EngineConfig, EngineEvent};
pub use history::{HistoryFilter, HistoryStats};
pub use protocol::ClipboardContent;
pub use settings::{ClipboardTemplate, PeerSettings, Settings, SettingsStore};
