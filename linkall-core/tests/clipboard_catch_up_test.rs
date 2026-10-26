//! Clipboard copied while a device was away must reach it when it comes
//! back, unless that device copied something newer in the meantime.

mod common;

use common::{connected, eventually, start_node, trusts, twin_config, Node};
use linkall_core::engine::EngineEvent;
use linkall_core::protocol::ClipboardContent;
use std::time::Duration;
use tempfile::TempDir;
use uuid::Uuid;

struct Pair {
    tmp: TempDir,
    phone_id: Uuid,
    computer: Node,
}

/// A computer and a phone that paired, talked, and then lost each other
/// because the phone's app died (no goodbye).
async fn pair_then_lose_the_phone() -> Pair {
    let tmp = TempDir::new().unwrap();
    let (phone_id, computer_id) = (Uuid::new_v4(), Uuid::new_v4());
    trusts(&tmp, "phone", computer_id, "Computer", "computer");
    trusts(&tmp, "computer", phone_id, "Phone", "phone");
    let computer = start_node(twin_config(&tmp, "computer", computer_id, "Computer")).await;
    let port = computer.engine.bound_port().await;

    let cfg = twin_config(&tmp, "phone", phone_id, "Phone");
    let (rt, phone) = std::thread::spawn(move || {
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
    rt.spawn(async move { engine.connect_to_peer("127.0.0.1".into(), port).await })
        .await
        .unwrap()
        .expect("the phone dials the computer");
    eventually("the computer sees the phone", async || {
        connected(&computer, phone_id).await
    })
    .await;

    drop(phone);
    std::thread::spawn(move || rt.shutdown_timeout(Duration::from_secs(2)))
        .join()
        .unwrap();
    eventually("the computer notices the phone is gone", async || {
        !connected(&computer, phone_id).await
    })
    .await;
    Pair {
        tmp,
        phone_id,
        computer,
    }
}

/// The phone's app starts again (same stores) and reaches the computer.
async fn bring_the_phone_back(pair: &Pair) -> Node {
    let phone = start_node(twin_config(&pair.tmp, "phone", pair.phone_id, "Phone")).await;
    let port = pair.computer.engine.bound_port().await;
    phone
        .engine
        .connect_to_peer("127.0.0.1".into(), port)
        .await
        .expect("the phone dials the computer again");
    phone
}

async fn next_text(node: &mut Node, within: Duration) -> Option<String> {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        let event = tokio::time::timeout_at(deadline, node.events.recv())
            .await
            .ok()??;
        if let EngineEvent::ClipboardReceived { content, .. } = event {
            if let ClipboardContent::Text(text) = content.as_ref() {
                return Some(text.clone());
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_copy_made_while_the_phone_was_away_reaches_it_on_return() {
    let pair = pair_then_lose_the_phone().await;
    pair.computer
        .engine
        .push_clipboard(ClipboardContent::Text("copied while you were away".into()))
        .await;

    let mut phone = bring_the_phone_back(&pair).await;
    assert_eq!(
        next_text(&mut phone, Duration::from_secs(8))
            .await
            .as_deref(),
        Some("copied while you were away")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_newer_copy_wins_when_both_copied_while_apart() {
    let mut pair = pair_then_lose_the_phone().await;
    pair.computer
        .engine
        .push_clipboard(ClipboardContent::Text("older, from the computer".into()))
        .await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    // The phone copies something newer before it finds the computer again.
    let mut phone = start_node(twin_config(&pair.tmp, "phone", pair.phone_id, "Phone")).await;
    phone
        .engine
        .push_clipboard(ClipboardContent::Text("newer, from the phone".into()))
        .await;
    phone
        .engine
        .connect_to_peer("127.0.0.1".into(), pair.computer.engine.bound_port().await)
        .await
        .expect("the phone dials the computer again");

    assert_eq!(
        next_text(&mut pair.computer, Duration::from_secs(8))
            .await
            .as_deref(),
        Some("newer, from the phone")
    );
    assert_eq!(
        next_text(&mut phone, Duration::from_secs(3)).await,
        None,
        "the phone keeps its newer copy"
    );
}

/// Copying A, then B, then A again on one device quickly must leave A on the
/// other: the second A is a new copy, not a repeat delivery.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copying_back_to_an_earlier_clip_reaches_the_other_device() {
    let tmp = TempDir::new().unwrap();
    let (mut phone, mut computer) = common::start_pair(&tmp, true, true).await;
    common::connect(&phone, &computer).await;
    eventually("both linked", async || {
        connected(&phone, computer.id).await && connected(&computer, phone.id).await
    })
    .await;
    while phone.events.try_recv().is_ok() {}

    let mut received = Vec::new();
    for text in ["first", "second", "first"] {
        phone
            .engine
            .push_clipboard(ClipboardContent::Text(text.into()))
            .await;
        let got = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(EngineEvent::ClipboardReceived { content, .. }) =
                    computer.events.recv().await
                {
                    if let ClipboardContent::Text(t) = content.as_ref() {
                        return t.clone();
                    }
                }
            }
        })
        .await;
        received.push(got.unwrap_or_else(|_| format!("<nothing for {text}>")));
    }
    assert_eq!(received, ["first", "second", "first"]);
}
