//! Link All settings — persistent user configuration.
//!
//! Settings are stored as JSON at:
//!   Linux/macOS: $XDG_CONFIG_HOME/linkall/settings.json
//!   Windows:     %APPDATA%\linkall\settings.json
//!
//! The file is written atomically (tmp → rename) on every change.
//! Platform layers can watch the file for changes to hot-reload.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SyncMode {
    #[default]
    Auto,
    Manual,
}

// ── Settings struct ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // ── Network ──────────────────────────────────────────────────────────────
    /// TCP port for the Link All service.
    pub port: u16,

    /// Override the device name shown to peers. Empty = use hostname.
    pub device_name: String,

    // ── Sync behaviour ───────────────────────────────────────────────────────
    /// Enable clipboard syncing (master switch).
    pub sync_enabled: bool,

    /// Sync plain text content.
    pub sync_text: bool,

    /// Sync image content.
    pub sync_images: bool,

    /// Sync file content.
    pub sync_files: bool,

    /// Auto sync on local copy, or wait for explicit user send.
    pub sync_mode: SyncMode,

    /// Maximum payload size in bytes that will be synced (0 = unlimited).
    pub max_payload_bytes: u64,

    /// Number of clipboard history entries retained locally.
    pub history_limit: usize,

    /// Maximum text bytes stored per history entry for re-paste.
    pub max_history_text_bytes: usize,

    /// Automatic history expiration retention period in days (0 = never purge by age).
    /// Unpinned items older than this are automatically pruned from disk.
    pub history_retention_days: u64,

    // ── Privacy ──────────────────────────────────────────────────────────────
    /// If true, show a native notification on every clipboard receive.
    pub show_receive_notification: bool,

    /// If true, require explicit TOFU confirmation for every new device.
    /// If false, auto-trust devices on the same LAN (lower security).
    pub require_tofu_confirmation: bool,

    /// List of device UUIDs that are explicitly blocked (revoked).
    pub blocked_device_ids: Vec<String>,

    /// If true, heuristic filtering blocks likely passwords and secrets.
    pub block_sensitive_text: bool,

    /// Case-insensitive substrings that should suppress syncing.
    pub ignore_patterns: Vec<String>,

    // ── Performance ──────────────────────────────────────────────────────────
    /// How often to poll the local clipboard for changes (milliseconds).
    pub clipboard_poll_ms: u64,

    /// Maximum clipboard pushes per second per peer (rate limiter).
    pub max_pushes_per_sec: f64,

    /// Burst allowance for the rate limiter.
    pub rate_limit_burst: f64,

    /// Suppress identical clipboard pushes repeated inside this window.
    pub smart_sync_duplicate_window_ms: u64,

    /// Coalesce rapid repeat copy events before syncing.
    pub smart_sync_debounce_ms: u64,

    // ── Clipboard UX ─────────────────────────────────────────────────────────
    /// Timeline-first mode: remote clipboard items land in the activity feed
    /// and are NOT auto-applied to the local clipboard.
    /// Users tap/click an item in the feed to apply it.
    /// Default: true (safer for multi-device environments).
    pub timeline_first_mode: bool,

    /// Auto-apply remote clipboard even in timeline-first mode.
    /// Only applies when `timeline_first_mode` is true.
    /// When false (default), user must explicitly apply from the feed.
    pub auto_apply_remote_clipboard: bool,

    /// Only auto-apply clipboard from these device IDs (UUID strings).
    /// Empty = apply from all trusted devices if auto_apply is on.
    pub auto_apply_allowed_devices: Vec<String>,

    /// Debounce rapid remote clipboard updates (ms) before auto-apply.
    pub auto_apply_debounce_ms: u64,

    // ── File Transfer ─────────────────────────────────────────────────────────
    /// Auto-accept incoming file transfers from trusted devices.
    /// When false, user is prompted for each transfer.
    pub auto_accept_file_transfers: bool,

    /// Maximum file size in bytes to auto-accept (0 = unlimited).
    pub auto_accept_max_bytes: u64,

    // ── UI ───────────────────────────────────────────────────────────────────
    /// Start Link All automatically on login.
    pub start_on_login: bool,

    // ── Advanced filtering ────────────────────────────────────────────────────
    /// Minimum number of non-whitespace characters for a text clip to be synced.
    /// 0 = no minimum (default). Useful to suppress trivial single-char copies.
    pub min_text_length: usize,

    /// When true, only sync text content that starts with a recognised URL scheme
    /// (http://, https://, ftp://, ssh://, git://, mailto:).
    pub sync_urls_only: bool,

    // ── Clipboard templates ───────────────────────────────────────────────────
    /// Named preset text snippets the user can push on demand.
    /// Push via `linkall-cli template push <name>`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clipboard_templates: Vec<ClipboardTemplate>,

    // ── Network Binding Override ──────────────────────────────────────────────
    /// Optional LAN interface IP address or CIDR prefix to bind discovery to
    /// (e.g. "192.168.1.0/24" or "192.168.1.50").
    /// None = listen/broadcast on default interfaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind_interface: Option<String>,

    // ── Per-peer overrides ────────────────────────────────────────────────────
    /// Per-device overrides keyed by UUID string.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub per_peer: std::collections::HashMap<String, PeerSettings>,
}

