//! A file transfer whose connection drops part way (not a pause): the
//! devices reconnect by themselves, the transfer picks up where it stopped,
//! and the received file is whole, with nothing half-written left behind.

mod common;

use common::{connected, eventually, eventually_within, start_pair, wait_for_disconnect, Node};
use linkall_core::engine::EngineEvent;
use std::time::Duration;
use tempfile::TempDir;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::timeout;

const SIZE: usize = 80 * 1024 * 1024; // 20 chunks

/// Forwards one TCP port to another; `cut` closes every connection through
/// it at once, like Wi-Fi dropping, and new connections still go through.
struct Relay {
    port: u16,
    cut: watch::Sender<u32>,
}

impl Relay {
    async fn start(target: u16) -> Relay {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (cut, cuts) = watch::channel(0u32);
        tokio::spawn(async move {
            while let Ok((inbound, _)) = listener.accept().await {
                let Ok(outbound) = TcpStream::connect(("127.0.0.1", target)).await else {
                    continue;
                };
                let mut cuts = cuts.clone();
                cuts.mark_unchanged();
                tokio::spawn(async move {
                    let (mut inbound, mut outbound) = (inbound, outbound);
                    tokio::select! {
                        _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound) => {}
                        _ = cuts.changed() => {}
                    }
                });
            }
        });
        Relay { port, cut }
    }

    fn cut(&self) {
        self.cut.send_modify(|n| *n += 1);
    }
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
    .expect("transfer never completed after the drop")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transfer_cut_off_by_a_drop_finishes_after_reconnecting() {
    let tmp = TempDir::new().unwrap();
    let (mut phone, mut computer) = start_pair(&tmp, true, true).await;
    let relay = Relay::start(computer.engine.bound_port().await).await;
    phone
        .engine
        .connect_to_peer("127.0.0.1".into(), relay.port)
        .await
        .expect("the phone dials the computer through the relay");
    eventually("both sides connected", async || {
        connected(&computer, phone.id).await && connected(&phone, computer.id).await
    })
    .await;

    let data: Vec<u8> = (0..SIZE).map(|i| (i % 251) as u8).collect();
    let path = tmp.path().join("video.bin");
    std::fs::write(&path, &data).unwrap();
    phone
        .engine
        .send_file_path(
            path,
            "video.bin".into(),
            "application/octet-stream".into(),
            Some(computer.id),
            None,
            false,
            1,
        )
        .await
        .expect("send");
    wait_for_progress(&mut phone, 40).await;

    relay.cut();
    // Watch the phone, which dialled and loses the link first. The computer may
    // be slow to notice (a debug build on a busy runner), and when the phone has
    // already reconnected it replaces the old session without reporting a drop.
    wait_for_disconnect(&mut phone, computer.id, Duration::from_secs(20)).await;
    eventually_within(
        Duration::from_secs(60),
        "they reconnect by themselves",
        async || connected(&computer, phone.id).await && connected(&phone, computer.id).await,
    )
    .await;

    let dest = wait_for_completion(&mut computer).await;
    let got = std::fs::read(&dest).unwrap();
    assert_eq!(got.len(), data.len());
    assert!(got == data, "received file differs from the original");

    let leftovers: Vec<_> = std::fs::read_dir(dest.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name != "video.bin")
        .collect();
    assert!(
        leftovers.is_empty(),
        "half-written files left beside it: {leftovers:?}"
    );
}
