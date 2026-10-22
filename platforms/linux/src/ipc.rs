//! Talking to `linkall-daemon` over its Unix socket: one JSON request per
//! line, one JSON reply per line, the same protocol `linkall-cli`, the Mac app
//! and the Windows app use.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Socket I/O runs on a small Tokio runtime so the GTK thread never blocks.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("linkall-ipc")
            .enable_all()
            .build()
            .expect("IPC runtime")
    })
}

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

async fn send(req: Value, timeout: Duration) -> Result<Value> {
    let path = linkall_core::ipc::socket_path();
    let mut stream = tokio::time::timeout(Duration::from_millis(800), UnixStream::connect(&path))
        .await
        .context("Link All's background service isn't answering")?
        .context("Link All's background service isn't running")?;
    let (reader, mut writer) = stream.split();
    let mut line = serde_json::to_vec(&req)?;
    line.push(b'\n');
    writer.write_all(&line).await?;
    let mut reply = String::new();
    tokio::time::timeout(timeout, BufReader::new(reader).read_line(&mut reply))
        .await
        .context("Link All's background service took too long to answer")??;
    let reply: Value = serde_json::from_str(&reply).context("unreadable reply")?;
    match reply.get("status").and_then(Value::as_str) {
        Some("error") => bail!(reply
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Something went wrong")
            .to_string()),
        _ => Ok(reply.get("data").cloned().unwrap_or(Value::Null)),
    }
}

/// Sends a request without blocking the caller's thread; await it from a
/// `glib` future on the GTK thread.
pub async fn call(req: Value) -> Result<Value> {
    call_with(req, DEFAULT_TIMEOUT).await
}

pub async fn call_with(req: Value, timeout: Duration) -> Result<Value> {
    runtime().spawn(send(req, timeout)).await?
}

/// For threads that aren't the GTK thread (clipboard watcher, tray).
pub fn call_blocking(req: Value) -> Result<Value> {
    call_blocking_with(req, DEFAULT_TIMEOUT)
}

pub fn call_blocking_with(req: Value, timeout: Duration) -> Result<Value> {
    runtime().block_on(send(req, timeout))
}

pub fn is_running() -> bool {
    call_blocking(serde_json::json!({ "cmd": "ping" })).is_ok()
}
