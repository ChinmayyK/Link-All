//! A paired device remembered at one of this computer's own addresses.
//! Kept in its own binary: it needs a fixed port, and `free_port` races
//! with other tests choosing ports at the same time.

mod common;

use common::{connected, eventually_within, peer, start_node, trusts, twin_config};
use std::time::Duration;
use tempfile::TempDir;
use uuid::Uuid;

/// A free port on localhost, for a node that must listen on a known one.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A paired device remembered at an address that is this computer's own (a
/// VM reached through a port forward on localhost) is dialled once, answers
/// as this computer, and that address is dropped. Keeping it made every
/// reconnect cycle handshake with itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_address_that_leads_back_here_is_not_redialled() {
    let tmp = TempDir::new().unwrap();
    let (computer_id, vm_id) = (Uuid::new_v4(), Uuid::new_v4());
    let port = free_port();
    let mut cfg = twin_config(&tmp, "computer", computer_id, "Computer");
    cfg.port = port;
    trusts(&tmp, "computer", vm_id, "Ubuntu", "vm");
    linkall_core::peer_manager::PeerManager::load(&cfg.peer_store_path)
        .unwrap()
        .upsert_peer(
            vm_id,
            "Ubuntu".into(),
            std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            true,
            linkall_core::peer_manager::DiscoverySource::Mdns,
        )
        .unwrap();

    let computer = start_node(cfg).await;
    eventually_within(
        Duration::from_secs(20),
        "the computer drops its own address from the VM's",
        async || peer(&computer, vm_id).await.ips.is_empty(),
    )
    .await;
    assert!(!connected(&computer, vm_id).await);
}
