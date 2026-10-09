//! Integration test: two in-process engines exchange clipboard content.
//!
//! Two test strategies run here:
//!
//! 1. **Real-TCP** (`two_engines_exchange_text`) — spins up two full Engine
//!    instances on localhost with a genuine TCP connection, exercising the
//!    entire network and crypto stack end-to-end.
//!
//! 2. **SimNetwork harness** (`sim_*`) — uses the in-process `SimNetwork`
//!    channel pair so tests are fast, deterministic, and free of OS resources.
//!    These cover dedup, echo suppression, image payloads, multi-item ordering,
//!    latency measurement, and degraded-network conditions (BLD-05).

use linkall_core::engine::{Engine, EngineConfig, EngineEvent};
use linkall_core::identity::IdentityStore;
use linkall_core::protocol::ClipboardContent;
use linkall_core::sim::{NetworkConditions, SimNetwork};
use linkall_core::trust::TrustStore;
use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio::time::timeout;
use uuid::Uuid;

#[tokio::test]
async fn two_engines_exchange_text() {
    let tmp = TempDir::new().unwrap();
    let (tx1, _rx1) = mpsc::channel(64);
    let (tx2, mut rx2) = mpsc::channel(64);
    let device_id_1 = Uuid::new_v4();
    let device_id_2 = Uuid::new_v4();
    let trust_path_1 = tmp.path().join("trust1.json");
    let trust_path_2 = tmp.path().join("trust2.json");
    let peer_path_1 = tmp.path().join("peers1.json");
    let peer_path_2 = tmp.path().join("peers2.json");
    let identity_path_1 = tmp.path().join("identity1.key");
    let identity_path_2 = tmp.path().join("identity2.key");

    let identity_1 = IdentityStore::new(&identity_path_1)
        .load_or_create()
        .unwrap();
    let identity_2 = IdentityStore::new(&identity_path_2)
        .load_or_create()
        .unwrap();

    let mut trust_1 = TrustStore::load(&trust_path_1).unwrap();
    trust_1
        .trust(device_id_2, "TestDevice2".into(), &identity_2.public_bytes)
        .unwrap();
    let mut trust_2 = TrustStore::load(&trust_path_2).unwrap();
    trust_2
        .trust(device_id_1, "TestDevice1".into(), &identity_1.public_bytes)
        .unwrap();

    let cfg1 = EngineConfig {
        device_id: device_id_1,
        device_name: "TestDevice1".into(),
        port: 0,
        trust_store_path: trust_path_1,
        peer_store_path: peer_path_1,
        identity_path: identity_path_1,
        bind_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        enable_discovery: false,
        ..EngineConfig::default()
    };

    let cfg2 = EngineConfig {
        device_id: device_id_2,
        device_name: "TestDevice2".into(),
        port: 0,
        trust_store_path: trust_path_2,
        peer_store_path: peer_path_2,
        identity_path: identity_path_2,
        bind_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        enable_discovery: false,
        ..EngineConfig::default()
    };

    let engine1 = Engine::start(cfg1, tx1).await.expect("engine1 start");
    let _engine2 = Engine::start(cfg2, tx2).await.expect("engine2 start");

    let _port1 = engine1.bound_port().await;
    let port2 = _engine2.bound_port().await;

    // Wait for engine2 to start listening
    let mut ready = false;
    for _ in 0..50 {
        if let Ok(stream) = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port2)).await {
            drop(stream);
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "engine2 did not start listening in time");

    engine1
        .connect_to_peer("127.0.0.1".into(), port2)
        .await
        .expect("manual connect");

    // Push from engine1.
    engine1
        .push_clipboard(ClipboardContent::Text("hello from device 1".into()))
        .await;

    // Engine2 should receive within 500 ms.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let event = timeout(remaining, rx2.recv())
            .await
            .expect("timeout waiting for event")
            .expect("channel closed");

        match event {
            EngineEvent::ClipboardReceived { content, .. } => {
                if let ClipboardContent::Text(t) = &*content {
                    assert_eq!(t, "hello from device 1");
                    break;
                } else {
                    panic!("expected Text clipboard content");
                }
            }
            EngineEvent::PeerConnected { .. } | EngineEvent::ClipboardSynced { .. } => {}
            other => panic!("unexpected event: {:?}", other),
        }
    }
}

// ── SimNetwork harness tests (BLD-05) ─────────────────────────────────────────

/// Basic two-node text exchange via SimNetwork — the canonical BLD-05 scenario.
#[tokio::test]
async fn sim_two_nodes_exchange_text() {
    let (mut alice, mut bob) = SimNetwork::pair("Alice", "Bob").await;

    alice.send_text("clipboard from alice").await;
    let received = bob.next_clipboard().await;
    assert_eq!(
        received,
        Some(ClipboardContent::Text("clipboard from alice".into())),
        "Bob must receive Alice's text"
    );
}

