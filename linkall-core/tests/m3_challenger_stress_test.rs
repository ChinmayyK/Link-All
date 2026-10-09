//! Challenger M3 Stress Test Suite
//! Empirical verification of peer disconnect fast-path and waiter map resilience.

use linkall_core::engine::{Engine, EngineConfig, EngineEvent};
use linkall_core::identity::IdentityStore;
use linkall_core::trust::TrustStore;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::sync::mpsc;
use uuid::Uuid;

async fn setup_test_nodes() -> (
    Engine,
    Uuid,
    Engine,
    Uuid,
    mpsc::Receiver<EngineEvent>,
    TempDir,
) {
    let tmp = TempDir::new().unwrap();
    let (tx_a, _rx_a) = mpsc::channel(128);
    let (tx_b, rx_b) = mpsc::channel(128);

    let dev_a = Uuid::new_v4();
    let dev_b = Uuid::new_v4();

    let id_path_a = tmp.path().join("id_a.key");
    let id_path_b = tmp.path().join("id_b.key");
    let trust_path_a = tmp.path().join("trust_a.json");
    let trust_path_b = tmp.path().join("trust_b.json");

    let id_a = IdentityStore::new(&id_path_a).load_or_create().unwrap();
    let id_b = IdentityStore::new(&id_path_b).load_or_create().unwrap();

    let mut trust_a = TrustStore::load(&trust_path_a).unwrap();
    trust_a
        .trust(dev_b, "NodeB".into(), &id_b.public_bytes)
        .unwrap();

    let mut trust_b = TrustStore::load(&trust_path_b).unwrap();
    trust_b
        .trust(dev_a, "NodeA".into(), &id_a.public_bytes)
        .unwrap();

    let cfg_a = EngineConfig {
        device_id: dev_a,
        device_name: "NodeA".into(),
        port: 0,
        trust_store_path: trust_path_a,
        peer_store_path: tmp.path().join("peer_a.json"),
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
        peer_store_path: tmp.path().join("peer_b.json"),
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

    tokio::time::sleep(Duration::from_millis(150)).await;

    (engine_a, dev_a, engine_b, dev_b, rx_b, tmp)
}

/// Test Bug 1: explicit engine.disconnect_peer(dev_b) fails to drain remote_file_waiters.
/// The waiter hangs for the full 10s timeout instead of triggering immediate fast-path error ("Peer disconnected").
#[tokio::test]
async fn test_reproduce_disconnect_peer_waiter_leak() {
    let (engine_a, _dev_a, _engine_b, dev_b, mut rx_b, _tmp) = setup_test_nodes().await;

    // Non-responding node B receiver
    tokio::spawn(async move { while let Some(_event) = rx_b.recv().await {} });

    let engine_a = Arc::new(engine_a);
    let eng = engine_a.clone();

    // Start a 10s query
    let query_handle = tokio::spawn(async move {
        eng.query_remote_files_sync(dev_b, false, None, None, None, 0, 50, 10)
            .await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let start_disconnect = Instant::now();
    // Call explicit disconnect_peer
    let disconnected = engine_a.disconnect_peer(dev_b).await.unwrap();
    assert!(
        disconnected,
        "disconnect_peer should return true for active peer"
    );

    // Wait for the query to resolve
    let result = query_handle.await.unwrap();
    let elapsed = start_disconnect.elapsed();

    println!("Query result after disconnect_peer: {:?}", result);
    println!("Elapsed time after disconnect_peer: {:?}", elapsed);

    // Expect fast-path disconnect error (< 500ms), NOT 10s timeout error
    assert!(
        elapsed < Duration::from_millis(1000),
        "Query took {:?} to fail after explicit disconnect_peer. Fast-path disconnect failed!",
        elapsed
    );

    let err_str = result.unwrap_err().to_string();
    assert!(
        err_str.contains("Peer disconnected"),
        "Expected 'Peer disconnected' fast-path error, got: {}",
        err_str
    );
}

/// Test 2: Granularity of dynamic timeouts
#[tokio::test]
async fn test_dynamic_timeouts_granularity() {
    let (engine_a, _dev_a, _engine_b, dev_b, mut rx_b, _tmp) = setup_test_nodes().await;

    tokio::spawn(async move { while let Some(_event) = rx_b.recv().await {} });

    let t1_start = Instant::now();
    let err1 = engine_a
        .query_remote_files_sync(dev_b, false, None, None, None, 0, 50, 1)
        .await
        .unwrap_err();
    let t1_elapsed = t1_start.elapsed();

    assert!(err1.to_string().contains("timed out after 1s"));
    assert!(
        t1_elapsed >= Duration::from_millis(900) && t1_elapsed <= Duration::from_millis(2000),
        "1s timeout took {:?}",
        t1_elapsed
    );
}

/// Test 3: forget_device drains both remote_file_waiters and remote_thumb_waiters immediately
#[tokio::test]
async fn test_forget_device_drains_remote_file_and_thumb_waiters() {
    let (engine_a, _dev_a, _engine_b, dev_b, mut rx_b, _tmp) = setup_test_nodes().await;

    tokio::spawn(async move { while let Some(_event) = rx_b.recv().await {} });

    let engine_a = Arc::new(engine_a);
    let mut file_tasks = Vec::new();
    let mut thumb_tasks = Vec::new();

    for _ in 0..5 {
        let eng = engine_a.clone();
        file_tasks.push(tokio::spawn(async move {
            eng.query_remote_files_sync(dev_b, false, None, None, None, 0, 50, 10)
                .await
        }));

        let eng_t = engine_a.clone();
        thumb_tasks.push(tokio::spawn(async move {
            eng_t
                .request_remote_thumbnail_sync(dev_b, 42, 128, 10)
                .await
        }));
    }

    tokio::time::sleep(Duration::from_millis(50)).await;

    let start = Instant::now();
    let forgot = engine_a.forget_device(dev_b).await.unwrap();
    assert!(forgot, "forget_device should return true for known peer");

    for task in file_tasks {
        let res = task.await.unwrap();
        assert!(
            res.is_err(),
            "Pending file query should fail on forget_device"
        );
    }

    for task in thumb_tasks {
        let res = task.await.unwrap();
        let err_str = match res {
            Ok(ref thumb_res) => thumb_res.error.clone().unwrap_or_default(),
            Err(e) => e.to_string(),
        };
        assert!(
            err_str.contains("Peer disconnected"),
            "Pending thumbnail request should report 'Peer disconnected' on forget_device, got: {}",
            err_str
        );
    }

    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(500),
        "forget_device waiter drain took {:?}, expected < 500ms",
        elapsed
    );
}

/// Test 4: Heavy concurrent waiters (50 concurrent queries) on explicit disconnect_peer
#[tokio::test]
async fn test_concurrent_waiters_disconnect_drain() {
    let (engine_a, _dev_a, _engine_b, dev_b, mut rx_b, _tmp) = setup_test_nodes().await;

    tokio::spawn(async move { while let Some(_event) = rx_b.recv().await {} });

    let engine_a = Arc::new(engine_a);
    let mut file_tasks = Vec::new();
    let mut thumb_tasks = Vec::new();

    for i in 0..25 {
        let eng1 = engine_a.clone();
        file_tasks.push(tokio::spawn(async move {
            eng1.query_remote_files_sync(dev_b, false, None, None, None, 0, 50, 10)
                .await
        }));

        let eng2 = engine_a.clone();
        thumb_tasks.push(tokio::spawn(async move {
            eng2.request_remote_thumbnail_sync(dev_b, i as u64, 128, 10)
                .await
        }));
    }

    tokio::time::sleep(Duration::from_millis(50)).await;

    let start = Instant::now();
    let disconnected = engine_a.disconnect_peer(dev_b).await.unwrap();
    assert!(disconnected);

    for task in file_tasks {
        let res = task.await.unwrap();
        assert!(
            res.is_err(),
            "All concurrent file waiters must fail immediately on disconnect"
        );
    }

    for task in thumb_tasks {
        let res = task.await.unwrap();
        let err_str = match res {
            Ok(ref thumb_res) => thumb_res.error.clone().unwrap_or_default(),
            Err(e) => e.to_string(),
        };
        assert!(
            err_str.contains("Peer disconnected"),
            "All concurrent thumb waiters must report 'Peer disconnected' on disconnect, got: {}",
            err_str
        );
    }

    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(500),
        "Concurrent disconnect drain took {:?}, expected < 500ms",
        elapsed
    );
}

/// Test 5: Session shutdown race condition on explicit peer disconnect during active queries
#[tokio::test]
async fn test_session_shutdown_race_drains_waiters() {
    let (engine_a, _dev_a, engine_b, dev_b, _rx_b, _tmp) = setup_test_nodes().await;

    let engine_a = Arc::new(engine_a);
    let mut file_tasks = Vec::new();
    let mut thumb_tasks = Vec::new();

    for i in 0..10 {
        let eng = engine_a.clone();
        file_tasks.push(tokio::spawn(async move {
            eng.query_remote_files_sync(dev_b, false, None, None, None, 0, 50, 10)
                .await
        }));

        let eng_t = engine_a.clone();
        thumb_tasks.push(tokio::spawn(async move {
            eng_t
                .request_remote_thumbnail_sync(dev_b, i as u64, 128, 10)
                .await
        }));
    }

    tokio::time::sleep(Duration::from_millis(30)).await;

    let start = Instant::now();
    // Explicitly disconnect engine_b on engine_a to trigger session shutdown
    engine_a.disconnect_peer(dev_b).await.unwrap();

    for task in file_tasks {
        let res = task.await.unwrap();
        assert!(res.is_err(), "Query must fail on peer session shutdown");
    }

    for task in thumb_tasks {
        let res = task.await.unwrap();
        let err_str = match res {
            Ok(ref thumb_res) => thumb_res.error.clone().unwrap_or_default(),
            Err(e) => e.to_string(),
        };
        assert!(
            err_str.contains("Peer disconnected"),
            "Thumbnail query must report 'Peer disconnected' on peer session shutdown, got: {}",
            err_str
        );
    }

    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(1000),
        "Session shutdown drain took {:?}, expected < 1000ms",
        elapsed
    );

    drop(engine_b);
}
