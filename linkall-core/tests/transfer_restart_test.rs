//! The receiving app quits in the middle of a file transfer and starts again.
//! The transfer resumes on the new session and the file arrives whole. It used
//! to fail: quitting cancelled the pending disk write, which was reported to
//! both sides as a disk error.

mod common;

use common::{connected, eventually, eventually_within, start_node, trusts, twin_config, Node};
use linkall_core::engine::EngineEvent;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;
use uuid::Uuid;

const SIZE: usize = 80 * 1024 * 1024;

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn wait_for_progress(node: &mut Node, percent: u8) {
    timeout(Duration::from_secs(120), async {
        loop {
            if let Some(EngineEvent::FileTransferProgress {
                percent: p,
                outbound: true,
                ..
            }) = node.events.recv().await
            {
                if p >= percent {
                    return;
                }
            }
        }
    })
    .await
    .expect("transfer never got going");
}

enum Outcome {
    Received(std::path::PathBuf),
    Failed(String),
}

/// The first of: the receiver has the file, or the sender gives up on it.
async fn outcome(phone: &mut Node, computer: &mut Node) -> Outcome {
    timeout(Duration::from_secs(180), async {
        loop {
            tokio::select! {
                Some(e) = computer.events.recv() => {
                    if let EngineEvent::FileTransferComplete { dest_path, .. } = e {
                        return Outcome::Received(dest_path);
                    }
                }
                Some(e) = phone.events.recv() => {
                    if let EngineEvent::FileTransferFailed { reason, .. } = e {
                        return Outcome::Failed(reason);
                    }
                }
            }
        }
    })
    .await
    .expect("the transfer neither finished nor failed after the restart")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transfer_resumes_after_the_receiver_restarts() {
    let tmp = TempDir::new().unwrap();
    let (phone_id, computer_id) = (Uuid::new_v4(), Uuid::new_v4());
    let computer_port = free_port();
    let computer_config = || {
        let mut cfg = twin_config(&tmp, "computer", computer_id, "Computer");
        cfg.port = computer_port;
        cfg
    };
    trusts(&tmp, "phone", computer_id, "Computer", "computer");
    trusts(&tmp, "computer", phone_id, "Phone", "phone");
    let mut phone = start_node(twin_config(&tmp, "phone", phone_id, "Phone")).await;

    // The computer runs on its own runtime so the test can kill it outright.
    let cfg = computer_config();
    let (computer_rt, computer) = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let node = rt.block_on(start_node(cfg));
        (rt, node)
    })
    .join()
    .unwrap();
    // A fixed port is reported before the listener is up, so keep dialling.
    eventually("both sides connected", async || {
        let _ = phone
            .engine
            .connect_to_peer("127.0.0.1".into(), computer_port)
            .await;
        connected(&phone, computer_id).await && connected(&computer, phone_id).await
    })
    .await;

    let data: Vec<u8> = (0..SIZE).map(|i| (i % 239) as u8).collect();
    let path = tmp.path().join("backup.bin");
    std::fs::write(&path, &data).unwrap();
    phone
        .engine
        .send_file_path(
            path,
            "backup.bin".into(),
            "application/octet-stream".into(),
            Some(computer_id),
            None,
            false,
            1,
        )
        .await
        .expect("send");
    wait_for_progress(&mut phone, 40).await;

    // The computer's app dies mid-transfer, then starts again.
    drop(computer);
    std::thread::spawn(move || computer_rt.shutdown_timeout(Duration::from_secs(2)))
        .join()
        .unwrap();
    let mut computer = start_node(computer_config()).await;
    eventually_within(
        Duration::from_secs(60),
        "they reconnect after the restart",
        async || {
            if !connected(&phone, computer_id).await {
                let _ = phone
                    .engine
                    .connect_to_peer("127.0.0.1".into(), computer_port)
                    .await;
            }
            connected(&phone, computer_id).await && connected(&computer, phone_id).await
        },
    )
    .await;

    match outcome(&mut phone, &mut computer).await {
        Outcome::Received(dest) => {
            let got = std::fs::read(&dest).unwrap();
            assert!(got == data, "received file differs from the original");
        }
        Outcome::Failed(reason) => panic!("the transfer failed instead of resuming: {reason}"),
    }
}
