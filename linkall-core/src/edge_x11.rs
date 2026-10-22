//! Sharing this X11 computer's keyboard and mouse with the computer next to
//! it. The pointer is watched until it is pushed against a linked screen edge;
//! then the other computer is asked to take over, the pointer and keyboard are
//! grabbed so nothing else sees them, and what the user does is sent on in
//! batches until the pointer comes back, the user presses Ctrl+Alt+Esc, or the
//! other side lets go.
//!
//! The talking to the other computer is the [`Host`] trait: the Linux app
//! implements it over the daemon's socket, a test with a recorder. Wayland
//! sessions have no X server to grab and are refused up front.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use x11_dl::xlib::{self, Xlib};

/// How the capture loop reaches the other computer.
pub trait Host: Send + 'static {
    /// The pointer left by `edge` (0 left, 1 right, 2 top, 3 bottom), `pos`
    /// of the way along it. Ok when the other computer took over.
    fn begin(&mut self, edge: u8, pos: u16) -> Result<(), String>;
    /// One batch of input, in the order it happened.
    fn input(&mut self, batch: &Batch) -> Result<(), String>;
    /// Takes the keyboard and mouse back.
    fn end(&mut self);
    /// What happened to the hand-off since the last call.
    fn status(&mut self) -> Result<Status, String>;
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Batch {
    pub dx: i32,
    pub dy: i32,
    pub scroll_x: i32,
    pub scroll_y: i32,
    pub buttons: Vec<(u8, bool)>,
    pub keys: Vec<(u16, bool)>,
}

impl Batch {
    fn is_empty(&self) -> bool {
        self.dx == 0
            && self.dy == 0
            && self.scroll_x == 0
            && self.scroll_y == 0
            && self.buttons.is_empty()
            && self.keys.is_empty()
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Status {
    /// The pointer came back this far along the edge it left by (0..=65535).
    pub returned_pos: Option<u16>,
    /// The hand-off ended without the pointer coming back, and why.
    pub ended_reason: Option<String>,
}

/// What the user has set, shared with the running loop.
#[derive(Default)]
pub struct Control {
    pub enabled: AtomicBool,
    /// Which edges (0 left, 1 right, 2 top, 3 bottom) have a computer linked.
    pub linked: Mutex<[bool; 4]>,
    pub stop: AtomicBool,
    /// Why the last hand-off ended, for the app to show.
    pub last_note: Mutex<Option<String>>,
}

const POLL: Duration = Duration::from_millis(25);
const EDGE_HITS: u32 = 4;
const EDGE_DWELL: Duration = Duration::from_millis(120);
const COOLDOWN: Duration = Duration::from_millis(800);
const BATCH_EVERY: Duration = Duration::from_millis(8);
const STATUS_EVERY: Duration = Duration::from_millis(100);

/// One wheel notch as the other end counts scrolling.
const NOTCH: i32 = 20;

/// Starts the watcher on its own thread. Returns at once; an X11 display that
/// cannot be opened is reported through `Control::last_note` and the thread
/// ends.
pub fn spawn(host: Box<dyn Host>, control: Arc<Control>) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("linkall-edge-x11".into())
        .spawn(move || {
            if let Err(e) = run(host, &control) {
                *control.last_note.lock().unwrap() = Some(e);
            }
        })
        .expect("edge thread")
}

struct Display {
    xlib: Xlib,
    display: *mut xlib::Display,
    root: xlib::Window,
    width: i32,
    height: i32,
}

impl Display {
    fn open() -> Result<Display, String> {
        let xlib = Xlib::open().map_err(|_| "this computer has no X11 library".to_string())?;
        let display = unsafe { (xlib.XOpenDisplay)(std::ptr::null()) };
        if display.is_null() {
            return Err(
                "no X11 display: Wayland sessions can't share a keyboard and mouse yet".into(),
            );
        }
        unsafe {
            let screen = (xlib.XDefaultScreen)(display);
            Ok(Display {
                root: (xlib.XDefaultRootWindow)(display),
                width: (xlib.XDisplayWidth)(display, screen),
                height: (xlib.XDisplayHeight)(display, screen),
                xlib,
                display,
            })
        }
    }

    /// (x, y, button mask) of the pointer.
    fn pointer(&self) -> Option<(i32, i32, u32)> {
        unsafe {
            let (mut root_ret, mut child_ret) = (0, 0);
            let (mut rx, mut ry, mut wx, mut wy) = (0, 0, 0, 0);
            let mut mask = 0;
            let ok = (self.xlib.XQueryPointer)(
                self.display,
                self.root,
                &mut root_ret,
                &mut child_ret,
                &mut rx,
                &mut ry,
                &mut wx,
                &mut wy,
                &mut mask,
            );
            (ok != 0).then_some((rx, ry, mask))
        }
    }

