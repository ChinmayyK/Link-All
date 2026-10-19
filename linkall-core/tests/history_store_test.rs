//! A host that records clipboard history (the desktop daemon) shares the
//! engine's store. Two copies of history.json, each saving on every clip,
//! overwrote each other: a clip copied here vanished once one arrived.

mod common;

use common::{connect, start_pair, wait_for};
use linkall_core::engine::EngineEvent;
use linkall_core::history::History;
use linkall_core::protocol::ClipboardContent;
use std::time::Duration;
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_local_clip_survives_a_clip_from_another_device() {
    let tmp = TempDir::new().unwrap();
    let (mut a, b) = start_pair(&tmp, true, true).await;
    connect(&a, &b).await;

    // The host records a clip copied on this device.
    a.engine
        .history_store()
        .lock()
        .await
        .push_with_options(
            &ClipboardContent::Text("copied here".into()),
            "NodeA".into(),
            4096,
        )
        .unwrap();

    // Then a clip arrives from the other device and the engine saves it.
    b.engine
        .push_clipboard(ClipboardContent::Text("copied there".into()))
        .await;
    wait_for(&mut a.events, "the clip from B", |e| {
        matches!(e, EngineEvent::ClipboardReceived { .. })
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    let on_disk =
        History::load_with_limit(tmp.path().join("data0").join("history.json"), 500).unwrap();
    let texts: Vec<String> = on_disk
        .entries()
        .iter()
        .map(|e| serde_json::to_string(&e.payload).unwrap())
        .collect();
    assert!(texts.iter().any(|t| t.contains("copied here")), "{texts:?}");
    assert!(
        texts.iter().any(|t| t.contains("copied there")),
        "{texts:?}"
    );
}
