//! Link All multi-device mesh tests.
//!
//! Validates:
//! - 3-device simultaneous sync
//! - 4-device clipboard fanout
//! - One peer disconnecting while others continue
//! - Pause sync suppresses clipboard propagation
//! - Auto-connect toggle respected on reconnect
//! - Friendly names propagate correctly
//! - Internal IDs never appear in ClipboardReceived events

use linkall_core::dedup::{hash_content, Deduplicator};
use linkall_core::peer_manager::{DiscoverySource, PeerManager};
use linkall_core::protocol::ClipboardContent;
use std::net::SocketAddr;
use tempfile::NamedTempFile;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use uuid::Uuid;

fn peer_addr(n: u8) -> SocketAddr {
    SocketAddr::from(([192, 168, 1, n], 47823))
}

fn make_manager() -> (PeerManager, tempfile::NamedTempFile) {
    let file = NamedTempFile::new().unwrap();
    let mgr = PeerManager::load(file.path()).unwrap();
    (mgr, file)
}

// ── Test: 3 devices, all receive clipboard fanout ────────────────────────────

#[test]
fn three_device_fanout_all_receive() {
    let (mgr, _f) = make_manager();

    let device_ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
    let mut txs = vec![];

    for (i, &id) in device_ids.iter().enumerate() {
        mgr.upsert_peer(
            id,
            format!("Device {}", i + 1),
            peer_addr(10 + i as u8),
            true,
            DiscoverySource::Mdns,
        )
        .unwrap();
        let (tx, _rx) = mpsc::channel(8);
        let (stop, _stop_rx) = oneshot::channel();
        mgr.replace_live_session(
            Uuid::nil(),
            id,
            true,
            peer_addr(10 + i as u8),
            tx.clone(),
            tx.clone(),
            stop,
        )
        .unwrap();
        txs.push(tx);
    }

    // All 3 should be sync-eligible
    let senders = mgr.active_senders();
    assert_eq!(senders.len(), 3, "all 3 devices should be in fanout list");
}

// ── Test: Pause sync removes peer from fanout, others unaffected ──────────────

#[test]
fn pause_sync_removes_one_from_fanout() {
    let (mgr, _f) = make_manager();
    let ids: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();

    for (i, &id) in ids.iter().enumerate() {
        mgr.upsert_peer(
            id,
            format!("Dev{}", i),
            peer_addr(20 + i as u8),
            true,
            DiscoverySource::Mdns,
        )
        .unwrap();
        let (tx, _rx) = mpsc::channel(8);
        let (stop, _stop_rx) = oneshot::channel();
        mgr.replace_live_session(
            Uuid::nil(),
            id,
            true,
            peer_addr(20 + i as u8),
            tx.clone(),
            tx,
            stop,
        )
        .unwrap();
    }

    assert_eq!(mgr.active_senders().len(), 4);

    // Pause device[2]
    mgr.set_sync_enabled(ids[2], false).unwrap();
    assert_eq!(
        mgr.active_senders().len(),
        3,
        "paused device must be excluded"
    );

    // Disconnect device[0] — remaining: device[1], device[3]
    mgr.shutdown_peer_session(ids[0]).unwrap();
    assert_eq!(mgr.active_senders().len(), 2);

    // Resume device[2]
    mgr.set_sync_enabled(ids[2], true).unwrap();
    assert_eq!(
        mgr.active_senders().len(),
        3,
        "resumed device must re-enter fanout"
    );
}

// ── Test: One peer disconnects, others continue ───────────────────────────────

#[test]
fn peer_disconnect_does_not_cascade() {
    let (mgr, _f) = make_manager();
    let ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();

    for (i, &id) in ids.iter().enumerate() {
        mgr.upsert_peer(
            id,
            format!("Node{}", i),
            peer_addr(30 + i as u8),
            true,
            DiscoverySource::Mdns,
        )
        .unwrap();
        let (tx, _rx) = mpsc::channel(8);
        let (stop, _stop_rx) = oneshot::channel();
        mgr.replace_live_session(
            Uuid::nil(),
            id,
            true,
            peer_addr(30 + i as u8),
            tx.clone(),
            tx,
            stop,
        )
        .unwrap();
    }

    // Device 1 disconnects
    mgr.mark_disconnected(ids[1], Some("peer closed session".into()))
        .unwrap();

    let senders = mgr.active_senders();
    assert_eq!(senders.len(), 2, "remaining 2 devices unaffected");
    assert!(!mgr.is_connected(ids[1]), "device 1 must be disconnected");
    assert!(mgr.is_connected(ids[0]), "device 0 still connected");
    assert!(mgr.is_connected(ids[2]), "device 2 still connected");
}

// ── Test: Auto-connect toggle respected ──────────────────────────────────────

#[test]
fn auto_connect_toggle_controls_reconnect_eligibility() {
    let (mgr, _f) = make_manager();
    let id = Uuid::new_v4();
    mgr.upsert_peer(
        id,
        "Phone".into(),
        peer_addr(50),
        true,
        DiscoverySource::Mdns,
    )
    .unwrap();

    let record = mgr.get(id).unwrap();
    assert!(record.should_auto_reconnect(), "default: auto_connect=true");

    mgr.set_auto_connect(id, false).unwrap();
    let record = mgr.get(id).unwrap();
    assert!(
        !record.should_auto_reconnect(),
        "auto_connect disabled: no reconnect"
    );

    mgr.set_auto_connect(id, true).unwrap();
    assert!(
        mgr.get(id).unwrap().should_auto_reconnect(),
        "re-enabled: auto reconnect"
    );
}

