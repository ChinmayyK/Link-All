//! Only one daemon may own the IPC socket. A second one used to delete the
//! first one's socket and bind its own, leaving two daemons running: the old
//! one unreachable but still connected to peers, the new one fighting it for
//! the same devices.
#![cfg(unix)]

use linkall_core::ipc::{server, IpcResponse};
use std::sync::Arc;

#[tokio::test]
async fn a_second_daemon_does_not_take_over_a_live_socket() {
    let dir = tempfile::TempDir::new().unwrap();
    // Only test in this binary, so setting the variable races nothing.
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };

    server::spawn(Arc::new(|_req| async { IpcResponse::ok_empty() }))
        .await
        .expect("the first daemon starts");
    assert!(linkall_core::ipc::daemon_is_listening().await);

    let second = server::spawn(Arc::new(|_req| async { IpcResponse::ok_empty() })).await;
    assert!(second.is_err(), "a second daemon took over the live socket");
}

#[tokio::test]
async fn a_stale_socket_left_by_a_crash_is_replaced() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("stale.sock");
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
    assert!(path.exists());
    assert!(!linkall_core::ipc::is_listening(&path).await);
}
