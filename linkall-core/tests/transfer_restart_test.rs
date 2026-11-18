//! A file being received goes on after the receiving app restarts.

mod common;

use common::{connect, start_node, trusts, twin_config};
use linkall_core::engine::EngineEvent;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;
use uuid::Uuid;

const CHUNK: u64 = 4 * 1024 * 1024;

/// The phone kills the app part-way through a large file. When the app is
/// back and the sender reconnects, the file continues from what had arrived
/// instead of starting again, and arrives intact.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_received_file_continues_after_the_receiver_restarts() {
    let tmp = TempDir::new().unwrap();
    let (a_id, b_id) = (Uuid::new_v4(), Uuid::new_v4());
    trusts(&tmp, "a", b_id, "NodeB", "b");
    trusts(&tmp, "b", a_id, "NodeA", "a");
    let a = start_node(twin_config(&tmp, "a", a_id, "NodeA")).await;
    let mut b = start_node(twin_config(&tmp, "b", b_id, "NodeB")).await;
    connect(&a, &b).await;

    let data: Vec<u8> = (0..40 * CHUNK as usize + 777)
        .map(|i| (i.wrapping_mul(31) % 251) as u8)
        .collect();
    let src = tmp.path().join("film.bin");
    std::fs::write(&src, &data).unwrap();
    a.engine
        .send_file_path(
            src,
            "film.bin".into(),
            "application/octet-stream".into(),
            Some(b_id),
            None,
            false,
            1,
        )
        .await
        .unwrap();

    // Let some of it arrive, then stop the receiver without finishing.
    loop {
        let event = timeout(Duration::from_secs(20), b.events.recv())
            .await
            .expect("progress on B")
            .expect("B's events");
        match event {
            EngineEvent::FileTransferProgress { bytes_received, .. }
                if bytes_received >= 3 * CHUNK =>
            {
                break
            }
            EngineEvent::FileTransferComplete { .. } => {
                panic!("the file finished before the receiver could be stopped")
            }
            _ => {}
        }
    }
    b.engine.shutdown();
    drop(b);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // The same device starts again and the sender finds it.
    let mut b = start_node(twin_config(&tmp, "b", b_id, "NodeB")).await;
    connect(&a, &b).await;

    let mut least_seen = u64::MAX;
    let dest = loop {
        let event = timeout(Duration::from_secs(60), b.events.recv())
            .await
            .expect("the file finishes on the restarted receiver")
            .expect("B's events");
        match event {
            EngineEvent::FileTransferProgress { bytes_received, .. } => {
                least_seen = least_seen.min(bytes_received);
            }
            EngineEvent::FileTransferComplete { dest_path, .. } => break dest_path,
            EngineEvent::FileTransferFailed { reason, .. } => panic!("failed: {reason}"),
            _ => {}
        }
    };

    assert!(
        least_seen >= 3 * CHUNK,
        "started again from {least_seen} bytes instead of resuming"
    );
    assert_eq!(dest.file_name().unwrap(), "film.bin");
    assert!(std::fs::read(&dest).unwrap() == data, "the file differs");
    let leftovers: Vec<_> = std::fs::read_dir(dest.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .filter(|n| n != "film.bin")
        .collect();
    assert!(leftovers.is_empty(), "left behind: {leftovers:?}");
}