    fn warp(&self, x: i32, y: i32) {
        unsafe {
            (self.xlib.XWarpPointer)(self.display, 0, self.root, 0, 0, 0, 0, x, y);
            (self.xlib.XFlush)(self.display);
        }
    }

    /// A cursor that draws nothing, so the frozen pointer is not left on screen.
    fn blank_cursor(&self) -> xlib::Cursor {
        unsafe {
            let data = [0u8; 1];
            let pixmap = (self.xlib.XCreateBitmapFromData)(
                self.display,
                self.root,
                data.as_ptr().cast(),
                1,
                1,
            );
            let mut color: xlib::XColor = std::mem::zeroed();
            let cursor = (self.xlib.XCreatePixmapCursor)(
                self.display,
                pixmap,
                pixmap,
                &mut color,
                &mut color,
                0,
                0,
            );
            (self.xlib.XFreePixmap)(self.display, pixmap);
            cursor
        }
    }

    /// Takes the pointer and keyboard away from every other program.
    fn grab(&self, cursor: xlib::Cursor) -> bool {
        unsafe {
            let mask =
                (xlib::ButtonPressMask | xlib::ButtonReleaseMask | xlib::PointerMotionMask) as u32;
            let pointer = (self.xlib.XGrabPointer)(
                self.display,
                self.root,
                0,
                mask,
                xlib::GrabModeAsync,
                xlib::GrabModeAsync,
                0,
                cursor,
                xlib::CurrentTime,
            );
            let keyboard = (self.xlib.XGrabKeyboard)(
                self.display,
                self.root,
                0,
                xlib::GrabModeAsync,
                xlib::GrabModeAsync,
                xlib::CurrentTime,
            );
            if pointer != xlib::GrabSuccess || keyboard != xlib::GrabSuccess {
                self.ungrab();
                return false;
            }
            true
        }
    }

