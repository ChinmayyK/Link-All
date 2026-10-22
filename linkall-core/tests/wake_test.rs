//! Waking a paired computer: what the engine refuses, and that a computer's
//! network address reaches the devices paired with it. Nothing here sends a
//! packet to a real sleeping machine.

mod common;

use common::{connect, peer, start_pair};
use tempfile::TempDir;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn waking_is_refused_when_it_cannot_work() {
    let tmp = TempDir::new().unwrap();
    let (phone, desk) = start_pair(&tmp, true, true).await;

    // A computer nobody has heard of.
    let err = phone.engine.wake_peer(Uuid::new_v4()).await.unwrap_err();
    assert!(err.contains("isn't known"), "{err}");

    connect(&phone, &desk).await;

    // Awake and connected: nothing to wake.
    let err = phone.engine.wake_peer(desk.id).await.unwrap_err();
    assert!(err.contains("already awake"), "{err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_computer_tells_its_peers_its_network_address() {
    let tmp = TempDir::new().unwrap();
    let (phone, desk) = start_pair(&tmp, true, true).await;
    connect(&phone, &desk).await;

    // Both ends are this computer, so each hears the address it would send.
    let Some(mac) = linkall_core::wol::local_mac() else {
        eprintln!("no readable network card here: skipping");
        return;
    };
    let seen = peer(&phone, desk.id).await.mac_address;
    assert_eq!(seen.as_deref(), Some(mac.as_str()));
    assert!(
        linkall_core::wol::parse_mac(&mac).is_some(),
        "it is a usable address"
    );
}
