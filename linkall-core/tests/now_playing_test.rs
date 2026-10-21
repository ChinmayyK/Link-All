//! What one device plays shows up on a paired one, with its cover art, and clears when it stops.

mod common;

use common::{connect, eventually, start_pair};
use linkall_core::now_playing::NowPlaying;
use tempfile::TempDir;

fn track(title: &str, playing: bool, position_ms: u64) -> NowPlaying {
    NowPlaying {
        title: title.into(),
        artist: "Marconi Union".into(),
        album: "Weightless".into(),
        playing,
        position_ms,
        duration_ms: 480_000,
        app: "Spotify".into(),
        art: Some(vec![1, 2, 3, 4]),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_track_reaches_the_other_device_with_its_cover_and_clears_when_it_stops() {
    let tmp = TempDir::new().unwrap();
    let (a, b) = start_pair(&tmp, true, true).await;
    connect(&a, &b).await;

    a.engine
        .set_now_playing(Some(track("Weightless", true, 1_000)));
    eventually("the track reaches the other device", async || {
        b.engine
            .now_playing_list()
            .iter()
            .any(|n| n.title == "Weightless" && n.playing && n.art_base64.is_some())
    })
    .await;
    assert_eq!(b.engine.now_playing_art(a.id), Some(vec![1, 2, 3, 4]));
    // Without covers, as the apps poll: the key stays so they know when to fetch it.
    let light = b.engine.now_playing_list_with(false);
    assert!(light[0].art_base64.is_none());
    assert_eq!(light[0].art_key, b.engine.now_playing_list()[0].art_key);
    assert!(light[0].art_key.is_some());

    // A pause is a change and goes out at once; the cover is kept.
    a.engine
        .set_now_playing(Some(track("Weightless", false, 5_000)));
    eventually("the pause reaches the other device", async || {
        b.engine
            .now_playing_list()
            .iter()
            .any(|n| !n.playing && n.position_ms == 5_000 && n.art_base64.is_some())
    })
    .await;

    a.engine.set_now_playing(None);
    eventually("it clears when playing stops", async || {
        b.engine.now_playing_list().is_empty()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn what_was_playing_before_a_device_connected_is_sent_on_connect() {
    let tmp = TempDir::new().unwrap();
    let (a, b) = start_pair(&tmp, true, true).await;
    a.engine.set_now_playing(Some(track("Already on", true, 0)));
    connect(&a, &b).await;
    eventually("it arrives on connect", async || {
        b.engine
            .now_playing_list()
            .iter()
            .any(|n| n.title == "Already on")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_untrusted_device_cannot_show_a_track() {
    let tmp = TempDir::new().unwrap();
    let (a, b) = start_pair(&tmp, true, false).await;
    connect(&a, &b).await;
    a.engine.set_now_playing(Some(track("Secret", true, 0)));
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert!(b.engine.now_playing_list().is_empty());
}

#[test]
fn now_playing_list_keeps_covers_unless_asked_not_to() {
    use linkall_core::ipc::IpcRequest;
    let plain: IpcRequest = serde_json::from_str(r#"{"cmd":"now_playing_list"}"#).unwrap();
    assert!(matches!(plain, IpcRequest::NowPlayingList { art: true }));
    let light: IpcRequest =
        serde_json::from_str(r#"{"cmd":"now_playing_list","art":false}"#).unwrap();
    assert!(matches!(light, IpcRequest::NowPlayingList { art: false }));
}
