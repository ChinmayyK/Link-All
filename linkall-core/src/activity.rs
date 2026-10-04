//! Link All Activity Feed — unified cross-device event log.
//!
//! Every meaningful event across all connected devices is recorded here:
//!   - Clipboard copies (from any device)
//!   - File transfers (sent/received)
//!   - Device connect/disconnect
//!   - Sync pause/resume
//!
//! The feed drives the Timeline-First clipboard UX: remote clipboard items
//! land here first, and users explicitly "apply" them rather than having
//! their local clipboard overwritten automatically.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Truncate `text` to at most `max_chars` Unicode scalar values, appending `…`.
/// Never slices mid-codepoint.
fn safe_truncate(text: &str, max_chars: usize) -> String {
    let mut chars = text.char_indices().peekable();
    let mut last_valid = text.len();
    let mut count = 0;
    while let Some((i, _)) = chars.next() {
        if count == max_chars {
            last_valid = i;
            break;
        }
        count += 1;
        if chars.peek().is_none() {
            // All chars consumed within limit.
            return text.to_string();
        }
    }
    format!("{}…", &text[..last_valid])
}

/// "12 files", or "11 of 12 files" when some failed.
fn folder_files_label(file_count: u32, failed_count: u32) -> String {
    let noun = if file_count == 1 { "file" } else { "files" };
    if failed_count == 0 {
        format!("{} {}", file_count, noun)
    } else {
        format!(
            "{} of {} {}",
            file_count.saturating_sub(failed_count),
            file_count,
            noun
        )
    }
}

// ── Activity kinds ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    /// A device copied text to its clipboard.
    ClipboardText,
    /// A device copied an image.
    ClipboardImage,
    /// A file transfer was initiated.
    FileTransferStarted,
    /// A file transfer completed successfully.
    FileTransferComplete,
    /// A file transfer was cancelled or failed.
    FileTransferFailed,
    /// A peer connected to the mesh.
    PeerConnected,
    /// A peer disconnected from the mesh.
    PeerDisconnected,
    /// Sync was paused for a peer.
    SyncPaused,
    /// Sync was resumed for a peer.
    SyncResumed,
    /// A clipboard item from a remote device is available to apply locally.
    RemoteClipboardAvailable,
    /// User explicitly applied a remote clipboard item.
    ClipboardApplied,
    /// A remote push notification was relayed.
    RemoteNotification,
    /// Every file of a folder transfer finished (sent or received).
    FolderTransferComplete,
}

/// A single entry in the activity feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEntry {
    /// Monotonically incrementing ID within this session.
    pub id: u64,
    /// Milliseconds since Unix epoch.
    pub timestamp_ms: u64,
    /// Device that originated this event.
    pub device_id: Uuid,
    /// Human-readable device name (never a raw UUID in the UI).
    pub device_name: String,
    /// What kind of event this is.
    pub kind: ActivityKind,
    /// Short human-readable description for the feed.
    pub summary: String,
    /// Optional content hash (for clipboard items, used for dedup + apply).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// For clipboard items: the text preview (truncated to PREVIEW_CHARS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_preview: Option<String>,
    /// For file transfers: file name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// For file transfers: total bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_bytes: Option<u64>,
    /// Transfer ID for file events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer_id: Option<String>,
    /// Local filesystem path where a received file was saved.
    /// Set by the daemon when it writes to ~/Downloads (or the configured save dir).
    /// Nil for non-file events or outgoing transfers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dest_path: Option<String>,
    /// Whether this item has been applied locally.
    pub applied_locally: bool,
    /// Relay path: which devices forwarded this event (mesh traceability).
    pub relay_path: Vec<String>,
}

/// Maximum chars stored in `text_preview`.  Enough for BigText notification
/// expansion on Android (400 chars) and the monospaced preview block on Mac.
const PREVIEW_CHARS: usize = 400;

