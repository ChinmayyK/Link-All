//! A phone asking a computer to press a key: the computer must be paired with
//! the phone, have remote control switched on, and be asked for one of the
//! allowed actions. None of these tests presses a real key.

mod common;

use common::{connect, start_pair, wait_for, Node};
use linkall_core::engine::EngineEvent;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;

/// Waits for the computer's answer and returns (success, error).
async fn answer(phone: &mut Node) -> (bool, Option<String>) {
    let got = std::cell::RefCell::new(None);
    wait_for(&mut phone.events, "remote control answer", |e| {
        if let EngineEvent::RemoteControlAckReceived { success, error, .. } = e {
            *got.borrow_mut() = Some((*success, error.clone()));
            true
        } else {
            false
        }
    })
    .await;
    got.into_inner().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_computer_refuses_while_remote_control_is_off() {
    let tmp = TempDir::new().unwrap();
    let (mut phone, desk) = start_pair(&tmp, true, true).await;
    connect(&phone, &desk).await;

    phone
        .engine
        .send_remote_control(desk.id, "mute")
        .await
        .expect("sent");
    let (success, error) = answer(&mut phone).await;
    assert!(!success);
    assert!(error.unwrap().contains("off"), "says it is switched off");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_action_outside_the_list_is_refused_even_when_it_is_on() {
    let tmp = TempDir::new().unwrap();
    let (mut phone, desk) = start_pair(&tmp, true, true).await;
    let mut settings = desk.engine.current_settings().await;
    settings.allow_remote_control = true;
    desk.engine.apply_settings(settings).await;
    connect(&phone, &desk).await;

    for bad in ["shutdown", "mute; reboot", "PLAY_PAUSE", ""] {
        phone
            .engine
            .send_remote_control(desk.id, bad)
            .await
            .expect("sent");
        let (success, error) = answer(&mut phone).await;
        assert!(!success, "{bad:?} must not run");
        assert!(error.unwrap().contains("isn't allowed"), "{bad:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_device_that_is_not_paired_gets_no_answer() {
    let tmp = TempDir::new().unwrap();
    // The phone trusts the computer, but the computer has never paired with the phone.
    let (mut phone, desk) = start_pair(&tmp, true, false).await;
    let mut settings = desk.engine.current_settings().await;
    settings.allow_remote_control = true;
    desk.engine.apply_settings(settings).await;
    connect(&phone, &desk).await;

    let _ = phone.engine.send_remote_control(desk.id, "mute").await;
    let nothing = timeout(Duration::from_secs(2), async {
        wait_for(&mut phone.events, "an answer", |e| {
            matches!(e, EngineEvent::RemoteControlAckReceived { .. })
        })
        .await
    })
    .await;
    assert!(
        nothing.is_err(),
        "a stranger gets no answer, not even a refusal"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sending_to_a_computer_that_is_not_connected_says_why() {
    let tmp = TempDir::new().unwrap();
    let (phone, desk) = start_pair(&tmp, true, true).await;
    // Never connected.
    let err = phone
        .engine
        .send_remote_control(desk.id, "mute")
        .await
        .unwrap_err();
    assert!(!err.is_empty());
}
