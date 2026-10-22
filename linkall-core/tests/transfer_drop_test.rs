//! A file transfer whose connection drops part way (not a pause): the
//! devices reconnect by themselves, the transfer picks up where it stopped,
//! and the received file is whole, with nothing half-written left behind.

mod common;

use common::{connected, eventually, eventually_within, start_pair, Node};
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
    freeze: watch::Sender<u32>,
}

impl Relay {
    async fn start(target: u16) -> Relay {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (cut, cuts) = watch::channel(0u32);
        let (freeze, freezes) = watch::channel(0u32);
        tokio::spawn(async move {
            while let Ok((inbound, _)) = listener.accept().await {
                let Ok(outbound) = TcpStream::connect(("127.0.0.1", target)).await else {
                    continue;
                };
                let mut cuts = cuts.clone();
                cuts.mark_unchanged();
                let mut freezes = freezes.clone();
                freezes.mark_unchanged();
                tokio::spawn(async move {
                    let (mut inbound, mut outbound) = (inbound, outbound);
                    let frozen = tokio::select! {
                        _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound) => false,
                        _ = cuts.changed() => false,
                        _ = freezes.changed() => true,
                    };
                    if frozen {
                        // Keep both sockets open and read nothing: a link that
                        // went silent, with no reset to say so.
                        std::future::pending::<()>().await;
                        drop((inbound, outbound));
                    }
                });
            }
        });
        Relay { port, cut, freeze }
    }

    fn cut(&self) {
        self.cut.send_modify(|n| *n += 1);
    }

    fn freeze(&self) {
        self.freeze.send_modify(|n| *n += 1);
    }
}

/// Waits for `node` to report losing `peer`, or connecting to it again (a
/// session replaced at once reports no drop in between).
async fn wait_for_drop_or_redial(node: &mut Node, peer: uuid::Uuid, limit: Duration) {
    timeout(limit, async {
        loop {
            match node.events.recv().await {
                Some(EngineEvent::PeerDisconnected { device_id, .. })
                | Some(EngineEvent::PeerConnected { device_id, .. })
                    if device_id == peer =>
                {
                    return
                }
                Some(_) => {}
                None => panic!("event channel closed"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out: {peer} neither dropped nor reconnected"))
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
    // be slow to notice (a debug build on a busy runner). The phone itself may
    // redial so fast that the new session replaces the old one before the old
    // one reports a drop, so a fresh connection counts as the drop too.
    wait_for_drop_or_redial(&mut phone, computer.id, Duration::from_secs(20)).await;
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

/// A link that goes silent in the middle of a transfer (Wi-Fi gone, no reset)
/// leaves the sender's writes waiting on a full buffer. The sender must still
/// notice, within about a heartbeat, instead of hanging until TCP gives up.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_that_goes_silent_mid_transfer_is_noticed_by_the_sender() {
    let tmp = TempDir::new().unwrap();
    let (mut phone, computer) = start_pair(&tmp, true, true).await;
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

    let path = tmp.path().join("big.bin");
    std::fs::write(&path, vec![7u8; SIZE]).unwrap();
    phone
        .engine
        .send_file_path(
            path,
            "big.bin".into(),
            "application/octet-stream".into(),
            Some(computer.id),
            None,
            false,
            1,
        )
        .await
        .expect("send");
    wait_for_progress(&mut phone, 20).await;

    relay.freeze();
    wait_for_drop_or_redial(&mut phone, computer.id, Duration::from_secs(45)).await;
}