impl ActivityEntry {
    fn new(
        id: u64,
        device_id: Uuid,
        device_name: String,
        kind: ActivityKind,
        summary: String,
    ) -> Self {
        Self {
            id,
            timestamp_ms: now_ms(),
            device_id,
            device_name,
            kind,
            summary,
            content_hash: None,
            text_preview: None,
            file_name: None,
            file_bytes: None,
            transfer_id: None,
            dest_path: None,
            applied_locally: false,
            relay_path: Vec::new(),
        }
    }
}

// ── ActivityFeed ──────────────────────────────────────────────────────────────

/// Aggregate statistics snapshot for an `ActivityFeed` window.
#[derive(Debug, Clone, Default)]
pub struct FeedStats {
    pub total: usize,
    pub local_clipboard_events: usize,
    pub remote_clipboard_events: usize,
    /// Remote clipboard items the user has explicitly applied.
    pub applied_count: usize,
    /// Remote clipboard items waiting for the user to apply.
    pub pending_count: usize,
    pub file_transfers_completed: usize,
    pub file_transfers_failed: usize,
    /// Total bytes of successfully transferred files.
    pub total_file_bytes: u64,
    pub peer_connect_events: usize,
    pub peer_disconnect_events: usize,
    /// Age in seconds of the oldest entry in the feed.
    pub oldest_entry_age_secs: u64,
}

impl FeedStats {
    /// Total file transfer events (started + completed + failed).
    pub fn file_transfer_success_rate(&self) -> f64 {
        let total = self.file_transfers_completed + self.file_transfers_failed;
        if total == 0 {
            return 1.0;
        }
        self.file_transfers_completed as f64 / total as f64
    }
}

/// Bounded ring-buffer activity feed.
pub struct ActivityFeed {
    entries: VecDeque<ActivityEntry>,
    capacity: usize,
    next_id: u64,
}

