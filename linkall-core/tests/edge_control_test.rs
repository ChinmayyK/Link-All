//! One keyboard and mouse across two computers, end to end between two real
//! engines. A fake screen stands in for the operating system on the receiving
//! side, so nothing here moves this computer's pointer or presses a key.

mod common;

use common::{connect, start_pair, wait_for};
use linkall_core::engine::EngineEvent;
use linkall_core::protocol::AppMessage;
use linkall_core::remote_input::testing::{self, Backend};
use linkall_core::remote_input::Button;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;

const WIDTH: i32 = 1920;
const HEIGHT: i32 = 1080;

/// A 1920x1080 screen that records what was played on it.
#[derive(Default)]
struct FakeScreen {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    x: i32,
    y: i32,
    keys_down: Vec<u16>,
    buttons_down: Vec<Button>,
    log: Vec<String>,
}

impl FakeScreen {
    fn cursor_now(&self) -> (i32, i32) {
        let s = self.state.lock().unwrap();
        (s.x, s.y)
    }
    fn log(&self) -> Vec<String> {
        self.state.lock().unwrap().log.clone()
    }
    fn held(&self) -> (Vec<u16>, Vec<Button>) {
        let s = self.state.lock().unwrap();
        (s.keys_down.clone(), s.buttons_down.clone())
    }
}

impl Backend for FakeScreen {
    fn screen_size(&self) -> Option<(i32, i32)> {
        Some((WIDTH, HEIGHT))
    }
    fn cursor(&self) -> Option<(i32, i32)> {
        Some(self.cursor_now())
    }
    fn set_cursor(&self, x: i32, y: i32) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        s.x = x;
        s.y = y;
        Ok(())
    }
    fn pointer_move(&self, dx: i32, dy: i32) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        s.x = (s.x + dx).clamp(0, WIDTH - 1);
        s.y = (s.y + dy).clamp(0, HEIGHT - 1);
        Ok(())
    }
    fn button(&self, button: Button, down: bool) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        s.buttons_down.retain(|b| *b != button);
        if down {
            s.buttons_down.push(button);
        }
        s.log.push(format!(
            "button {button:?} {}",
            if down { "down" } else { "up" }
        ));
        Ok(())
    }
    fn key(&self, scancode: u16, down: bool) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        s.keys_down.retain(|k| *k != scancode);
        if down {
            s.keys_down.push(scancode);
        }
        s.log.push(format!(
            "key {scancode:#x} {}",
            if down { "down" } else { "up" }
        ));
        Ok(())
    }
}

async fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..100 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}");
}

/// One test, in phases: the fake screen is process-wide, so tests that share
/// it must not run side by side.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_hand_off_runs_from_start_to_finish() {
    let tmp = TempDir::new().unwrap();
    let (mut host, desk) = start_pair(&tmp, true, true).await;
    connect(&host, &desk).await;

    let screen = Arc::new(FakeScreen::default());
    testing::install(screen.clone());

    // ── Refused while Remote control is off on the receiving computer ──
    let refused = host.engine.edge_begin(desk.id, 1, 32768).await;
    assert!(
        refused.as_ref().is_err_and(|e| e.contains("off")),
        "refusal says Remote control is off: {refused:?}"
    );
    assert!(
        host.engine
            .edge_input(AppMessage::RemotePointerMove { dx: 1, dy: 0 })
            .await
            .is_err(),
        "nothing is sent when the hand-off did not start"
    );

    let mut settings = desk.engine.current_settings().await;
    settings.allow_remote_control = true;
    desk.engine.apply_settings(settings).await;

    // ── Start: left the host over its right edge, halfway down ──
    host.engine
        .edge_begin(desk.id, 1, 32768)
        .await
        .expect("taken");
    let (x, y) = screen.cursor_now();
    assert_eq!(x, 0, "arrives at the left edge");
    assert!((538..=541).contains(&y), "halfway down, was {y}");

    // A second start while one is running is refused.
    assert!(host.engine.edge_begin(desk.id, 1, 0).await.is_err());

    // ── Mouse and keyboard drive the receiver ──
    host.engine
        .edge_input(AppMessage::RemotePointerMove { dx: 100, dy: 20 })
        .await
        .unwrap();
    eventually("the pointer to move", || screen.cursor_now().0 == 100).await;
    assert_eq!(screen.cursor_now().1, y + 20);

    host.engine
        .edge_input(AppMessage::RemoteKeyEvent {
            scancode: 0x1E,
            down: true,
        })
        .await
        .unwrap();
    host.engine
        .edge_input(AppMessage::RemotePointerButton {
            button: 0,
            down: true,
        })
        .await
        .unwrap();
    eventually("the key and button", || {
        let (keys, buttons) = screen.held();
        keys == [0x1E] && buttons == [Button::Left]
    })
    .await;

    // ── Pushing out the side it came in by hands control back ──
    // Down to 20 px from the edge, then onto it with a little to spare: not yet.
    for dx in [-80, -30] {
        host.engine
            .edge_input(AppMessage::RemotePointerMove { dx, dy: 0 })
            .await
            .unwrap();
    }
    eventually("the pointer to reach the edge", || {
        screen.cursor_now().0 == 0
    })
    .await;
    // Pushing on against it is.
    host.engine
        .edge_input(AppMessage::RemotePointerMove { dx: -30, dy: 0 })
        .await
        .unwrap();
    let returned = std::cell::Cell::new(None);
    wait_for(&mut host.events, "the pointer to come back", |e| {
        if let EngineEvent::EdgeReturned { pos } = e {
            returned.set(Some(*pos));
            true
        } else {
            false
        }
    })
    .await;
    let pos = returned.get().unwrap();
    assert!(
        (30000..=36000).contains(&pos),
        "came back near the middle, was {pos}"
    );
    assert_eq!(screen.held(), (vec![], vec![]), "nothing is left held down");
    assert!(
        host.engine
            .edge_input(AppMessage::RemotePointerMove { dx: 1, dy: 0 })
            .await
            .is_err(),
        "the host has its input back"
    );

    // ── A brush against the edge is not a return ──
    host.engine
        .edge_begin(desk.id, 1, 0)
        .await
        .expect("taken again");
    host.engine
        .edge_input(AppMessage::RemotePointerMove { dx: -10, dy: 0 })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        host.engine
            .edge_input(AppMessage::RemotePointerMove { dx: 40, dy: 0 })
            .await
            .is_ok(),
        "a small push does not end it"
    );

    // ── Ending it from the host lets go of whatever is held ──
    host.engine
        .edge_input(AppMessage::RemoteKeyEvent {
            scancode: 0x2A,
            down: true,
        })
        .await
        .unwrap();
    eventually("the key", || screen.held().0 == [0x2A]).await;
    host.engine.edge_end().await;
    eventually("the key to be released", || screen.held().0.is_empty()).await;
    assert!(screen.log().iter().any(|l| l == "key 0x2a up"));

    // ── Keys from a computer that is not driving are ignored ──
    // (nothing is driving now)
    let before = screen.log().len();
    let _ = desk
        .engine
        .edge_input(AppMessage::RemoteKeyEvent {
            scancode: 0x1F,
            down: true,
        })
        .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(screen.log().len(), before);

    testing::uninstall();
}
