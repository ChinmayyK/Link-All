//! A connected but unpaired device must not put text in front of the user.

mod common;

use common::{connect, start_pair};
use linkall_core::engine::EngineEvent;
use linkall_core::protocol::AppMessage;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unpaired_device_cannot_show_warnings() {
    let tmp = TempDir::new().unwrap();
    let (mut desk, stranger) = start_pair(&tmp, false, false).await;
    connect(&stranger, &desk).await;

    stranger
        .engine
        .send_message(
            desk.id,
            AppMessage::PermissionError {
                feature: "Calls".into(),
                message: "Your phone is compromised".into(),
                origin_device: stranger.id,
                origin_device_name: "stranger".into(),
            },
        )
        .await
        .expect("send");

    let got = timeout(Duration::from_secs(2), async {
        while let Some(event) = desk.events.recv().await {
            if matches!(&event, EngineEvent::Warning(text) if text.contains("compromised")) {
                return;
            }
        }
    })
    .await;
    assert!(
        got.is_err(),
        "a warning from an unpaired device must be ignored"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn qr_pairing_checks_the_computers_key() {
    let tmp = TempDir::new().unwrap();
    let (phone, desk) = start_pair(&tmp, false, false).await;
    connect(&phone, &desk).await;

    let token = desk.engine.generate_qr_token().await;
    let wrong_key = "ab".repeat(32);
    assert!(
        phone
            .engine
            .trust_peer_from_qr(desk.id, token.clone(), Some(&wrong_key))
            .await
            .is_err(),
        "a peer whose key differs from the QR code's must not be trusted"
    );

    let shown_key = desk.engine.local_fingerprint();
    phone
        .engine
        .trust_peer_from_qr(desk.id, token, Some(&shown_key))
        .await
        .expect("the key the computer shows must match the one it presents");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn disabling_auto_accept_prompts_for_incoming_file() {
    let tmp = TempDir::new().unwrap();
    let (mut receiver, sender) = start_pair(&tmp, true, true).await;
    connect(&sender, &receiver).await;

    // Disable auto-accept on the receiver
    receiver
        .engine
        .patch_settings(r#"{"auto_accept_file_transfers": false}"#.to_string())
        .await
        .unwrap();

    sender
        .engine
        .send_file(
            b"hello world".to_vec(),
            "test.txt".to_string(),
            "text/plain".to_string(),
            Some(receiver.id),
        )
        .await
        .unwrap();

    let got = timeout(Duration::from_secs(3), async {
        while let Some(event) = receiver.events.recv().await {
            if let EngineEvent::FileTransferIncoming { file_name, .. } = event {
                if file_name == "test.txt" {
                    return true;
                }
            }
        }
        false
    })
    .await;

    assert!(
        matches!(got, Ok(true)),
        "receiver must prompt via FileTransferIncoming when auto-accept is false"
    );
}

