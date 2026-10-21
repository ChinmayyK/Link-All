//! Scratchpad notes travel between paired devices and converge.

mod common;

use common::{connect, eventually, start_pair};
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_note_written_on_one_device_appears_on_the_other_and_deletes_there_too() {
    let tmp = TempDir::new().unwrap();
    let (a, b) = start_pair(&tmp, true, true).await;
    connect(&a, &b).await;

    let note = a
        .engine
        .scratchpad_save(None, "ssh ops@10.0.0.4", true)
        .await
        .unwrap();
    eventually("the note reaches the other device", async || {
        b.engine
            .scratchpad_list()
            .iter()
            .any(|n| n.id == note.id && n.text == "ssh ops@10.0.0.4" && n.pinned)
    })
    .await;

    // An edit on the other side comes back.
    b.engine
        .scratchpad_save(Some(note.id), "ssh ops@10.0.0.9", true)
        .await
        .unwrap();
    eventually("the edit reaches the first device", async || {
        a.engine
            .scratchpad_list()
            .iter()
            .any(|n| n.text == "ssh ops@10.0.0.9")
    })
    .await;

    a.engine.scratchpad_delete(note.id).await;
    eventually("the delete reaches the other device", async || {
        b.engine.scratchpad_list().is_empty()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn notes_made_while_apart_are_exchanged_on_connect() {
    let tmp = TempDir::new().unwrap();
    let (a, b) = start_pair(&tmp, true, true).await;
    a.engine
        .scratchpad_save(None, "from a", false)
        .await
        .unwrap();
    b.engine
        .scratchpad_save(None, "from b", false)
        .await
        .unwrap();
    connect(&a, &b).await;
    eventually("both devices hold both notes", async || {
        a.engine.scratchpad_list().len() == 2 && b.engine.scratchpad_list().len() == 2
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_untrusted_device_cannot_write_notes() {
    let tmp = TempDir::new().unwrap();
    let (a, b) = start_pair(&tmp, true, false).await;
    connect(&a, &b).await;
    a.engine
        .scratchpad_save(None, "private", false)
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert!(b.engine.scratchpad_list().is_empty());
}
