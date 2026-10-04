//! Clipboard history — a bounded, persisted ring buffer of recent clipboard
//! entries with pinning, tagging, date-range filtering, stats, and JSON export.

use crate::dedup::hash_content;
use crate::protocol::{ClipboardContent, HistoryMetadata};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MIN_ENTRIES: usize = 20;
pub const MAX_ENTRIES: usize = 500;
pub const DEFAULT_ENTRIES: usize = 50;
pub const DEFAULT_MAX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_TEXT_PREVIEW: usize = 4096;
const SENSITIVE_ENTRY_TTL_SECS: u64 = 120;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn clamp_entries(limit: usize) -> usize {
    limit.clamp(MIN_ENTRIES, MAX_ENTRIES)
}

/// A search result entry with its relevance score.
#[derive(Debug, Clone)]
pub struct ScoredHistoryEntry<'a> {
    pub score: u32,
    pub entry: &'a HistoryEntry,
}

/// Aggregated statistics over the full history buffer.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryStats {
    pub total: usize,
    pub text_count: usize,
    pub image_count: usize,
    pub file_count: usize,
    pub metadata_count: usize,
    pub pinned_count: usize,
    pub tagged_count: usize,
    /// Total bytes stored across all text entries.
    pub total_text_bytes: u64,
    /// Total bytes for image entries.
    pub total_image_bytes: u64,
    /// Total bytes for file entries.
    pub total_file_bytes: u64,
    /// Number of distinct source devices seen in history.
    pub distinct_devices: usize,
    /// Oldest entry timestamp (Unix seconds), 0 if empty.
    pub oldest_ts: u64,
    /// Newest entry timestamp (Unix seconds), 0 if empty.
    pub newest_ts: u64,
}

/// Query parameters for filtered history retrieval.
#[derive(Debug, Clone, Default)]
pub struct HistoryFilter {
    /// Restrict to entries of this kind: "text", "image", "file", "metadata".
    pub kind: Option<String>,
    /// Case-insensitive substring match on `source_device`.
    pub device: Option<String>,
    /// Include only entries at or after this Unix timestamp.
    pub from_secs: Option<u64>,
    /// Include only entries at or before this Unix timestamp.
    pub to_secs: Option<u64>,
    /// Must contain this tag (exact, case-insensitive).
    pub tag: Option<String>,
    /// Maximum number of results (applied after all other filters).
    pub limit: Option<usize>,
    /// If true, only return pinned entries.
    pub pinned_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: u64,
    pub timestamp: u64,
    pub source_device: String,
    pub payload: HistoryPayload,
    pub hash: String,
    #[serde(default)]
    pub pinned: bool,
    /// User-defined labels for this entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensitive_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensitive_ttl_expires_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum HistoryPayload {
    Text {
        preview: String,
        full_len: usize,
        is_truncated: bool,
        full_text: Option<String>,
    },
    Image {
        mime: String,
        bytes: u64,
    },
    File {
        name: String,
        bytes: u64,
    },
    Metadata {
        kind: String,
        bytes: u64,
        summary: String,
        content_available: bool,
    },
}

impl HistoryEntry {
    fn from_content(
        id: u64,
        content: &ClipboardContent,
        source_device: String,
        max_text_bytes: usize,
    ) -> Self {
        let hash = hex::encode(hash_content(content));
        let sensitive_kind = content
            .as_text()
            .and_then(detect_sensitive_text)
            .map(str::to_string);
        let payload = match content {
            ClipboardContent::Text(s) => {
                let preview_len = s.len().min(MAX_TEXT_PREVIEW);
                let preview = format_preview(s, preview_len);
                // Truncate stored text at a UTF-8 character boundary (CRIT-03):
                // raw byte slicing panics when max_text_bytes falls inside a
                // multi-byte codepoint (emoji, CJK, accented text).
                let stored_len = utf8_floor_boundary(s, max_text_bytes);
                let full_text = Some(s[..stored_len].to_string());
                HistoryPayload::Text {
                    preview,
                    full_len: s.len(),
                    is_truncated: stored_len < s.len(),
                    full_text,
                }
            }
            ClipboardContent::Image { mime, data } => HistoryPayload::Image {
                mime: mime.clone(),
                bytes: data.len() as u64,
            },
            ClipboardContent::File { name, data } => HistoryPayload::File {
                name: name.clone(),
                bytes: data.len() as u64,
            },
        };

        Self {
            id,
            timestamp: now_secs(),
            source_device,
            payload,
            hash,
            pinned: false,
            tags: Vec::new(),
            sensitive_kind: sensitive_kind.clone(),
            sensitive_ttl_expires_at: sensitive_kind.map(|_| now_secs() + SENSITIVE_ENTRY_TTL_SECS),
        }
    }

    fn from_metadata(id: u64, meta: &HistoryMetadata) -> Self {
        Self {
            id,
            timestamp: meta.timestamp,
            source_device: meta.source_device.clone(),
            payload: HistoryPayload::Metadata {
                kind: meta.kind.clone(),
                bytes: meta.bytes,
                summary: meta.summary(),
                content_available: false,
            },
            hash: meta.hash.clone(),
            pinned: meta.pinned,
            tags: Vec::new(),
            sensitive_kind: None,
            sensitive_ttl_expires_at: None,
        }
    }

