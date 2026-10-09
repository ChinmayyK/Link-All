//! Loopback file-transfer throughput benchmark.

use linkall_core::engine::{Engine, EngineConfig, EngineEvent};
use linkall_core::identity::IdentityStore;
use linkall_core::trust::TrustStore;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::mpsc;
use uuid::Uuid;

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
        file_save_dir: Some(tmp.path().join("recv_b")),
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

/// Loopback throughput benchmark for file transfer. Ignored by default;
/// run with `cargo test --release --test transfer_bench -- --ignored --nocapture`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn loopback_file_transfer_throughput() {
    let (engine_a, _dev_a, _rx_a, _engine_b, dev_b, mut rx_b, tmp) = setup_pair(true).await;
    let size: usize = std::env::var("BENCH_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(512)
        * 1024
        * 1024;
    let path = tmp.path().join("bench.bin");
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&path).unwrap();
        let mut state: u64 = 0x9E3779B97F4A7C15;
        let mut buf = vec![0u8; 1 << 20];
        for _ in 0..size / buf.len() {
            for b in buf.chunks_mut(8) {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                b.copy_from_slice(&state.to_le_bytes()[..b.len()]);
            }
            f.write_all(&buf).unwrap();
        }
    }
    let t0 = std::time::Instant::now();
    engine_a
        .send_file_path(
            path,
            "bench.bin".into(),
            "application/octet-stream".into(),
            Some(dev_b),
            None,
            false,
            1,
        )
        .await
        .unwrap();
    let mut first_progress = None;
    let mut last_bytes = 0u64;
    let mut last_change = std::time::Instant::now();
    loop {
        match tokio::time::timeout(Duration::from_secs(60), rx_b.recv()).await {
            Ok(Some(EngineEvent::FileTransferProgress { bytes_received, .. })) => {
                if first_progress.is_none() {
                    first_progress = Some(t0.elapsed());
                }
                if bytes_received != last_bytes {
                    let gap = last_change.elapsed();
                    if gap > Duration::from_millis(1500) {
                        println!("stall {:?} at {} MB", gap, last_bytes >> 20);
                    }
                    last_bytes = bytes_received;
                    last_change = std::time::Instant::now();
                }
            }
            Ok(Some(EngineEvent::FileTransferComplete { .. })) => break,
            Ok(Some(EngineEvent::FileTransferFailed { reason, .. })) => panic!("failed: {reason}"),
            Ok(Some(_)) => {}
            other => panic!("no completion: {:?}", other.map(|o| o.is_some())),
        }
    }
    let el = t0.elapsed();
    println!(
        "first progress after {:?}; {} MB in {:?} = {:.1} MB/s",
        first_progress,
        size >> 20,
        el,
        (size as f64 / 1048576.0) / el.as_secs_f64()
    );
}

/// Sending from an already-open handle (Android's content-URI path) delivers
/// the same bytes as the original file, across several chunks.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn send_open_file_delivers_identical_bytes() {
    let (engine_a, _dev_a, _rx_a, _engine_b, dev_b, mut rx_b, tmp) = setup_pair(true).await;
    let data: Vec<u8> = (0..(9 * 1024 * 1024 + 123))
        .map(|i| (i * 31 % 251) as u8)
        .collect();
    let src = tmp.path().join("source.bin");
    std::fs::write(&src, &data).unwrap();

    let file = std::fs::File::open(&src).unwrap();
    engine_a
        .send_open_file(
            file,
            "clip.bin".into(),
            "application/octet-stream".into(),
            Some(dev_b),
        )
        .await
        .unwrap();

    let dest = loop {
        match tokio::time::timeout(Duration::from_secs(20), rx_b.recv()).await {
            Ok(Some(EngineEvent::FileTransferComplete { dest_path, .. })) => break dest_path,
            Ok(Some(EngineEvent::FileTransferFailed { reason, .. })) => panic!("failed: {reason}"),
            Ok(Some(_)) => {}
            other => panic!("no completion: {:?}", other.map(|o| o.is_some())),
        }
    };
    assert_eq!(dest.file_name().unwrap(), "clip.bin");
    assert_eq!(std::fs::read(dest).unwrap(), data);
}

/// A connection that drops mid-transfer resumes on reconnect and still
/// delivers the exact bytes. Before the fix the receiver discarded every
/// chunk after the reconnect and the transfer sat at the same percent.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn transfer_resumes_after_reconnect() {
    if std::env::var("BENCH_LOG").is_ok() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("linkall_core=debug")
            .try_init();
    }
    let (engine_a, _dev_a, _rx_a, engine_b, dev_b, mut rx_b, tmp) = setup_pair(true).await;
    let data: Vec<u8> = (0..(96 * 1024 * 1024u32))
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    let src = tmp.path().join("resume.bin");
    std::fs::write(&src, &data).unwrap();

    engine_a
        .send_file_path(
            src,
            "resume.bin".into(),
            "application/octet-stream".into(),
            Some(dev_b),
            None,
            false,
            1,
        )
        .await
        .unwrap();

    let port_b = engine_b.bound_port().await;
    let mut dropped = false;
    let dest = loop {
        match tokio::time::timeout(Duration::from_secs(30), rx_b.recv()).await {
            Ok(Some(EngineEvent::FileTransferProgress { bytes_received, .. }))
                if !dropped && bytes_received > 0 =>
            {
                dropped = true;
                engine_a.disconnect_peer(dev_b).await.unwrap();
                tokio::time::sleep(Duration::from_millis(300)).await;
                engine_a
                    .connect_to_peer("127.0.0.1".into(), port_b)
                    .await
                    .unwrap();
            }
            Ok(Some(EngineEvent::FileTransferComplete { dest_path, .. })) => break dest_path,
            Ok(Some(EngineEvent::FileTransferFailed { reason, .. })) => panic!("failed: {reason}"),
            Ok(Some(_)) => {}
            other => panic!("no completion: {:?}", other.map(|o| o.is_some())),
        }
    };
    assert!(
        dropped,
        "the transfer finished before the disconnect was injected"
    );
    assert_eq!(std::fs::read(dest).unwrap(), data);
}
