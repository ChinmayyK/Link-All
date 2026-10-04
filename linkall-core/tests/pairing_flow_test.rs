//! The pairing ceremony between two devices: declining, withdrawing,
//! both sides asking at once, and the security code a request shows
//! before and after its session exists.

mod common;

use common::{connect, start_pair, wait_for, Node};
use linkall_core::engine::EngineEvent;
use linkall_core::peer_manager::{PairingOutcome, PeerRecord};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::time::timeout;
use uuid::Uuid;

async fn peer(node: &Node, id: Uuid) -> PeerRecord {
    node.engine
        .status_snapshot()
        .await
        .peers
        .into_iter()
        .find(|p| p.id == id)
        .expect("peer in snapshot")
}

/// Declining tells the requester and closes both sides, but is "not now":
/// the declined device can still connect, and only a re-ask inside the
/// cooldown is answered automatically.
#[tokio::test]
async fn decline_closes_both_sides_without_blocking() {
    let tmp = TempDir::new().unwrap();
    let (mut a, mut b) = start_pair(&tmp, false, false).await;
    connect(&a, &b).await;
    let (a_id, b_id) = (a.id, b.id);

    a.engine.send_pairing_request(b_id).await;
    wait_for(
        &mut b.events,
        "request on B",
        |e| matches!(e, EngineEvent::PairingRequested { device_id, .. } if *device_id == a_id),
    )
    .await;

    b.engine.respond_to_pairing(a_id, false).await.unwrap();
    wait_for(&mut a.events, "decline on A", |e| {
        matches!(e, EngineEvent::PairingResponse { device_id, accepted: false } if *device_id == b_id)
    })
    .await;

    let on_a = peer(&a, b_id).await;
    assert!(!on_a.outgoing_pairing_waiting);
    assert_eq!(on_a.pairing_outcome, Some(PairingOutcome::Declined));
    assert!(!peer(&b, a_id).await.pairing_requested);

    // Not a block: the declined device can still reach B.
    tokio::time::sleep(Duration::from_millis(300)).await;
    connect(&a, &b).await;

    // Asking again straight away is declined for B's user, without a prompt.
    a.engine.send_pairing_request(b_id).await;
    wait_for(&mut a.events, "cooldown decline on A", |e| {
        matches!(e, EngineEvent::PairingResponse { device_id, accepted: false } if *device_id == b_id)
    })
    .await;
    let deadline = Instant::now() + Duration::from_millis(300);
    while let Ok(Some(e)) = timeout(
        deadline.saturating_duration_since(Instant::now()),
        b.events.recv(),
    )
    .await
    {
        assert!(
            !matches!(e, EngineEvent::PairingRequested { .. }),
            "B was prompted inside the decline cooldown"
        );
    }
}

/// Cancelling on the requesting device closes the prompt on the other one.
#[tokio::test]
async fn cancel_withdraws_the_prompt_on_the_other_device() {
    let tmp = TempDir::new().unwrap();
    let (a, mut b) = start_pair(&tmp, false, false).await;
    connect(&a, &b).await;
    let a_id = a.id;

    a.engine.send_pairing_request(b.id).await;
    wait_for(
        &mut b.events,
        "request on B",
        |e| matches!(e, EngineEvent::PairingRequested { device_id, .. } if *device_id == a_id),
    )
    .await;

    a.engine.cancel_pairing_request(b.id).await;
    wait_for(
        &mut b.events,
        "withdrawal on B",
        |e| matches!(e, EngineEvent::PairingChanged { device_id } if *device_id == a_id),
    )
    .await;

    let on_b = peer(&b, a_id).await;
    assert!(!on_b.pairing_requested);
    assert_eq!(on_b.pairing_outcome, Some(PairingOutcome::Cancelled));
    let on_a = peer(&a, b.id).await;
    assert!(!on_a.outgoing_pairing_waiting);
    assert_eq!(
        on_a.pairing_outcome, None,
        "a local cancel needs no message"
    );

    // A prompt left on screen can't be accepted after the request is gone.
    assert!(b.engine.respond_to_pairing(a_id, true).await.is_err());
    assert!(!b.engine.is_trusted(a_id).await);
}

/// Both users tap Pair on each other: one Accept pairs both devices and
/// leaves neither side waiting.
#[tokio::test]
async fn simultaneous_requests_settle_with_one_accept() {
    let tmp = TempDir::new().unwrap();
    let (mut a, mut b) = start_pair(&tmp, false, false).await;
    connect(&a, &b).await;
    let (a_id, b_id) = (a.id, b.id);

    tokio::join!(
        a.engine.send_pairing_request(b_id),
        b.engine.send_pairing_request(a_id),
    );
    wait_for(
        &mut b.events,
        "request on B",
        |e| matches!(e, EngineEvent::PairingRequested { device_id, .. } if *device_id == a_id),
    )
    .await;

    b.engine.respond_to_pairing(a_id, true).await.unwrap();
    wait_for(&mut a.events, "acceptance on A", |e| {
        matches!(e, EngineEvent::PairingResponse { device_id, accepted: true } if *device_id == b_id)
    })
    .await;

    assert!(a.engine.is_trusted(b_id).await);
    assert!(b.engine.is_trusted(a_id).await);
    for (node, other) in [(&a, b_id), (&b, a_id)] {
        let p = peer(node, other).await;
        assert!(
            !p.pairing_requested && !p.outgoing_pairing_waiting,
            "still pending: {p:?}"
        );
    }
}