// ── ClipboardTemplate ─────────────────────────────────────────────────────────

/// A named preset text snippet that can be pushed to peers on demand.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClipboardTemplate {
    /// Short identifier (e.g. "email", "address", "meeting-link").
    pub name: String,
    /// The text content to push.
    pub text: String,
    /// Optional description shown in the CLI listing.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

// ── PeerSettings ──────────────────────────────────────────────────────────────

/// Per-device overrides that supplement the global settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PeerSettings {
    /// Override the display name for this peer in the UI.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display_name: String,
    /// If Some(true/false), override the global `auto_apply_remote_clipboard`
    /// for clipboard items arriving from this device.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_apply: Option<bool>,
    /// If true, clipboard sync is paused for this device (connection stays up).
    #[serde(default)]
    pub sync_paused: bool,
    /// If Some, override the global `max_payload_bytes` for this device.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_payload_bytes: Option<u64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            port: crate::protocol::DEFAULT_PORT,
            device_name: String::new(),
            sync_enabled: true,
            sync_text: true,
            sync_images: true,
            sync_files: true,
            sync_mode: SyncMode::Auto,
            max_payload_bytes: 10 * 1024 * 1024, // 10 MB default ceiling
            history_limit: 50,
            max_history_text_bytes: 64 * 1024,
            history_retention_days: 7,
            show_receive_notification: true,
            require_tofu_confirmation: true,
            blocked_device_ids: Vec::new(),
            block_sensitive_text: true,
            ignore_patterns: Vec::new(),
            clipboard_poll_ms: 500,
            max_pushes_per_sec: 10.0,
            rate_limit_burst: 3.0,
            smart_sync_duplicate_window_ms: 1_500,
            smart_sync_debounce_ms: 150,
            timeline_first_mode: false,
            auto_apply_remote_clipboard: false, // require explicit opt-in
            auto_apply_allowed_devices: Vec::new(),
            auto_apply_debounce_ms: 500,
            auto_accept_file_transfers: true, // auto-accept from trusted/paired peers without approval
            auto_accept_max_bytes: 0,         // 0 = unlimited size by default
            start_on_login: false,
            min_text_length: 0,
            sync_urls_only: false,
            clipboard_templates: Vec::new(),
            bind_interface: None,
            per_peer: std::collections::HashMap::new(),
        }
    }
}

impl Settings {
    /// Resolved device name: custom name if set, else hostname (cleaned).
    pub fn resolved_device_name(&self) -> String {
        if self.device_name.is_empty() {
            crate::engine::default_device_name()
        } else {
            self.device_name.clone()
        }
    }

    /// Is the given device UUID in the block list?
    pub fn is_blocked(&self, device_id: &str) -> bool {
        self.blocked_device_ids.iter().any(|id| id == device_id)
    }

    pub fn effective_history_limit(&self) -> usize {
        self.history_limit.clamp(20, 100)
    }

