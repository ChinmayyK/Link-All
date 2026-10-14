//! Starting the background service. Installs normally run `linkall-daemon` as
//! a systemd user service; when it isn't running (first launch, a portable
//! copy, a desktop without systemd) the app starts it itself.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

fn daemon_binary() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("linkall-daemon")))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("linkall-daemon"))
}

/// True once the service answers, starting it first if needed.
pub fn ensure_running() -> bool {
    if crate::ipc::is_running() {
        return true;
    }
    let started = Command::new(daemon_binary())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Err(error) = started {
        tracing::warn!("couldn't start linkall-daemon: {error}");
        return false;
    }
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if crate::ipc::is_running() {
            return true;
        }
    }
    false
}