/// A request sent before any session exists first shows a placeholder;
/// once the session is up the requester must be told the real code, the
/// same one the other device shows.
#[tokio::test]
async fn request_before_session_reports_the_real_code() {
    let tmp = TempDir::new().unwrap();
    let (mut a, mut b) = start_pair(&tmp, false, false).await;
    let port_b = b.engine.bound_port().await;
    let (a_id, b_id) = (a.id, b.id);

    a.engine
        .report_discovered_peer(b_id, "NodeB".into(), "127.0.0.1".into(), port_b)
        .await
        .unwrap();
    a.engine.send_pairing_request(b_id).await;

    let deadline = Instant::now() + Duration::from_secs(5);
    let a_pin = loop {
        match timeout(
            deadline.saturating_duration_since(Instant::now()),
            a.events.recv(),
        )
        .await
        {
            Ok(Some(EngineEvent::OutgoingPairingWaiting { device_id, pin, .. }))
                if device_id == b_id && pin != "------" =>
            {
                break pin
            }
            Ok(Some(_)) => {}
            _ => panic!("timeout waiting for the real code on A"),
        }
    };
    let b_pin = loop {
        match timeout(
            deadline.saturating_duration_since(Instant::now()),
            b.events.recv(),
        )
        .await
        {
            Ok(Some(EngineEvent::PairingRequested { device_id, pin, .. })) if device_id == a_id => {
                break pin
            }
            Ok(Some(_)) => {}
            _ => panic!("timeout waiting for the request on B"),
        }
    };
    assert_eq!(a_pin, b_pin, "requester and approver show different codes");

    let left = peer(&a, b_id)
        .await
        .pairing_expires_in_secs
        .expect("countdown");
    assert!((55..=60).contains(&left), "unexpected countdown {left}");
}

/// Nobody answers: after PAIRING_TIMEOUT both devices drop the request and
/// say why, and nothing is blocked. Takes a minute, so run on demand:
/// `cargo test --test pairing_flow_test -- --ignored`.
#[tokio::test]
#[ignore]
async fn unanswered_request_expires_on_both_devices() {
    let tmp = TempDir::new().unwrap();
    let (a, mut b) = start_pair(&tmp, false, false).await;
    connect(&a, &b).await;
    let (a_id, b_id) = (a.id, b.id);

    a.engine.send_pairing_request(b_id).await;
    wait_for(
        &mut b.events,
        "request on B",
        |e| matches!(e, EngineEvent::PairingRequested { device_id, .. } if *device_id == a_id),
    )
    .await;

    let timeout_secs = linkall_core::pairing::PAIRING_TIMEOUT.as_secs();
    tokio::time::sleep(Duration::from_secs(timeout_secs + 2)).await;

    let on_a = peer(&a, b_id).await;
    assert!(!on_a.outgoing_pairing_waiting);
    assert_eq!(on_a.pairing_outcome, Some(PairingOutcome::Expired));
    let on_b = peer(&b, a_id).await;
    assert!(!on_b.pairing_requested);
    // B's own clock or A's withdrawal, whichever landed first.
    assert!(matches!(
        on_b.pairing_outcome,
        Some(PairingOutcome::Expired | PairingOutcome::Cancelled)
    ));

    // Not blocked: asking again prompts B again.
    a.engine.send_pairing_request(b_id).await;
    wait_for(
        &mut b.events,
        "second request on B",
        |e| matches!(e, EngineEvent::PairingRequested { device_id, .. } if *device_id == a_id),
    )
    .await;
}

/// Clients also use Pair as "connect" on paired rows: that must not leave
/// an already-paired device waiting for an approval nobody will give.
#[tokio::test]
async fn pair_on_a_paired_device_does_not_open_a_request() {
    let tmp = TempDir::new().unwrap();
    let (a, b) = start_pair(&tmp, true, true).await;
    connect(&a, &b).await;

    a.engine.send_pairing_request(b.id).await;
    let on_a = peer(&a, b.id).await;
    assert!(!on_a.outgoing_pairing_waiting);
    assert!(!peer(&b, a.id).await.pairing_requested);
}