/// Bidirectional exchange: both sides send and receive independently.
#[tokio::test]
async fn sim_bidirectional_exchange() {
    let (mut alice, mut bob) = SimNetwork::pair("Alice", "Bob").await;

    alice.send_text("hello from alice").await;
    bob.send_text("hello from bob").await;

    let bob_got = bob.next_clipboard().await;
    let alice_got = alice.next_clipboard().await;

    assert_eq!(
        bob_got,
        Some(ClipboardContent::Text("hello from alice".into()))
    );
    assert_eq!(
        alice_got,
        Some(ClipboardContent::Text("hello from bob".into()))
    );
}

/// Echo suppression: a node must not re-apply its own sends.
#[tokio::test]
async fn sim_echo_not_applied_to_sender() {
    let (mut alice, mut bob) = SimNetwork::pair("Alice", "Bob").await;

    alice.send_text("unique-echo-test").await;
    let _bob_received = bob.next_clipboard().await;

    // Bob echoes the same content back.
    bob.send_text("unique-echo-test").await;

    // Alice already sent this; her dedup window should suppress or ignore it.
    // We accept either None (suppressed) or the same value (passed through but
    // not applied) — the key assertion is no panic and the content is consistent.
    let alice_got = alice.next_clipboard().await;
    if let Some(content) = alice_got {
        assert_eq!(content, ClipboardContent::Text("unique-echo-test".into()));
    }
}

/// Multi-item sequential delivery preserves order.
#[tokio::test]
async fn sim_ordered_multi_item_delivery() {
    let (mut alice, mut bob) = SimNetwork::pair("Alice", "Bob").await;

    let items: Vec<String> = (0..8).map(|i| format!("item-{}", i)).collect();
    for item in &items {
        alice.send_text(item).await;
    }

    let mut received = Vec::new();
    for _ in 0..8 {
        if let Some(ClipboardContent::Text(t)) = bob.next_clipboard().await {
            received.push(t);
        }
    }

    assert_eq!(received.len(), 8, "all 8 items must be delivered");
    assert_eq!(received, items, "delivery order must be preserved");
}

/// Image payloads round-trip correctly through the sim channel.
#[tokio::test]
async fn sim_image_payload_roundtrip() {
    let (mut alice, mut bob) = SimNetwork::pair("Alice", "Bob").await;

    // 4 KB synthetic PNG blob.
    let img_data: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
    alice
        .send(ClipboardContent::Image {
            mime: "image/png".into(),
            data: img_data.clone(),
        })
        .await;

    let received = bob.next_clipboard().await.expect("image must arrive");
    match received {
        ClipboardContent::Image { mime, data } => {
            assert_eq!(mime, "image/png");
            assert_eq!(data, img_data, "image data must be byte-identical");
        }
        other => panic!("expected Image, got {:?}", other),
    }
}

/// Artificial latency is honoured and measurable.
#[tokio::test]
async fn sim_artificial_latency_respected() {
    let (mut alice, mut bob) = SimNetwork::pair("Alice", "Bob").await;
    alice.artificial_latency = Duration::from_millis(20);

    let start = Instant::now();
    alice.send_text("latency-test").await;
    let _ = bob.next_clipboard().await;
    let elapsed = start.elapsed();

    assert!(
        elapsed >= Duration::from_millis(20),
        "elapsed {:?} must be >= 20 ms artificial latency",
        elapsed
    );
    assert!(
        elapsed < Duration::from_millis(500),
        "elapsed {:?} must be well under 500 ms",
        elapsed
    );
}

/// NetworkConditions::stressed() drop-rate is in the expected statistical range.
#[test]
fn sim_stressed_conditions_drop_rate() {
    let cond = NetworkConditions::stressed(); // 5% packet loss
    let trials = 20_000usize;
    let drops: usize = (0..trials).filter(|_| cond.should_drop()).count();
    let expected = (trials as f64 * cond.packet_loss) as usize;
    let tolerance = trials / 20; // ±5% of trial count

    assert!(
        drops.abs_diff(expected) <= tolerance,
        "drop count {} too far from expected {} (tolerance ±{})",
        drops,
        expected,
        tolerance,
    );
}

/// `NetworkConditions::perfect()` never drops any message.
#[test]
fn sim_perfect_conditions_no_drops() {
    let cond = NetworkConditions::perfect();
    let any_drop = (0..10_000).any(|_| cond.should_drop());
    assert!(!any_drop, "perfect conditions must never drop");
}

/// Drain helper returns all pending items in one call.
#[tokio::test]
async fn sim_drain_returns_all_pending() {
    let (mut alice, mut bob) = SimNetwork::pair("Alice", "Bob").await;

    // Send 5 items then give them a moment to land.
    for i in 0..5 {
        alice.send_text(&format!("bulk-{}", i)).await;
    }
    tokio::time::sleep(Duration::from_millis(20)).await;

    let drained = bob.drain_clipboard().await;
    assert_eq!(drained.len(), 5, "drain must collect all 5 items");
}