    /// Clamp all numeric fields to sane ranges, returning a sanitized clone.
    ///
    /// Call after loading from disk to guard against hand-edited files with
    /// extreme values (e.g. `clipboard_poll_ms: 0` would busy-loop the poller).
    pub fn sanitize(&self) -> Self {
        let mut s = self.clone();
        s.port = if s.port == 0 {
            crate::protocol::DEFAULT_PORT
        } else {
            s.port
        };
        s.history_limit = s
            .history_limit
            .clamp(MIN_HISTORY_ENTRIES, MAX_HISTORY_ENTRIES);
        s.max_history_text_bytes = s.max_history_text_bytes.clamp(1024, 4 * 1024 * 1024);
        // Minimum 10 ms poll to prevent busy-loop.
        s.clipboard_poll_ms = s.clipboard_poll_ms.max(10);
        // Rate limits must be positive.
        s.max_pushes_per_sec = if s.max_pushes_per_sec <= 0.0 {
            1.0
        } else {
            s.max_pushes_per_sec.min(100.0)
        };
        s.rate_limit_burst = if s.rate_limit_burst <= 0.0 {
            1.0
        } else {
            s.rate_limit_burst.min(50.0)
        };
        s.smart_sync_debounce_ms = s.smart_sync_debounce_ms.min(5_000);
        s.smart_sync_duplicate_window_ms = s.smart_sync_duplicate_window_ms.min(30_000);
        s.auto_apply_debounce_ms = s.auto_apply_debounce_ms.min(10_000);
        // Strip empty strings from lists.
        s.blocked_device_ids.retain(|id| !id.is_empty());
        s.ignore_patterns.retain(|p| !p.is_empty());
        s.auto_apply_allowed_devices.retain(|id| !id.is_empty());
        s
    }

    /// Validate settings, returning a list of human-readable problems.
    ///
    /// An empty Vec means the settings are valid.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.port == 0 {
            errors.push("port must not be 0".into());
        }
        if self.clipboard_poll_ms < 10 {
            errors.push(format!(
                "clipboard_poll_ms ({}) is below 10 ms — risk of busy-loop",
                self.clipboard_poll_ms
            ));
        }
        if self.max_pushes_per_sec <= 0.0 {
            errors.push("max_pushes_per_sec must be > 0".into());
        }
        if self.rate_limit_burst < 1.0 {
            errors.push("rate_limit_burst must be ≥ 1".into());
        }
        if self.history_limit < MIN_HISTORY_ENTRIES || self.history_limit > MAX_HISTORY_ENTRIES {
            errors.push(format!(
                "history_limit ({}) must be between {} and {}",
                self.history_limit, MIN_HISTORY_ENTRIES, MAX_HISTORY_ENTRIES
            ));
        }
        // Warn about insecure combinations.
        if !self.require_tofu_confirmation {
            errors.push(
                "require_tofu_confirmation is false — devices will be auto-trusted (security risk)"
                    .into(),
            );
        }
        errors
    }
}

/// Minimum/maximum bounds for history_limit (also used by history module).
pub const MIN_HISTORY_ENTRIES: usize = 20;
pub const MAX_HISTORY_ENTRIES: usize = 100;

// ── SettingsStore ─────────────────────────────────────────────────────────────

pub struct SettingsStore {
    settings: Settings,
    path: PathBuf,
}