    fn ungrab(&self) {
        unsafe {
            (self.xlib.XUngrabPointer)(self.display, xlib::CurrentTime);
            (self.xlib.XUngrabKeyboard)(self.display, xlib::CurrentTime);
            (self.xlib.XFlush)(self.display);
        }
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        unsafe {
            (self.xlib.XCloseDisplay)(self.display);
        }
    }
}

fn run(mut host: Box<dyn Host>, control: &Control) -> Result<(), String> {
    let d = Display::open()?;
    let cursor = d.blank_cursor();
    let mut hits = [0u32; 4];
    let mut since = [Instant::now(); 4];
    let mut cooldown_until = Instant::now();

    while !control.stop.load(Ordering::Relaxed) {
        std::thread::sleep(POLL);
        if !control.enabled.load(Ordering::Relaxed) || Instant::now() < cooldown_until {
            continue;
        }
        let Some((x, y, mask)) = d.pointer() else {
            continue;
        };
        let linked = *control.linked.lock().unwrap();
        let mut start = None;
        for e in 0..4 {
            if !linked[e] {
                continue;
            }
            let at = match e {
                0 => x <= 0,
                1 => x >= d.width - 1,
                2 => y <= 0,
                _ => y >= d.height - 1,
            };
            if !at {
                hits[e] = 0;
                continue;
            }
            if hits[e] == 0 {
                since[e] = Instant::now();
            }
            hits[e] += 1;
            // Dragging a window to the edge is not a push.
            let button_held = mask & (xlib::Button1Mask | xlib::Button3Mask) != 0;
            if hits[e] >= EDGE_HITS && since[e].elapsed() >= EDGE_DWELL && !button_held {
                hits[e] = 0;
                let pos = if e <= 1 {
                    crate::remote_input::to_fraction(y, d.height)
                } else {
                    crate::remote_input::to_fraction(x, d.width)
                };
                start = Some((e as u8, pos));
                break;
            }
        }
        let Some((edge, pos)) = start else { continue };
        if host.begin(edge, pos).is_err() {
            cooldown_until = Instant::now() + Duration::from_secs(3);
            continue;
        }
        let outcome = remote(&d, cursor, host.as_mut(), control);
        d.ungrab();
        let (place_pos, note) = match outcome {
            Outcome::Returned(p) => (p, None),
            Outcome::Ended(why) => (32768, Some(why)),
        };
        place_at_edge(&d, edge, place_pos);
        if let Some(why) = note {
            *control.last_note.lock().unwrap() = Some(why);
        }
        cooldown_until = Instant::now() + COOLDOWN;
    }
    Ok(())
}

/// Just inside the edge it left by, `pos` of the way along it.
fn place_at_edge(d: &Display, edge: u8, pos: u16) {
    const INSET: i32 = 6;
    let along = |total: i32| crate::remote_input::from_fraction(pos, total);
    let (x, y) = match edge {
        0 => (INSET, along(d.height)),
        1 => (d.width - 1 - INSET, along(d.height)),
        2 => (along(d.width), INSET),
        _ => (along(d.width), d.height - 1 - INSET),
    };
    d.warp(x, y);
}

enum Outcome {
    Returned(u16),
    Ended(String),
}

/// The keyboard and mouse belong to the other computer until this returns.
fn remote(d: &Display, cursor: xlib::Cursor, host: &mut dyn Host, control: &Control) -> Outcome {
    if !d.grab(cursor) {
        host.end();
        return Outcome::Ended("Another program is holding the keyboard or mouse.".into());
    }
    let (cx, cy) = (d.width / 2, d.height / 2);
    d.warp(cx, cy);

    let mut batch = Batch::default();
    let mut last_flush = Instant::now();
    let mut last_status = Instant::now();

    loop {
        if control.stop.load(Ordering::Relaxed) {
            host.end();
            return Outcome::Ended("Link All is closing.".into());
        }
        unsafe {
            while (d.xlib.XPending)(d.display) > 0 {
                let mut ev: xlib::XEvent = std::mem::zeroed();
                (d.xlib.XNextEvent)(d.display, &mut ev);
                match ev.get_type() {
                    xlib::MotionNotify => {
                        let m = ev.motion;
                        let (dx, dy) = (m.x_root - cx, m.y_root - cy);
                        if dx != 0 || dy != 0 {
                            batch.dx += dx;
                            batch.dy += dy;
                            d.warp(cx, cy);
                        }
                    }
                    xlib::ButtonPress | xlib::ButtonRelease => {
                        let b = ev.button;
                        let down = ev.get_type() == xlib::ButtonPress;
                        match b.button {
                            1 => batch.buttons.push((0, down)),
                            2 => batch.buttons.push((2, down)),
                            3 => batch.buttons.push((1, down)),
                            4 if down => batch.scroll_y += NOTCH,
                            5 if down => batch.scroll_y -= NOTCH,
                            6 if down => batch.scroll_x -= NOTCH,
                            7 if down => batch.scroll_x += NOTCH,
                            8 => batch.buttons.push((3, down)),
                            9 => batch.buttons.push((4, down)),
                            _ => {}
                        }
                    }
                    xlib::KeyPress | xlib::KeyRelease => {
                        let k = ev.key;
                        let down = ev.get_type() == xlib::KeyPress;
                        // Ctrl+Alt+Esc: the way back that needs nothing from the network.
                        if down
                            && k.keycode == 9
                            && k.state & xlib::ControlMask != 0
                            && k.state & xlib::Mod1Mask != 0
                        {
                            host.end();
                            return Outcome::Ended("You pressed Ctrl+Alt+Esc.".into());
                        }
                        // Auto-repeat shows up as a release followed at once by
                        // the same press; the other computer repeats on its own.
                        if !down && is_repeat(d, k.keycode, k.time) {
                            continue;
                        }
                        let evdev = k.keycode.saturating_sub(8) as u16;
                        if let Some(sc) = crate::remote_input::evdev_to_scancode(evdev) {
                            batch.keys.push((sc, down));
                        }
                    }
                    _ => {}
                }
            }
        }

        let now = Instant::now();
        if now.duration_since(last_flush) >= BATCH_EVERY {
            last_flush = now;
            if !batch.is_empty() {
                // A failed batch may have stopped part-way through, and
                // sending the next one could put its keys out of order, so
                // the first failure ends the hand-off. It most often fails
                // because the pointer just came back or the other side ended
                // it, which the status says.
                if host.input(&batch).is_err() {
                    if let Ok(s) = host.status() {
                        if let Some(pos) = s.returned_pos {
                            return Outcome::Returned(pos);
                        }
                        if let Some(why) = s.ended_reason {
                            return Outcome::Ended(why);
                        }
                    }
                    host.end();
                    return Outcome::Ended("The other computer stopped answering.".into());
                }
                batch = Batch::default();
            }
        }
        if now.duration_since(last_status) >= STATUS_EVERY {
            last_status = now;
            if let Ok(s) = host.status() {
                if let Some(pos) = s.returned_pos {
                    return Outcome::Returned(pos);
                }
                if let Some(why) = s.ended_reason {
                    return Outcome::Ended(why);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// True when the release just read is the first half of an auto-repeat: the
/// next event is a press of the same key at the same instant. That press is
/// consumed.
fn is_repeat(d: &Display, keycode: u32, time: xlib::Time) -> bool {
    unsafe {
        if (d.xlib.XPending)(d.display) == 0 {
            return false;
        }
        let mut next: xlib::XEvent = std::mem::zeroed();
        (d.xlib.XPeekEvent)(d.display, &mut next);
        if next.get_type() == xlib::KeyPress && next.key.keycode == keycode && next.key.time == time
        {
            (d.xlib.XNextEvent)(d.display, &mut next);
            return true;
        }
        false
    }
}
