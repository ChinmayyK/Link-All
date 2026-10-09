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