impl ActivityFeed {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
            next_id: 1,
        }
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn push(&mut self, entry: ActivityEntry) {
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    // ── Constructors for each event type ─────────────────────────────────────

    /// Record a remote clipboard text item landing in the feed.
    pub fn record_remote_clipboard_text(
        &mut self,
        device_id: Uuid,
        device_name: String,
        text: &str,
        content_hash: String,
        relay_path: Vec<String>,
    ) -> u64 {
        let preview = safe_truncate(text.trim(), PREVIEW_CHARS);
        let summary = format!(
            "[{}] copied: {}",
            device_name,
            safe_truncate(text.trim(), 40)
        );
        let id = self.alloc_id();
        let mut entry = ActivityEntry::new(
            id,
            device_id,
            device_name,
            ActivityKind::RemoteClipboardAvailable,
            summary,
        );
        entry.content_hash = Some(content_hash);
        entry.text_preview = Some(preview);
        entry.relay_path = relay_path;
        self.push(entry);
        id
    }

    /// Record a remote clipboard image item.
    pub fn record_remote_clipboard_image(
        &mut self,
        device_id: Uuid,
        device_name: String,
        mime: &str,
        bytes: u64,
        content_hash: String,
        relay_path: Vec<String>,
    ) -> u64 {
        let summary = format!(
            "[{}] copied image ({}, {} KB)",
            device_name,
            mime,
            bytes / 1024
        );
        let id = self.alloc_id();
        let mut entry = ActivityEntry::new(
            id,
            device_id,
            device_name,
            ActivityKind::RemoteClipboardAvailable,
            summary,
        );
        entry.content_hash = Some(content_hash);
        entry.file_bytes = Some(bytes);
        entry.relay_path = relay_path;
        self.push(entry);
        id
    }

    /// Record that a remote clipboard item was applied to local clipboard.
    pub fn record_clipboard_applied(
        &mut self,
        device_id: Uuid,
        device_name: String,
        content_hash: String,
    ) -> u64 {
        // Mark the original "available" entry as applied.
        for e in self.entries.iter_mut() {
            if e.content_hash.as_deref() == Some(&content_hash)
                && e.kind == ActivityKind::RemoteClipboardAvailable
            {
                e.applied_locally = true;
            }
        }
        let summary = format!("Applied clipboard from [{}]", device_name);
        let id = self.alloc_id();
        let mut entry = ActivityEntry::new(
            id,
            device_id,
            device_name,
            ActivityKind::ClipboardApplied,
            summary,
        );
        entry.content_hash = Some(content_hash);
        self.push(entry);
        id
    }

    /// Record a local clipboard copy (sent to peers).
    pub fn record_local_clipboard_text(
        &mut self,
        device_id: Uuid,
        device_name: String,
        text: &str,
        content_hash: String,
    ) -> u64 {
        let preview = safe_truncate(text.trim(), PREVIEW_CHARS);
        let summary = format!("[{}] copied text", device_name);
        let id = self.alloc_id();
        let mut entry = ActivityEntry::new(
            id,
            device_id,
            device_name,
            ActivityKind::ClipboardText,
            summary,
        );
        entry.content_hash = Some(content_hash);
        entry.text_preview = Some(preview);
        self.push(entry);
        id
    }

    /// Record file transfer start.
    pub fn record_file_transfer_started(
        &mut self,
        device_id: Uuid,
        device_name: String,
        file_name: String,
        file_bytes: u64,
        transfer_id: String,
        is_sender: bool,
    ) -> u64 {
        if crate::file_transfer::is_folder_item(&file_name) {
            return 0;
        }
        let summary = if is_sender {
            format!("[{}] sending file: {}", device_name, file_name)
        } else {
            format!("[{}] receiving file: {}", device_name, file_name)
        };
        let id = self.alloc_id();
        let mut entry = ActivityEntry::new(
            id,
            device_id,
            device_name,
            ActivityKind::FileTransferStarted,
            summary,
        );
        entry.file_name = Some(file_name);
        entry.file_bytes = Some(file_bytes);
        entry.transfer_id = Some(transfer_id);
        self.push(entry);
        id
    }

    /// Record file transfer completion.
    /// `dest_path` is the local filesystem path where the file was saved
    /// (e.g. `~/Downloads/filename.pdf`).  Pass `None` for outgoing transfers.
    pub fn record_file_transfer_complete(
        &mut self,
        device_id: Uuid,
        device_name: String,
        file_name: String,
        file_bytes: u64,
        transfer_id: String,
        dest_path: Option<String>,
    ) -> u64 {
        if crate::file_transfer::is_folder_item(&file_name) {
            return 0;
        }
        // Mark the corresponding FileTransferStarted entry to show it completed,
        // but DO NOT change its kind to FileTransferComplete, to avoid
        // duplicate notifications on clients that poll incrementally.
        let is_outbound = dest_path.is_none();
        for e in self.entries.iter_mut() {
            if e.transfer_id.as_deref() == Some(&transfer_id)
                && e.kind == ActivityKind::FileTransferStarted
            {
                e.summary = if is_outbound {
                    format!(
                        "[{}] sent file: {} ({} KB)",
                        device_name,
                        file_name,
                        file_bytes / 1024
                    )
                } else {
                    format!(
                        "[{}] received file: {} ({} KB)",
                        device_name,
                        file_name,
                        file_bytes / 1024
                    )
                };
                e.dest_path = dest_path.clone();
            }
        }
        let summary = if is_outbound {
            format!(
                "[{}] sent file: {} ({} KB)",
                device_name,
                file_name,
                file_bytes / 1024
            )
        } else {
            format!(
                "[{}] file ready: {} ({} KB)",
                device_name,
                file_name,
                file_bytes / 1024
            )
        };
        let id = self.alloc_id();
        let mut entry = ActivityEntry::new(
            id,
            device_id,
            device_name,
            ActivityKind::FileTransferComplete,
            summary,
        );
        entry.file_name = Some(file_name);
        entry.file_bytes = Some(file_bytes);
        entry.transfer_id = Some(transfer_id);
        entry.dest_path = dest_path;
        self.push(entry);
        id
    }

    /// Record file transfer failure.
    pub fn record_file_transfer_failed(
        &mut self,
        device_id: Uuid,
        device_name: String,
        file_name: Option<String>,
        transfer_id: String,
        reason: String,
    ) -> u64 {
        if file_name
            .as_deref()
            .is_some_and(crate::file_transfer::is_folder_item)
        {
            return 0;
        }
        for e in self.entries.iter_mut() {
            if e.transfer_id.as_deref() == Some(&transfer_id) {
                e.kind = ActivityKind::FileTransferFailed;
            }
        }
        let name_part = file_name.as_deref().unwrap_or("unknown file");
        let summary = format!(
            "[{}] transfer failed: {} — {}",
            device_name, name_part, reason
        );
        let id = self.alloc_id();
        let mut entry = ActivityEntry::new(
            id,
            device_id,
            device_name,
            ActivityKind::FileTransferFailed,
            summary,
        );
        entry.file_name = file_name;
        entry.transfer_id = Some(transfer_id);
        self.push(entry);
        id
    }

    /// Record a whole folder transfer finishing. `dest_dir` is the folder
    /// on this device; `None` when this device sent it.
    pub fn record_folder_transfer_complete(
        &mut self,
        device_id: Uuid,
        device_name: String,
        folder_name: String,
        file_count: u32,
        failed_count: u32,
        dest_dir: Option<String>,
    ) -> u64 {
        let files = folder_files_label(file_count, failed_count);
        let verb = if dest_dir.is_none() {
            "sent"
        } else {
            "received"
        };
        let summary = format!(
            "[{}] {} folder: {} ({})",
            device_name, verb, folder_name, files
        );
        let id = self.alloc_id();
        let mut entry = ActivityEntry::new(
            id,
            device_id,
            device_name,
            ActivityKind::FolderTransferComplete,
            summary,
        );
        entry.file_name = Some(folder_name);
        entry.text_preview = Some(files);
        entry.dest_path = dest_dir;
        self.push(entry);
        id
    }

    /// Record a peer connecting.
    pub fn record_peer_connected(&mut self, device_id: Uuid, device_name: String) -> u64 {
        let summary = format!("[{}] connected", device_name);
        let id = self.alloc_id();
        let entry = ActivityEntry::new(
            id,
            device_id,
            device_name,
            ActivityKind::PeerConnected,
            summary,
        );
        self.push(entry);
        id
    }

    /// Record a peer disconnecting.
    pub fn record_peer_disconnected(
        &mut self,
        device_id: Uuid,
        device_name: String,
        reason: Option<String>,
    ) -> u64 {
        let summary = match &reason {
            Some(r) => format!("[{}] disconnected: {}", device_name, r),
            None => format!("[{}] disconnected", device_name),
        };
        let id = self.alloc_id();
        let entry = ActivityEntry::new(
            id,
            device_id,
            device_name,
            ActivityKind::PeerDisconnected,
            summary,
        );
        self.push(entry);
        id
    }

    /// Record sync pause.
    pub fn record_sync_paused(&mut self, device_id: Uuid, device_name: String) -> u64 {
        let summary = format!("[{}] sync paused", device_name);
        let id = self.alloc_id();
        let entry = ActivityEntry::new(
            id,
            device_id,
            device_name,
            ActivityKind::SyncPaused,
            summary,
        );
        self.push(entry);
        id
    }

    /// Record sync resume.
    pub fn record_sync_resumed(&mut self, device_id: Uuid, device_name: String) -> u64 {
        let summary = format!("[{}] sync resumed", device_name);
        let id = self.alloc_id();
        let entry = ActivityEntry::new(
            id,
            device_id,
            device_name,
            ActivityKind::SyncResumed,
            summary,
        );
        self.push(entry);
        id
    }

    /// Record a remote push notification.
    pub fn record_remote_notification(
        &mut self,
        device_id: Uuid,
        device_name: String,
        _package: String,
        title: String,
        text: String,
    ) -> u64 {
        let summary = format!("[{}] {}: {}", device_name, title, text);
        let id = self.alloc_id();
        let mut entry = ActivityEntry::new(
            id,
            device_id,
            device_name,
            ActivityKind::RemoteNotification,
            summary,
        );
        entry.text_preview = Some(text);
        entry.file_name = Some(title); // We'll abuse file_name for title, or maybe we can just put it all in summary, but text_preview and file_name are handy. Let's use file_name for title.
        self.push(entry);
        id
    }

    // ── Query ─────────────────────────────────────────────────────────────────

    pub fn recent(&self, limit: usize) -> Vec<&ActivityEntry> {
        self.entries.iter().rev().take(limit).collect()
    }

    pub fn since(&self, since_id: u64) -> Vec<&ActivityEntry> {
        self.entries.iter().filter(|e| e.id > since_id).collect()
    }

    pub fn all(&self) -> Vec<&ActivityEntry> {
        self.entries.iter().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get pending (not yet locally applied) remote clipboard items.
    pub fn pending_remote_clipboards(&self) -> Vec<&ActivityEntry> {
        self.entries
            .iter()
            .filter(|e| e.kind == ActivityKind::RemoteClipboardAvailable && !e.applied_locally)
            .collect()
    }

    /// Filter entries by one or more `ActivityKind`s, most-recent first.
    pub fn filter_by_kind<'a>(
        &'a self,
        kinds: &'a [ActivityKind],
    ) -> impl Iterator<Item = &'a ActivityEntry> {
        self.entries
            .iter()
            .rev()
            .filter(move |e| kinds.contains(&e.kind))
    }

    /// All entries from a specific device, most-recent first.
    pub fn by_device(&self, device_id: Uuid) -> impl Iterator<Item = &ActivityEntry> {
        self.entries
            .iter()
            .rev()
            .filter(move |e| e.device_id == device_id)
    }

    /// Find the most recent entry whose `content_hash` matches, if any.
    pub fn find_by_hash(&self, hash: &str) -> Option<&ActivityEntry> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.content_hash.as_deref() == Some(hash))
    }

    /// Aggregate statistics for the current feed window.
    pub fn stats(&self) -> FeedStats {
        let mut stats = FeedStats::default();
        let now_ms = now_ms();
        for e in &self.entries {
            stats.total += 1;
            let age_secs = now_ms.saturating_sub(e.timestamp_ms) / 1000;
            match e.kind {
                ActivityKind::ClipboardText | ActivityKind::ClipboardImage => {
                    stats.local_clipboard_events += 1;
                }
                ActivityKind::RemoteClipboardAvailable => {
                    stats.remote_clipboard_events += 1;
                    if e.applied_locally {
                        stats.applied_count += 1;
                    } else {
                        stats.pending_count += 1;
                    }
                }
                ActivityKind::FileTransferComplete => {
                    stats.file_transfers_completed += 1;
                    if let Some(b) = e.file_bytes {
                        stats.total_file_bytes += b;
                    }
                }
                ActivityKind::FileTransferFailed => {
                    stats.file_transfers_failed += 1;
                }
                ActivityKind::PeerConnected => {
                    stats.peer_connect_events += 1;
                }
                ActivityKind::PeerDisconnected => {
                    stats.peer_disconnect_events += 1;
                }
                _ => {}
            }
            // Track oldest entry age.
            if age_secs > stats.oldest_entry_age_secs {
                stats.oldest_entry_age_secs = age_secs;
            }
        }
        stats
    }

    /// Serialize the entire feed to a compact JSON string for export/debug.
    pub fn export_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(&self.entries.iter().collect::<Vec<_>>())
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for ActivityFeed {
    fn default() -> Self {
        Self::new(200)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_records_and_queries() {
        let mut feed = ActivityFeed::new(50);
        let id = Uuid::new_v4();
        feed.record_peer_connected(id, "Pixel 8".into());
        feed.record_remote_clipboard_text(
            id,
            "Pixel 8".into(),
            "hello world",
            "abc123".into(),
            vec![],
        );
        assert_eq!(feed.len(), 2);
        let pending = feed.pending_remote_clipboards();
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn apply_marks_entry() {
        let mut feed = ActivityFeed::new(50);
        let id = Uuid::new_v4();
        feed.record_remote_clipboard_text(id, "Phone".into(), "text", "hash1".into(), vec![]);
        feed.record_clipboard_applied(id, "Phone".into(), "hash1".into());
        assert_eq!(feed.pending_remote_clipboards().len(), 0);
    }

    #[test]
    fn capacity_evicts_oldest() {
        let mut feed = ActivityFeed::new(3);
        let id = Uuid::new_v4();
        for i in 0..5 {
            feed.record_peer_connected(id, format!("Device {}", i));
        }
        assert_eq!(feed.len(), 3);
    }

    #[test]
    fn filter_by_kind_returns_only_matching() {
        let mut feed = ActivityFeed::new(50);
        let id = Uuid::new_v4();
        feed.record_peer_connected(id, "Mac".into());
        feed.record_peer_disconnected(id, "Mac".into(), None);
        feed.record_remote_clipboard_text(id, "Mac".into(), "hi", "h1".into(), vec![]);

        let clipboard: Vec<_> = feed
            .filter_by_kind(&[ActivityKind::RemoteClipboardAvailable])
            .collect();
        assert_eq!(clipboard.len(), 1);

        let conn: Vec<_> = feed
            .filter_by_kind(&[ActivityKind::PeerConnected, ActivityKind::PeerDisconnected])
            .collect();
        assert_eq!(conn.len(), 2);
    }

    #[test]
    fn by_device_filters_correctly() {
        let mut feed = ActivityFeed::new(50);
        let dev_a = Uuid::new_v4();
        let dev_b = Uuid::new_v4();
        feed.record_peer_connected(dev_a, "DevA".into());
        feed.record_peer_connected(dev_b, "DevB".into());
        feed.record_peer_connected(dev_a, "DevA".into());

        let a_events: Vec<_> = feed.by_device(dev_a).collect();
        assert_eq!(a_events.len(), 2);
        let b_events: Vec<_> = feed.by_device(dev_b).collect();
        assert_eq!(b_events.len(), 1);
    }

    #[test]
    fn stats_counts_correctly() {
        let mut feed = ActivityFeed::new(50);
        let id = Uuid::new_v4();
        feed.record_local_clipboard_text(id, "Mac".into(), "hello", "h1".into());
        feed.record_remote_clipboard_text(id, "Phone".into(), "world", "h2".into(), vec![]);
        feed.record_clipboard_applied(id, "Phone".into(), "h2".into());
        feed.record_peer_connected(id, "Tablet".into());

        let stats = feed.stats();
        assert_eq!(stats.local_clipboard_events, 1);
        assert_eq!(stats.remote_clipboard_events, 1);
        assert_eq!(stats.applied_count, 1);
        assert_eq!(stats.pending_count, 0);
        assert_eq!(stats.peer_connect_events, 1);
    }

    #[test]
    fn find_by_hash_works() {
        let mut feed = ActivityFeed::new(50);
        let id = Uuid::new_v4();
        feed.record_remote_clipboard_text(id, "Dev".into(), "test", "unique-hash".into(), vec![]);
        assert!(feed.find_by_hash("unique-hash").is_some());
        assert!(feed.find_by_hash("no-such-hash").is_none());
    }

    #[test]
    fn safe_truncate_handles_multibyte_chars() {
        // Japanese chars are 3 bytes each — naive byte-slicing would panic.
        let text = "こんにちは世界"; // 7 chars
        let truncated = safe_truncate(text, 4);
        assert_eq!(truncated, "こんにち…");
        // No panic, and the length in chars is correct.
        assert_eq!(truncated.chars().count(), 5); // 4 chars + …
    }

    #[test]
    fn safe_truncate_short_string_unchanged() {
        let text = "hi";
        assert_eq!(safe_truncate(text, 80), "hi");
    }

    #[test]
    fn export_json_is_valid() {
        let mut feed = ActivityFeed::new(10);
        feed.record_peer_connected(Uuid::new_v4(), "Test".into());
        let json = feed.export_json().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);
    }
}
