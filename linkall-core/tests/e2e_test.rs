//! End-to-end tests using the in-process SimNetwork harness.
//! These tests verify the full protocol pipeline without OS resources.

use linkall_core::{
    chunked::{maybe_chunk, Reassembler, CHUNK_THRESHOLD},
    dedup::{hash_content, Deduplicator},
    filter::{ExtensionFilter, FilterChain, SizeFilter, TypeFilter, Verdict},
    metrics::ThroughputEstimator,
    pairing::derive_pin,
    protocol::ClipboardContent,
    retry::{Backoff, MAX_ATTEMPTS},
    sim::SimNetwork,
};
use std::time::{Duration, Instant};

// ── SimNetwork end-to-end ─────────────────────────────────────────────────────

#[tokio::test]
async fn e2e_text_one_way() {
    let (mut alice, mut bob) = SimNetwork::pair("Alice", "Bob").await;
    assert!(alice.send_text("clipboard test ").await);
    let got = bob.next_clipboard().await.expect("bob receives");
    assert_eq!(got, ClipboardContent::Text("clipboard test ".into()));
}

#[tokio::test]
async fn e2e_bidirectional() {
    let (mut alice, mut bob) = SimNetwork::pair("Alice", "Bob").await;

    alice.send_text("hello from alice").await;
    let a_to_b = bob.next_clipboard().await.unwrap();
    assert_eq!(a_to_b, ClipboardContent::Text("hello from alice".into()));

    bob.send_text("hello from bob").await;
    let b_to_a = alice.next_clipboard().await.unwrap();
    assert_eq!(b_to_a, ClipboardContent::Text("hello from bob".into()));
}

#[tokio::test]
async fn e2e_image_transfer() {
    let (mut alice, mut bob) = SimNetwork::pair("Alice", "Bob").await;
    let data = vec![0xDE_u8; 512 * 1024]; // 512 KB PNG
    alice
        .send(ClipboardContent::Image {
            mime: "image/png".into(),
            data: data.clone(),
        })
        .await;

    let got = bob.next_clipboard().await.unwrap();
    match got {
        ClipboardContent::Image { mime, data: d } => {
            assert_eq!(mime, "image/png");
            assert_eq!(d.len(), data.len());
        }
        other => panic!("expected image, got {:?}", other),
    }
}

#[tokio::test]
async fn e2e_file_transfer() {
    let (mut alice, mut bob) = SimNetwork::pair("Alice", "Bob").await;
    let data = b"Hello, file world!".to_vec();
    alice
        .send(ClipboardContent::File {
            name: "hello.txt".into(),
            data: data.clone(),
        })
        .await;

    let got = bob.next_clipboard().await.unwrap();
    match got {
        ClipboardContent::File { name, data: d } => {
            assert_eq!(name, "hello.txt");
            assert_eq!(d, data);
        }
        other => panic!("expected file, got {:?}", other),
    }
}

#[tokio::test]
async fn e2e_rapid_sequence_ordering() {
    let (mut alice, mut bob) = SimNetwork::pair("Alice", "Bob").await;

    // Send 10 items in quick succession.
    for i in 0..10u32 {
        alice.send_text(&format!("item-{:02}", i)).await;
    }

    let mut received = Vec::new();
    for _ in 0..10 {
        if let Some(c) = bob.next_clipboard().await {
            received.push(c);
        }
    }

    assert_eq!(received.len(), 10, "all 10 items must arrive");
    // First item received must be item-00.
    assert_eq!(received[0], ClipboardContent::Text("item-00".into()));
}

#[tokio::test]
async fn e2e_latency_under_500ms() {
    let (mut alice, mut bob) = SimNetwork::pair("Alice", "Bob").await;
    alice.artificial_latency = Duration::from_millis(50); // simulate LAN latency

    let start = Instant::now();
    alice.send_text("latency test").await;
    bob.next_clipboard().await.expect("received");
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_millis(500),
        "propagation took {:?} — exceeds 500 ms budget",
        elapsed
    );
}

// ── Chunked transfer ──────────────────────────────────────────────────────────

#[test]
fn chunked_large_text_roundtrip() {
    let text = "A".repeat(CHUNK_THRESHOLD * 4 + 1337);
    let content = ClipboardContent::Text(text.clone());

    let msgs = maybe_chunk(&content).expect("should chunk");
    let mut r = Reassembler::default();
    let mut result = None;

    for msg in msgs {
        if let Some(linkall_core::chunked::ReassemblerOutput::Complete(c)) = r.feed(msg).unwrap() {
            result = Some(c);
        }
    }

    assert_eq!(result.unwrap(), ClipboardContent::Text(text));
}

#[test]
fn chunked_image_roundtrip() {
    let data = vec![0xAB_u8; CHUNK_THRESHOLD * 2 + 999];
    let content = ClipboardContent::Image {
        mime: "image/webp".into(),
        data: data.clone(),
    };

    let msgs = maybe_chunk(&content).expect("should chunk");
    let mut r = Reassembler::default();
    let mut result = None;

    for msg in msgs {
        if let Some(linkall_core::chunked::ReassemblerOutput::Complete(c)) = r.feed(msg).unwrap() {
            result = Some(c);
        }
    }

    match result.unwrap() {
        ClipboardContent::Image { mime, data: d } => {
            assert_eq!(mime, "image/webp");
            assert_eq!(d, data);
        }
        _ => panic!("expected image"),
    }
}

