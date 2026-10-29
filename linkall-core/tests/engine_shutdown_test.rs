//! Hosts that stop and start the engine inside one process (the Android
//! service, the Windows app) rely on `Engine::shutdown` to free the port.
//! Dropping the engine alone does not: its tasks hold their own references
//! and keep listening.

mod common;

use common::{start_node, twin_config};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shut_down_engine_frees_its_port_for_the_next_start() {
    let tmp = tempfile::TempDir::new().unwrap();
    let id = Uuid::new_v4();
    let first = start_node(twin_config(&tmp, "phone", id, "Phone")).await;
    let port = first.engine.bound_port().await;
    first.engine.shutdown();
    drop(first);

    common::eventually("the port is free", async || {
        tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_err()
    })
    .await;

    let mut cfg = twin_config(&tmp, "phone", id, "Phone");
    cfg.port = port;
    let second = start_node(cfg).await;
    common::eventually_within(
        Duration::from_secs(10),
        "the restart listens on the same port",
        async || {
            tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_ok()
        },
    )
    .await;
    assert_eq!(second.engine.bound_port().await, port);
}
