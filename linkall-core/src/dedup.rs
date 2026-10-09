//! Clipboard deduplication — per-peer aware, multi-device mesh safe.
//!
//! # Echo storm prevention
//! When Device A broadcasts to B and C, B and C must not re-broadcast back.
//! We track per-peer send hashes so A never re-sends what it originated.
//!
//! # Multi-peer dedup
//! Two peers may deliver the same content simultaneously. The second delivery
//! is suppressed using a per-peer recent-hashes window instead of a single
//! global `last_received` value.

use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub type ContentHash = [u8; 32];

pub fn hash_content(content: &crate::protocol::ClipboardContent) -> ContentHash {
    let mut h = Sha256::new();
    match content {
        crate::protocol::ClipboardContent::Text(s) => {
            h.update(b"T");
            h.update(s.as_bytes());
        }
        crate::protocol::ClipboardContent::Image { mime, data } => {
            h.update(b"I");
            h.update(mime.as_bytes());
            h.update(data);
        }
        crate::protocol::ClipboardContent::File { name, data } => {
            h.update(b"F");
            h.update(name.as_bytes());
            h.update(data);
        }
    }
    h.finalize().into()
}

/// Window within which the same hash from the same peer is a duplicate.
const PEER_DEDUP_WINDOW: Duration = Duration::from_secs(5);

/// Per-peer receive dedup entry.
struct PeerWindow {
    hashes: HashMap<ContentHash, Instant>,
}

impl PeerWindow {
    fn new() -> Self {
        Self {
            hashes: HashMap::new(),
        }
    }

    fn seen_recently(&mut self, hash: ContentHash) -> bool {
        let now = Instant::now();
        self.hashes
            .retain(|_, t| now.duration_since(*t) < PEER_DEDUP_WINDOW);
        if self.hashes.contains_key(&hash) {
            return true;
        }
        if self.hashes.len() >= 256 {
            self.hashes.clear();
        }
        self.hashes.insert(hash, now);
        false
    }
}

/// Diagnostic counters for the deduplicator.
///
/// Useful for metrics dashboards and debugging noisy mesh behaviour.
#[derive(Debug, Clone, Default)]
pub struct DedupStats {
    /// Times a local send was allowed through.
    pub sends_allowed: u64,
    /// Times a local send was suppressed (content just received — echo guard).
    pub sends_suppressed: u64,
    /// Times an incoming payload was applied to the local clipboard.
    pub applies_allowed: u64,
    /// Times an incoming payload was dropped as an echo of our own send.
    pub echo_suppressed: u64,
    /// Times an incoming payload was dropped as a duplicate from a second peer.
    pub cross_peer_duplicates: u64,
    /// Times an incoming payload was dropped inside the per-peer dedup window.
    pub windowed_duplicates: u64,
}

impl DedupStats {
    /// Total suppressed (all categories).
    pub fn total_suppressed(&self) -> u64 {
        self.sends_suppressed
            + self.echo_suppressed
            + self.cross_peer_duplicates
            + self.windowed_duplicates
    }
}

/// Mesh-aware deduplicator.
///
/// Tracks:
/// - What we have sent (to suppress echoes back from any peer).
/// - Per-peer receive windows (to suppress duplicate delivery from two peers).
/// - What we last applied locally (to suppress spurious re-sends).
pub struct Deduplicator {
    /// Hashes of content we originated and sent to peers.
    sent_hashes: HashSet<ContentHash>,
    /// Per-peer receive windows.
    peer_windows: HashMap<Uuid, PeerWindow>,
    /// Hash of content most recently applied to the local clipboard.
    last_applied: Option<ContentHash>,
    /// Sent hash expiry — clear after this long to allow re-send.
    sent_at: HashMap<ContentHash, Instant>,
    /// Running statistics.
    stats: DedupStats,
}

const SENT_HASH_TTL: Duration = Duration::from_secs(10);

impl Deduplicator {
    pub fn new() -> Self {
        Self {
            sent_hashes: HashSet::new(),
            peer_windows: HashMap::new(),
            last_applied: None,
            sent_at: HashMap::new(),
            stats: DedupStats::default(),
        }
    }

    fn evict_stale_sent(&mut self) {
        let now = Instant::now();
        let stale: Vec<ContentHash> = self
            .sent_at
            .iter()
            .filter(|(_, t)| now.duration_since(**t) > SENT_HASH_TTL)
            .map(|(h, _)| *h)
            .collect();
        for h in stale {
            self.sent_hashes.remove(&h);
            self.sent_at.remove(&h);
        }
        if self.sent_hashes.len() >= 1024 {
            self.sent_hashes.clear();
            self.sent_at.clear();
        }
    }

    /// Call before broadcasting local clipboard content.
    /// Returns `true` if we should proceed.
    pub fn should_send(&mut self, hash: ContentHash) -> bool {
        self.evict_stale_sent();
        if self.last_applied == Some(hash) {
            // Content was just received — don't bounce back.
            self.stats.sends_suppressed += 1;
            return false;
        }
        self.sent_hashes.insert(hash);
        self.sent_at.insert(hash, Instant::now());
        self.stats.sends_allowed += 1;
        true
    }

    /// Call when incoming content arrives from a specific peer.
    /// Returns `true` if we should apply it to the local clipboard.
    pub fn should_apply(&mut self, from_peer: Uuid, hash: ContentHash) -> bool {
        self.evict_stale_sent();

        // Echo: we sent this, it's bouncing back.
        if self.sent_hashes.contains(&hash) {
            self.stats.echo_suppressed += 1;
            return false;
        }

        // Already applied (duplicate from two peers simultaneously).
        if self.last_applied == Some(hash) {
            self.stats.cross_peer_duplicates += 1;
            return false;
        }

        // Per-peer duplicate within the dedup window.
        let window = self
            .peer_windows
            .entry(from_peer)
            .or_insert_with(PeerWindow::new);
        if window.seen_recently(hash) {
            self.stats.windowed_duplicates += 1;
            return false;
        }

        self.last_applied = Some(hash);
        self.stats.applies_allowed += 1;
        true
    }

