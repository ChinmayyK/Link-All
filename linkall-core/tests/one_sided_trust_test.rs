//! Regression: when only one side still trusts the other (one device
//! reinstalled or forgot the other), connecting must surface a pairing
//! prompt on the side that lost trust instead of a silent, half-working
//! session.

mod common;

use common::{start_pair, wait_for};
use linkall_core::engine::EngineEvent;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::time::timeout;

/// A (initiator) still trusts B, B forgot A: B must be asked to pair, and
/// accepting it must complete mutual trust on A.
#[tokio::test]
async fn responder_that_forgot_initiator_is_prompted() {
    let tmp = TempDir::new().unwrap();
    let (mut a, mut b) = start_pair(&tmp, true, false).await;
    let port_b = b.engine.bound_port().await;

    a.engine
        .connect_to_peer("127.0.0.1".into(), port_b)
        .await
        .expect("connect");

    let a_id = a.id;
    wait_for(
        &mut b.events,
        "PairingRequested on B",
        |e| matches!(e, EngineEvent::PairingRequested { device_id, .. } if *device_id == a_id),
    )
    .await;

    b.engine.respond_to_pairing(a.id, true).await.unwrap();

    let b_id = b.id;
    wait_for(&mut a.events, "PairingResponse on A", |e| {
        matches!(e, EngineEvent::PairingResponse { device_id, accepted: true } if *device_id == b_id)
    })
    .await;
}

/// A (initiator) forgot B, B still trusts A: A's own user must be asked.
#[tokio::test]
async fn initiator_that_forgot_responder_is_prompted() {
    let tmp = TempDir::new().unwrap();
    let (mut a, b) = start_pair(&tmp, false, true).await;
    let port_b = b.engine.bound_port().await;

    a.engine
        .connect_to_peer("127.0.0.1".into(), port_b)
        .await
        .expect("connect");

    let b_id = b.id;
    wait_for(
        &mut a.events,
        "PairingRequested on A",
        |e| matches!(e, EngineEvent::PairingRequested { device_id, .. } if *device_id == b_id),
    )
    .await;
}

/// Both devices dial each other at once, so two handshakes (two different
/// PINs) race and one session loses the tie-break. The code the requester
/// shows must be the one the approver is asked to compare.
#[tokio::test]
async fn simultaneous_dial_shows_same_pin_on_both_sides() {
    for _ in 0..5 {
        let tmp = TempDir::new().unwrap();
        let (a, mut b) = start_pair(&tmp, false, false).await;
        let port_a = a.engine.bound_port().await;
        let port_b = b.engine.bound_port().await;

        let (ra, rb) = tokio::join!(
            a.engine.connect_to_peer("127.0.0.1".into(), port_b),
            b.engine.connect_to_peer("127.0.0.1".into(), port_a),
        );
        ra.expect("a connect");
        rb.expect("b connect");
        // Let the losing session finish tearing down.
        tokio::time::sleep(Duration::from_millis(300)).await;

        a.engine.send_pairing_request(b.id).await;

        let a_id = a.id;
        let deadline = Instant::now() + Duration::from_secs(5);
        let b_pin = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match timeout(remaining, b.events.recv()).await {
                Ok(Some(EngineEvent::PairingRequested { device_id, pin, .. }))
                    if device_id == a_id =>
                {
                    break pin
                }
                Ok(Some(_)) => {}
                _ => panic!("timeout waiting for PairingRequested on B"),
            }
        };

        let a_pin = a
            .engine
            .status_snapshot()
            .await
            .peers
            .into_iter()
            .find(|p| p.id == b.id)
            .and_then(|p| p.pairing_pin);
        assert_eq!(
            a_pin.as_deref(),
            Some(b_pin.as_str()),
            "requester and approver show different codes"
        );
    }
}
