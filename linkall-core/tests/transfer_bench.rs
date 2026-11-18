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

        settings_path: tmp.path().join("settings_a.json"),
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

        settings_path: tmp.path().join("settings_b.json"),
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
/// run with `cargo test --profile perf --test transfer_bench loopback -- --ignored --nocapture`.
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
        // Flushed first, so the source's write-back doesn't compete with the
        // receiver's writes and skew the number.
        f.sync_all().unwrap();
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

/// How fast each stage of a file transfer runs on its own, per 4 MB chunk,
/// to show which one bounds the whole. Ignored by default; run with
/// `cargo test --profile perf --test transfer_bench stage -- --ignored --nocapture`.
#[test]
#[ignore]
fn stage_throughput() {
    use linkall_core::crypto::EphemeralKeypair;
    use sha2::{Digest, Sha256};
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::time::Instant;

    const CHUNK: usize = 4 * 1024 * 1024;
    const ROUNDS: usize = 64;
    let mut state: u64 = 0x9E3779B97F4A7C15;
    let chunk: Vec<u8> = (0..CHUNK)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    let rate = |label: &str, started: Instant| {
        let mb = (ROUNDS * CHUNK) as f64 / 1048576.0;
        println!(
            "{label:<28} {:>8.0} MB/s",
            mb / started.elapsed().as_secs_f64()
        );
    };

    let t = Instant::now();
    let mut hasher = Sha256::new();
    for _ in 0..ROUNDS {
        hasher.update(&chunk);
    }
    std::hint::black_box(hasher.finalize());
    rate("sha-256", t);

    let (a, b) = (EphemeralKeypair::generate(), EphemeralKeypair::generate());
    let (mut tx_key, _, _) = a.derive_session_key(b.public_bytes, true).unwrap();
    let (mut rx_key, _, _) = b.derive_session_key(a.public_bytes, false).unwrap();
    let mut sealed = Vec::with_capacity(ROUNDS);
    let t = Instant::now();
    for _ in 0..ROUNDS {
        let mut buf = Vec::with_capacity(CHUNK + 32);
        buf.extend_from_slice(&chunk);
        let nonce = tx_key.encrypt_in_place(&mut buf).unwrap();
        sealed.push((nonce, buf));
    }
    rate("copy + aes-gcm encrypt", t);

    let t = Instant::now();
    for (nonce, buf) in &mut sealed {
        let nonce: [u8; 12] = nonce.as_slice().try_into().unwrap();
        rx_key.decrypt_body_in_place(&nonce, buf).unwrap();
    }
    rate("aes-gcm decrypt", t);

    let t = Instant::now();
    for _ in 0..ROUNDS {
        std::hint::black_box(lz4_flex::block::compress(&chunk[..4096]));
    }
    rate("lz4 sample (per chunk)", t);

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("stage.bin");
    let mut file = std::fs::File::create(&path).unwrap();
    let t = Instant::now();
    for _ in 0..ROUNDS {
        file.write_all(&chunk).unwrap();
    }
    rate("file write (page cache)", t);
    let t2 = Instant::now();
    file.sync_all().unwrap();
    println!(
        "{:<28} {:>8.0} ms",
        "fsync of it all",
        t2.elapsed().as_millis()
    );

    let mut file = std::fs::File::open(&path).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    let mut buf = vec![0u8; CHUNK];
    let t = Instant::now();
    for _ in 0..ROUNDS {
        file.read_exact(&mut buf).unwrap();
    }
    rate("file read (page cache)", t);
}

/// How long a folder of many small files takes: the per-file cost (announce,
/// accept, create, verify, rename), not the bytes. Ignored by default; run
/// with `cargo test --profile perf --test transfer_bench folder -- --ignored --nocapture`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn folder_of_small_files_throughput() {
    let (engine_a, _dev_a, mut rx_a, _engine_b, dev_b, mut rx_b, tmp) = setup_pair(true).await;
    let files: usize = std::env::var("BENCH_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    let root = tmp.path().join("Photos");
    for i in 0..files {
        let dir = root.join(format!("album {}", i % 20));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("img_{i}.jpg")),
            vec![(i % 251) as u8; 20_000],
        )
        .unwrap();
    }
    tokio::spawn(async move { while rx_a.recv().await.is_some() {} });

    let t0 = std::time::Instant::now();
    engine_a.send_folder(root, Some(dev_b)).await.unwrap();
    loop {
        match tokio::time::timeout(Duration::from_secs(300), rx_b.recv()).await {
            Ok(Some(EngineEvent::FolderTransferComplete {
                file_count,
                failed_count,
                ..
            })) => {
                assert_eq!((file_count as usize, failed_count), (files, 0));
                break;
            }
            Ok(Some(_)) => {}
            other => panic!("no folder result: {:?}", other.map(|o| o.is_some())),
        }
    }
    let el = t0.elapsed();
    println!(
        "{files} files of 20 KB in {el:?} = {:.0} files/s",
        files as f64 / el.as_secs_f64()
    );
}
