//! Getting back together after one side's app dies and starts again.

mod common;

use common::{connected, eventually, eventually_within, peer, start_node, trusts, twin_config};
use std::time::Duration;
use tempfile::TempDir;
use uuid::Uuid;

/// A free port on localhost, for a node that must come back on the same one.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The phone dials the computer, so the computer sees the connection come
/// from the phone's outgoing port. When the phone's app dies and starts
/// again on its usual port, the computer must dial it back there by itself:
/// a phone spaces its own retries minutes apart while its screen is off.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_computer_redials_a_phone_that_had_dialled_in() {
    let tmp = TempDir::new().unwrap();
    let (phone_id, computer_id) = (Uuid::new_v4(), Uuid::new_v4());
    trusts(&tmp, "phone", computer_id, "Computer", "computer");
    trusts(&tmp, "computer", phone_id, "Phone", "phone");
    let computer = start_node(twin_config(&tmp, "computer", computer_id, "Computer")).await;
    let computer_port = computer.engine.bound_port().await;
    let phone_port = free_port();
    let phone_config = |peer_store: &str| {
        let mut cfg = twin_config(&tmp, "phone", phone_id, "Phone");
        cfg.port = phone_port;
        cfg.peer_store_path = tmp.path().join(peer_store);
        cfg
    };

    // The phone runs in its own runtime, so it can die like an app.
    let cfg = phone_config("peers_phone.json");
    let (phone_rt, phone) = std::thread::spawn(move || {
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
    let engine = phone.engine.clone();
    phone_rt
        .spawn(async move {
            engine
                .connect_to_peer("127.0.0.1".into(), computer_port)
                .await
        })
        .await
        .unwrap()
        .expect("the phone dials the computer");
    eventually("the computer sees the phone", async || {
        connected(&computer, phone_id).await
    })
    .await;
    assert_eq!(
        peer(&computer, phone_id).await.port,
        phone_port,
        "the computer keeps the port the phone listens on, not its outgoing one"
    );

    // The phone's app dies.
    drop(phone);
    std::thread::spawn(move || phone_rt.shutdown_timeout(Duration::from_secs(2)))
        .join()
        .unwrap();
    eventually("the computer notices the phone is gone", async || {
        !connected(&computer, phone_id).await
    })
    .await;

    // It starts again on its port, but without its list of devices, so it
    // can't dial the computer itself: only the computer can bring them back.
    let phone = start_node(phone_config("peers_phone_fresh.json")).await;
    assert_eq!(phone.engine.bound_port().await, phone_port);
    eventually_within(
        Duration::from_secs(45),
        "the computer reconnects to the phone",
        async || connected(&computer, phone_id).await && connected(&phone, computer_id).await,
    )
    .await;
}
