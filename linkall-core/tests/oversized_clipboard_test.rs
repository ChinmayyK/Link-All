//! A clipboard bigger than a device accepts is not sent. Sent anyway, a copy
//! over the 40 MB frame limit made the receiver drop the link, and catching
//! the device up on reconnect sent it again: the link dropped every 6 s.

mod common;

use common::{connect, start_pair};
use linkall_core::engine::EngineEvent;
use linkall_core::protocol::{ClipboardContent, MAX_TEXT_BYTES};
use std::time::Duration;
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_oversized_clipboard_is_not_sent_and_the_link_stays_up() {
    let tmp = TempDir::new().unwrap();
    let (a, mut b) = start_pair(&tmp, true, true).await;
    connect(&a, &b).await;

    let huge = ClipboardContent::Text("x".repeat(45 << 20));
    assert_eq!(a.engine.push_clipboard(huge).await, 0, "nothing is sent");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        tokio::select! {
            ev = b.events.recv() => match ev {
                Some(EngineEvent::PeerDisconnected { reason, .. }) => {
                    panic!("the link dropped: {reason:?}")
                }
                Some(_) => {}
                None => panic!("event channel closed"),
            },
            _ = tokio::time::sleep_until(deadline) => break,
        }
    }

    // A clip within the limit still goes through.
    let fits = ClipboardContent::Text("y".repeat(MAX_TEXT_BYTES));
    assert_eq!(a.engine.push_clipboard(fits).await, 1);
}