    /// Snapshot of deduplication statistics since the last `reset()`.
    pub fn stats(&self) -> &DedupStats {
        &self.stats
    }

    pub fn remove_peer(&mut self, peer_id: Uuid) {
        self.peer_windows.remove(&peer_id);
    }

    pub fn reset(&mut self) {
        self.sent_hashes.clear();
        self.sent_at.clear();
        self.last_applied = None;
        self.stats = DedupStats::default();
    }
}

impl Default for Deduplicator {
    fn default() -> Self {
        Self::new()
    }
}

// ── Per-peer rate limiter ─────────────────────────────────────────────────────

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    rate: f64,
    capacity: f64,
}

impl TokenBucket {
    fn new(rate: f64, capacity: f64) -> Self {
        Self {
            tokens: capacity,
            last_refill: Instant::now(),
            rate,
            capacity,
        }
    }

    fn try_consume(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.rate).min(self.capacity);
        self.last_refill = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

pub struct RateLimiter {
    buckets: HashMap<Uuid, TokenBucket>,
    rate_per_sec: f64,
    burst: f64,
    violations: HashMap<Uuid, u64>,
}

impl RateLimiter {
    pub fn new(rate_per_sec: f64, burst: f64) -> Self {
        Self {
            buckets: HashMap::new(),
            rate_per_sec,
            burst,
            violations: HashMap::new(),
        }
    }

    pub fn check(&mut self, peer_id: Uuid) -> bool {
        let bucket = self
            .buckets
            .entry(peer_id)
            .or_insert_with(|| TokenBucket::new(self.rate_per_sec, self.burst));
        if bucket.try_consume() {
            true
        } else {
            *self.violations.entry(peer_id).or_insert(0) += 1;
            false
        }
    }

    pub fn remove_peer(&mut self, peer_id: Uuid) {
        self.buckets.remove(&peer_id);
        self.violations.remove(&peer_id);
    }

    pub fn violation_count(&self, peer_id: Uuid) -> u64 {
        self.violations.get(&peer_id).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ClipboardContent;

    #[test]
    fn echo_suppression() {
        let mut dedup = Deduplicator::new();
        let content = ClipboardContent::Text("hello".into());
        let hash = hash_content(&content);
        assert!(dedup.should_send(hash));
        let peer = Uuid::new_v4();
        assert!(!dedup.should_apply(peer, hash)); // echo — suppress
    }

    #[test]
    fn duplicate_from_two_peers() {
        let mut dedup = Deduplicator::new();
        let content = ClipboardContent::Text("same".into());
        let hash = hash_content(&content);
        let peer_a = Uuid::new_v4();
        let peer_b = Uuid::new_v4();
        assert!(dedup.should_apply(peer_a, hash));
        assert!(!dedup.should_apply(peer_b, hash)); // duplicate
    }

    #[test]
    fn same_content_from_same_peer_in_window_is_dedup() {
        let mut dedup = Deduplicator::new();
        let hash = hash_content(&ClipboardContent::Text("x".into()));
        let peer = Uuid::new_v4();
        assert!(dedup.should_apply(peer, hash));
        assert!(!dedup.should_apply(peer, hash)); // same peer, same window
    }

    #[test]
    fn different_content_passes() {
        let mut dedup = Deduplicator::new();
        let peer = Uuid::new_v4();
        let h1 = hash_content(&ClipboardContent::Text("a".into()));
        let h2 = hash_content(&ClipboardContent::Text("b".into()));
        assert!(dedup.should_apply(peer, h1));
        assert!(dedup.should_apply(peer, h2));
    }

    #[test]
    fn rate_limiter_allows_burst() {
        let mut rl = RateLimiter::new(10.0, 3.0);
        let id = Uuid::new_v4();
        assert!(rl.check(id));
        assert!(rl.check(id));
        assert!(rl.check(id));
        assert!(!rl.check(id));
        assert_eq!(rl.violation_count(id), 1);
    }

    #[test]
    fn stats_track_echo_suppression() {
        let mut dedup = Deduplicator::new();
        let content = ClipboardContent::Text("hello".into());
        let hash = hash_content(&content);
        let peer = Uuid::new_v4();

        assert!(dedup.should_send(hash));
        assert!(!dedup.should_apply(peer, hash)); // echo

        let stats = dedup.stats();
        assert_eq!(stats.sends_allowed, 1);
        assert_eq!(stats.echo_suppressed, 1);
        assert_eq!(stats.applies_allowed, 0);
    }

    #[test]
    fn stats_track_cross_peer_duplicate() {
        let mut dedup = Deduplicator::new();
        let hash = hash_content(&ClipboardContent::Text("same".into()));
        let peer_a = Uuid::new_v4();
        let peer_b = Uuid::new_v4();

        assert!(dedup.should_apply(peer_a, hash));
        assert!(!dedup.should_apply(peer_b, hash));

        let stats = dedup.stats();
        assert_eq!(stats.applies_allowed, 1);
        assert_eq!(stats.cross_peer_duplicates, 1);
    }

    #[test]
    fn stats_reset_on_reset() {
        let mut dedup = Deduplicator::new();
        let hash = hash_content(&ClipboardContent::Text("x".into()));
        dedup.should_send(hash);
        assert_eq!(dedup.stats().sends_allowed, 1);
        dedup.reset();
        assert_eq!(dedup.stats().sends_allowed, 0);
        assert_eq!(dedup.stats().total_suppressed(), 0);
    }
}
