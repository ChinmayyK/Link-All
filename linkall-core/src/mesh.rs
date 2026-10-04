//! Link All Mesh — true multi-device fanout engine.
//!
//! Replaces pairwise session assumptions with an independent per-peer model.
//!
//! # Key guarantees
//! - Each peer connects, disconnects, and reconnects completely independently.
//! - Clipboard fanout reaches ALL eligible peers (trusted + sync-enabled).
//! - Relay path metadata tracks MacBook → Pixel 8 → Linux Desktop chains.
//! - Echo suppression: origin device is never re-sent its own content.
//! - Relay storms are prevented by TTL + relay path length checks.
//! - One peer crashing never affects healthy peers.

use crate::dedup::ContentHash;
use crate::protocol::AppMessage;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Maximum relay hops to prevent infinite mesh loops.
pub const MAX_RELAY_HOPS: usize = 4;

/// How long to remember a content hash for echo/storm suppression.
pub const RELAY_DEDUP_TTL: Duration = Duration::from_secs(8);

// ── Relay record ──────────────────────────────────────────────────────────────

/// Tracks a content hash's propagation across the mesh.
struct RelayRecord {
    /// Which devices have already received this content.
    seen_by: HashSet<Uuid>,
    /// When this record was first created.
    created_at: Instant,
}

impl RelayRecord {
    fn new(origin: Uuid) -> Self {
        let mut seen_by = HashSet::new();
        seen_by.insert(origin);
        Self {
            seen_by,
            created_at: Instant::now(),
        }
    }

    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > RELAY_DEDUP_TTL
    }

    fn mark_seen(&mut self, device: Uuid) {
        self.seen_by.insert(device);
    }

    fn already_seen(&self, device: Uuid) -> bool {
        self.seen_by.contains(&device)
    }
}

// ── MeshRouter ────────────────────────────────────────────────────────────────

/// Active sender handle for one peer session.
pub struct PeerSender {
    pub device_id: Uuid,
    pub device_name: String,
    pub trusted: bool,
    pub sync_enabled: bool,
    pub tx: mpsc::Sender<AppMessage>,
}

/// Mesh routing engine: manages fanout to all connected peers.
pub struct MeshRouter {
    /// Per-content relay dedup records.
    relay_records: HashMap<ContentHash, RelayRecord>,
    /// This device's own ID (never send back to self).
    local_device_id: Uuid,
    /// This device's name (for relay path metadata).
    local_device_name: String,
}

impl MeshRouter {
    pub fn new(local_device_id: Uuid, local_device_name: String) -> Self {
        Self {
            relay_records: HashMap::new(),
            local_device_id,
            local_device_name,
        }
    }

    /// Evict expired relay records to bound memory.
    fn evict_expired(&mut self) {
        self.relay_records.retain(|_, r| !r.is_expired());
    }

    /// Determine whether this content should be relayed to a specific peer.
    ///
    /// Returns `false` if:
    /// - The peer IS the origin (echo suppression)
    /// - The peer has already seen this content (duplicate suppression)
    /// - The relay path is too long (storm prevention)
    pub fn should_relay_to(
        &mut self,
        hash: ContentHash,
        origin: Uuid,
        target: Uuid,
        relay_path: &[String],
    ) -> bool {
        // Storm prevention: cap relay depth.
        if relay_path.len() >= MAX_RELAY_HOPS {
            return false;
        }
        // Never relay back to origin.
        if target == origin {
            return false;
        }
        // Never relay back to self (if we somehow appear as target).
        if target == self.local_device_id {
            return false;
        }
        // Check relay dedup record.
        let record = self
            .relay_records
            .entry(hash)
            .or_insert_with(|| RelayRecord::new(origin));
        if record.already_seen(target) {
            return false;
        }
        record.mark_seen(target);
        true
    }

    /// Register that we are originating content (prevents echo back to us).
    pub fn register_local_send(&mut self, hash: ContentHash) {
        self.evict_expired();
        let record = self
            .relay_records
            .entry(hash)
            .or_insert_with(|| RelayRecord::new(self.local_device_id));
        record.mark_seen(self.local_device_id);
    }

