//! The X11 capture loop against a real X server (Xvfb in the Docker test
//! image). Skipped when there is no `DISPLAY` or no `xdotool`.
#![cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]

use linkall_core::edge_x11::{spawn, Batch, Control, Host, Status};
use std::process::Command;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Recorded {
    begun: Vec<(u8, u16)>,
    batches: Vec<Batch>,
    ended: usize,
    status: Status,
}

struct Recorder(Arc<Mutex<Recorded>>);

impl Host for Recorder {
    fn begin(&mut self, edge: u8, pos: u16) -> Result<(), String> {
        self.0.lock().unwrap().begun.push((edge, pos));
        Ok(())
    }
    fn input(&mut self, batch: &Batch) -> Result<(), String> {
        self.0.lock().unwrap().batches.push(batch.clone());
        Ok(())
    }
    fn end(&mut self) {
        self.0.lock().unwrap().ended += 1;
    }
    fn status(&mut self) -> Result<Status, String> {
        Ok(std::mem::take(&mut self.0.lock().unwrap().status))
    }
}

fn xdotool(args: &[&str]) {
    let ok = Command::new("xdotool").args(args).status().map(|s| s.success()).unwrap_or(false);
    assert!(ok, "xdotool {args:?}");
}

fn wait(what: &str, mut check: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        if check() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn pushing_a_linked_edge_hands_the_keyboard_and_mouse_over_and_back() {
    if std::env::var_os("DISPLAY").is_none() || Command::new("xdotool").arg("version").output().is_err() {
        eprintln!("no X server or xdotool: skipping");
        return;
    }
    let rec = Arc::new(Mutex::new(Recorded::default()));
    let control = Arc::new(Control::default());
    control.enabled.store(true, Ordering::Relaxed);
    control.linked.lock().unwrap()[1] = true; // a computer to the right
    let handle = spawn(Box::new(Recorder(rec.clone())), control.clone());

    // Away from the edge nothing happens.
    xdotool(&["mousemove", "400", "300"]);
    std::thread::sleep(Duration::from_millis(300));
    assert!(rec.lock().unwrap().begun.is_empty());

    // Pushing against the right edge starts it, halfway down or so.
    for _ in 0..12 {
        xdotool(&["mousemove", "5000", "300"]);
        std::thread::sleep(Duration::from_millis(30));
    }
    wait("the hand-off to begin", || !rec.lock().unwrap().begun.is_empty());
    let (edge, pos) = rec.lock().unwrap().begun[0];
    assert_eq!(edge, 1);
    assert!(pos > 0 && pos < 65535, "pos {pos}");

    // Moving and typing reach the other side.
    xdotool(&["mousemove_relative", "--", "30", "10"]);
    xdotool(&["key", "a"]);
    xdotool(&["click", "1"]);
    let seen = |rec: &Arc<Mutex<Recorded>>| {
        let r = rec.lock().unwrap();
        let moved = r.batches.iter().any(|b| b.dx != 0 || b.dy != 0);
        let key = r.batches.iter().any(|b| b.keys.contains(&(0x1E, true)))
            && r.batches.iter().any(|b| b.keys.contains(&(0x1E, false)));
        let click = r.batches.iter().any(|b| b.buttons.contains(&(0, true)));
        (moved, key, click)
    };
    let until = Instant::now() + Duration::from_secs(4);
    while Instant::now() < until && seen(&rec) != (true, true, true) {
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(
        seen(&rec),
        (true, true, true),
        "(moved, key, click) - batches: {:?}",
        rec.lock().unwrap().batches
    );

    // The other side hands it back: the pointer is placed at the edge again.
    rec.lock().unwrap().status = Status { returned_pos: Some(32768), ended_reason: None };
    std::thread::sleep(Duration::from_millis(600));

    // Pushing again after the cooldown starts a second hand-off, which
    // Ctrl+Alt+Esc ends from the keyboard alone.
    for _ in 0..12 {
        xdotool(&["mousemove", "5000", "300"]);
        std::thread::sleep(Duration::from_millis(30));
    }
    wait("a second hand-off", || rec.lock().unwrap().begun.len() == 2);
    xdotool(&["key", "ctrl+alt+Escape"]);
    wait("the escape to end it", || rec.lock().unwrap().ended >= 1);
    assert!(
        control.last_note.lock().unwrap().as_deref().is_some_and(|n| n.contains("Ctrl+Alt+Esc")),
        "the user is told why"
    );

    control.stop.store(true, Ordering::Relaxed);
    handle.join().unwrap();
}
