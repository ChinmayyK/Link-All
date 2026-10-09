//! A call a phone reported must not outlive the call: declining it here, or
//! the phone disconnecting, clears it even when the phone's "idle" never comes.

mod common;

use common::{connect, start_pair, Node};
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::{sleep, timeout};

/// Waits until `node` reports an active call (`true`) or none (`false`).
async fn call_shown(node: &Node, want: bool) {
    timeout(Duration::from_secs(10), async {
        while node.engine.active_call().await.is_some() != want {
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("call shown should be {want}"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn declining_clears_a_call_the_phone_never_ends() {
    let tmp = TempDir::new().unwrap();
    let (phone, desk) = start_pair(&tmp, true, true).await;
    connect(&phone, &desk).await;

    phone
        .engine
        .push_call_state("offhook".into(), "+10000000000".into(), "Mum".into())
        .await;
    call_shown(&desk, true).await;

    // The phone sends nothing more, as when its call had already ended.
    desk.engine
        .send_call_action("decline".into(), phone.id)
        .await;
    call_shown(&desk, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_disconnecting_phone_takes_its_call_with_it() {
    let tmp = TempDir::new().unwrap();
    let (phone, desk) = start_pair(&tmp, true, true).await;
    connect(&phone, &desk).await;

    phone
        .engine
        .push_call_state("ringing".into(), "+10000000000".into(), "Mum".into())
        .await;
    call_shown(&desk, true).await;

    desk.engine.disconnect_peer(phone.id).await.unwrap();
    call_shown(&desk, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_call_the_phone_stops_reporting_ends_by_itself() {
    let tmp = TempDir::new().unwrap();
    let (phone, desk) = start_pair(&tmp, true, true).await;
    connect(&phone, &desk).await;

    phone
        .engine
        .push_call_state("ringing".into(), "+10000000000".into(), "Mum".into())
        .await;
    call_shown(&desk, true).await;

    // Still connected, but the phone never says the call ended.
    let lease = linkall_core::engine::CALL_LEASE;
    timeout(lease + Duration::from_secs(10), async {
        while desk.engine.active_call().await.is_some() {
            sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .expect("call outlived its lease");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unpaired_device_cannot_answer_calls() {
    let tmp = TempDir::new().unwrap();
    let (mut phone, stranger) = start_pair(&tmp, false, false).await;
    connect(&stranger, &phone).await;

    stranger
        .engine
        .send_call_action("accept".into(), phone.id)
        .await;

    let got = timeout(Duration::from_secs(2), async {
        while let Some(event) = phone.events.recv().await {
            if matches!(
                event,
                linkall_core::engine::EngineEvent::CallActionRequest { .. }
            ) {
                return;
            }
        }
    })
    .await;
    assert!(
        got.is_err(),
        "a call action from an unpaired device must be ignored"
    );
}