    /// Fan out a ClipboardPush to all eligible peers.
    ///
    /// - `origin_device`: the original sender UUID
    /// - `origin_name`: human-readable name of originating device
    /// - `relay_path`: devices that have already forwarded this message
    /// - `hash`: content hash for dedup tracking
    /// - `senders`: all currently connected peer senders
    ///
    /// Returns the list of (device_id, delivered) pairs.
    pub fn fanout(
        &mut self,
        msg: &AppMessage,
        hash: ContentHash,
        origin_device: Uuid,
        relay_path: &[String],
        senders: &[PeerSender],
    ) -> Vec<FanoutResult> {
        self.evict_expired();

        // Build extended relay path (add this node if we are relaying, not originating).
        let our_extended_path: Vec<String> = if origin_device == self.local_device_id {
            // We are the origin — path starts here.
            vec![self.local_device_name.clone()]
        } else {
            // We received from someone else and are relaying onward.
            let mut p = relay_path.to_vec();
            p.push(self.local_device_name.clone());
            p
        };

        if our_extended_path.len() > MAX_RELAY_HOPS {
            return Vec::new(); // Storm guard.
        }

        let mut results = Vec::new();
        for sender in senders {
            // Skip non-eligible peers.
            if !sender.trusted || !sender.sync_enabled {
                results.push(FanoutResult {
                    device_id: sender.device_id,
                    device_name: sender.device_name.clone(),
                    delivered: false,
                    skip_reason: Some(
                        if !sender.trusted {
                            "not trusted"
                        } else {
                            "sync paused"
                        }
                        .into(),
                    ),
                });
                continue;
            }

            // Dedup check.
            if !self.should_relay_to(hash, origin_device, sender.device_id, &our_extended_path) {
                results.push(FanoutResult {
                    device_id: sender.device_id,
                    device_name: sender.device_name.clone(),
                    delivered: false,
                    skip_reason: Some("relay dedup".into()),
                });
                continue;
            }

            // Inject updated relay path into message.
            let msg_with_path = inject_relay_path(msg.clone(), our_extended_path.clone());

            let delivered = sender.tx.try_send(msg_with_path).is_ok();
            results.push(FanoutResult {
                device_id: sender.device_id,
                device_name: sender.device_name.clone(),
                delivered,
                skip_reason: if delivered {
                    None
                } else {
                    Some("queue full".into())
                },
            });
        }
        results
    }
}

#[derive(Debug, Clone)]
pub struct FanoutResult {
    pub device_id: Uuid,
    pub device_name: String,
    pub delivered: bool,
    pub skip_reason: Option<String>,
}

/// Inject a relay_path into an AppMessage::ClipboardPush.
/// Other message types pass through unchanged.
fn inject_relay_path(msg: AppMessage, path: Vec<String>) -> AppMessage {
    match msg {
        AppMessage::ClipboardPush {
            seq,
            content,
            origin_device,
            origin_device_name,
            ..
        } => AppMessage::ClipboardPush {
            seq,
            content,
            origin_device,
            origin_device_name,
            relay_path: path,
        },
        other => other,
    }
}

// ── Clipboard apply policy ────────────────────────────────────────────────────

/// Decides whether received clipboard content should be auto-applied to
/// the local clipboard, or only recorded in the activity feed.
pub struct ClipboardApplyPolicy {
    /// True = timeline-first (default for ≥3 device environments).
    pub timeline_first: bool,
    /// If timeline_first=false OR auto_apply=true, apply automatically.
    pub auto_apply: bool,
    /// Allowed device IDs for auto-apply (empty = all trusted).
    pub auto_apply_allowed: Vec<Uuid>,
    /// Debounce: don't apply if last auto-apply was within this window.
    pub debounce: Duration,
    last_auto_apply: HashMap<Uuid, Instant>,
}

impl ClipboardApplyPolicy {
    pub fn new(
        timeline_first: bool,
        auto_apply: bool,
        allowed: Vec<Uuid>,
        debounce_ms: u64,
    ) -> Self {
        Self {
            timeline_first,
            auto_apply,
            auto_apply_allowed: allowed,
            debounce: Duration::from_millis(debounce_ms),
            last_auto_apply: HashMap::new(),
        }
    }

