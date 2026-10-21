//! A computer asking a phone to ring. The phone side only exists on Android,
//! so on this host every receiver answers as a computer would: a refusal. None
//! of these tests makes a sound.

mod common;

use common::{connect, start_pair, wait_for};
use linkall_core::engine::EngineEvent;
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_computer_says_only_a_phone_can_ring() {
    let tmp = TempDir::new().unwrap();
    let (mut asker, other) = start_pair(&tmp, true, true).await;
    connect(&asker, &other).await;

    asker
        .engine
        .send_find_device(other.id, true)
        .await
        .expect("sent");

    let got = std::cell::RefCell::new(None);
    wait_for(&mut asker.events, "find answer", |e| {
        if let EngineEvent::FindDeviceAckReceived { success, error, .. } = e {
            *got.borrow_mut() = Some((*success, error.clone()));
            true
        } else {
            false
        }
    })
    .await;
    let (success, error) = got.into_inner().unwrap();
    assert!(!success);
    assert!(error.unwrap().contains("phone"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn asking_a_device_that_is_not_known_fails_without_sending() {
    let tmp = TempDir::new().unwrap();
    let (asker, _other) = start_pair(&tmp, true, true).await;
    let err = asker
        .engine
        .send_find_device(uuid::Uuid::new_v4(), true)
        .await
        .unwrap_err();
    assert!(err.contains("isn't known"));
}
