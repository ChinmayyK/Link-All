//! End-to-end tests for cross-device link handoff (`OpenUrlOnDevice`).
//!
//! Mirrors the dual in-process `Engine` + real loopback TCP harness used by
//! `remote_files_e2e_test.rs` / `m3_challenger_stress_test.rs`.

use linkall_core::engine::{Engine, EngineConfig, EngineEvent};
use linkall_core::identity::IdentityStore;
use linkall_core::trust::TrustStore;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Setup two in-process `Engine` instances with mutual trust already
/// established, connected over real loopback TCP. Keeps both sides' event
/// receivers (unlike `remote_files_e2e_test.rs`'s helper, which discards
/// node A's) since these tests need to observe the delivery ack on the
/// sending side too.
async fn setup_trusted_pair() -> (
    Engine,
    Uuid,
    mpsc::Receiver<EngineEvent>,
    Engine,
    Uuid,
    mpsc::Receiver<EngineEvent>,
    TempDir,
) {
    setup_pair(true).await
}

/// Same as `setup_trusted_pair`, but the two devices are connected without
/// ever trusting each other (simulates a stranger/pending-pairing peer).
async fn setup_untrusted_pair() -> (
    Engine,
    Uuid,
    mpsc::Receiver<EngineEvent>,
    Engine,
    Uuid,
    mpsc::Receiver<EngineEvent>,
    TempDir,
) {
    setup_pair(false).await
}

async fn setup_pair(
    trusted: bool,
) -> (
    Engine,
    Uuid,
    mpsc::Receiver<EngineEvent>,
    Engine,
    Uuid,
    mpsc::Receiver<EngineEvent>,
    TempDir,
) {
    let tmp = TempDir::new().unwrap();
    let (tx_a, rx_a) = mpsc::channel(64);
    let (tx_b, rx_b) = mpsc::channel(64);

    let dev_a = Uuid::new_v4();
    let dev_b = Uuid::new_v4();

    let id_path_a = tmp.path().join("id_a.key");
    let id_path_b = tmp.path().join("id_b.key");
    let trust_path_a = tmp.path().join("trust_a.json");
    let trust_path_b = tmp.path().join("trust_b.json");
    let peer_path_a = tmp.path().join("peer_a.json");
    let peer_path_b = tmp.path().join("peer_b.json");

    let id_a = IdentityStore::new(&id_path_a).load_or_create().unwrap();
    let id_b = IdentityStore::new(&id_path_b).load_or_create().unwrap();

    if trusted {
        let mut trust_a = TrustStore::load(&trust_path_a).unwrap();
        trust_a
            .trust(dev_b, "NodeB".into(), &id_b.public_bytes)
            .unwrap();

        let mut trust_b = TrustStore::load(&trust_path_b).unwrap();
        trust_b
            .trust(dev_a, "NodeA".into(), &id_a.public_bytes)
            .unwrap();
    }

    let cfg_a = EngineConfig {
        device_id: dev_a,
        device_name: "NodeA".into(),
        port: 0,
        trust_store_path: trust_path_a,
        peer_store_path: peer_path_a,
        identity_path: id_path_a,
        data_dir: tmp.path().join("data_a"),
        bind_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        enable_discovery: false,
        ..Default::default()
    };

    let cfg_b = EngineConfig {
        device_id: dev_b,
        device_name: "NodeB".into(),
        port: 0,
        trust_store_path: trust_path_b,
        peer_store_path: peer_path_b,
        identity_path: id_path_b,
        data_dir: tmp.path().join("data_b"),
        bind_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        enable_discovery: false,
        ..Default::default()
    };

    let engine_a = Engine::start(cfg_a, tx_a).await.unwrap();
    let engine_b = Engine::start(cfg_b, tx_b).await.unwrap();

    let port_b = engine_b.bound_port().await;

    let mut ready = false;
    for _ in 0..50 {
        if tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port_b))
            .await
            .is_ok()
        {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(ready, "NodeB failed to listen on port");

    engine_a
        .connect_to_peer("127.0.0.1".into(), port_b)
        .await
        .unwrap();

    // Allow connection handshake to settle.
    tokio::time::sleep(Duration::from_millis(150)).await;

    (engine_a, dev_a, rx_a, engine_b, dev_b, rx_b, tmp)
}