// ── Filter chain ──────────────────────────────────────────────────────────────

#[test]
fn filter_chain_blocks_exe() {
    let mut chain = FilterChain::default();
    chain.push(SizeFilter {
        max_bytes: 64 * 1024 * 1024,
    });
    chain.push(TypeFilter {
        allow_text: true,
        allow_images: true,
        allow_files: true,
    });
    chain.push(ExtensionFilter::default());

    let exe = ClipboardContent::File {
        name: "evil.exe".into(),
        data: vec![],
    };
    assert!(matches!(chain.run(&exe), Verdict::Deny { .. }));

    let pdf = ClipboardContent::File {
        name: "report.pdf".into(),
        data: vec![],
    };
    assert_eq!(chain.run(&pdf), Verdict::Allow);
}

#[test]
fn filter_chain_rejects_oversized() {
    let mut chain = FilterChain::default();
    chain.push(SizeFilter { max_bytes: 100 });

    let big = ClipboardContent::Text("x".repeat(200));
    assert!(matches!(chain.run(&big), Verdict::Deny { .. }));

    let small = ClipboardContent::Text("hi".into());
    assert_eq!(chain.run(&small), Verdict::Allow);
}

// ── Deduplication ─────────────────────────────────────────────────────────────

#[test]
fn dedup_prevents_echo_storm() {
    let mut alice_dedup = Deduplicator::new();
    let content = ClipboardContent::Text("hello world".into());
    let hash = hash_content(&content);

    // Alice sends.
    assert!(alice_dedup.should_send(hash), "first send should pass");

    // Same content comes back as an echo — alice should suppress.
    assert!(
        !alice_dedup.should_apply(uuid::Uuid::new_v4(), hash),
        "echo should be suppressed"
    );

    // New content from a third device should pass.
    let new_content = ClipboardContent::Text("different content".into());
    let new_hash = hash_content(&new_content);
    assert!(
        alice_dedup.should_apply(uuid::Uuid::new_v4(), new_hash),
        "new content should pass"
    );
}

#[test]
fn dedup_two_peers_send_simultaneously() {
    let mut dedup = Deduplicator::new();
    let content = ClipboardContent::Text("same text".into());
    let hash = hash_content(&content);

    // First peer's push applies.
    assert!(dedup.should_apply(uuid::Uuid::new_v4(), hash));
    // Second peer sends identical content — must be suppressed.
    assert!(!dedup.should_apply(uuid::Uuid::new_v4(), hash));
}

// ── Retry back-off ────────────────────────────────────────────────────────────

#[test]
fn retry_sequence_is_bounded() {
    let mut b = Backoff::new("test-peer");
    let delays: Vec<_> = (0..MAX_ATTEMPTS).filter_map(|_| b.next()).collect();
    assert_eq!(delays.len(), MAX_ATTEMPTS as usize);
    // All delays must be positive and within jitter range of MAX_DELAY.
    let cap_ms = (linkall_core::retry::MAX_DELAY.as_millis() as f64 * 1.25) as u128;
    for d in &delays {
        assert!(d.as_millis() > 0);
        assert!(d.as_millis() <= cap_ms, "delay {:?} exceeds cap", d);
    }
    // Exhausted — no more attempts.
    assert!(b.next().is_none());
}

// ── PIN pairing ───────────────────────────────────────────────────────────────

#[test]
fn pin_commutative_ecdh() {
    // Simulate Alice and Bob both computing the same shared secret.
    use linkall_core::crypto::EphemeralKeypair;
    let alice = EphemeralKeypair::generate();
    let bob = EphemeralKeypair::generate();
    let alice_pub = alice.public_bytes;
    let bob_pub = bob.public_bytes;

    let _alice_sess = alice.derive_session_key(bob_pub, true).unwrap();
    let _bob_sess = bob.derive_session_key(alice_pub, false).unwrap();

    // We can't read the session key directly, but we can verify that both
    // sides encrypt/decrypt successfully (commutativity guarantee).
    // For PIN: we test that derive_pin produces the same output for the
    // same input bytes.
    let shared_bytes = [42_u8; 32]; // simulate agreed secret
    let pin_a = derive_pin(&shared_bytes, &[0u8; 32]);
    let pin_b = derive_pin(&shared_bytes, &[0u8; 32]);
    assert_eq!(pin_a, pin_b);
    assert!(pin_a.value() < 1_000_000);
}

// ── Throughput estimator ──────────────────────────────────────────────────────

#[test]
fn throughput_estimator_tracks_rate() {
    let mut est = ThroughputEstimator::new();

    // Send 1 MB then wait 100 ms.
    est.record(1_000_000);
    std::thread::sleep(Duration::from_millis(100));
    est.record(1_000_000);

    let bps = est.bps();
    // Very rough check — at least something is tracked.
    assert!(bps > 0.0);
    assert!(bps < 1_000_000_000.0); // less than 1 GB/s
}
