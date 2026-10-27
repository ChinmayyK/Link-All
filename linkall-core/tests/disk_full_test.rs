//! Files the receiving disk has no room for are refused once, with a reason
//! the user can make sense of.
#![cfg(unix)]

use linkall_core::engine::{Engine, EngineConfig, EngineEvent};
use linkall_core::identity::IdentityStore;
use linkall_core::trust::TrustStore;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::mpsc;
use uuid::Uuid;

fn config(tmp: &TempDir, tag: &str, id: Uuid) -> EngineConfig {
    EngineConfig {
        device_id: id,
        device_name: format!("Node{tag}"),
        port: 0,
        trust_store_path: tmp.path().join(format!("trust_{tag}.json")),
        peer_store_path: tmp.path().join(format!("peer_{tag}.json")),
        identity_path: tmp.path().join(format!("id_{tag}.key")),
        data_dir: tmp.path().join(format!("data_{tag}")),

        settings_path: tmp.path().join(format!("settings_{tag}.json")),
        file_save_dir: Some(tmp.path().join(format!("recv_{tag}"))),
        bind_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        enable_discovery: false,
        heartbeat_interval: Duration::from_millis(500),
        heartbeat_timeout: Duration::from_secs(3),
        ..Default::default()
    }
}

/// More than a test machine has free (and under the 1 TB cap), taking no
/// space itself: the file is sparse.
const HUGE: u64 = 512 << 30;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn files_too_big_for_the_disk_are_refused_once_each() {
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

    let (tx_a, _rx_a) = mpsc::channel(256);
    let (tx_b, mut rx_b) = mpsc::channel(256);
    let a = Engine::start(cfg_a, tx_a).await.unwrap();
    let b = Engine::start(cfg_b, tx_b).await.unwrap();
    a.connect_to_peer("127.0.0.1".into(), b.bound_port().await)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    for i in 0..6 {
        let src = tmp.path().join(format!("huge{i}.bin"));
        std::fs::File::create(&src).unwrap().set_len(HUGE).unwrap();
        a.send_file_path(
            src,
            format!("huge{i}.bin"),
            "application/octet-stream".into(),
            Some(id_b),
            None,
            false,
            1,
        )
        .await
        .unwrap();
    }

    let mut failures: HashMap<[u8; 16], Vec<String>> = HashMap::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        tokio::select! {
            ev = rx_b.recv() => match ev {
                Some(EngineEvent::FileTransferFailed { transfer_id, reason, .. }) => {
                    failures.entry(transfer_id).or_default().push(reason);
                }
                Some(_) => {}
                None => break,
            },
            _ = tokio::time::sleep_until(deadline) => break,
        }
    }
    assert_eq!(failures.len(), 6, "{failures:?}");
    for reasons in failures.values() {
        assert_eq!(
            reasons.len(),
            1,
            "one transfer refused more than once: {reasons:?}"
        );
    }
}