impl SettingsStore {
    /// Load settings from disk, or return defaults if the file doesn't exist.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let settings = if path.exists() {
            let bytes = std::fs::read(&path).context("reading settings")?;
            if bytes.is_empty() {
                Settings::default()
            } else {
                // Use serde default for any missing fields so old configs work.
                serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                    tracing::warn!("Settings parse error ({}), using defaults", e);
                    Settings::default()
                })
            }
        } else {
            Settings::default()
        }
        .sanitize();
        Ok(Self { settings, path })
    }

    pub fn get(&self) -> &Settings {
        &self.settings
    }

    pub fn get_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    /// Persist to disk atomically.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(parent)
                    .context("creating config dir")?;
            }
            #[cfg(not(unix))]
            {
                std::fs::create_dir_all(parent).context("creating config dir")?;
            }
        }
        use std::io::Write;
        let tmp = self.path.with_extension("tmp");
        let bytes = serde_json::to_vec_pretty(&self.settings.sanitize())?;
        {
            let mut file = std::fs::File::create(&tmp).context("creating settings tmp")?;
            file.write_all(&bytes).context("writing settings tmp")?;
            let _ = file.sync_all();
        }
        std::fs::rename(&tmp, &self.path).context("renaming settings")?;
        Ok(())
    }

    /// Apply a partial JSON patch to settings, then save.
    pub fn patch(&mut self, patch: &str) -> Result<()> {
        let mut value = serde_json::to_value(&self.settings)?;
        let patch_value: serde_json::Value = serde_json::from_str(patch)?;
        json_merge(&mut value, &patch_value);
        self.settings = serde_json::from_value::<Settings>(value)?.sanitize();
        self.save()
    }

    /// Reset to defaults and save.
    pub fn reset(&mut self) -> Result<()> {
        self.settings = Settings::default();
        self.save()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Recursive JSON merge (patch overwrites matching keys).
fn json_merge(target: &mut serde_json::Value, patch: &serde_json::Value) {
    if let (Some(t), Some(p)) = (target.as_object_mut(), patch.as_object()) {
        for (k, v) in p {
            json_merge(t.entry(k).or_insert(serde_json::Value::Null), v);
        }
    } else {
        *target = patch.clone();
    }
}

/// For a store file that no longer parses: keep it beside the original as
/// `<name>.corrupt` and start empty. Failing instead would stop the app from
/// ever starting again; an empty trust store only means pairing again.
pub(crate) fn set_aside_corrupt<T: Default>(path: &Path, err: &serde_json::Error) -> T {
    let mut aside = path.as_os_str().to_owned();
    aside.push(".corrupt");
    tracing::warn!(error = %err, path = ?path, "store does not parse; starting empty");
    if let Err(e) = std::fs::rename(path, &aside) {
        tracing::warn!(error = %e, "could not set the unreadable store aside");
    }
    T::default()
}

// ── Platform config paths ─────────────────────────────────────────────────────

/// The app's own folder under `base`. The app was called Deskdrop before it
/// became Link All; an install from then keeps its pairings, settings and
/// history by having its old `deskdrop` folder moved here the first time.
pub fn app_dir(base: &Path) -> PathBuf {
    let dir = base.join("linkall");
    let old = base.join("deskdrop");
    if !dir.exists() && old.is_dir() {
        if let Err(err) = std::fs::rename(&old, &dir) {
            tracing::warn!(error = %err, from = ?old, to = ?dir, "could not move the old Deskdrop folder; using it as is");
            return old;
        }
    }
    dir
}

pub fn default_settings_path() -> PathBuf {
    #[cfg(target_os = "android")]
    let base = PathBuf::from("/data/user/0/app.linkall.debug/files");
    #[cfg(target_os = "windows")]
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    #[cfg(all(not(target_os = "windows"), not(target_os = "android")))]
    let base = dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")));

    app_dir(&base).join("settings.json")
}

pub fn default_trust_store_path() -> PathBuf {
    #[cfg(target_os = "android")]
    let base = PathBuf::from("/data/user/0/app.linkall.debug/files");
    #[cfg(not(target_os = "android"))]
    let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));

    app_dir(&base).join("trust.json")
}

pub fn default_peer_store_path() -> PathBuf {
    #[cfg(target_os = "android")]
    let base = PathBuf::from("/data/user/0/app.linkall.debug/files");
    #[cfg(not(target_os = "android"))]
    let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));

    app_dir(&base).join("peers.json")
}

