//! A file sent over a slow link must not cost the link its heartbeat.

use linkall_core::engine::{Engine, EngineConfig, EngineEvent};
use linkall_core::identity::IdentityStore;
use linkall_core::trust::TrustStore;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Copies one direction, at most `bytes_per_sec`.
async fn throttled_copy(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
    bytes_per_sec: usize,
) {
    let slice = bytes_per_sec / 20; // one slice per 50 ms
    let mut buf = vec![0u8; slice];
    loop {
        let n = match from.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        if to.write_all(&buf[..n]).await.is_err() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let _ = to.shutdown().await;
}

/// A localhost proxy to `target` that carries `bytes_per_sec` each way.
async fn slow_proxy(target: u16, bytes_per_sec: usize) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((inbound, _)) = listener.accept().await {
            let outbound = TcpStream::connect(("127.0.0.1", target)).await.unwrap();
            inbound.set_nodelay(true).ok();
            outbound.set_nodelay(true).ok();
            let (ir, iw) = inbound.into_split();
            let (or, ow) = outbound.into_split();
            tokio::spawn(throttled_copy(ir, ow, bytes_per_sec));
            tokio::spawn(throttled_copy(or, iw, bytes_per_sec));
        }
    });
    port
}

fn config(tmp: &TempDir, tag: &str, id: Uuid) -> EngineConfig {
    EngineConfig {
        device_id: id,
        device_name: format!("Node{tag}"),
        port: 0,
        trust_store_path: tmp.path().join(format!("trust_{tag}.json")),
        peer_store_path: tmp.path().join(format!("peer_{tag}.json")),
        identity_path: tmp.path().join(format!("id_{tag}.key")),
        data_dir: tmp.path().join(format!("data_{tag}")),
        file_save_dir: Some(tmp.path().join(format!("recv_{tag}"))),
        bind_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        enable_discovery: false,
        heartbeat_interval: Duration::from_millis(500),
        heartbeat_timeout: Duration::from_secs(3),
        ..Default::default()
    }
}

/// Over a 400 KB/s link one 4 MB chunk takes ten seconds to cross, longer
/// than the heartbeat allows. The receiver hears nothing complete in that
/// time; the link must still hold and the file arrive whole.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_large_chunk_on_a_slow_link_keeps_the_heartbeat() {
    let tmp = TempDir::new().unwrap();
    let (id_a, id_b) = (Uuid::new_v4(), Uuid::new_v4());
    let (cfg_a, cfg_b) = (config(&tmp, "a", id_a), config(&tmp, "b", id_b));
    let key_a = IdentityStore::new(&cfg_a.identity_path)
        .load_or_create()
        .unwrap();
    let key_b = IdentityStore::new(&cfg_b.identity_path)
        .load_or_create()
        .unwrap();
    TrustStore::load(&cfg_a.trust_store_path)
        .unwrap()
        .trust(id_b, "Nodeb".into(), &key_b.public_bytes)
        .unwrap();
    TrustStore::load(&cfg_b.trust_store_path)
        .unwrap()
        .trust(id_a, "Nodea".into(), &key_a.public_bytes)
        .unwrap();

    let (tx_a, mut rx_a) = mpsc::channel(256);
    let (tx_b, mut rx_b) = mpsc::channel(256);
    let a = Engine::start(cfg_a, tx_a).await.unwrap();
    let b = Engine::start(cfg_b, tx_b).await.unwrap();
    let proxy = slow_proxy(b.bound_port().await, 400 * 1024).await;
    a.connect_to_peer("127.0.0.1".into(), proxy).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    let data: Vec<u8> = (0..(5 * 1024 * 1024u32))
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    let src = tmp.path().join("slow.bin");
    std::fs::write(&src, &data).unwrap();
    a.send_file_path(
        src,
        "slow.bin".into(),
        "application/octet-stream".into(),
        Some(id_b),
        None,
        false,
        1,
    )
    .await
    .unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    let dest = loop {
        tokio::select! {
            ev = rx_b.recv() => match ev {
                Some(EngineEvent::FileTransferComplete { dest_path, .. }) => break dest_path,
                Some(EngineEvent::FileTransferFailed { reason, .. }) => panic!("transfer failed: {reason}"),
                Some(EngineEvent::PeerDisconnected { reason, .. }) => panic!("receiver dropped the link: {reason:?}"),
                Some(_) => {}
                None => panic!("receiver stopped"),
            },
            ev = rx_a.recv() => if let Some(EngineEvent::PeerDisconnected { reason, .. }) = ev {
                panic!("sender dropped the link: {reason:?}");
            },
            _ = tokio::time::sleep_until(deadline) => panic!("no completion in 60 s"),
        }
    };
    assert_eq!(std::fs::read(dest).unwrap(), data);
}