/// The approver trusts us but its answer never arrived (it went out on a
/// session that was being replaced). The requester must not wait forever:
/// its next re-ask is auto-accepted and pairing completes on its own.
#[tokio::test]
async fn lost_acceptance_heals_by_re_asking() {
    let tmp = TempDir::new().unwrap();
    let (mut a, mut b) = start_pair(&tmp, false, false).await;
    connect(&a, &b).await;
    let (a_id, b_id) = (a.id, b.id);

    a.engine.send_pairing_request(b_id).await;
    wait_for(
        &mut b.events,
        "request on B",
        |e| matches!(e, EngineEvent::PairingRequested { device_id, .. } if *device_id == a_id),
    )
    .await;

    // B trusts A without its answer reaching A.
    b.engine.trust_peer(a_id).await.unwrap();
    assert!(peer(&a, b_id).await.outgoing_pairing_waiting);

    let resend = linkall_core::pairing::PAIRING_RESEND;
    let deadline = Instant::now() + resend + Duration::from_secs(3);
    loop {
        match timeout(
            deadline.saturating_duration_since(Instant::now()),
            a.events.recv(),
        )
        .await
        {
            Ok(Some(EngineEvent::PairingResponse {
                device_id,
                accepted: true,
            })) if device_id == b_id => break,
            Ok(Some(_)) => {}
            _ => panic!("A never learned that B accepted"),
        }
    }
    assert!(a.engine.is_trusted(b_id).await);
    assert!(!peer(&a, b_id).await.outgoing_pairing_waiting);
    assert!(
        !peer(&b, a_id).await.pairing_requested,
        "B's stale prompt stayed up"
    );
}

/// QR pairing: the scanner proves it saw the code on screen. A wrong code
/// (stale QR, another device) must not burn the real one, and the right
/// one leaves both devices trusting each other.
#[tokio::test]
async fn qr_code_pairs_both_ways_and_survives_a_bad_scan() {
    let tmp = TempDir::new().unwrap();
    let (a, b) = start_pair(&tmp, false, false).await;
    connect(&a, &b).await;
    let (a_id, b_id) = (a.id, b.id);

    let token = b.engine.generate_qr_token().await;
    a.engine.send_qr_auth(b_id, "not-the-code".into()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!b.engine.is_trusted(a_id).await);

    // What the scanning device does: trust what it scanned, then prove it.
    a.engine.trust_peer(b_id).await.unwrap();
    a.engine.send_qr_auth(b_id, token).await;
    let deadline = Instant::now() + Duration::from_secs(3);
    while !b.engine.is_trusted(a_id).await {
        assert!(Instant::now() < deadline, "B never trusted the scanner");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(a.engine.is_trusted(b_id).await);
}

/// B forgets A. A's user taps Pair later: B must let A in and ask its user,
/// not refuse every connection (A's user would only ever see "no answer").
/// B's automatic reconnects stay off until then.
#[tokio::test]
async fn forgotten_device_can_ask_to_pair_again() {
    let tmp = TempDir::new().unwrap();
    let (mut a, mut b) = start_pair(&tmp, true, true).await;
    connect(&a, &b).await;
    let (a_id, b_id) = (a.id, b.id);

    assert!(b.engine.forget_device(a_id).await.unwrap());
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!b.engine.is_trusted(a_id).await);

    a.engine.send_pairing_request(b_id).await;
    wait_for(
        &mut b.events,
        "request on B after forget",
        |e| matches!(e, EngineEvent::PairingRequested { device_id, .. } if *device_id == a_id),
    )
    .await;

    b.engine.respond_to_pairing(a_id, true).await.unwrap();
    wait_for(&mut a.events, "acceptance on A", |e| {
        matches!(e, EngineEvent::PairingResponse { device_id, accepted: true } if *device_id == b_id)
    })
    .await;
    assert!(a.engine.is_trusted(b_id).await && b.engine.is_trusted(a_id).await);
}

/// B's app was reinstalled: same name, new identity. Pairing the new B
/// must replace the old B on A instead of listing the device twice.
#[tokio::test]
async fn pairing_a_reinstalled_device_replaces_its_old_entry() {
    let tmp = TempDir::new().unwrap();
    let (mut a, b) = start_pair(&tmp, false, false).await;
    let old_b = Uuid::new_v4();
    a.engine
        .approve_device(old_b, "NodeB".into(), vec![7u8; 32])
        .await
        .unwrap();
    connect(&a, &b).await;
    let (a_id, b_id) = (a.id, b.id);
    // Both ends of the handshake learn each other's version and OS.
    let current = Some(linkall_core::protocol::APP_VERSION.to_string());
    assert_eq!(peer(&a, b_id).await.app_version, current);
    assert_eq!(peer(&b, a_id).await.app_version, current);
    let os = Some(linkall_core::network::MY_PLATFORM.to_string());
    assert_eq!(peer(&a, b_id).await.platform, os);
    assert_eq!(peer(&b, a_id).await.platform, os);

    b.engine.send_pairing_request(a_id).await;
    wait_for(
        &mut a.events,
        "request on A",
        |e| matches!(e, EngineEvent::PairingRequested { device_id, .. } if *device_id == b_id),
    )
    .await;
    a.engine.respond_to_pairing(b_id, true).await.unwrap();

    let trusted: Vec<Uuid> = a
        .engine
        .trusted_devices()
        .await
        .into_iter()
        .map(|r| r.device_id)
        .collect();
    assert_eq!(trusted, vec![b_id]);
}