/// Wait for a specific event to arrive, ignoring unrelated events (this
/// engine also emits PeerConnected etc. during the handshake window).
async fn recv_matching<F>(
    rx: &mut mpsc::Receiver<EngineEvent>,
    mut matches: F,
) -> Option<EngineEvent>
where
    F: FnMut(&EngineEvent) -> bool,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(ev)) if matches(&ev) => return Some(ev),
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => return None,
        }
    }
}

#[tokio::test]
async fn trusted_peer_receives_open_url_request() {
    let (engine_a, dev_a, _rx_a, _engine_b, _dev_b, mut rx_b, _tmp) = setup_trusted_pair().await;

    engine_a
        .open_url_on_device(_dev_b, "https://example.com/article".to_string())
        .await;

    let ev = recv_matching(&mut rx_b, |e| {
        matches!(e, EngineEvent::OpenUrlOnDeviceRequested { .. })
    })
    .await
    .expect("NodeB should receive OpenUrlOnDeviceRequested");

    match ev {
        EngineEvent::OpenUrlOnDeviceRequested {
            from_device, url, ..
        } => {
            assert_eq!(from_device, dev_a);
            assert_eq!(url, "https://example.com/article");
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn non_http_scheme_is_rejected_with_ack_not_delivered() {
    let (engine_a, _dev_a, mut rx_a, _engine_b, dev_b, mut rx_b, _tmp) = setup_trusted_pair().await;

    engine_a
        .open_url_on_device(dev_b, "file:///etc/passwd".to_string())
        .await;

    // The rejected scheme must never reach the platform layer as a request.
    let bad_request = recv_matching(&mut rx_b, |e| {
        matches!(e, EngineEvent::OpenUrlOnDeviceRequested { .. })
    })
    .await;
    assert!(
        bad_request.is_none(),
        "non-http(s) scheme must not be forwarded as an open request"
    );

    // The sender should instead see a failure ack.
    let ack = recv_matching(&mut rx_a, |e| {
        matches!(e, EngineEvent::OpenUrlOnDeviceAckReceived { .. })
    })
    .await
    .expect("sender should receive a rejection ack");

    match ack {
        EngineEvent::OpenUrlOnDeviceAckReceived { success, error, .. } => {
            assert!(!success);
            assert!(error.is_some());
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn untrusted_peer_request_is_silently_ignored() {
    let (engine_a, _dev_a, _rx_a, _engine_b, dev_b, mut rx_b, _tmp) = setup_untrusted_pair().await;

    engine_a
        .open_url_on_device(dev_b, "https://example.com".to_string())
        .await;

    let ev = recv_matching(&mut rx_b, |e| {
        matches!(
            e,
            EngineEvent::OpenUrlOnDeviceRequested { .. }
                | EngineEvent::OpenUrlOnDeviceAckReceived { .. }
        )
    })
    .await;
    assert!(
        ev.is_none(),
        "an untrusted peer's OpenUrlOnDevice must be ignored entirely, not even acked"
    );
}

#[tokio::test]
async fn ack_reaches_original_requester() {
    let (engine_a, dev_a, mut rx_a, engine_b, dev_b, mut rx_b, _tmp) = setup_trusted_pair().await;

    engine_a
        .open_url_on_device(dev_b, "https://example.com".to_string())
        .await;

    let _ = recv_matching(&mut rx_b, |e| {
        matches!(e, EngineEvent::OpenUrlOnDeviceRequested { .. })
    })
    .await
    .expect("NodeB should receive the request");

    // Simulate the platform having successfully opened the browser.
    engine_b.ack_open_url_on_device(dev_a, true, None).await;

    let ack = recv_matching(&mut rx_a, |e| {
        matches!(e, EngineEvent::OpenUrlOnDeviceAckReceived { .. })
    })
    .await
    .expect("NodeA should receive the success ack");

    match ack {
        EngineEvent::OpenUrlOnDeviceAckReceived { success, error, .. } => {
            assert!(success);
            assert!(error.is_none());
        }
        _ => unreachable!(),
    }
}
