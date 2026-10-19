//! Getting back together after one side's app dies and starts again.

mod common;

use common::{
    connected, eventually, eventually_within, peer, start_node, trusts, twin_config, wait_for,
};
use linkall_core::engine::EngineEvent;
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

/// Devices that pair the usual way (seen first, then a PIN accepted) must
/// be ones each side reconnects to by itself. Pairing used to mark a device
/// trusted but not remembered, and the reconnector only dials remembered
/// devices, so after any drop the computer waited for the phone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_device_paired_with_a_pin_is_redialled_after_a_drop() {
    let tmp = TempDir::new().unwrap();
    let (phone_id, computer_id) = (Uuid::new_v4(), Uuid::new_v4());
    let mut computer = start_node(twin_config(&tmp, "computer", computer_id, "Computer")).await;
    let computer_port = computer.engine.bound_port().await;
    let phone_port = free_port();
    let phone_config = |peer_store: &str| {
        let mut cfg = twin_config(&tmp, "phone", phone_id, "Phone");
        cfg.port = phone_port;
        cfg.peer_store_path = tmp.path().join(peer_store);
        cfg
    };

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
                .await?;
            engine.send_pairing_request(computer_id).await;
            anyhow::Ok(())
        })
        .await
        .unwrap()
        .expect("the phone asks to pair");
    wait_for(
        &mut computer.events,
        "the pairing request",
        |e| matches!(e, EngineEvent::PairingRequested { device_id, .. } if *device_id == phone_id),
    )
    .await;
    computer
        .engine
        .respond_to_pairing(phone_id, true)
        .await
        .unwrap();
    eventually("both sides paired and connected", async || {
        computer.engine.is_trusted(phone_id).await && connected(&computer, phone_id).await
    })
    .await;
    assert!(
        peer(&computer, phone_id).await.remembered,
        "a paired device is remembered"
    );

    // The phone's app dies, then starts again without its device list.
    drop(phone);
    std::thread::spawn(move || phone_rt.shutdown_timeout(Duration::from_secs(2)))
        .join()
        .unwrap();
    eventually("the computer notices the phone is gone", async || {
        !connected(&computer, phone_id).await
    })
    .await;
    let phone = start_node(phone_config("peers_phone_fresh.json")).await;
    eventually_within(
        Duration::from_secs(45),
        "the computer reconnects to the phone",
        async || connected(&computer, phone_id).await && connected(&phone, computer_id).await,
    )
    .await;
}

/// Reinstalling the phone app gives the phone a new identity on the same
/// address. The computer still redials the old, paired identity there, and
/// used to close the connection on finding another device. That also ended
/// the session the phone already had with it, every reconnect cycle, so the
/// reinstalled phone could never stay connected long enough to pair.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reinstalled_phone_on_the_old_address_stays_connected() {
    let tmp = TempDir::new().unwrap();
    let (old_phone_id, new_phone_id, computer_id) =
        (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    trusts(&tmp, "old_phone", computer_id, "Computer", "computer");
    trusts(&tmp, "computer", old_phone_id, "Phone", "old_phone");
    let computer = start_node(twin_config(&tmp, "computer", computer_id, "Computer")).await;
    let computer_port = computer.engine.bound_port().await;
    let phone_port = free_port();

    let mut cfg = twin_config(&tmp, "old_phone", old_phone_id, "Phone");
    cfg.port = phone_port;
    let (phone_rt, old_phone) = std::thread::spawn(move || {
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
    let engine = old_phone.engine.clone();
    phone_rt
        .spawn(async move {
            engine
                .connect_to_peer("127.0.0.1".into(), computer_port)
                .await
        })
        .await
        .unwrap()
        .expect("the old phone dials the computer");
    eventually("the computer sees the old phone", async || {
        connected(&computer, old_phone_id).await
    })
    .await;

    // The app is uninstalled, then installed again: a new identity, no
    // pairings, listening where the old install did.
    drop(old_phone);
    std::thread::spawn(move || phone_rt.shutdown_timeout(Duration::from_secs(2)))
        .join()
        .unwrap();
    eventually("the computer notices the old phone is gone", async || {
        !connected(&computer, old_phone_id).await
    })
    .await;
    let mut cfg = twin_config(&tmp, "new_phone", new_phone_id, "Phone");
    cfg.port = phone_port;
    let mut new_phone = start_node(cfg).await;

    eventually_within(
        Duration::from_secs(45),
        "the computer's redial reaches the new phone",
        async || {
            connected(&computer, new_phone_id).await && connected(&new_phone, computer_id).await
        },
    )
    .await;
    while new_phone.events.try_recv().is_ok() {}

    // The link must hold through further reconnect cycles, or a pairing
    // request has nothing to travel on.
    let hold = Duration::from_secs(15);
    let ended = tokio::time::timeout(hold, async {
        loop {
            match new_phone.events.recv().await {
                Some(EngineEvent::PeerDisconnected { device_id, .. })
                    if device_id == computer_id =>
                {
                    return;
                }
                Some(_) => {}
                None => std::future::pending::<()>().await,
            }
        }
    })
    .await;
    assert!(ended.is_err(), "the new phone lost the computer");
    assert!(connected(&computer, new_phone_id).await);
    assert!(
        peer(&computer, old_phone_id).await.ips.is_empty(),
        "the computer stops dialling the old identity at the new phone's address"
    );
}