    /// Returns `true` if the clipboard should be auto-applied (overwrite local).
    /// Returns `false` if it should only be added to the activity feed.
    pub fn should_auto_apply(&mut self, from_device: Uuid) -> bool {
        // Timeline-first + auto_apply OFF: never auto-apply.
        if self.timeline_first && !self.auto_apply {
            return false;
        }
        // Check device allowlist (if set).
        if !self.auto_apply_allowed.is_empty() && !self.auto_apply_allowed.contains(&from_device) {
            return false;
        }
        // Debounce rapid updates per-peer to prevent one peer from starving others.
        if let Some(last) = self.last_auto_apply.get(&from_device) {
            if last.elapsed() < self.debounce {
                return false;
            }
        }
        self.last_auto_apply.insert(from_device, Instant::now());
        true
    }

    pub fn update_from_settings(&mut self, settings: &crate::settings::Settings) {
        self.timeline_first = settings.timeline_first_mode;
        self.auto_apply = settings.auto_apply_remote_clipboard;
        self.debounce = Duration::from_millis(settings.auto_apply_debounce_ms);
        self.auto_apply_allowed = settings
            .auto_apply_allowed_devices
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect();
    }

    /// Enable or disable timeline-first mode at runtime.
    pub fn set_timeline_first(&mut self, enabled: bool) {
        self.timeline_first = enabled;
    }

    /// Enable or disable automatic clipboard application at runtime.
    pub fn set_auto_apply(&mut self, enabled: bool) {
        self.auto_apply = enabled;
    }
}

impl Default for ClipboardApplyPolicy {
    fn default() -> Self {
        Self::new(true, false, Vec::new(), 500)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dedup::hash_content;
    use crate::protocol::ClipboardContent;

    fn make_hash(s: &str) -> ContentHash {
        hash_content(&ClipboardContent::Text(s.into()))
    }

    #[test]
    fn echo_suppression_prevents_relay_back_to_origin() {
        let local = Uuid::new_v4();
        let origin = Uuid::new_v4();
        let mut router = MeshRouter::new(local, "LocalDevice".into());
        let hash = make_hash("hello");

        // First relay to a third peer: allowed.
        let peer3 = Uuid::new_v4();
        assert!(router.should_relay_to(hash, origin, peer3, &[]));
        // Relay back to origin: blocked.
        assert!(!router.should_relay_to(hash, origin, origin, &[]));
    }

    #[test]
    fn duplicate_suppression_per_peer() {
        let local = Uuid::new_v4();
        let origin = Uuid::new_v4();
        let peer = Uuid::new_v4();
        let mut router = MeshRouter::new(local, "L".into());
        let hash = make_hash("dup");
        assert!(router.should_relay_to(hash, origin, peer, &[]));
        // Same peer again: blocked.
        assert!(!router.should_relay_to(hash, origin, peer, &[]));
    }

    #[test]
    fn storm_prevention_caps_relay_depth() {
        let local = Uuid::new_v4();
        let origin = Uuid::new_v4();
        let peer = Uuid::new_v4();
        let mut router = MeshRouter::new(local, "L".into());
        let hash = make_hash("storm");
        // Path already at max hops.
        let deep_path: Vec<String> = (0..MAX_RELAY_HOPS).map(|i| format!("hop{}", i)).collect();
        assert!(!router.should_relay_to(hash, origin, peer, &deep_path));
    }

    #[test]
    fn apply_policy_timeline_first_blocks_auto_apply() {
        let mut policy = ClipboardApplyPolicy::new(true, false, vec![], 0);
        let device = Uuid::new_v4();
        assert!(!policy.should_auto_apply(device));
    }

    #[test]
    fn apply_policy_auto_apply_with_allowlist() {
        let allowed = Uuid::new_v4();
        let blocked = Uuid::new_v4();
        let mut policy = ClipboardApplyPolicy::new(false, true, vec![allowed], 0);
        assert!(policy.should_auto_apply(allowed));
        assert!(!policy.should_auto_apply(blocked));
    }
}
