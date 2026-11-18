//! Many files sent at once. Concurrent queue pumps on the receiver used to
//! accept the same transfer twice, so a third of the files failed.

mod common;

use common::{connect, start_pair};
use linkall_core::engine::EngineEvent;
use std::time::Duration;
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn thirty_files_sent_at_once_all_arrive() {
    let tmp = TempDir::new().unwrap();
    let (mut a, b) = start_pair(&tmp, true, true).await;
    connect(&a, &b).await;
    let dir = tmp.path().join("src");
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..30 {
        let path = dir.join(format!("f{i}.bin"));
        std::fs::write(&path, vec![7u8; 1000 + i]).unwrap();
        a.engine
            .send_file_path(
                path,
                format!("f{i}.bin"),
                "application/octet-stream".into(),
                Some(b.id),
                None,
                false,
                1,
            )
            .await
            .unwrap();
    }
    let mut sent = 0;
    tokio::time::timeout(Duration::from_secs(60), async {
        while sent < 30 {
            match a.events.recv().await {
                Some(EngineEvent::FileTransferComplete { .. }) => sent += 1,
                Some(EngineEvent::FileTransferFailed { reason, .. }) => {
                    panic!("a file failed after {sent} arrived: {reason}")
                }
                Some(_) => {}
                None => panic!("event channel closed"),
            }
        }
    })
    .await
    .expect("not every file arrived");
    let mut names: Vec<_> = std::fs::read_dir(tmp.path().join("received1"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names.len(), 30, "{names:?}");
    drop(b);
}

/// Both devices send each other the same large file at the same moment. Each
/// side ends with one whole copy, and neither transfer gets mixed up with the
/// other, even though they share a name and contents.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_same_file_sent_both_ways_at_once_arrives_whole_on_each_side() {
    const SIZE: usize = 40 * 1024 * 1024;
    let tmp = TempDir::new().unwrap();
    let (mut a, mut b) = start_pair(&tmp, true, true).await;
    connect(&a, &b).await;
    let data: Vec<u8> = (0..SIZE).map(|i| (i % 233) as u8).collect();
    let mut sends = Vec::new();
    for (tag, from, to) in [("a", &a, b.id), ("b", &b, a.id)] {
        let path = tmp.path().join(format!("src_{tag}.bin"));
        std::fs::write(&path, &data).unwrap();
        let engine = from.engine.clone();
        sends.push(tokio::spawn(async move {
            engine
                .send_file_path(
                    path,
                    "shared.bin".into(),
                    "application/octet-stream".into(),
                    Some(to),
                    None,
                    false,
                    1,
                )
                .await
        }));
    }
    for send in sends {
        send.await.unwrap().expect("send");
    }

    // Each side sees two completions: the file it sent and the one it got.
    for node in [&mut a, &mut b] {
        let mut done = 0;
        tokio::time::timeout(Duration::from_secs(120), async {
            while done < 2 {
                match node.events.recv().await {
                    Some(EngineEvent::FileTransferComplete { .. }) => done += 1,
                    Some(EngineEvent::FileTransferFailed { reason, .. }) => {
                        panic!("a transfer failed: {reason}")
                    }
                    Some(_) => {}
                    None => panic!("event channel closed"),
                }
            }
        })
        .await
        .expect("both transfers did not finish");
    }
    for dir in ["received0", "received1"] {
        let names: Vec<_> = std::fs::read_dir(tmp.path().join(dir))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, ["shared.bin"], "{dir} holds {names:?}");
        let got = std::fs::read(tmp.path().join(dir).join("shared.bin")).unwrap();
        assert!(got == data, "{dir}/shared.bin differs from the original");
    }
}
