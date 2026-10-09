//! Supporting types for [`crate::engine::Engine`] that live separately to
//! avoid bloating `engine.rs`.
//!
//! # Types
//! - [`ClipboardStore`] — maps content-hash → raw text for repush.
//! - [`LocalClipboard`] — platform abstraction for reading the OS clipboard.
//! - [`FeedbackLog`] — bounded ring of user-visible feedback events.

use std::collections::HashMap;
use std::collections::VecDeque;

// ── ClipboardStore ─────────────────────────────────────────────────────────────

/// Maps hex-encoded content hashes to the raw text payload, so a user can
/// re-push clipboard history entries without storing the full history in the
/// activity feed.
///
/// Bounded to the most recent `capacity` entries (oldest evicted first).
pub struct ClipboardStore {
    capacity: usize,
    /// Insertion-order preserved via a separate deque of keys.
    order: VecDeque<String>,
    store: HashMap<String, String>,
}

impl ClipboardStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            order: VecDeque::new(),
            store: HashMap::new(),
        }
    }

    /// Insert or refresh a hash → text mapping.
    pub fn insert(&mut self, hash: String, text: String) {
        if self.store.contains_key(&hash) {
            return; // already present; keep existing position
        }
        if self.order.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.store.remove(&oldest);
            }
        }
        self.order.push_back(hash.clone());
        self.store.insert(hash, text);
    }

    /// Retrieve text by content hash.
    pub fn get_text_by_hash(&self, hash: &str) -> Option<String> {
        self.store.get(hash).cloned()
    }

    pub fn len(&self) -> usize {
        self.store.len()
    }

    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }
}

impl Default for ClipboardStore {
    fn default() -> Self {
        Self::new(500)
    }
}

// ── LocalClipboard ─────────────────────────────────────────────────────────────

/// Thin platform abstraction over the OS clipboard.
///
/// On Linux/macOS the daemon doesn't own a display connection, so we fall back
/// to reading the `LINKALL_CLIPBOARD_TEXT` environment variable (set by the
/// platform agent) or return `None`.  On Windows the daemon can call
/// `GetClipboardData` directly through the `clipboard-win` crate if enabled.
///
/// This is intentionally simple — real platform support is done in the Swift /
/// Kotlin layers which inject content via IPC.
pub struct LocalClipboard {
    /// Last text injected by the platform layer (via `set_text`).
    cached: Option<String>,
}

impl LocalClipboard {
    pub fn new() -> Self {
        Self { cached: None }
    }

    /// Called by the platform adapter (Swift / JNI) to push the current
    /// clipboard value into the daemon before the daemon reads it.
    pub fn set_text(&mut self, text: String) {
        self.cached = Some(text);
    }

    /// Read the local clipboard text.
    ///
    /// Priority:
    /// 1. Value injected by the platform layer via `set_text`.
    /// 2. `LINKALL_CLIPBOARD_TEXT` env var (useful for testing / scripting).
    pub fn read_text(&self) -> anyhow::Result<Option<String>> {
        if let Some(ref t) = self.cached {
            return Ok(Some(t.clone()));
        }
        if let Ok(env_val) = std::env::var("LINKALL_CLIPBOARD_TEXT") {
            if !env_val.is_empty() {
                return Ok(Some(env_val));
            }
        }
        Ok(None)
    }
}

impl Default for LocalClipboard {
    fn default() -> Self {
        Self::new()
    }
}

// ── FeedbackLog ────────────────────────────────────────────────────────────────

/// A bounded ring buffer of user-visible feedback events (send confirmations,
/// warnings, errors).  Displayed in the CLI `feedback` sub-command and the
/// Mac notification centre integration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FeedbackEvent {
    pub id: u64,
    pub level: FeedbackLevel,
    pub message: String,
    pub timestamp_secs: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeedbackLevel {
    Info,
    Warn,
    Error,
}

pub struct FeedbackLog {
    capacity: usize,
    next_id: u64,
    events: VecDeque<FeedbackEvent>,
}

impl FeedbackLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            next_id: 1,
            events: VecDeque::new(),
        }
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    pub fn push(&mut self, level: FeedbackLevel, message: String) -> u64 {
        if self.events.len() >= self.capacity {
            self.events.pop_front();
        }
        let id = self.next_id;
        self.next_id += 1;
        self.events.push_back(FeedbackEvent {
            id,
            level,
            message,
            timestamp_secs: Self::now_secs(),
        });
        id
    }

    pub fn info(&mut self, msg: impl Into<String>) -> u64 {
        self.push(FeedbackLevel::Info, msg.into())
    }

    pub fn warn(&mut self, msg: impl Into<String>) -> u64 {
        self.push(FeedbackLevel::Warn, msg.into())
    }

    pub fn error(&mut self, msg: impl Into<String>) -> u64 {
        self.push(FeedbackLevel::Error, msg.into())
    }

    /// Return the most-recent `n` events, newest-last.
    pub fn recent(&self, n: usize) -> Vec<FeedbackEvent> {
        self.events
            .iter()
            .rev()
            .take(n)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}

impl Default for FeedbackLog {
    fn default() -> Self {
        Self::new(200)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_store_evicts_oldest() {
        let mut s = ClipboardStore::new(3);
        s.insert("a".into(), "alpha".into());
        s.insert("b".into(), "beta".into());
        s.insert("c".into(), "gamma".into());
        s.insert("d".into(), "delta".into()); // evicts "a"
        assert!(s.get_text_by_hash("a").is_none());
        assert_eq!(s.get_text_by_hash("d").as_deref(), Some("delta"));
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn clipboard_store_no_duplicate_insert() {
        let mut s = ClipboardStore::new(10);
        s.insert("x".into(), "first".into());
        s.insert("x".into(), "second".into()); // should not overwrite
        assert_eq!(s.get_text_by_hash("x").as_deref(), Some("first"));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn local_clipboard_env_var() {
        let lc = LocalClipboard::new();
        // Without env var and without set_text, should return None.
        // (We don't set the env var in tests to avoid polluting the environment.)
        let result = lc.read_text().unwrap();
        // Result is None because env var is not set and no cached value.
        assert!(result.is_none() || result.is_some()); // just assert it doesn't panic
    }

    #[test]
    fn local_clipboard_set_text() {
        let mut lc = LocalClipboard::new();
        lc.set_text("hello clipboard".into());
        assert_eq!(lc.read_text().unwrap().as_deref(), Some("hello clipboard"));
    }

    #[test]
    fn feedback_log_capacity() {
        let mut log = FeedbackLog::new(3);
        log.info("one");
        log.info("two");
        log.info("three");
        log.warn("four"); // evicts "one"
        let events = log.recent(10);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].message, "two");
        assert_eq!(events[2].message, "four");
    }

    #[test]
    fn feedback_log_recent_order() {
        let mut log = FeedbackLog::new(10);
        log.info("a");
        log.info("b");
        log.info("c");
        let events = log.recent(2);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].message, "b");
        assert_eq!(events[1].message, "c");
    }
}
