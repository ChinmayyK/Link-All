//! Rust-side tests validating behaviour that the Android notification UX depends on.
//!
//! The Android notification layer is thin — the real logic lives in:
//!   - `peer_manager::active_senders()` — respects `sync_enabled`
//!   - `dedup::Deduplicator`            — prevents echo storms
//!   - `engine` lifecycle methods       — pause/resume/forget
//!
//! These tests verify the contracts the Android side depends on.

use linkall_core::dedup::{hash_content, Deduplicator};
use linkall_core::peer_manager::{DiscoverySource, PeerManager};
use linkall_core::protocol::ClipboardContent;
use std::net::SocketAddr;
use tempfile::NamedTempFile;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

fn mgr() -> (PeerManager, NamedTempFile) {
    let f = NamedTempFile::new().unwrap();
    (PeerManager::load(f.path()).unwrap(), f)
}

fn addr(n: u8) -> SocketAddr {
    SocketAddr::from(([192, 168, 1, n], 47823))
}

// ── Sync pause suppresses fanout (notification silence depends on this) ───────

#[test]
fn paused_peer_never_receives_clipboard_push() {
    let (mgr, _f) = mgr();
    let id = Uuid::new_v4();
    mgr.upsert_peer(id, "Phone".into(), addr(10), true, DiscoverySource::Mdns)
        .unwrap();
    let (tx, _rx) = mpsc::channel(4);
    let (stop, _) = oneshot::channel();
    mgr.replace_live_session(Uuid::nil(), id, true, addr(10), tx.clone(), tx, stop)
        .unwrap();

    // Pause sync — peer must not appear in active_senders
    mgr.set_sync_enabled(id, false).unwrap();
    assert!(
        mgr.active_senders().is_empty(),
        "paused peer must not receive clipboard push"
    );

    // Resume — reappears
    mgr.set_sync_enabled(id, true).unwrap();
    assert_eq!(mgr.active_senders().len(), 1);
}

// ── Echo suppression prevents duplicate activity feed entries ─────────────────

#[test]
fn originating_device_does_not_echo_to_own_feed() {
    let mut dedup = Deduplicator::new();
    let content = ClipboardContent::Text("my own copy".into());
    let hash = hash_content(&content);

    // We send this — mark as sent
    assert!(dedup.should_send(hash));

    // If it echoes back from 3 peers, none should apply
    let peers: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
    for peer in &peers {
        assert!(
            !dedup.should_apply(*peer, hash),
            "echo from {} must be suppressed",
            peer
        );
    }
}

// ── Forget device: stops reconnect, preserves trust ───────────────────────────

#[test]
fn forgotten_peer_not_in_auto_reconnect_list() {
    let (mgr, _f) = mgr();

    let auto = Uuid::new_v4();
    let manual = Uuid::new_v4();
    let forgotten = Uuid::new_v4();

    for (id, name) in [(auto, "Auto"), (manual, "Manual"), (forgotten, "Forgotten")] {
        mgr.upsert_peer(id, name.into(), addr(20), true, DiscoverySource::Mdns)
            .unwrap();
    }

    mgr.forget_device(forgotten).unwrap();

    let peers = mgr.list();
    for p in &peers {
        if p.id == forgotten {
            assert!(
                !p.should_auto_reconnect(),
                "forgotten peer must not auto-reconnect"
            );
            assert!(!p.trusted, "forgotten peer trust must be revoked");
        } else {
            assert!(p.should_auto_reconnect(), "other peers must auto-reconnect");
        }
    }
}

// ── Four devices: only sync-enabled ones receive clipboard ────────────────────

#[test]
fn four_device_mesh_respects_pause() {
    let (mgr, _f) = mgr();

    let ids: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
    for (i, &id) in ids.iter().enumerate() {
        mgr.upsert_peer(
            id,
            format!("Dev{i}"),
            addr(30 + i as u8),
            true,
            DiscoverySource::Mdns,
        )
        .unwrap();
        let (tx, _) = mpsc::channel(4);
        let (stop, _) = oneshot::channel();
        mgr.replace_live_session(
            Uuid::nil(),
            id,
            true,
            addr(30 + i as u8),
            tx.clone(),
            tx,
            stop,
        )
        .unwrap();
    }

    assert_eq!(mgr.active_senders().len(), 4);

    // Pause two devices
    mgr.set_sync_enabled(ids[1], false).unwrap();
    mgr.set_sync_enabled(ids[3], false).unwrap();
    assert_eq!(mgr.active_senders().len(), 2);

    // Disconnect one active device
    mgr.mark_disconnected(ids[0], None).unwrap();
    assert_eq!(mgr.active_senders().len(), 1, "only ids[2] should remain");

    // Resume ids[1] — reconnect it too
    mgr.set_sync_enabled(ids[1], true).unwrap();
    let (tx2, _) = mpsc::channel(4);
    let (stop2, _) = oneshot::channel();
    mgr.replace_live_session(Uuid::nil(), ids[1], true, addr(31), tx2.clone(), tx2, stop2)
        .unwrap();
    assert_eq!(mgr.active_senders().len(), 2);
}

// ── All-connected senders still includes paused peers (heartbeats work) ───────

#[test]
fn all_connected_senders_includes_paused_peers() {
    let (mgr, _f) = mgr();
    let id = Uuid::new_v4();
    mgr.upsert_peer(id, "Tablet".into(), addr(50), true, DiscoverySource::Mdns)
        .unwrap();
    let (tx, _) = mpsc::channel(4);
    let (stop, _) = oneshot::channel();
    mgr.replace_live_session(Uuid::nil(), id, true, addr(50), tx.clone(), tx, stop)
        .unwrap();

    mgr.set_sync_enabled(id, false).unwrap();
    assert_eq!(mgr.active_senders().len(), 0, "paused: no clipboard push");
    assert_eq!(
        mgr.all_connected_senders().len(),
        1,
        "paused: heartbeat still flows"
    );
}
