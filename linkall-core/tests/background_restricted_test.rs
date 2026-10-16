//! A phone tells computers whether its OS may pause Link All in the
//! background, so they can explain why it drops off while its screen is off.

mod common;

use common::{connect, eventually, peer, start_pair};
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_computer_learns_the_phone_is_restricted_when_the_phone_dials() {
    let tmp = TempDir::new().unwrap();
    let (phone, computer) = start_pair(&tmp, true, true).await;
    phone.engine.set_background_restricted(true);
    connect(&phone, &computer).await;
    eventually("the computer has the phone's setting", async || {
        peer(&computer, phone.id).await.background_restricted == Some(true)
    })
    .await;
    assert_eq!(
        peer(&phone, computer.id).await.background_restricted,
        None,
        "a computer that never said stays unknown"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_computer_learns_the_phone_is_allowed_when_it_dials_the_phone() {
    let tmp = TempDir::new().unwrap();
    let (phone, computer) = start_pair(&tmp, true, true).await;
    phone.engine.set_background_restricted(false);
    connect(&computer, &phone).await;
    eventually("the computer has the phone's setting", async || {
        peer(&computer, phone.id).await.background_restricted == Some(false)
    })
    .await;
}