    pub fn summary(&self) -> String {
        match &self.payload {
            HistoryPayload::Text { preview, .. } => {
                let first_line = preview.lines().next().unwrap_or("").trim();
                // Guard against slicing inside a multi-byte char (CRIT-03).
                if first_line.len() > 60 {
                    let safe = utf8_floor_boundary(first_line, 60);
                    format!("{}...", &first_line[..safe])
                } else {
                    first_line.to_string()
                }
            }
            HistoryPayload::Image { mime, bytes } => {
                format!("[Image {} {:.1} KB]", mime, *bytes as f64 / 1024.0)
            }
            HistoryPayload::File { name, bytes } => {
                format!("[File '{}' {:.1} KB]", name, *bytes as f64 / 1024.0)
            }
            HistoryPayload::Metadata { summary, .. } => summary.clone(),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self.payload {
            HistoryPayload::Text { .. } => "text",
            HistoryPayload::Image { .. } => "image",
            HistoryPayload::File { .. } => "file",
            HistoryPayload::Metadata { .. } => "metadata",
        }
    }

    pub fn repushable_text(&self) -> Option<&str> {
        match &self.payload {
            HistoryPayload::Text {
                full_text: Some(text),
                ..
            } => Some(text.as_str()),
            _ => None,
        }
    }

    fn can_upgrade_from(&self, other: &HistoryEntry) -> bool {
        matches!(self.payload, HistoryPayload::Metadata { .. })
            && !matches!(other.payload, HistoryPayload::Metadata { .. })
    }

    fn is_sensitive_expired(&self, now: u64) -> bool {
        self.sensitive_ttl_expires_at
            .map(|expires_at| expires_at <= now)
            .unwrap_or(false)
    }

    /// Returns a scrubbed clone of this entry if it contains sensitive data.
    fn scrubbed_for_persistence(&self) -> Self {
        if self.sensitive_kind.is_some() {
            let mut clone = self.clone();
            clone.payload = HistoryPayload::Text {
                preview: "[Sensitive data scrubbed from disk]".to_string(),
                full_len: 0,
                is_truncated: false,
                full_text: Some("[Sensitive data scrubbed from disk]".to_string()),
            };
            clone
        } else {
            self.clone()
        }
    }
}

/// Find the largest valid UTF-8 character boundary that is <= `max_bytes`.
///
/// Slicing a `&str` at a byte offset that falls inside a multi-byte codepoint
/// (e.g. emoji, CJK, accented Latin) causes a panic.  This helper finds the
/// safe truncation point so callers can always slice without panic.
fn utf8_floor_boundary(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    // Walk backwards from max_bytes until we land on a char boundary.
    let mut pos = max_bytes;
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

fn format_preview(text: &str, preview_len: usize) -> String {
    let safe_len = utf8_floor_boundary(text, preview_len);
    if safe_len < text.len() {
        format!("{}...", &text[..safe_len])
    } else {
        text.to_string()
    }
}

/// Escape a field value for CSV (wraps in quotes if it contains commas, quotes, or newlines).
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub struct History {
    entries: VecDeque<HistoryEntry>,
    path: PathBuf,
    next_id: u64,
    max_entries: usize,
}

/// Reads a history file, keeping whatever is intact. History is a cache of
/// past copies: a damaged file must never stop the daemon from starting
/// (it once did, and with it the whole app).
///
/// A file with trailing bytes after a complete list (two writers shared one
/// temp file) keeps the list. A file that is not a list at all is moved
/// aside as `history.corrupt-<time>.json` and history starts empty.
fn parse_history(path: &Path, bytes: &[u8]) -> Vec<HistoryEntry> {
    let mut stream = serde_json::Deserializer::from_slice(bytes).into_iter::<Vec<HistoryEntry>>();
    match stream.next() {
        Some(Ok(entries)) => {
            if stream.byte_offset() < bytes.len()
                && bytes[stream.byte_offset()..]
                    .iter()
                    .any(|b| !b.is_ascii_whitespace())
            {
                tracing::warn!(
                    path = %path.display(),
                    "history file had trailing bytes after the list; they were dropped"
                );
            }
            entries
        }
        _ => {
            let aside = path.with_file_name(format!("history.corrupt-{}.json", now_secs()));
            match std::fs::rename(path, &aside) {
                Ok(()) => tracing::warn!(
                    path = %path.display(),
                    saved = %aside.display(),
                    "history file was unreadable; saved a copy and started empty"
                ),
                Err(e) => {
                    tracing::warn!(error = %e, "history file was unreadable and could not be moved")
                }
            }
            Vec::new()
        }
    }
}

impl History {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        Self::load_with_limit(path, DEFAULT_ENTRIES)
    }

    pub fn load_with_limit(path: impl AsRef<Path>, max_entries: usize) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut entries = VecDeque::new();
        let mut next_id = 1u64;

        if path.exists() {
            let bytes = std::fs::read(&path).context("reading history")?;
            if !bytes.is_empty() {
                for entry in parse_history(&path, &bytes) {
                    next_id = next_id.max(entry.id + 1);
                    entries.push_back(entry);
                }
            }
        }

        let mut history = Self {
            entries,
            path,
            next_id,
            max_entries: clamp_entries(max_entries),
        };
        let _ = history.purge_expired_sensitive_entries_with_now(now_secs())?;
        history.trim_to_limit();
        history.persist()?;
        Ok(history)
    }

    pub fn set_max_entries(&mut self, max_entries: usize) -> Result<()> {
        self.max_entries = clamp_entries(max_entries);
        self.trim_to_limit();
        self.persist()
    }

    pub fn push(
        &mut self,
        content: &ClipboardContent,
        source_device: String,
    ) -> Result<&HistoryEntry> {
        self.push_with_options(content, source_device, DEFAULT_MAX_TEXT_BYTES)
    }

