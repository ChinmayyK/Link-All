//! Sending a folder between two engines: the tree arrives intact, more
//! files than a receiver takes at once (50 per peer) still all arrive, both
//! sides get one folder result, and a second copy lands beside the first.

mod common;

use common::{connect, start_pair, Node};
use linkall_core::engine::EngineEvent;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;

struct FolderResult {
    file_count: u32,
    failed_count: u32,
    outbound: bool,
    dest_dir: Option<PathBuf>,
}

/// Drains `node` until its folder result arrives.
async fn folder_result(node: &mut Node) -> FolderResult {
    timeout(Duration::from_secs(180), async {
        loop {
            match node.events.recv().await {
                Some(EngineEvent::FolderTransferComplete {
                    file_count,
                    failed_count,
                    outbound,
                    dest_dir,
                    ..
                }) => {
                    return FolderResult {
                        file_count,
                        failed_count,
                        outbound,
                        dest_dir,
                    }
                }
                Some(_) => {}
                None => panic!("event channel closed"),
            }
        }
    })
    .await
    .expect("no folder result")
}

/// Every file under `root` by path inside it.
fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    out
}

fn make_folder(parent: &Path) -> PathBuf {
    let root = parent.join("Trip");
    for i in 0..120 {
        let dir = root
            .join(format!("day {}", i % 6))
            .join(if i % 2 == 0 { "raw" } else { "" });
        std::fs::create_dir_all(&dir).unwrap();
        let body: Vec<u8> = (0..(i * 997) % 40_000).map(|b| (b % 251) as u8).collect();
        std::fs::write(dir.join(format!("photo {i}.jpg")), body).unwrap();
    }
    // Bigger than one 4 MB chunk.
    let big: Vec<u8> = (0..6 * 1024 * 1024).map(|b| (b % 241) as u8).collect();
    std::fs::write(root.join("video.mp4"), big).unwrap();
    std::fs::write(root.join(".DS_Store"), b"clutter").unwrap();
    root
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_folder_arrives_whole_and_a_second_copy_lands_beside_it() {
    let tmp = TempDir::new().unwrap();
    let (mut a, mut b) = start_pair(&tmp, true, true).await;
    connect(&a, &b).await;
    let src = make_folder(&tmp.path().join("src"));

    for (round, expected_dir) in [(0, "Trip"), (1, "Trip (2)")] {
        let send = a.engine.send_folder(src.clone(), Some(b.id)).await.unwrap();
        assert_eq!(send.file_count, 121, "clutter is not sent");
        if round == 0 {
            // The folder shows up in status at once, counting every file.
            let listed = a.engine.folders().await;
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].file_count, 121);
            assert!((0.0..=1.0).contains(&listed[0].progress));
        }
        let (sent, received) = tokio::join!(folder_result(&mut a), folder_result(&mut b));

        assert!(sent.outbound && !received.outbound);
        assert_eq!((sent.file_count, sent.failed_count), (121, 0));
        assert_eq!((received.file_count, received.failed_count), (121, 0));
        assert_eq!(sent.dest_dir, None);
        let dest = received.dest_dir.expect("receiver knows the folder");
        assert_eq!(
            dest,
            tmp.path().join("received1").join(expected_dir),
            "round {round}"
        );

        let mut want = tree(&src);
        want.remove(".DS_Store");
        assert_eq!(tree(&dest), want, "round {round}: tree differs");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_a_folder_stops_the_rest() {
    let tmp = TempDir::new().unwrap();
    let (mut a, b) = start_pair(&tmp, true, true).await;
    connect(&a, &b).await;
    let src = make_folder(&tmp.path().join("src"));

    let send = a.engine.send_folder(src, Some(b.id)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    a.engine.cancel_folder(&send.batch_id).await.unwrap();

    let sent = folder_result(&mut a).await;
    assert_eq!(sent.file_count, 121);
    assert!(
        sent.failed_count > 100,
        "most files were never sent, got {} failed",
        sent.failed_count
    );
    drop(b);
}

/// Makes `node` ask before taking any file bigger than one byte.
async fn ask_before_accepting(node: &Node) {
    let mut s = node.engine.current_settings().await;
    s.auto_accept_max_bytes = 1;
    node.engine.apply_settings(s).await;
}

/// Drains `node` until a file offer arrives; returns its id.
async fn first_offer(node: &mut Node) -> [u8; 16] {
    timeout(Duration::from_secs(30), async {
        loop {
            if let Some(EngineEvent::FileTransferIncoming { transfer_id, .. }) =
                node.events.recv().await
            {
                return transfer_id;
            }
        }
    })
    .await
    .expect("no file offer")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn accepting_one_file_of_a_folder_accepts_the_folder() {
    let tmp = TempDir::new().unwrap();
    let (mut a, mut b) = start_pair(&tmp, true, true).await;
    ask_before_accepting(&b).await;
    connect(&a, &b).await;
    let src = make_folder(&tmp.path().join("src"));

    a.engine.send_folder(src, Some(b.id)).await.unwrap();
    let offer = first_offer(&mut b).await;
    b.engine.accept_file_transfer(offer).await.unwrap();

    let (sent, received) = tokio::join!(folder_result(&mut a), folder_result(&mut b));
    assert_eq!((sent.file_count, sent.failed_count), (121, 0));
    assert_eq!((received.file_count, received.failed_count), (121, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn declining_one_file_of_a_folder_declines_the_folder() {
    let tmp = TempDir::new().unwrap();
    let (mut a, mut b) = start_pair(&tmp, true, true).await;
    ask_before_accepting(&b).await;
    connect(&a, &b).await;
    let src = make_folder(&tmp.path().join("src"));

    a.engine.send_folder(src, Some(b.id)).await.unwrap();
    let offer = first_offer(&mut b).await;
    b.engine
        .reject_file_transfer(offer, "user_declined".into())
        .await
        .unwrap();

    let sent = folder_result(&mut a).await;
    assert_eq!(sent.file_count, 121);
    assert!(
        sent.failed_count >= 115,
        "the rest was not sent, got {} failed",
        sent.failed_count
    );
    // The receiver reports nothing for a declined folder.
    let quiet = timeout(Duration::from_secs(3), folder_result(&mut b)).await;
    assert!(quiet.is_err(), "declined folder still reported");
}