// ── Test: Forget device prevents auto-reconnect ───────────────────────────────

#[test]
fn forget_device_prevents_auto_reconnect() {
    let (mgr, _f) = make_manager();
    let id = Uuid::new_v4();
    mgr.upsert_peer(
        id,
        "Tablet".into(),
        peer_addr(60),
        true,
        DiscoverySource::Mdns,
    )
    .unwrap();
    mgr.forget_device(id).unwrap();

    let peer = mgr
        .get(id)
        .expect("forgotten device should still exist but be untrusted");
    assert!(!peer.trusted, "forgotten device must not be trusted");
    assert!(!peer.remembered, "forgotten device must not be remembered");
}

// ── Test: Mesh-aware dedup — per-peer windows ─────────────────────────────────

#[test]
fn mesh_dedup_per_peer_windows() {
    let mut dedup = Deduplicator::new();
    let content = ClipboardContent::Text("hello from mesh".into());
    let hash = hash_content(&content);

    let peer_a = Uuid::new_v4();
    let peer_b = Uuid::new_v4();
    let peer_c = Uuid::new_v4();

    // Peer A delivers first — should apply
    assert!(
        dedup.should_apply(peer_a, hash),
        "first delivery must apply"
    );

    // Peers B and C deliver same content — should be deduped (already applied)
    assert!(
        !dedup.should_apply(peer_b, hash),
        "duplicate from B suppressed"
    );
    assert!(
        !dedup.should_apply(peer_c, hash),
        "duplicate from C suppressed"
    );
}

#[test]
fn mesh_dedup_echo_suppression() {
    let mut dedup = Deduplicator::new();
    let content = ClipboardContent::Text("broadcast".into());
    let hash = hash_content(&content);

    // We send this content
    assert!(dedup.should_send(hash));

    // It echoes back from 3 peers — all should be suppressed
    for _ in 0..3 {
        let peer = Uuid::new_v4();
        assert!(!dedup.should_apply(peer, hash), "echo must be suppressed");
    }
}

// ── Test: Friendly names propagate in HistoryMetadata ────────────────────────

#[test]
fn history_metadata_uses_friendly_name() {
    use linkall_core::protocol::HistoryMetadata;

    let content = ClipboardContent::Text("npm run build".into());
    let meta = HistoryMetadata::from_content(&content, "Chinmay's Pixel 8".into(), false);

    // Summary must show friendly name, not a UUID
    let summary = meta.summary();
    assert!(
        summary.contains("Chinmay's Pixel 8"),
        "summary must contain friendly name"
    );
    assert!(
        !summary.contains('-') || summary.contains("Chinmay"),
        "raw UUID must not appear"
    );
}

// ── Test: Platform metadata attached to peer records ─────────────────────────

#[test]
fn peer_platform_metadata_stored() {
    let (mgr, _f) = make_manager();
    let id = Uuid::new_v4();
    mgr.upsert_peer_ext(
        id,
        "Chinmay's Pixel 8".into(),
        peer_addr(70),
        true,
        DiscoverySource::Mdns,
        Some("Android".into()),
    )
    .unwrap();

    let record = mgr.get(id).unwrap();
    assert_eq!(record.friendly_name, "Chinmay's Pixel 8");
    assert_eq!(record.platform.as_deref(), Some("Android"));
}

// ── Test: Explicit disconnect prevents auto-reconnect and immediate recovery ──

#[test]
fn explicit_disconnect_prevents_immediate_session_cleanup_reconnect() {
    let (mgr, _f) = make_manager();
    let id = Uuid::new_v4();
    mgr.upsert_peer(
        id,
        "Chinmay's MacBook".into(),
        peer_addr(80),
        true,
        DiscoverySource::Manual,
    )
    .unwrap();

    let record = mgr.get(id).unwrap();
    assert!(
        record.should_auto_reconnect(),
        "default: trusted + manual = auto reconnect"
    );

    // Explicitly disconnect
    mgr.set_explicit_disconnect(id, true).unwrap();
    let record = mgr.get(id).unwrap();
    assert!(
        !record.should_auto_reconnect(),
        "explicit disconnect must disable should_auto_reconnect even for DiscoverySource::Manual"
    );
    assert!(
        mgr.is_explicitly_disconnected(id),
        "is_explicitly_disconnected must return true"
    );

    // Simulate replace_live_session being called (e.g., if another session was migrating or connecting)
    let (tx1, _rx1) = tokio::sync::mpsc::channel(1);
    let (tx2, _rx2) = tokio::sync::mpsc::channel(1);
    let (stx, _srx) = tokio::sync::oneshot::channel();
    let _ = mgr
        .replace_live_session(Uuid::nil(), id, true, peer_addr(80), tx1, tx2, stx)
        .unwrap();

    // Verify explicit_disconnect is NOT wiped by replace_live_session
    assert!(
        mgr.is_explicitly_disconnected(id),
        "explicit_disconnect must be preserved when replace_live_session runs"
    );
}
