//! Pausing and resuming a transfer near the end (90%), from either side.
//! Chunks still in flight when the pause lands, and the resume racing the
//! resent chunks, used to leave a gap the receiver never recovered from.

mod common;

use common::{connect, start_pair, Node};
use linkall_core::engine::EngineEvent;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;

const SIZE: usize = 80 * 1024 * 1024; // 20 chunks

fn payload() -> Vec<u8> {
    (0..SIZE).map(|i| (i % 251) as u8).collect()
}

/// Waits for the outbound transfer to pass `percent` on the sender.
async fn wait_for_progress(node: &mut Node, percent: u8) -> [u8; 16] {
    timeout(Duration::from_secs(180), async {
        loop {
            let ev = node.events.recv().await;
            if let Some(EngineEvent::FileTransferProgress {
                transfer_id,
                percent: p,
                outbound: true,
                ..
            }) = ev
            {
                if p >= percent {
                    return transfer_id;
                }
            }
        }
    })
    .await
    .expect("transfer never reached the pause point")
}

async fn wait_for_completion(node: &mut Node) -> std::path::PathBuf {
    timeout(Duration::from_secs(180), async {
        loop {
            match node.events.recv().await {
                Some(EngineEvent::FileTransferComplete { dest_path, .. }) => return dest_path,
                Some(EngineEvent::FileTransferFailed { reason, .. }) => {
                    panic!("transfer failed: {reason}")
                }
                Some(_) => {}
                None => panic!("event channel closed"),
            }
        }
    })
    .await
    .expect("transfer never completed after resume")
}

async fn pause_and_resume_near_end(sender_pauses: bool, resumer_is_sender: bool) {
    pause_and_resume(sender_pauses, resumer_is_sender, false).await;
}

async fn pause_and_resume(sender_pauses: bool, resumer_is_sender: bool, from_disk: bool) {
    let tmp = TempDir::new().unwrap();
    let (mut a, mut b) = start_pair(&tmp, true, true).await;
    connect(&a, &b).await;

    let data = payload();
    if from_disk {
        // How phones and desktops send: read from disk as the transfer goes.
        let path = tmp.path().join("big.bin");
        std::fs::write(&path, &data).unwrap();
        a.engine
            .send_file_path(
                path,
                "big.bin".into(),
                "application/octet-stream".into(),
                Some(b.id),
                None,
                false,
                1,
            )
            .await
            .expect("send");
    } else {
        a.engine
            .send_file(
                data.clone(),
                "big.bin".into(),
                "application/octet-stream".into(),
                Some(b.id),
            )
            .await
            .expect("send");
    }
    let tid = wait_for_progress(&mut a, 90).await;

    let pauser = if sender_pauses { &a } else { &b };
    pauser.engine.pause_file_transfer(tid).await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    let resumer = if resumer_is_sender { &a } else { &b };
    resumer.engine.resume_file_transfer(tid).await.unwrap();

    let dest = wait_for_completion(&mut b).await;
    let got = std::fs::read(&dest).unwrap();
    assert_eq!(got.len(), data.len());
    assert!(got == data, "received file differs from the original");
}

#[tokio::test(flavor = "multi_thread")]
async fn sender_pauses_and_resumes_near_the_end() {
    pause_and_resume_near_end(true, true).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn receiver_pauses_and_resumes_near_the_end() {
    pause_and_resume_near_end(false, false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sender_pauses_and_receiver_resumes() {
    pause_and_resume_near_end(true, false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn receiver_pauses_and_sender_resumes() {
    pause_and_resume_near_end(false, true).await;
}

/// From disk the sender resumes from saved checksum state instead of
/// reading the delivered part of the file again.
#[tokio::test(flavor = "multi_thread")]
async fn file_on_disk_pauses_and_resumes_near_the_end() {
    pause_and_resume(true, false, true).await;
}