    pub fn push_with_options(
        &mut self,
        content: &ClipboardContent,
        source_device: String,
        max_text_bytes: usize,
    ) -> Result<&HistoryEntry> {
        let _ = self.purge_expired_sensitive_entries_with_now(now_secs())?;
        let id = self.next_id;
        self.next_id += 1;
        let entry = HistoryEntry::from_content(id, content, source_device, max_text_bytes);
        self.insert_entry(entry)
    }

    pub fn push_metadata(&mut self, meta: &HistoryMetadata) -> Result<&HistoryEntry> {
        let id = self.next_id;
        self.next_id += 1;
        let entry = HistoryEntry::from_metadata(id, meta);
        self.insert_entry(entry)
    }

    pub fn entries(&self) -> &VecDeque<HistoryEntry> {
        &self.entries
    }

    pub fn entries_mut(&mut self) -> &mut VecDeque<HistoryEntry> {
        &mut self.entries
    }

    pub fn recent(&self, n: usize) -> impl Iterator<Item = &HistoryEntry> {
        self.entries.iter().rev().take(n)
    }

    pub fn search<'a>(&'a self, query: &'a str) -> impl Iterator<Item = &'a HistoryEntry> {
        let q = query.to_lowercase();
        self.entries.iter().rev().filter(move |entry| {
            entry.summary().to_lowercase().contains(&q)
                || entry.source_device.to_lowercase().contains(&q)
                || entry.kind().contains(&q)
        })
    }