pub fn default_history_path() -> PathBuf {
    #[cfg(target_os = "android")]
    let base = PathBuf::from("/data/user/0/app.linkall.debug/files");
    #[cfg(not(target_os = "android"))]
    let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));

    app_dir(&base).join("history.json")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_deskdrop_folder_moves_to_linkall_once() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("deskdrop")).unwrap();
        std::fs::write(tmp.path().join("deskdrop").join("trust.json"), b"{}").unwrap();

        let dir = app_dir(tmp.path());
        assert_eq!(dir, tmp.path().join("linkall"));
        assert!(dir.join("trust.json").exists(), "pairings came along");
        assert!(!tmp.path().join("deskdrop").exists());
        // A fresh install has nothing to move.
        assert_eq!(app_dir(tmp.path()), tmp.path().join("linkall"));
    }
    use tempfile::NamedTempFile;

    #[test]
    fn settings_roundtrip() {
        let tmp = NamedTempFile::new().unwrap();
        let mut store = SettingsStore::load(tmp.path()).unwrap();
        store.get_mut().device_name = "Test Device".into();
        store.get_mut().port = 9999;
        store.save().unwrap();

        let store2 = SettingsStore::load(tmp.path()).unwrap();
        assert_eq!(store2.get().device_name, "Test Device");
        assert_eq!(store2.get().port, 9999);
    }

    #[test]
    fn patch_applies_partial_update() {
        let tmp = NamedTempFile::new().unwrap();
        let mut store = SettingsStore::load(tmp.path()).unwrap();
        store
            .patch(r#"{"sync_images": false, "port": 12345}"#)
            .unwrap();
        assert!(!store.get().sync_images);
        assert_eq!(store.get().port, 12345);
        assert!(store.get().sync_text); // untouched
    }

    #[test]
    fn reset_returns_defaults() {
        let tmp = NamedTempFile::new().unwrap();
        let mut store = SettingsStore::load(tmp.path()).unwrap();
        store.get_mut().port = 9999;
        store.reset().unwrap();
        assert_eq!(store.get().port, crate::protocol::DEFAULT_PORT);
    }

    #[test]
    fn validate_detects_bad_poll_interval() {
        let s = Settings {
            clipboard_poll_ms: 0,
            ..Default::default()
        };
        let errs = s.validate();
        assert!(errs.iter().any(|e| e.contains("clipboard_poll_ms")));
    }

    #[test]
    fn validate_clean_on_defaults() {
        // Defaults should only produce the TOFU warning (intentional trade-off).
        let s = Settings::default();
        let errs = s.validate();
        // All defaults are valid except the TOFU security advisory.
        let hard_errors: Vec<_> = errs.iter().filter(|e| !e.contains("tofu")).collect();
        assert!(
            hard_errors.is_empty(),
            "unexpected errors: {:?}",
            hard_errors
        );
    }

    #[test]
    fn sanitize_clamps_poll_ms() {
        let s = Settings {
            clipboard_poll_ms: 0,
            ..Default::default()
        };
        let clean = s.sanitize();
        assert!(clean.clipboard_poll_ms >= 10);
    }

    #[test]
    fn sanitize_strips_empty_patterns() {
        let s = Settings {
            ignore_patterns: vec!["".into(), "foo".into(), "".into()],
            ..Default::default()
        };
        let clean = s.sanitize();
        assert_eq!(clean.ignore_patterns, vec!["foo"]);
    }

    #[test]
    fn load_sanitizes_unsafe_values_from_disk() {
        let tmp = NamedTempFile::new().unwrap();
        std::fs::write(
            tmp.path(),
            r#"{
                "clipboard_poll_ms": 0,
                "history_limit": 999,
                "ignore_patterns": ["", "token"]
            }"#,
        )
        .unwrap();

        let store = SettingsStore::load(tmp.path()).unwrap();
        assert_eq!(store.get().clipboard_poll_ms, 10);
        assert_eq!(store.get().history_limit, MAX_HISTORY_ENTRIES);
        assert_eq!(store.get().ignore_patterns, vec!["token"]);
    }

    #[test]
    fn patch_sanitizes_persisted_values() {
        let tmp = NamedTempFile::new().unwrap();
        let mut store = SettingsStore::load(tmp.path()).unwrap();
        store
            .patch(r#"{"clipboard_poll_ms": 0, "history_limit": 0}"#)
            .unwrap();

        assert_eq!(store.get().clipboard_poll_ms, 10);
        assert_eq!(store.get().history_limit, MIN_HISTORY_ENTRIES);
    }
}