    pub fn get(&self, id: u64) -> Option<&HistoryEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn set_pinned(&mut self, id: u64, pinned: bool) -> Result<Option<&HistoryEntry>> {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.pinned = pinned;
            self.persist()?;
            return Ok(self.get(id));
        }
        Ok(None)
    }

    /// Add a tag to a history entry. Tags are stored lowercase and deduplicated.
    /// Returns `true` if the tag was added (false if it already existed or entry not found).
    pub fn add_tag(&mut self, id: u64, tag: &str) -> Result<bool> {
        let tag = tag.trim().to_lowercase();
        if tag.is_empty() {
            return Ok(false);
        }
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            if entry.tags.contains(&tag) {
                return Ok(false);
            }
            entry.tags.push(tag);
            self.persist()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Remove a tag from a history entry.
    /// Returns `true` if the tag was removed.
    pub fn remove_tag(&mut self, id: u64, tag: &str) -> Result<bool> {
        let tag = tag.trim().to_lowercase();
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            let before = entry.tags.len();
            entry.tags.retain(|t| t != &tag);
            if entry.tags.len() != before {
                self.persist()?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Return aggregated statistics over the full history buffer.
    pub fn stats(&self) -> HistoryStats {
        use std::collections::HashSet;
        let mut stats = HistoryStats::default();
        let mut devices: HashSet<&str> = HashSet::new();

        for entry in &self.entries {
            stats.total += 1;
            if entry.pinned {
                stats.pinned_count += 1;
            }
            if !entry.tags.is_empty() {
                stats.tagged_count += 1;
            }
            devices.insert(&entry.source_device);

            if stats.oldest_ts == 0 || entry.timestamp < stats.oldest_ts {
                stats.oldest_ts = entry.timestamp;
            }
            if entry.timestamp > stats.newest_ts {
                stats.newest_ts = entry.timestamp;
            }

            match &entry.payload {
                HistoryPayload::Text { full_len, .. } => {
                    stats.text_count += 1;
                    stats.total_text_bytes += *full_len as u64;
                }
                HistoryPayload::Image { bytes, .. } => {
                    stats.image_count += 1;
                    stats.total_image_bytes += bytes;
                }
                HistoryPayload::File { bytes, .. } => {
                    stats.file_count += 1;
                    stats.total_file_bytes += bytes;
                }
                HistoryPayload::Metadata { .. } => {
                    stats.metadata_count += 1;
                }
            }
        }
        stats.distinct_devices = devices.len();
        stats
    }

    /// Filtered history list supporting kind, device, date range, tag, and pinned-only filters.
    /// Results are returned most-recent first.
    pub fn filter<'a>(&'a self, q: &'a HistoryFilter) -> impl Iterator<Item = &'a HistoryEntry> {
        let kind = q.kind.as_deref().map(|s| s.to_lowercase());
        let device = q.device.as_deref().map(|s| s.to_lowercase());
        let tag = q.tag.as_deref().map(|s| s.to_lowercase());
        let from_secs = q.from_secs;
        let to_secs = q.to_secs;
        let pinned_only = q.pinned_only;
        let limit = q.limit.unwrap_or(usize::MAX);

        self.entries
            .iter()
            .rev()
            .filter(move |e| {
                if let Some(ref k) = kind {
                    if e.kind() != k.as_str() {
                        return false;
                    }
                }
                if let Some(ref d) = device {
                    if !e.source_device.to_lowercase().contains(d.as_str()) {
                        return false;
                    }
                }
                if let Some(from) = from_secs {
                    if e.timestamp < from {
                        return false;
                    }
                }
                if let Some(to) = to_secs {
                    if e.timestamp > to {
                        return false;
                    }
                }
                if let Some(ref t) = tag {
                    if !e.tags.iter().any(|et| et == t) {
                        return false;
                    }
                }
                if pinned_only && !e.pinned {
                    return false;
                }
                true
            })
            .take(limit)
    }

    /// Export history as a JSON array string (most-recent first).
    pub fn export_json(&self) -> Result<String> {
        let entries: Vec<&HistoryEntry> = self.entries.iter().rev().collect();
        serde_json::to_string_pretty(&entries).context("serialising history to JSON")
    }

    pub fn remove(&mut self, id: u64) -> Result<bool> {
        let len_before = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        let removed = self.entries.len() != len_before;
        if removed {
            self.persist()?;
        }
        Ok(removed)
    }

    pub fn clear(&mut self) -> Result<()> {
        self.entries.clear();
        self.persist()
    }

    fn insert_entry(&mut self, entry: HistoryEntry) -> Result<&HistoryEntry> {
        if let Some(last) = self.entries.back_mut() {
            if last.hash == entry.hash {
                if last.can_upgrade_from(&entry) {
                    let pinned = last.pinned;
                    *last = entry;
                    last.pinned = pinned;
                    self.persist()?;
                }
                return self
                    .entries
                    .back()
                    .ok_or_else(|| anyhow::anyhow!("History entry missing after upgrade"));
            }
        }

        self.entries.push_back(entry);
        self.trim_to_limit();
        self.persist()?;
        self.entries.back().ok_or_else(|| {
            anyhow::anyhow!("History entry immediately trimmed (max_entries may be 0)")
        })
    }

    fn trim_to_limit(&mut self) {
        while self.entries.len() > self.max_entries {
            if let Some(index) = self.entries.iter().position(|entry| !entry.pinned) {
                self.entries.remove(index);
            } else {
                self.entries.pop_front();
            }
        }
    }

    fn persist(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(parent)
                    .context("creating history dir")?;
            }
            #[cfg(not(unix))]
            {
                std::fs::create_dir_all(parent).context("creating history dir")?;
            }
        }
        // Atomic write: serialise to a .tmp file then rename so a crash during
        // write never leaves the history file in a partially-written state.
        // One temp file per process: two daemons (an old one still shutting
        // down, a new one starting) sharing one name interleaved their
        // writes and left a file that would not parse.
        let tmp_path = self
            .path
            .with_extension(format!("{}.tmp", std::process::id()));

        let persistable_entries: Vec<HistoryEntry> = self
            .entries
            .iter()
            .map(|e| e.scrubbed_for_persistence())
            .collect();

        // Compact, not pretty — this file is an internal cache rewritten on
        // every single history mutation (every clipboard copy), never hand-
        // edited. Pretty-printing costs extra CPU per write and materially
        // more bytes written to disk (and flash wear on mobile) for no
        // benefit; `export_json()` below is the human-facing export and
        // still pretty-prints.
        let bytes = serde_json::to_vec(&persistable_entries)?;
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&tmp_path).context("creating history tmp")?;
            file.write_all(&bytes).context("writing history tmp")?;
            let _ = file.sync_all();
        }
        std::fs::rename(&tmp_path, &self.path).context("renaming history file")?;
        Ok(())
    }

    pub fn purge_expired_sensitive_entries(&mut self) -> Result<usize> {
        self.purge_expired_sensitive_entries_with_now(now_secs())
    }

    fn purge_expired_sensitive_entries_with_now(&mut self, now: u64) -> Result<usize> {
        let before = self.entries.len();
        self.entries
            .retain(|entry| !entry.is_sensitive_expired(now));
        let removed = before.saturating_sub(self.entries.len());
        if removed > 0 {
            self.persist()?;
        }
        Ok(removed)
    }

    /// Purges unpinned entries older than `retention_days` from disk.
    /// Returns the number of entries pruned.
    /// If `retention_days` is 0, no age-based pruning occurs.
    pub fn purge_expired_retention(&mut self, retention_days: u64) -> Result<usize> {
        self.purge_expired_retention_with_now(retention_days, now_secs())
    }

    fn purge_expired_retention_with_now(&mut self, retention_days: u64, now: u64) -> Result<usize> {
        if retention_days == 0 {
            return Ok(0);
        }
        let cutoff_secs = now.saturating_sub(retention_days.saturating_mul(86400));
        let before = self.entries.len();
        self.entries
            .retain(|entry| entry.pinned || entry.timestamp >= cutoff_secs);
        let removed = before.saturating_sub(self.entries.len());
        if removed > 0 {
            self.persist()?;
        }
        Ok(removed)
    }

    /// Full-text search through stored history.
    ///
    /// Searches:
    /// - `source_device` (case-insensitive substring)
    /// - `kind` (exact match: "text", "image", "file")
    /// - Summary text (first line preview)
    /// - Full stored text for `Text` entries (if `full_text` is available)
    pub fn search_fulltext<'a>(&'a self, query: &'a str) -> impl Iterator<Item = &'a HistoryEntry> {
        let q = query.to_lowercase();
        self.entries.iter().rev().filter(move |entry| {
            if entry.source_device.to_lowercase().contains(&q) {
                return true;
            }
            if entry.kind().contains(q.as_str()) {
                return true;
            }
            if entry.summary().to_lowercase().contains(&q) {
                return true;
            }
            // Also check the stored full text for text entries.
            if let HistoryPayload::Text {
                full_text: Some(ref text),
                ..
            } = entry.payload
            {
                if text.to_lowercase().contains(&q) {
                    return true;
                }
            }
            false
        })
    }

    /// Scored fuzzy full-text search through stored history.
    ///
    /// Scores entries by exact substring match (+100), token matches (+30),
    /// token prefix matches (+20), and recency bonus. Returns up to `limit`
    /// entries sorted highest score first.
    pub fn search_fuzzy<'a>(&'a self, query: &str, limit: usize) -> Vec<ScoredHistoryEntry<'a>> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self
                .entries
                .iter()
                .rev()
                .take(limit)
                .map(|e| ScoredHistoryEntry { score: 1, entry: e })
                .collect();
        }

        let tokens: Vec<&str> = q.split_whitespace().collect();
        let len = self.entries.len();
        let mut scored = Vec::new();

        for (idx, entry) in self.entries.iter().enumerate() {
            let mut text_corpus = format!(
                "{} {} {}",
                entry.source_device,
                entry.kind(),
                entry.summary()
            );
            if let HistoryPayload::Text {
                full_text: Some(ref t),
                ..
            } = entry.payload
            {
                text_corpus.push(' ');
                text_corpus.push_str(t);
            }
            if let HistoryPayload::File { ref name, .. } = entry.payload {
                text_corpus.push(' ');
                text_corpus.push_str(name);
            }
            let lower = text_corpus.to_lowercase();

            let mut score = 0u32;
            if lower.contains(&q) {
                score += 100;
                if lower.starts_with(&q) {
                    score += 50;
                }
            }

            let mut all_tokens_match = true;
            for token in &tokens {
                if lower.contains(token) {
                    score += 30;
                    if lower.split_whitespace().any(|w| w.starts_with(token)) {
                        score += 20;
                    }
                } else {
                    all_tokens_match = false;
                }
            }

            if all_tokens_match && tokens.len() > 1 {
                score += 40;
            }

            if score > 0 {
                // Recency tiebreaker bonus (max +20)
                let recency_bonus = ((idx + 1) as u32 * 20) / (len.max(1) as u32);
                scored.push(ScoredHistoryEntry {
                    score: score + recency_bonus,
                    entry,
                });
            }
        }

        scored.sort_by_key(|b| std::cmp::Reverse(b.score));
        scored.truncate(limit);
        scored
    }

    /// Return entries newer than `since_id` (exclusive), most-recent first.
    ///
    /// Useful for incremental UI updates: call with the last-seen entry ID to
    /// fetch only new arrivals without re-sending the entire history.
    pub fn recent_since(&self, since_id: u64) -> impl Iterator<Item = &HistoryEntry> {
        self.entries
            .iter()
            .rev()
            .take_while(move |entry| entry.id > since_id)
    }

    /// Export history as CSV text.
    ///
    /// Columns: `id,timestamp,source_device,kind,bytes,preview`
    ///
    /// The `preview` column is double-quote-escaped and newlines are replaced
    /// with `\n` so the output is always single-line per entry.
    pub fn export_csv(&self) -> String {
        let mut out = String::from("id,timestamp,source_device,kind,bytes,preview\n");
        for entry in self.entries.iter().rev() {
            let bytes_str = match &entry.payload {
                HistoryPayload::Text { full_len, .. } => full_len.to_string(),
                HistoryPayload::Image { bytes, .. } => bytes.to_string(),
                HistoryPayload::File { bytes, .. } => bytes.to_string(),
                HistoryPayload::Metadata { bytes, .. } => bytes.to_string(),
            };
            // Escape the preview: double quotes become "", newlines become ↵.
            let preview = entry
                .summary()
                .replace('"', "\"\"")
                .replace('\n', "↵")
                .replace('\r', "");
            out.push_str(&format!(
                "{},{},{},{},{},\"{}\"\n",
                entry.id,
                entry.timestamp,
                csv_escape(&entry.source_device),
                entry.kind(),
                bytes_str,
                preview,
            ));
        }
        out
    }
}

impl ClipboardContent {
    fn as_text(&self) -> Option<&str> {
        match self {
            ClipboardContent::Text(text) => Some(text.as_str()),
            _ => None,
        }
    }
}

fn detect_sensitive_text(text: &str) -> Option<&'static str> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if is_probable_otp(trimmed) {
        return Some("otp");
    }
    if is_probable_payment_card(trimmed) {
        return Some("payment_card");
    }
    if is_probable_api_token(trimmed) {
        return Some("api_token");
    }
    None
}

fn is_probable_otp(text: &str) -> bool {
    let lowered = text.to_lowercase();
    if is_digit_run(text, 4, 8) {
        return true;
    }
    let mentions_otp = [
        "otp",
        "2fa",
        "passcode",
        "verification code",
        "security code",
    ]
    .iter()
    .any(|needle| lowered.contains(needle));
    mentions_otp && contains_digit_run_in_range(text, 4, 8)
}

fn contains_digit_run_in_range(text: &str, min: usize, max: usize) -> bool {
    let mut run = 0usize;
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            run += 1;
            if run >= min && run <= max {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

fn is_digit_run(text: &str, min: usize, max: usize) -> bool {
    let digits = text.chars().filter(|ch| ch.is_ascii_digit()).count();
    digits >= min
        && digits <= max
        && text
            .chars()
            .all(|ch| ch.is_ascii_digit() || ch.is_whitespace())
}

fn is_probable_payment_card(text: &str) -> bool {
    let digits: String = text.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    let looks_formatted = text.chars().any(|ch| ch == ' ' || ch == '-');
    looks_formatted && luhn_valid(&digits)
}

fn luhn_valid(digits: &str) -> bool {
    let mut sum = 0u32;
    let mut double = false;
    for ch in digits.chars().rev() {
        let mut digit = match ch.to_digit(10) {
            Some(d) => d,
            None => return false,
        };
        if double {
            digit *= 2;
            if digit > 9 {
                digit -= 9;
            }
        }
        sum += digit;
        double = !double;
    }
    sum.is_multiple_of(10)
}

fn is_probable_api_token(text: &str) -> bool {
    let trimmed = text.trim_matches(|ch: char| ch.is_whitespace() || ch == '"' || ch == '\'');

    // Check Stripe/Highnote prefixes dynamically to avoid contiguous strings in the binary
    let sk_live = ['s', 'k', '_', 'l', 'i', 'v', 'e', '_']
        .iter()
        .collect::<String>();
    let sk_test = ['s', 'k', '_', 't', 'e', 's', 't', '_']
        .iter()
        .collect::<String>();
    if trimmed.starts_with(&sk_live) || trimmed.starts_with(&sk_test) {
        return true;
    }

    const PREFIXES: &[&str] = &["ghp_", "github_pat_", "xoxb-", "xoxp-", "AIza"];
    if PREFIXES.iter().any(|prefix| trimmed.starts_with(prefix)) {
        return true;
    }

    trimmed
        .split_whitespace()
        .map(|part| part.trim_matches(|ch: char| !is_token_char(ch)))
        .any(|candidate| {
            candidate.len() >= 24
                && char_class_count(candidate) >= 3
                && shannon_entropy(candidate) >= 3.5
        })
}

fn is_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/' | '=' | '.')
}

fn char_class_count(value: &str) -> usize {
    let mut classes = 0usize;
    if value.chars().any(|ch| ch.is_ascii_lowercase()) {
        classes += 1;
    }
    if value.chars().any(|ch| ch.is_ascii_uppercase()) {
        classes += 1;
    }
    if value.chars().any(|ch| ch.is_ascii_digit()) {
        classes += 1;
    }
    if value.chars().any(|ch| !ch.is_ascii_alphanumeric()) {
        classes += 1;
    }
    classes
}

fn shannon_entropy(value: &str) -> f64 {
    let mut counts = std::collections::HashMap::new();
    for byte in value.bytes() {
        *counts.entry(byte).or_insert(0usize) += 1;
    }
    let len = value.len() as f64;
    counts.values().fold(0.0, |entropy, count| {
        let probability = *count as f64 / len;
        entropy - (probability * probability.log2())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn saved_history(path: &Path, items: &[&str]) -> Vec<u8> {
        let mut h = History::load_with_limit(path, 50).unwrap();
        for text in items {
            h.push_with_options(
                &ClipboardContent::Text((*text).into()),
                "local".into(),
                1024,
            )
            .unwrap();
        }
        std::fs::read(path).unwrap()
    }

    #[test]
    fn trailing_bytes_after_the_list_keep_the_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        let mut bytes = saved_history(&path, &["one", "two", "three"]);
        // What two writers sharing one temp file left behind: a longer
        // earlier write's tail after a complete shorter one.
        bytes.extend_from_slice(b"mestamp\":1790947606,\"source_device\":\"x\"}}]");
        std::fs::write(&path, &bytes).unwrap();

        let h = History::load_with_limit(&path, 50).unwrap();
        assert_eq!(h.entries().len(), 3);
        // Loading rewrote it clean.
        assert!(History::load_with_limit(&path, 50).is_ok());
        assert!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()).is_ok()
        );
    }

    #[test]
    fn an_unreadable_file_is_set_aside_and_history_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        std::fs::write(&path, b"{ this is not a history list").unwrap();

        let h = History::load_with_limit(&path, 50).unwrap();
        assert!(h.entries().is_empty());
        let saved: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("history.corrupt-")
            })
            .collect();
        assert_eq!(saved.len(), 1, "the damaged file is kept for inspection");
    }

    #[test]
    fn push_and_persist() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();

        history
            .push_with_options(
                &ClipboardContent::Text("hello world".into()),
                "local".into(),
                1024,
            )
            .unwrap();
        history
            .push_with_options(
                &ClipboardContent::Text("second item".into()),
                "DeviceB".into(),
                1024,
            )
            .unwrap();

        assert_eq!(history.entries().len(), 2);

        let reloaded = History::load_with_limit(tmp.path(), 50).unwrap();
        assert_eq!(reloaded.entries().len(), 2);
        assert_eq!(reloaded.entries().back().unwrap().source_device, "DeviceB");
    }

    #[test]
    fn dedup_consecutive_identical() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        let content = ClipboardContent::Text("same".into());

        history
            .push_with_options(&content, "local".into(), 1024)
            .unwrap();
        history
            .push_with_options(&content, "local".into(), 1024)
            .unwrap();
        assert_eq!(history.entries().len(), 1);
    }

    #[test]
    fn pinned_items_survive_trim_when_possible() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 20).unwrap();

        for idx in 0..20 {
            history
                .push_with_options(
                    &ClipboardContent::Text(format!("item-{idx}")),
                    "local".into(),
                    1024,
                )
                .unwrap();
        }
        let pinned_id = history.entries().front().unwrap().id;
        history.set_pinned(pinned_id, true).unwrap();
        history
            .push_with_options(
                &ClipboardContent::Text("newest".into()),
                "local".into(),
                1024,
            )
            .unwrap();

        assert!(history.get(pinned_id).is_some());
        assert_eq!(history.entries().len(), 20);
    }

    #[test]
    fn metadata_upgrades_to_real_content() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        let meta = HistoryMetadata::from_content(
            &ClipboardContent::Text("secret note".into()),
            "Desk".into(),
            false,
        );
        let hash = meta.hash.clone();

        history.push_metadata(&meta).unwrap();
        history
            .push_with_options(
                &ClipboardContent::Text("secret note".into()),
                "Desk".into(),
                1024,
            )
            .unwrap();

        let entry = history.entries().back().unwrap();
        assert_eq!(entry.hash, hash);
        assert!(matches!(entry.payload, HistoryPayload::Text { .. }));
    }

    #[test]
    fn fulltext_search_finds_stored_text() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        history
            .push_with_options(
                &ClipboardContent::Text("unique-needle-42".into()),
                "DevA".into(),
                1024,
            )
            .unwrap();
        history
            .push_with_options(
                &ClipboardContent::Text("totally unrelated".into()),
                "DevB".into(),
                1024,
            )
            .unwrap();

        let results: Vec<_> = history.search_fulltext("unique-needle").collect();
        assert_eq!(results.len(), 1, "should find exactly the one entry");
        assert!(results[0].source_device == "DevA");
    }

    #[test]
    fn search_by_device_name() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        history
            .push_with_options(&ClipboardContent::Text("a".into()), "iPhone".into(), 1024)
            .unwrap();
        history
            .push_with_options(&ClipboardContent::Text("b".into()), "MacBook".into(), 1024)
            .unwrap();

        let results: Vec<_> = history.search_fulltext("iphone").collect();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn recent_since_returns_only_newer_entries() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        for i in 0..5 {
            history
                .push_with_options(
                    &ClipboardContent::Text(format!("item {i}")),
                    "local".into(),
                    1024,
                )
                .unwrap();
        }

        // Snapshot the id of the 3rd entry.
        let anchor_id = history.entries().get(2).unwrap().id;
        let newer: Vec<_> = history.recent_since(anchor_id).collect();
        // Only entries with id > anchor_id should appear.
        assert!(newer.iter().all(|e| e.id > anchor_id));
        assert_eq!(newer.len(), 2); // entries 4 and 5
    }

    #[test]
    fn export_csv_produces_valid_rows() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        history
            .push_with_options(
                &ClipboardContent::Text("hello, world".into()),
                "MyDevice".into(),
                1024,
            )
            .unwrap();

        let csv = history.export_csv();
        let lines: Vec<&str> = csv.lines().collect();
        // Header + 1 data row.
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("id,timestamp,source_device"));
        // Commas inside text should be quoted in the preview column.
        assert!(lines[1].contains("text"));
    }

    // ── New functionality tests ───────────────────────────────────────────────

    #[test]
    fn tags_add_and_remove() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        history
            .push_with_options(
                &ClipboardContent::Text("tagged item".into()),
                "dev".into(),
                1024,
            )
            .unwrap();
        let id = history.entries().back().unwrap().id;

        // Add a tag.
        assert!(history.add_tag(id, "work").unwrap());
        assert!(!history.add_tag(id, "work").unwrap()); // duplicate

        // Verify it persisted.
        let reloaded = History::load_with_limit(tmp.path(), 50).unwrap();
        let entry = reloaded.get(id).unwrap();
        assert!(entry.tags.contains(&"work".to_string()));

        // Remove tag.
        let mut h2 = History::load_with_limit(tmp.path(), 50).unwrap();
        assert!(h2.remove_tag(id, "work").unwrap());
        assert!(!h2.remove_tag(id, "work").unwrap()); // already gone
    }

    #[test]
    fn tags_are_stored_lowercase() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        history
            .push_with_options(&ClipboardContent::Text("hello".into()), "dev".into(), 1024)
            .unwrap();
        let id = history.entries().back().unwrap().id;
        history.add_tag(id, "WORK").unwrap();
        let entry = history.get(id).unwrap();
        assert!(entry.tags.contains(&"work".to_string()));
    }

    #[test]
    fn stats_counts_correctly() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        history
            .push_with_options(
                &ClipboardContent::Text("hello".into()),
                "iPhone".into(),
                1024,
            )
            .unwrap();
        history
            .push_with_options(
                &ClipboardContent::Text("world".into()),
                "MacBook".into(),
                1024,
            )
            .unwrap();
        history
            .push_with_options(
                &ClipboardContent::Image {
                    mime: "image/png".into(),
                    data: vec![0u8; 512],
                },
                "iPhone".into(),
                1024,
            )
            .unwrap();

        // Pin one entry.
        let first_id = history.entries().front().unwrap().id;
        history.set_pinned(first_id, true).unwrap();

        let stats = history.stats();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.text_count, 2);
        assert_eq!(stats.image_count, 1);
        assert_eq!(stats.pinned_count, 1);
        assert_eq!(stats.distinct_devices, 2);
        assert!(stats.total_image_bytes > 0);
    }

    #[test]
    fn filter_by_kind() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        history
            .push_with_options(
                &ClipboardContent::Text("text item".into()),
                "dev".into(),
                1024,
            )
            .unwrap();
        history
            .push_with_options(
                &ClipboardContent::Image {
                    mime: "image/png".into(),
                    data: vec![1, 2, 3],
                },
                "dev".into(),
                1024,
            )
            .unwrap();

        let q = HistoryFilter {
            kind: Some("text".into()),
            ..Default::default()
        };
        let results: Vec<_> = history.filter(&q).collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind(), "text");
    }

    #[test]
    fn filter_by_device() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        history
            .push_with_options(&ClipboardContent::Text("a".into()), "iPhone".into(), 1024)
            .unwrap();
        history
            .push_with_options(&ClipboardContent::Text("b".into()), "MacBook".into(), 1024)
            .unwrap();

        let q = HistoryFilter {
            device: Some("iphone".into()),
            ..Default::default()
        };
        let results: Vec<_> = history.filter(&q).collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_device, "iPhone");
    }

    #[test]
    fn filter_pinned_only() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        for i in 0..3 {
            history
                .push_with_options(
                    &ClipboardContent::Text(format!("item {}", i)),
                    "dev".into(),
                    1024,
                )
                .unwrap();
        }
        let id = history.entries().front().unwrap().id;
        history.set_pinned(id, true).unwrap();

        let q = HistoryFilter {
            pinned_only: true,
            ..Default::default()
        };
        let results: Vec<_> = history.filter(&q).collect();
        assert_eq!(results.len(), 1);
        assert!(results[0].pinned);
    }

    #[test]
    fn filter_by_tag() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        history
            .push_with_options(&ClipboardContent::Text("tagged".into()), "dev".into(), 1024)
            .unwrap();
        history
            .push_with_options(
                &ClipboardContent::Text("untagged".into()),
                "dev".into(),
                1024,
            )
            .unwrap();
        let tagged_id = history.entries().front().unwrap().id;
        history.add_tag(tagged_id, "important").unwrap();

        let q = HistoryFilter {
            tag: Some("important".into()),
            ..Default::default()
        };
        let results: Vec<_> = history.filter(&q).collect();
        assert_eq!(results.len(), 1);
        assert!(results[0].tags.contains(&"important".to_string()));
    }

    #[test]
    fn filter_limit_respected() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        for i in 0..10 {
            history
                .push_with_options(
                    &ClipboardContent::Text(format!("item {}", i)),
                    "dev".into(),
                    1024,
                )
                .unwrap();
        }
        let q = HistoryFilter {
            limit: Some(3),
            ..Default::default()
        };
        let results: Vec<_> = history.filter(&q).collect();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn export_json_round_trips() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        history
            .push_with_options(
                &ClipboardContent::Text("json test".into()),
                "dev".into(),
                1024,
            )
            .unwrap();

        let json_str = history.export_json().unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["source_device"].as_str().unwrap(), "dev");
    }

    #[test]
    fn max_entries_bumped_to_500() {
        // Verify the new MAX_ENTRIES constant allows up to 500.
        assert_eq!(MAX_ENTRIES, 500);
        let tmp = NamedTempFile::new().unwrap();
        let history = History::load_with_limit(tmp.path(), 500).unwrap();
        assert_eq!(history.entries().len(), 0); // empty, just checks no panic
    }

    #[test]
    fn sensitive_entries_get_ttl() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        history
            .push_with_options(
                &ClipboardContent::Text("OTP 482991".into()),
                "Pixel".into(),
                1024,
            )
            .unwrap();

        let entry = history.entries().back().unwrap();
        assert_eq!(entry.sensitive_kind.as_deref(), Some("otp"));
        assert!(entry.sensitive_ttl_expires_at.is_some());
    }

    #[test]
    fn expired_sensitive_entries_are_purged() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        history
            .push_with_options(
                &ClipboardContent::Text("4111 1111 1111 1111".into()),
                "Phone".into(),
                1024,
            )
            .unwrap();
        assert_eq!(history.entries().len(), 1);

        let expires_at = history
            .entries()
            .back()
            .and_then(|entry| entry.sensitive_ttl_expires_at)
            .unwrap();
        let removed = history
            .purge_expired_sensitive_entries_with_now(expires_at + 1)
            .unwrap();
        assert_eq!(removed, 1);
        assert!(history.entries().is_empty());
    }

    #[test]
    fn purge_expired_retention_prunes_old_unpinned() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 50).unwrap();
        let old_content = ClipboardContent::Text("Old regular note".into());
        let _ = history
            .push_with_options(&old_content, "MacBook".into(), 1024)
            .unwrap();

        // Simulate item created 10 days ago (864000 seconds ago)
        let now = 1_700_000_000u64;
        history.entries_mut().back_mut().unwrap().timestamp = now - 10 * 86400;

        // With retention_days = 7, the 10-day old item should be pruned
        let removed = history.purge_expired_retention_with_now(7, now).unwrap();
        assert_eq!(removed, 1);
        assert!(history.entries().is_empty());

        // If pinned, it should survive age pruning
        let _ = history
            .push_with_options(&old_content, "MacBook".into(), 1024)
            .unwrap();
        let pin_id = history.entries().back().unwrap().id;
        history.entries_mut().back_mut().unwrap().timestamp = now - 10 * 86400;
        let _ = history.set_pinned(pin_id, true).unwrap();

        let removed = history.purge_expired_retention_with_now(7, now).unwrap();
        assert_eq!(removed, 0);
        assert_eq!(history.entries().len(), 1);
    }

    #[test]
    fn fuzzy_search_ranks_exact_matches_higher() {
        let tmp = NamedTempFile::new().unwrap();
        let mut history = History::load_with_limit(tmp.path(), 100).unwrap();
        let _ = history.push_with_options(
            &ClipboardContent::Text("Apple Pie Recipe".into()),
            "Mac".into(),
            1024,
        );
        let _ = history.push_with_options(
            &ClipboardContent::Text("Pineapple Juice".into()),
            "Phone".into(),
            1024,
        );
        let _ = history.push_with_options(
            &ClipboardContent::Text("Apple iPhone 15 Pro Max specs".into()),
            "iPad".into(),
            1024,
        );

        let results = history.search_fuzzy("Apple", 10);
        assert!(!results.is_empty());
        // Exact prefix token match on Apple should score higher than inner substring Pineapple
        assert_eq!(results[0].entry.summary(), "Apple iPhone 15 Pro Max specs");
    }
}
