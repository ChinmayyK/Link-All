//! Mouse and keyboard input a paired phone sends to this computer: the phone
//! as a trackpad and keyboard. Like [`crate::remote_control`], it is only
//! reachable when the user has turned "Remote control" on, and what a phone
//! can press is a fixed list (keys, modifiers and named shortcuts); typed
//! text is sent as characters, never run as a command.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A mouse button, as numbered on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
    /// The side button that goes back.
    Back,
    /// The side button that goes forward.
    Forward,
}

impl Button {
    pub fn from_wire(n: u8) -> Option<Button> {
        match n {
            0 => Some(Button::Left),
            1 => Some(Button::Right),
            2 => Some(Button::Middle),
            3 => Some(Button::Back),
            4 => Some(Button::Forward),
            _ => None,
        }
    }
}

/// A key by what it is, mapped to each system's own key code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    Letter(char),
    Zero,
    Plus,
    Minus,
    LeftBracket,
    RightBracket,
    Space,
    Tab,
    Enter,
    Escape,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    /// F1 to F12.
    Function(u8),
    /// The Super / Windows key on its own.
    Super,
}

/// A key without a character of its own that a phone can press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Backspace,
    Enter,
    Escape,
    Tab,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    /// F1 to F12.
    Function(u8),
}

impl Key {
    /// Every key except the function keys, which are F1 to F12.
    pub const ALL: [Key; 13] = [
        Key::Backspace,
        Key::Enter,
        Key::Escape,
        Key::Tab,
        Key::Left,
        Key::Right,
        Key::Up,
        Key::Down,
        Key::Home,
        Key::End,
        Key::PageUp,
        Key::PageDown,
        Key::Delete,
    ];

    pub fn name(self) -> String {
        match self {
            Key::Backspace => "backspace".into(),
            Key::Enter => "enter".into(),
            Key::Escape => "escape".into(),
            Key::Tab => "tab".into(),
            Key::Left => "left".into(),
            Key::Right => "right".into(),
            Key::Up => "up".into(),
            Key::Down => "down".into(),
            Key::Home => "home".into(),
            Key::End => "end".into(),
            Key::PageUp => "page_up".into(),
            Key::PageDown => "page_down".into(),
            Key::Delete => "delete".into(),
            Key::Function(n) => format!("f{n}"),
        }
    }

    pub fn parse(name: &str) -> Option<Key> {
        if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
            // "f1" to "f12" only: no "f01", "f+1" or "f13".
            return ((1..=12).contains(&n) && name == format!("f{n}")).then_some(Key::Function(n));
        }
        Key::ALL.into_iter().find(|k| k.name() == name)
    }

    fn code(self) -> Code {
        match self {
            Key::Backspace => Code::Backspace,
            Key::Enter => Code::Enter,
            Key::Escape => Code::Escape,
            Key::Tab => Code::Tab,
            Key::Left => Code::Left,
            Key::Right => Code::Right,
            Key::Up => Code::Up,
            Key::Down => Code::Down,
            Key::Home => Code::Home,
            Key::End => Code::End,
            Key::PageUp => Code::PageUp,
            Key::PageDown => Code::PageDown,
            Key::Delete => Code::Delete,
            Key::Function(n) => Code::Function(n),
        }
    }
}

/// A modifier a phone can hold down, for a click with Shift or Control held,
/// or a shortcut it doesn't have a button for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Shift,
    Ctrl,
    /// Alt, or Option on a Mac.
    Alt,
    /// The Windows or Super key, or Command on a Mac.
    Meta,
}

impl Modifier {
    pub const ALL: [Modifier; 4] = [
        Modifier::Shift,
        Modifier::Ctrl,
        Modifier::Alt,
        Modifier::Meta,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Modifier::Shift => "shift",
            Modifier::Ctrl => "ctrl",
            Modifier::Alt => "alt",
            Modifier::Meta => "meta",
        }
    }

    pub fn parse(name: &str) -> Option<Modifier> {
        Modifier::ALL.into_iter().find(|m| m.name() == name)
    }

    fn index(self) -> usize {
        match self {
            Modifier::Shift => 0,
            Modifier::Ctrl => 1,
            Modifier::Alt => 2,
            Modifier::Meta => 3,
        }
    }
}

/// A named shortcut, the same idea on every system: "copy" is Command-C on a
/// Mac and Control-C elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
    SelectAll,
    Find,
    Save,
    NewTab,
    CloseTab,
    NextTab,
    PreviousTab,
    Back,
    Forward,
    Reload,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    SwitchApp,
    Overview,
    ShowDesktop,
    DesktopLeft,
    DesktopRight,
}

impl Shortcut {
    pub const ALL: [Shortcut; 23] = [
        Shortcut::Copy,
        Shortcut::Cut,
        Shortcut::Paste,
        Shortcut::Undo,
        Shortcut::Redo,
        Shortcut::SelectAll,
        Shortcut::Find,
        Shortcut::Save,
        Shortcut::NewTab,
        Shortcut::CloseTab,
        Shortcut::NextTab,
        Shortcut::PreviousTab,
        Shortcut::Back,
        Shortcut::Forward,
        Shortcut::Reload,
        Shortcut::ZoomIn,
        Shortcut::ZoomOut,
        Shortcut::ZoomReset,
        Shortcut::SwitchApp,
        Shortcut::Overview,
        Shortcut::ShowDesktop,
        Shortcut::DesktopLeft,
        Shortcut::DesktopRight,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Shortcut::Copy => "copy",
            Shortcut::Cut => "cut",
            Shortcut::Paste => "paste",
            Shortcut::Undo => "undo",
            Shortcut::Redo => "redo",
            Shortcut::SelectAll => "select_all",
            Shortcut::Find => "find",
            Shortcut::Save => "save",
            Shortcut::NewTab => "new_tab",
            Shortcut::CloseTab => "close_tab",
            Shortcut::NextTab => "next_tab",
            Shortcut::PreviousTab => "previous_tab",
            Shortcut::Back => "back",
            Shortcut::Forward => "forward",
            Shortcut::Reload => "reload",
            Shortcut::ZoomIn => "zoom_in",
            Shortcut::ZoomOut => "zoom_out",
            Shortcut::ZoomReset => "zoom_reset",
            Shortcut::SwitchApp => "switch_app",
            Shortcut::Overview => "overview",
            Shortcut::ShowDesktop => "show_desktop",
            Shortcut::DesktopLeft => "desktop_left",
            Shortcut::DesktopRight => "desktop_right",
        }
    }

    pub fn parse(name: &str) -> Option<Shortcut> {
        Shortcut::ALL.into_iter().find(|s| s.name() == name)
    }

    /// The modifiers and key that make this shortcut on `system`.
    fn chord(self, system: System) -> (Vec<Modifier>, Code) {
        use Modifier::{Alt, Ctrl, Meta, Shift};
        let mac = system == System::Mac;
        let primary = if mac { Meta } else { Ctrl };
        let letter = |c| (vec![primary], Code::Letter(c));
        match self {
            Shortcut::Copy => letter('c'),
            Shortcut::Cut => letter('x'),
            Shortcut::Paste => letter('v'),
            Shortcut::Undo => letter('z'),
            Shortcut::Redo if mac => (vec![Meta, Shift], Code::Letter('z')),
            Shortcut::Redo => letter('y'),
            Shortcut::SelectAll => letter('a'),
            Shortcut::Find => letter('f'),
            Shortcut::Save => letter('s'),
            Shortcut::NewTab => letter('t'),
            Shortcut::CloseTab => letter('w'),
            Shortcut::NextTab => (vec![Ctrl], Code::Tab),
            Shortcut::PreviousTab => (vec![Ctrl, Shift], Code::Tab),
            Shortcut::Back if mac => (vec![Meta], Code::LeftBracket),
            Shortcut::Back => (vec![Alt], Code::Left),
            Shortcut::Forward if mac => (vec![Meta], Code::RightBracket),
            Shortcut::Forward => (vec![Alt], Code::Right),
            Shortcut::Reload => letter('r'),
            Shortcut::ZoomIn => (vec![primary], Code::Plus),
            Shortcut::ZoomOut => (vec![primary], Code::Minus),
            Shortcut::ZoomReset => (vec![primary], Code::Zero),
            Shortcut::SwitchApp if mac => (vec![Meta], Code::Tab),
            Shortcut::SwitchApp => (vec![Alt], Code::Tab),
            Shortcut::Overview if mac => (vec![Ctrl], Code::Up),
            Shortcut::Overview if system == System::Windows => (vec![Meta], Code::Tab),
            Shortcut::Overview => (vec![], Code::Super),
            Shortcut::ShowDesktop if mac => (vec![], Code::Function(11)),
            Shortcut::ShowDesktop => (vec![Meta], Code::Letter('d')),
            Shortcut::DesktopLeft if mac => (vec![Ctrl], Code::Left),
            Shortcut::DesktopLeft if system == System::Windows => (vec![Ctrl, Meta], Code::Left),
            Shortcut::DesktopLeft => (vec![Ctrl, Alt], Code::Left),
            Shortcut::DesktopRight if mac => (vec![Ctrl], Code::Right),
            Shortcut::DesktopRight if system == System::Windows => (vec![Ctrl, Meta], Code::Right),
            Shortcut::DesktopRight => (vec![Ctrl, Alt], Code::Right),
        }
    }
}

/// Which system's key conventions apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum System {
    Mac,
    Windows,
    Other,
}

fn this_system() -> System {
    if cfg!(target_os = "macos") {
        System::Mac
    } else if cfg!(windows) {
        System::Windows
    } else {
        System::Other
    }
}

/// The most text one message may type; a phone sends a few characters at a
/// time, so more than this is a mistake or abuse.
pub const MAX_TEXT_CHARS: usize = 256;

/// The largest single pointer or scroll step accepted. A phone's finger moves
/// a few dozen units per frame; this only stops a runaway value.
pub const MAX_STEP: i32 = 2000;

fn clamp_step(v: i32) -> i32 {
    v.clamp(-MAX_STEP, MAX_STEP)
}

/// A held modifier is let go of after this long without being released, so a
/// dropped connection can't leave a key stuck down.
const HOLD_LIMIT: Duration = Duration::from_secs(30);

struct Held {
    down: [bool; 4],
    since: Option<Instant>,
}

static HELD: Mutex<Held> = Mutex::new(Held {
    down: [false; 4],
    since: None,
});

/// Lets go of every held modifier that has been down too long.
fn release_stale() {
    let stale: Vec<Modifier> = {
        let mut held = HELD.lock().unwrap();
        if held.since.is_some_and(|t| t.elapsed() > HOLD_LIMIT) {
            held.since = None;
            Modifier::ALL
                .into_iter()
                .filter(|m| std::mem::replace(&mut held.down[m.index()], false))
                .collect()
        } else {
            Vec::new()
        }
    };
    for m in stale {
        let _ = imp::modifier(m, false);
    }
}

/// Presses or releases a modifier and keeps it held until released.
pub fn set_modifier(modifier: Modifier, down: bool) -> Result<(), String> {
    {
        let mut held = HELD.lock().unwrap();
        held.down[modifier.index()] = down;
        held.since = held.down.iter().any(|d| *d).then(Instant::now);
    }
    imp::modifier(modifier, down)
}

/// Presses and releases `code` with `mods` held for that moment.
fn chord(mods: &[Modifier], code: Code) -> Result<(), String> {
    let held = HELD.lock().unwrap().down;
    // Only press the modifiers that aren't already held.
    let extra: Vec<Modifier> = mods.iter().copied().filter(|m| !held[m.index()]).collect();
    for m in &extra {
        imp::modifier(*m, true)?;
    }
    let result = imp::key(code, true).and_then(|()| imp::key(code, false));
    for m in extra.iter().rev() {
        let _ = imp::modifier(*m, false);
    }
    result
}

/// A phone number a computer asked the phone to call, or None when it isn't
/// one: digits and the usual `+ - ( ) . * #` and spaces, 3 to 32 characters.
/// Whatever else it holds, it is never handed to anything that runs it.
pub fn clean_phone_number(raw: &str) -> Option<String> {
    let number = raw.trim();
    let count = number.chars().count();
    if !(3..=32).contains(&count) {
        return None;
    }
    if !number.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    if number
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '(' | ')' | '.' | '*' | '#' | ' '))
    {
        Some(number.to_string())
    } else {
        None
    }
}

/// Moves the pointer by (`dx`, `dy`) from where it is.
pub fn pointer_move(dx: i32, dy: i32) -> Result<(), String> {
    release_stale();
    if let Some(b) = testing::backend() {
        return b.pointer_move(clamp_step(dx), clamp_step(dy));
    }
    imp::pointer_move(clamp_step(dx), clamp_step(dy))
}

/// Presses or releases a mouse button.
pub fn pointer_button(button: Button, down: bool) -> Result<(), String> {
    release_stale();
    note_button(button, down);
    if let Some(b) = testing::backend() {
        return b.button(button, down);
    }
    imp::pointer_button(button, down)
}

/// Scrolls by (`dx`, `dy`) in pixel-like units; positive `dy` moves the
/// content up, as with a trackpad's natural direction off.
pub fn scroll(dx: i32, dy: i32) -> Result<(), String> {
    release_stale();
    imp::scroll(clamp_step(dx), clamp_step(dy))
}

/// Types `text` as characters.
pub fn type_text(text: &str) -> Result<(), String> {
    release_stale();
    let text: String = text.chars().take(MAX_TEXT_CHARS).collect();
    if text.is_empty() {
        return Ok(());
    }
    imp::type_text(&text)
}

/// Presses and releases one of the fixed keys.
pub fn press_key(key: Key) -> Result<(), String> {
    release_stale();
    chord(&[], key.code())
}

/// Runs one of the named shortcuts the way this system spells it.
pub fn shortcut(shortcut: Shortcut) -> Result<(), String> {
    release_stale();
    let (mods, code) = shortcut.chord(this_system());
    chord(&mods, code)
}

/// Pastes what is on the clipboard into the focused app (Command-V, or
/// Control-V). Used by "Paste" on a verification-code popup.
pub fn paste() -> Result<(), String> {
    shortcut(Shortcut::Paste)
}

/// The one-time code in a phone notification that Link All sent for a
/// verification code: the title starts "Verification code" and the text is
/// just 4 to 8 digits. Anything else is an ordinary notification.
pub fn verification_code(title: &str, text: &str) -> Option<String> {
    let code = text.trim();
    let digits = (4..=8).contains(&code.len()) && code.chars().all(|c| c.is_ascii_digit());
    (title.starts_with("Verification code") && digits).then(|| code.to_string())
}

/// A stand-in for the operating system, so a test can run the whole hand-off
/// without moving this computer's real pointer.
#[doc(hidden)]
pub mod testing {
    use super::Button;
    use std::sync::{Arc, Mutex};

    pub trait Backend: Send + Sync {
        fn screen_size(&self) -> Option<(i32, i32)>;
        fn cursor(&self) -> Option<(i32, i32)>;
        fn set_cursor(&self, x: i32, y: i32) -> Result<(), String>;
        fn pointer_move(&self, dx: i32, dy: i32) -> Result<(), String>;
        fn button(&self, button: Button, down: bool) -> Result<(), String>;
        fn key(&self, scancode: u16, down: bool) -> Result<(), String>;
    }

    static BACKEND: Mutex<Option<Arc<dyn Backend>>> = Mutex::new(None);

    pub fn install(backend: Arc<dyn Backend>) {
        *BACKEND.lock().unwrap() = Some(backend);
    }

    pub fn uninstall() {
        *BACKEND.lock().unwrap() = None;
    }

    pub(super) fn backend() -> Option<Arc<dyn Backend>> {
        BACKEND.lock().unwrap().clone()
    }
}

// ── Keyboard and mouse hand-off ──────────────────────────────────────────────

/// One side of a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    pub fn from_wire(n: u8) -> Option<Edge> {
        match n {
            0 => Some(Edge::Left),
            1 => Some(Edge::Right),
            2 => Some(Edge::Top),
            3 => Some(Edge::Bottom),
            _ => None,
        }
    }

    pub fn to_wire(self) -> u8 {
        match self {
            Edge::Left => 0,
            Edge::Right => 1,
            Edge::Top => 2,
            Edge::Bottom => 3,
        }
    }

    /// The side a pointer arrives on after leaving the other screen by `self`.
    pub fn opposite(self) -> Edge {
        match self {
            Edge::Left => Edge::Right,
            Edge::Right => Edge::Left,
            Edge::Top => Edge::Bottom,
            Edge::Bottom => Edge::Top,
        }
    }
}

/// `value` out of `total` as a fraction of 0..=65535.
pub fn to_fraction(value: i32, total: i32) -> u16 {
    if total <= 1 {
        return 0;
    }
    ((i64::from(value.clamp(0, total - 1)) * 65535) / i64::from(total - 1)) as u16
}

/// The inverse of [`to_fraction`]: a pixel in 0..total.
pub fn from_fraction(pos: u16, total: i32) -> i32 {
    if total <= 1 {
        return 0;
    }
    ((i64::from(pos) * i64::from(total - 1)) / 65535) as i32
}

/// Where the pointer appears on a `width` x `height` screen after it came in
/// over `arrived`, `pos` of the way along that edge.
pub fn entry_point(arrived: Edge, pos: u16, width: i32, height: i32) -> (i32, i32) {
    match arrived {
        Edge::Left => (0, from_fraction(pos, height)),
        Edge::Right => (width - 1, from_fraction(pos, height)),
        Edge::Top => (from_fraction(pos, width), 0),
        Edge::Bottom => (from_fraction(pos, width), height - 1),
    }
}

/// How far a move of (`dx`, `dy`) pushes outward through `edge`; 0 or less
/// when it moves away from it.
pub fn outward_push(edge: Edge, dx: i32, dy: i32) -> i32 {
    match edge {
        Edge::Left => -dx,
        Edge::Right => dx,
        Edge::Top => -dy,
        Edge::Bottom => dy,
    }
}

/// True when (`x`, `y`) is on `edge` of a `width` x `height` screen, and where
/// along it.
pub fn on_edge(edge: Edge, x: i32, y: i32, width: i32, height: i32) -> Option<u16> {
    match edge {
        Edge::Left if x <= 0 => Some(to_fraction(y, height)),
        Edge::Right if x >= width - 1 => Some(to_fraction(y, height)),
        Edge::Top if y <= 0 => Some(to_fraction(x, width)),
        Edge::Bottom if y >= height - 1 => Some(to_fraction(x, width)),
        _ => None,
    }
}

/// Keys and buttons pressed through a hand-off and not yet released, so a
/// dropped link can't leave one stuck down.
struct HeldRaw {
    keys: Vec<u16>,
    buttons: Vec<Button>,
}

static HELD_RAW: Mutex<HeldRaw> = Mutex::new(HeldRaw {
    keys: Vec::new(),
    buttons: Vec::new(),
});

/// The pointer's position on the screen it moves on.
pub fn cursor_position() -> Option<(i32, i32)> {
    if let Some(b) = testing::backend() {
        return b.cursor();
    }
    edge_imp::cursor_position()
}

/// The width and height of the screen the pointer moves on.
pub fn screen_size() -> Option<(i32, i32)> {
    if let Some(b) = testing::backend() {
        return b.screen_size();
    }
    edge_imp::screen_size()
}

/// Puts the pointer at (`x`, `y`).
pub fn set_cursor_position(x: i32, y: i32) -> Result<(), String> {
    if let Some(b) = testing::backend() {
        return b.set_cursor(x, y);
    }
    edge_imp::set_cursor_position(x, y)
}

/// Presses or releases the key with this scan code (PS/2 set 1, with 0xE000
/// added for an extended key), as the sender's keyboard reported it.
pub fn raw_key(scancode: u16, down: bool) -> Result<(), String> {
    {
        let mut held = HELD_RAW.lock().unwrap();
        held.keys.retain(|k| *k != scancode);
        if down {
            held.keys.push(scancode);
        }
    }
    if let Some(b) = testing::backend() {
        return b.key(scancode, down);
    }
    edge_imp::raw_key(scancode, down)
}

/// Remembers which buttons a hand-off holds. Called for every button change.
fn note_button(button: Button, down: bool) {
    let mut held = HELD_RAW.lock().unwrap();
    held.buttons.retain(|b| *b != button);
    if down {
        held.buttons.push(button);
    }
}

/// Lets go of every key and button still held.
pub fn release_all() {
    let (keys, buttons) = {
        let mut held = HELD_RAW.lock().unwrap();
        (
            std::mem::take(&mut held.keys),
            std::mem::take(&mut held.buttons),
        )
    };
    for k in keys {
        let _ = match testing::backend() {
            Some(be) => be.key(k, false),
            None => edge_imp::raw_key(k, false),
        };
    }
    for b in buttons {
        let _ = match testing::backend() {
            Some(be) => be.button(b, false),
            None => imp::pointer_button(b, false),
        };
    }
}

#[cfg(windows)]
mod edge_imp {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetSystemMetrics, SetCursorPos, SM_CXSCREEN, SM_CYSCREEN,
    };

    pub fn cursor_position() -> Option<(i32, i32)> {
        let mut p = POINT { x: 0, y: 0 };
        (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x, p.y))
    }

    pub fn screen_size() -> Option<(i32, i32)> {
        let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        (w > 0 && h > 0).then_some((w, h))
    }

    pub fn set_cursor_position(x: i32, y: i32) -> Result<(), String> {
        if unsafe { SetCursorPos(x, y) } != 0 {
            Ok(())
        } else {
            Err("Windows didn't move the pointer".to_string())
        }
    }

    pub fn raw_key(scancode: u16, down: bool) -> Result<(), String> {
        let mut flags = KEYEVENTF_SCANCODE;
        if scancode & 0xE000 == 0xE000 {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !down {
            flags |= KEYEVENTF_KEYUP;
        }
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: scancode & 0xFF,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let sent = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
        if sent == 1 {
            Ok(())
        } else {
            Err("Windows didn't accept the key".to_string())
        }
    }
}

/// The Linux (evdev) key code for a PS/2 set 1 scan code. The plain keys have
/// the same number in both; the extended ones (0xE000 added) do not.
pub fn scancode_to_evdev(scancode: u16) -> Option<u16> {
    match scancode {
        0x01..=0x58 => Some(scancode),
        0xE01C => Some(96),  // keypad enter
        0xE01D => Some(97),  // right control
        0xE035 => Some(98),  // keypad slash
        0xE037 => Some(99),  // print screen
        0xE038 => Some(100), // right alt
        0xE047 => Some(102), // home
        0xE048 => Some(103), // up
        0xE049 => Some(104), // page up
        0xE04B => Some(105), // left
        0xE04D => Some(106), // right
        0xE04F => Some(107), // end
        0xE050 => Some(108), // down
        0xE051 => Some(109), // page down
        0xE052 => Some(110), // insert
        0xE053 => Some(111), // delete
        0xE05B => Some(125), // left super
        0xE05C => Some(126), // right super
        0xE05D => Some(127), // menu
        _ => None,
    }
}

/// The inverse of [`scancode_to_evdev`]: the PS/2 set 1 scan code for a Linux
/// key code, or None for a key the hand-off does not carry.
pub fn evdev_to_scancode(code: u16) -> Option<u16> {
    match code {
        0x01..=0x58 => Some(code),
        96 => Some(0xE01C),
        97 => Some(0xE01D),
        98 => Some(0xE035),
        99 => Some(0xE037),
        100 => Some(0xE038),
        102 => Some(0xE047),
        103 => Some(0xE048),
        104 => Some(0xE049),
        105 => Some(0xE04B),
        106 => Some(0xE04D),
        107 => Some(0xE04F),
        108 => Some(0xE050),
        109 => Some(0xE051),
        110 => Some(0xE052),
        111 => Some(0xE053),
        125 => Some(0xE05B),
        126 => Some(0xE05C),
        127 => Some(0xE05D),
        _ => None,
    }
}

/// The macOS virtual key code for a PS/2 set 1 scan code, on a US layout. The
/// shortcut keys follow the Mac's habits rather than the key's position:
/// Control acts as Command, Alt as Option and the Windows key as Control, so
/// Ctrl+C on the other keyboard copies here.
pub fn scancode_to_mac(scancode: u16) -> Option<u16> {
    Some(match scancode {
        0x01 => 53, // escape
        0x02 => 18,
        0x03 => 19,
        0x04 => 20,
        0x05 => 21,
        0x06 => 23,
        0x07 => 22,
        0x08 => 26,
        0x09 => 28,
        0x0A => 25,
        0x0B => 29,
        0x0C => 27,
        0x0D => 24,
        0x0E => 51, // backspace
        0x0F => 48, // tab
        0x10 => 12, // q
        0x11 => 13,
        0x12 => 14,
        0x13 => 15,
        0x14 => 17,
        0x15 => 16,
        0x16 => 32,
        0x17 => 34,
        0x18 => 31,
        0x19 => 35,
        0x1A => 33,
        0x1B => 30,
        0x1C => 36, // enter
        0x1D => 55, // left control -> left command
        0x1E => 0,  // a
        0x1F => 1,
        0x20 => 2,
        0x21 => 3,
        0x22 => 5,
        0x23 => 4,
        0x24 => 38,
        0x25 => 40,
        0x26 => 37,
        0x27 => 41,
        0x28 => 39,
        0x29 => 50,
        0x2A => 56, // left shift
        0x2B => 42,
        0x2C => 6, // z
        0x2D => 7,
        0x2E => 8,
        0x2F => 9,
        0x30 => 11,
        0x31 => 45,
        0x32 => 46,
        0x33 => 43,
        0x34 => 47,
        0x35 => 44,
        0x36 => 60, // right shift
        0x37 => 67, // keypad *
        0x38 => 58, // left alt -> left option
        0x39 => 49, // space
        0x3A => 57, // caps lock
        0x3B => 122,
        0x3C => 120,
        0x3D => 99,
        0x3E => 118,
        0x3F => 96,
        0x40 => 97,
        0x41 => 98,
        0x42 => 100,
        0x43 => 101,
        0x44 => 109,
        0x45 => 71, // num lock -> clear
        0x47 => 89,
        0x48 => 91,
        0x49 => 92,
        0x4A => 78,
        0x4B => 86,
        0x4C => 87,
        0x4D => 88,
        0x4E => 69,
        0x4F => 83,
        0x50 => 84,
        0x51 => 85,
        0x52 => 82,
        0x53 => 65,
        0x57 => 103,  // f11
        0x58 => 111,  // f12
        0xE01C => 76, // keypad enter
        0xE01D => 54, // right control -> right command
        0xE035 => 75, // keypad /
        0xE038 => 61, // right alt -> right option
        0xE047 => 115,
        0xE048 => 126,
        0xE049 => 116,
        0xE04B => 123,
        0xE04D => 124,
        0xE04F => 119,
        0xE050 => 125,
        0xE051 => 121,
        0xE052 => 114,
        0xE053 => 117,
        0xE05B => 59, // left windows -> left control
        0xE05C => 62, // right windows -> right control
        _ => return None,
    })
}

/// The flag a macOS modifier key sets while it is held (0 for other keys).
pub fn mac_modifier_flag(keycode: u16) -> u64 {
    match keycode {
        56 | 60 => 0x0002_0000, // shift
        59 | 62 => 0x0004_0000, // control
        58 | 61 => 0x0008_0000, // option
        55 | 54 => 0x0010_0000, // command
        _ => 0,
    }
}

#[cfg(target_os = "macos")]
mod edge_imp {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(obj: *const c_void);
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreate(source: *const c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *const c_void) -> CGPoint;
        fn CGMainDisplayID() -> u32;
        fn CGDisplayBounds(display: u32) -> CGRect;
        fn CGWarpMouseCursorPosition(to: CGPoint) -> i32;
        fn CGAssociateMouseAndMouseCursorPosition(connected: bool) -> i32;
        fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> *mut c_void;
        fn CGEventSetFlags(event: *mut c_void, flags: u64);
        fn CGEventPost(tap: u32, event: *mut c_void);
    }

    /// Modifier flags held through the hand-off.
    static FLAGS: AtomicU64 = AtomicU64::new(0);

    pub fn cursor_position() -> Option<(i32, i32)> {
        unsafe {
            let probe = CGEventCreate(std::ptr::null());
            if probe.is_null() {
                return None;
            }
            let at = CGEventGetLocation(probe);
            CFRelease(probe);
            Some((at.x.round() as i32, at.y.round() as i32))
        }
    }

    pub fn screen_size() -> Option<(i32, i32)> {
        let bounds = unsafe { CGDisplayBounds(CGMainDisplayID()) };
        let (w, h) = (
            bounds.size.width.round() as i32,
            bounds.size.height.round() as i32,
        );
        (w > 0 && h > 0).then_some((w, h))
    }

    pub fn set_cursor_position(x: i32, y: i32) -> Result<(), String> {
        let to = CGPoint {
            x: f64::from(x),
            y: f64::from(y),
        };
        unsafe {
            if CGWarpMouseCursorPosition(to) != 0 {
                return Err("macOS didn't move the pointer".to_string());
            }
            // Without this the pointer ignores the mouse for a moment after a warp.
            CGAssociateMouseAndMouseCursorPosition(true);
        }
        Ok(())
    }

    pub fn raw_key(scancode: u16, down: bool) -> Result<(), String> {
        let keycode = super::scancode_to_mac(scancode)
            .ok_or_else(|| "that key has no Mac equivalent".to_string())?;
        let flag = super::mac_modifier_flag(keycode);
        if flag != 0 {
            if down {
                FLAGS.fetch_or(flag, Ordering::Relaxed);
            } else {
                FLAGS.fetch_and(!flag, Ordering::Relaxed);
            }
        }
        unsafe {
            let event = CGEventCreateKeyboardEvent(std::ptr::null(), keycode, down);
            if event.is_null() {
                return Err("macOS didn't create the key event".to_string());
            }
            CGEventSetFlags(event, FLAGS.load(Ordering::Relaxed));
            CGEventPost(0, event);
            CFRelease(event);
        }
        Ok(())
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
mod edge_imp {
    use std::sync::Mutex;
    use x11_dl::xlib::{Display, Xlib};

    /// One connection to the X server, opened on first use. Without X (a
    /// Wayland-only session, a console) opening fails and the hand-off is
    /// refused, with a reason.
    struct X {
        lib: Xlib,
        display: *mut Display,
    }
    // SAFETY: the connection is only ever used while holding the mutex.
    unsafe impl Send for X {}

    static X11: Mutex<Option<X>> = Mutex::new(None);

    fn with_x<T>(f: impl FnOnce(&X) -> T) -> Result<T, String> {
        let mut guard = X11.lock().unwrap();
        if guard.is_none() {
            let lib = Xlib::open().map_err(|_| "this computer has no X11 library".to_string())?;
            let display = unsafe { (lib.XOpenDisplay)(std::ptr::null()) };
            if display.is_null() {
                return Err(
                    "no X11 display: Wayland sessions can't be handed a pointer yet".to_string(),
                );
            }
            *guard = Some(X { lib, display });
        }
        Ok(f(guard.as_ref().unwrap()))
    }

    pub fn cursor_position() -> Option<(i32, i32)> {
        with_x(|x| unsafe {
            let (mut root_ret, mut child_ret) = (0, 0);
            let (mut rx, mut ry, mut wx, mut wy) = (0, 0, 0, 0);
            let mut mask = 0;
            let root = (x.lib.XDefaultRootWindow)(x.display);
            let ok = (x.lib.XQueryPointer)(
                x.display,
                root,
                &mut root_ret,
                &mut child_ret,
                &mut rx,
                &mut ry,
                &mut wx,
                &mut wy,
                &mut mask,
            );
            (ok != 0).then_some((rx, ry))
        })
        .ok()
        .flatten()
    }

    pub fn screen_size() -> Option<(i32, i32)> {
        with_x(|x| unsafe {
            let screen = (x.lib.XDefaultScreen)(x.display);
            (
                (x.lib.XDisplayWidth)(x.display, screen),
                (x.lib.XDisplayHeight)(x.display, screen),
            )
        })
        .ok()
        .filter(|(w, h)| *w > 0 && *h > 0)
    }

    pub fn set_cursor_position(px: i32, py: i32) -> Result<(), String> {
        with_x(|x| unsafe {
            let root = (x.lib.XDefaultRootWindow)(x.display);
            (x.lib.XWarpPointer)(x.display, 0, root, 0, 0, 0, 0, px, py);
            (x.lib.XFlush)(x.display);
        })
    }

    pub fn raw_key(scancode: u16, down: bool) -> Result<(), String> {
        let code = super::scancode_to_evdev(scancode)
            .ok_or_else(|| "that key has no Linux equivalent".to_string())?;
        super::imp::evdev_key(code, down)
    }
}

#[cfg(target_os = "android")]
mod edge_imp {
    const NO: &str = "this device can't be handed a keyboard and mouse";

    pub fn cursor_position() -> Option<(i32, i32)> {
        None
    }
    pub fn screen_size() -> Option<(i32, i32)> {
        None
    }
    pub fn set_cursor_position(_: i32, _: i32) -> Result<(), String> {
        Err(NO.into())
    }
    pub fn raw_key(_: u16, _: bool) -> Result<(), String> {
        Err(NO.into())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{Button, Code, Modifier};
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::Instant;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(obj: *const c_void);
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreate(source: *const c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *const c_void) -> CGPoint;
        fn CGEventCreateMouseEvent(
            source: *const c_void,
            kind: u32,
            at: CGPoint,
            button: u32,
        ) -> *mut c_void;
        fn CGEventCreateScrollWheelEvent(
            source: *const c_void,
            units: u32,
            wheel_count: u32,
            ...
        ) -> *mut c_void;
        fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> *mut c_void;
        fn CGEventKeyboardSetUnicodeString(event: *mut c_void, len: usize, chars: *const u16);
        fn CGEventSetIntegerValueField(event: *mut c_void, field: u32, value: i64);
        fn CGEventSetFlags(event: *mut c_void, flags: u64);
        fn CGEventPost(tap: u32, event: *mut c_void);
    }

    const HID_TAP: u32 = 0;
    const CLICK_STATE: u32 = 1;
    const MOVED: u32 = 5;

    /// The modifier flags held down now, put on every event we post.
    static FLAGS: AtomicU64 = AtomicU64::new(0);

    struct State {
        down: [bool; 5],
        last_click: Option<(Instant, u8)>,
    }

    static STATE: Mutex<State> = Mutex::new(State {
        down: [false; 5],
        last_click: None,
    });

    fn post(event: *mut c_void) -> Result<(), String> {
        if event.is_null() {
            return Err("macOS didn't create the event".into());
        }
        unsafe {
            CGEventSetFlags(event, FLAGS.load(Ordering::Relaxed));
            CGEventPost(HID_TAP, event);
            CFRelease(event);
        }
        Ok(())
    }

    fn location() -> CGPoint {
        unsafe {
            let probe = CGEventCreate(std::ptr::null());
            if probe.is_null() {
                return CGPoint { x: 0.0, y: 0.0 };
            }
            let at = CGEventGetLocation(probe);
            CFRelease(probe);
            at
        }
    }

    fn index(button: Button) -> usize {
        match button {
            Button::Left => 0,
            Button::Right => 1,
            Button::Middle => 2,
            Button::Back => 3,
            Button::Forward => 4,
        }
    }

    pub fn pointer_move(dx: i32, dy: i32) -> Result<(), String> {
        let at = location();
        let to = CGPoint {
            x: at.x + f64::from(dx),
            y: at.y + f64::from(dy),
        };
        // A move with a button held is a drag, or apps won't follow it.
        let held = STATE.lock().unwrap().down;
        let (kind, button) = if held[0] {
            (6, 0)
        } else if held[1] {
            (7, 1)
        } else if held[2] {
            (27, 2)
        } else {
            (MOVED, 0)
        };
        post(unsafe { CGEventCreateMouseEvent(std::ptr::null(), kind, to, button) })
    }

    pub fn pointer_button(button: Button, down: bool) -> Result<(), String> {
        let i = index(button);
        let (kind, cg_button) = match (button, down) {
            (Button::Left, true) => (1, 0),
            (Button::Left, false) => (2, 0),
            (Button::Right, true) => (3, 1),
            (Button::Right, false) => (4, 1),
            (Button::Middle, true) => (25, 2),
            (Button::Middle, false) => (26, 2),
            (Button::Back, true) => (25, 3),
            (Button::Back, false) => (26, 3),
            (Button::Forward, true) => (25, 4),
            (Button::Forward, false) => (26, 4),
        };
        let clicks = {
            let mut st = STATE.lock().unwrap();
            st.down[i] = down;
            if down {
                let now = Instant::now();
                let count = match st.last_click {
                    Some((at, n)) if now.duration_since(at).as_millis() < 400 => {
                        n.saturating_add(1)
                    }
                    _ => 1,
                };
                st.last_click = Some((now, count));
                count
            } else {
                st.last_click.map_or(1, |(_, n)| n)
            }
        };
        let event =
            unsafe { CGEventCreateMouseEvent(std::ptr::null(), kind, location(), cg_button) };
        if !event.is_null() {
            unsafe { CGEventSetIntegerValueField(event, CLICK_STATE, i64::from(clicks)) };
        }
        post(event)
    }

    pub fn scroll(dx: i32, dy: i32) -> Result<(), String> {
        // Pixel units; vertical first.
        post(unsafe { CGEventCreateScrollWheelEvent(std::ptr::null(), 0, 2, dy, dx) })
    }

    pub fn type_text(text: &str) -> Result<(), String> {
        let units: Vec<u16> = text.encode_utf16().collect();
        for chunk in units.chunks(16) {
            for down in [true, false] {
                let event = unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), 0, down) };
                if !event.is_null() {
                    unsafe { CGEventKeyboardSetUnicodeString(event, chunk.len(), chunk.as_ptr()) };
                }
                post(event)?;
            }
        }
        Ok(())
    }

    /// The system's own key code for `code` (ANSI layout).
    fn key_code(code: Code) -> u16 {
        match code {
            Code::Letter(c) => match c {
                'a' => 0,
                's' => 1,
                'd' => 2,
                'f' => 3,
                'h' => 4,
                'g' => 5,
                'z' => 6,
                'x' => 7,
                'c' => 8,
                'v' => 9,
                'b' => 11,
                'q' => 12,
                'w' => 13,
                'e' => 14,
                'r' => 15,
                'y' => 16,
                't' => 17,
                'o' => 31,
                'u' => 32,
                'i' => 34,
                'p' => 35,
                'l' => 37,
                'j' => 38,
                'k' => 40,
                'n' => 45,
                'm' => 46,
                _ => 0,
            },
            Code::Zero => 29,
            Code::Plus => 24,
            Code::Minus => 27,
            Code::LeftBracket => 33,
            Code::RightBracket => 30,
            Code::Space => 49,
            Code::Tab => 48,
            Code::Enter => 36,
            Code::Escape => 53,
            Code::Backspace => 51,
            Code::Delete => 117,
            Code::Left => 123,
            Code::Right => 124,
            Code::Down => 125,
            Code::Up => 126,
            Code::Home => 115,
            Code::End => 119,
            Code::PageUp => 116,
            Code::PageDown => 121,
            Code::Function(n) => match n {
                1 => 122,
                2 => 120,
                3 => 99,
                4 => 118,
                5 => 96,
                6 => 97,
                7 => 98,
                8 => 100,
                9 => 101,
                10 => 109,
                11 => 103,
                _ => 111,
            },
            Code::Super => 55,
        }
    }

    pub fn key(code: Code, down: bool) -> Result<(), String> {
        post(unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), key_code(code), down) })
    }

    /// Holds or lets go of a modifier: later events carry its flag, and the
    /// key press itself tells the system.
    pub fn modifier(m: Modifier, down: bool) -> Result<(), String> {
        let (key, flag): (u16, u64) = match m {
            Modifier::Shift => (56, 0x0002_0000),
            Modifier::Ctrl => (59, 0x0004_0000),
            Modifier::Alt => (58, 0x0008_0000),
            Modifier::Meta => (55, 0x0010_0000),
        };
        if down {
            FLAGS.fetch_or(flag, Ordering::Relaxed);
        } else {
            FLAGS.fetch_and(!flag, Ordering::Relaxed);
        }
        post(unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), key, down) })
    }
}

#[cfg(windows)]
mod imp {
    use super::{Button, Code, Modifier};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN,
        MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE,
        MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN,
        MOUSEEVENTF_XUP, MOUSEINPUT,
    };

    fn send(inputs: &[INPUT]) -> Result<(), String> {
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent as usize == inputs.len() {
            Ok(())
        } else {
            Err("Windows didn't accept the input".to_string())
        }
    }

    fn mouse(dx: i32, dy: i32, data: i32, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data as u32,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn key_input(vk: u16, scan: u16, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    pub fn pointer_move(dx: i32, dy: i32) -> Result<(), String> {
        send(&[mouse(dx, dy, 0, MOUSEEVENTF_MOVE)])
    }

    pub fn pointer_button(button: Button, down: bool) -> Result<(), String> {
        let flags = match (button, down) {
            (Button::Left, true) => MOUSEEVENTF_LEFTDOWN,
            (Button::Left, false) => MOUSEEVENTF_LEFTUP,
            (Button::Right, true) => MOUSEEVENTF_RIGHTDOWN,
            (Button::Right, false) => MOUSEEVENTF_RIGHTUP,
            (Button::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
            (Button::Middle, false) => MOUSEEVENTF_MIDDLEUP,
            (Button::Back | Button::Forward, true) => MOUSEEVENTF_XDOWN,
            (Button::Back | Button::Forward, false) => MOUSEEVENTF_XUP,
        };
        // The side buttons say which one in the data: 1 is back, 2 is forward.
        let data = match button {
            Button::Back => 1,
            Button::Forward => 2,
            _ => 0,
        };
        send(&[mouse(0, 0, data, flags)])
    }

    pub fn scroll(dx: i32, dy: i32) -> Result<(), String> {
        // One wheel notch is 120; a notch is about 20 pixels of finger travel.
        let mut inputs = Vec::new();
        if dy != 0 {
            inputs.push(mouse(0, 0, dy * 6, MOUSEEVENTF_WHEEL));
        }
        if dx != 0 {
            inputs.push(mouse(0, 0, dx * 6, MOUSEEVENTF_HWHEEL));
        }
        if inputs.is_empty() {
            return Ok(());
        }
        send(&inputs)
    }

    pub fn type_text(text: &str) -> Result<(), String> {
        let mut inputs = Vec::new();
        for unit in text.encode_utf16() {
            inputs.push(key_input(0, unit, KEYEVENTF_UNICODE));
            inputs.push(key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
        }
        send(&inputs)
    }

    /// The system's virtual key for `code`, and whether it is an "extended" key.
    fn virtual_key(code: Code) -> (u16, bool) {
        match code {
            Code::Letter(c) => (c.to_ascii_uppercase() as u16, false),
            Code::Zero => (0x30, false),
            Code::Plus => (0xBB, false),
            Code::Minus => (0xBD, false),
            Code::LeftBracket => (0xDB, false),
            Code::RightBracket => (0xDD, false),
            Code::Space => (0x20, false),
            Code::Tab => (0x09, false),
            Code::Enter => (0x0D, false),
            Code::Escape => (0x1B, false),
            Code::Backspace => (0x08, false),
            Code::Delete => (0x2E, true),
            Code::Left => (0x25, true),
            Code::Up => (0x26, true),
            Code::Right => (0x27, true),
            Code::Down => (0x28, true),
            Code::Home => (0x24, true),
            Code::End => (0x23, true),
            Code::PageUp => (0x21, true),
            Code::PageDown => (0x22, true),
            Code::Function(n) => (0x6F + u16::from(n.clamp(1, 12)), false),
            Code::Super => (0x5B, false),
        }
    }

    pub fn key(code: Code, down: bool) -> Result<(), String> {
        let (vk, extended) = virtual_key(code);
        let mut flags = if extended { KEYEVENTF_EXTENDEDKEY } else { 0 };
        if !down {
            flags |= KEYEVENTF_KEYUP;
        }
        send(&[self::key_input(vk, 0, flags)])
    }

    pub fn modifier(m: Modifier, down: bool) -> Result<(), String> {
        let vk = match m {
            Modifier::Shift => 0x10,
            Modifier::Ctrl => 0x11,
            Modifier::Alt => 0x12,
            Modifier::Meta => 0x5B,
        };
        send(&[self::key_input(
            vk,
            0,
            if down { 0 } else { KEYEVENTF_KEYUP },
        )])
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
mod imp {
    use super::{Button, Code, Modifier};
    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::process::{Command, Stdio};
    use std::sync::Mutex;

    const EV_SYN: u16 = 0;
    const EV_KEY: u16 = 1;
    const EV_REL: u16 = 2;
    const REL_X: u16 = 0;
    const REL_Y: u16 = 1;
    const REL_HWHEEL: u16 = 6;
    const REL_WHEEL: u16 = 8;
    const BTN_LEFT: u16 = 0x110;
    const BTN_RIGHT: u16 = 0x111;
    const BTN_MIDDLE: u16 = 0x112;
    const BTN_SIDE: u16 = 0x113;
    const BTN_EXTRA: u16 = 0x114;

    const UI_SET_EVBIT: libc::c_ulong = 0x4004_5564;
    const UI_SET_KEYBIT: libc::c_ulong = 0x4004_5565;
    const UI_SET_RELBIT: libc::c_ulong = 0x4004_5566;
    const UI_DEV_CREATE: libc::c_ulong = 0x5501;

    #[repr(C)]
    struct InputEvent {
        time: libc::timeval,
        kind: u16,
        code: u16,
        value: i32,
    }

    /// The legacy `struct uinput_user_dev`, written once to name the device.
    #[repr(C)]
    struct UserDev {
        name: [u8; 80],
        bustype: u16,
        vendor: u16,
        product: u16,
        version: u16,
        ff_effects_max: u32,
        absmax: [i32; 64],
        absmin: [i32; 64],
        absfuzz: [i32; 64],
        absflat: [i32; 64],
    }

    struct Device {
        file: File,
        wheel_x: i32,
        wheel_y: i32,
    }

    static DEVICE: Mutex<Option<Device>> = Mutex::new(None);

    fn ioctl(file: &File, request: libc::c_ulong, arg: libc::c_int) -> Result<(), String> {
        // SAFETY: plain integer argument ioctls on an fd we own.
        let rc = unsafe { libc::ioctl(file.as_raw_fd(), request as _, arg) };
        if rc < 0 {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            Ok(())
        }
    }

    fn create() -> Result<Device, String> {
        let mut file = OpenOptions::new()
            .write(true)
            .open("/dev/uinput")
            .map_err(|e| {
                format!(
                    "can't open /dev/uinput ({e}); give your user access to it to use the trackpad"
                )
            })?;
        ioctl(&file, UI_SET_EVBIT, EV_KEY as i32)?;
        ioctl(&file, UI_SET_EVBIT, EV_REL as i32)?;
        ioctl(&file, UI_SET_EVBIT, EV_SYN as i32)?;
        for b in [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE, BTN_SIDE, BTN_EXTRA] {
            ioctl(&file, UI_SET_KEYBIT, b as i32)?;
        }
        // The keyboard keys too, for a keyboard handed over from another computer.
        for k in 1u16..=248 {
            ioctl(&file, UI_SET_KEYBIT, k as i32)?;
        }
        for r in [REL_X, REL_Y, REL_WHEEL, REL_HWHEEL] {
            ioctl(&file, UI_SET_RELBIT, r as i32)?;
        }
        let mut dev = UserDev {
            name: [0; 80],
            bustype: 0x06, // BUS_VIRTUAL
            vendor: 0x4c41,
            product: 0x0001,
            version: 1,
            ff_effects_max: 0,
            absmax: [0; 64],
            absmin: [0; 64],
            absfuzz: [0; 64],
            absflat: [0; 64],
        };
        let name = b"Link All virtual trackpad";
        dev.name[..name.len()].copy_from_slice(name);
        // SAFETY: UserDev is repr(C) plain data.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (&dev as *const UserDev).cast::<u8>(),
                std::mem::size_of::<UserDev>(),
            )
        };
        file.write_all(bytes).map_err(|e| e.to_string())?;
        ioctl(&file, UI_DEV_CREATE, 0)?;
        Ok(Device {
            file,
            wheel_x: 0,
            wheel_y: 0,
        })
    }

    fn emit(dev: &mut Device, events: &[(u16, u16, i32)]) -> Result<(), String> {
        let mut buf = Vec::with_capacity((events.len() + 1) * std::mem::size_of::<InputEvent>());
        for &(kind, code, value) in events.iter().chain(&[(EV_SYN, 0, 0)]) {
            let ev = InputEvent {
                time: libc::timeval {
                    tv_sec: 0,
                    tv_usec: 0,
                },
                kind,
                code,
                value,
            };
            // SAFETY: InputEvent is repr(C) plain data.
            buf.extend_from_slice(unsafe {
                std::slice::from_raw_parts(
                    (&ev as *const InputEvent).cast::<u8>(),
                    std::mem::size_of::<InputEvent>(),
                )
            });
        }
        dev.file.write_all(&buf).map_err(|e| e.to_string())
    }

    fn with_device(f: impl FnOnce(&mut Device) -> Result<(), String>) -> Result<(), String> {
        let mut guard = DEVICE.lock().unwrap();
        if guard.is_none() {
            *guard = Some(create()?);
        }
        let result = f(guard.as_mut().unwrap());
        if result.is_err() {
            // A stale device (the file went away): build a new one next time.
            *guard = None;
        }
        result
    }

    pub fn pointer_move(dx: i32, dy: i32) -> Result<(), String> {
        with_device(|d| emit(d, &[(EV_REL, REL_X, dx), (EV_REL, REL_Y, dy)]))
    }

    /// Presses or releases the key with this evdev code.
    pub fn evdev_key(code: u16, down: bool) -> Result<(), String> {
        with_device(|d| emit(d, &[(EV_KEY, code, i32::from(down))]))
    }

    pub fn pointer_button(button: Button, down: bool) -> Result<(), String> {
        let code = match button {
            Button::Left => BTN_LEFT,
            Button::Right => BTN_RIGHT,
            Button::Middle => BTN_MIDDLE,
            Button::Back => BTN_SIDE,
            Button::Forward => BTN_EXTRA,
        };
        with_device(|d| emit(d, &[(EV_KEY, code, i32::from(down))]))
    }

    pub fn scroll(dx: i32, dy: i32) -> Result<(), String> {
        // The wheel counts notches; collect finger travel until it adds up to one.
        const PER_NOTCH: i32 = 20;
        with_device(|d| {
            d.wheel_y += dy;
            d.wheel_x += dx;
            let notches_y = d.wheel_y / PER_NOTCH;
            let notches_x = d.wheel_x / PER_NOTCH;
            d.wheel_y -= notches_y * PER_NOTCH;
            d.wheel_x -= notches_x * PER_NOTCH;
            let mut events = Vec::new();
            if notches_y != 0 {
                events.push((EV_REL, REL_WHEEL, notches_y));
            }
            if notches_x != 0 {
                events.push((EV_REL, REL_HWHEEL, notches_x));
            }
            if events.is_empty() {
                return Ok(());
            }
            emit(d, &events)
        })
    }

    fn first_that_works(candidates: &[Vec<String>]) -> Result<(), String> {
        let mut missing = Vec::new();
        for c in candidates {
            match Command::new(&c[0])
                .args(&c[1..])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
            {
                Ok(s) if s.success() => return Ok(()),
                Ok(_) => {}
                Err(_) => missing.push(c[0].clone()),
            }
        }
        Err(format!("needs one of: {}", missing.join(", ")))
    }

    pub fn type_text(text: &str) -> Result<(), String> {
        first_that_works(&[
            vec!["xdotool".into(), "type".into(), "--".into(), text.into()],
            vec!["wtype".into(), "--".into(), text.into()],
        ])
    }

    /// The X11 key name for `code`.
    fn key_name(code: Code) -> String {
        match code {
            Code::Letter(c) => c.to_string(),
            Code::Zero => "0".into(),
            Code::Plus => "equal".into(),
            Code::Minus => "minus".into(),
            Code::LeftBracket => "bracketleft".into(),
            Code::RightBracket => "bracketright".into(),
            Code::Space => "space".into(),
            Code::Tab => "Tab".into(),
            Code::Enter => "Return".into(),
            Code::Escape => "Escape".into(),
            Code::Backspace => "BackSpace".into(),
            Code::Delete => "Delete".into(),
            Code::Left => "Left".into(),
            Code::Right => "Right".into(),
            Code::Up => "Up".into(),
            Code::Down => "Down".into(),
            Code::Home => "Home".into(),
            Code::End => "End".into(),
            Code::PageUp => "Prior".into(),
            Code::PageDown => "Next".into(),
            Code::Function(n) => format!("F{n}"),
            Code::Super => "Super_L".into(),
        }
    }

    fn press_or_release(name: &str, down: bool) -> Result<(), String> {
        if down {
            first_that_works(&[
                vec!["xdotool".into(), "keydown".into(), name.into()],
                vec!["wtype".into(), "-P".into(), name.into()],
            ])
        } else {
            first_that_works(&[
                vec!["xdotool".into(), "keyup".into(), name.into()],
                vec!["wtype".into(), "-p".into(), name.into()],
            ])
        }
    }

    pub fn key(code: Code, down: bool) -> Result<(), String> {
        press_or_release(&key_name(code), down)
    }

    pub fn modifier(m: Modifier, down: bool) -> Result<(), String> {
        let name = match m {
            Modifier::Shift => "Shift_L",
            Modifier::Ctrl => "Control_L",
            Modifier::Alt => "Alt_L",
            Modifier::Meta => "Super_L",
        };
        press_or_release(name, down)
    }
}

#[cfg(target_os = "android")]
mod imp {
    use super::{Button, Code, Modifier};

    const NO: &str = "this device can't be controlled";

    pub fn pointer_move(_: i32, _: i32) -> Result<(), String> {
        Err(NO.into())
    }
    pub fn pointer_button(_: Button, _: bool) -> Result<(), String> {
        Err(NO.into())
    }
    pub fn scroll(_: i32, _: i32) -> Result<(), String> {
        Err(NO.into())
    }
    pub fn type_text(_: &str) -> Result<(), String> {
        Err(NO.into())
    }
    pub fn key(_: Code, _: bool) -> Result<(), String> {
        Err(NO.into())
    }
    pub fn modifier(_: Modifier, _: bool) -> Result<(), String> {
        Err(NO.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_round_trips_by_name() {
        for k in Key::ALL {
            assert_eq!(Key::parse(&k.name()), Some(k));
        }
    }

    #[test]
    fn odd_key_names_are_refused() {
        for bad in ["", "ctrl", "Enter", "insert", "f13", "enter; reboot"] {
            assert_eq!(Key::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn five_buttons_exist() {
        assert_eq!(Button::from_wire(0), Some(Button::Left));
        assert_eq!(Button::from_wire(1), Some(Button::Right));
        assert_eq!(Button::from_wire(2), Some(Button::Middle));
        assert_eq!(Button::from_wire(3), Some(Button::Back));
        assert_eq!(Button::from_wire(4), Some(Button::Forward));
        assert_eq!(Button::from_wire(5), None);
    }

    #[test]
    fn function_keys_are_f1_to_f12_only() {
        for n in 1..=12 {
            let key = Key::Function(n);
            assert_eq!(Key::parse(&key.name()), Some(key));
        }
        for bad in ["f0", "f13", "f01", "f+1", "f", "F1", "ff1"] {
            assert_eq!(Key::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn every_modifier_and_shortcut_round_trips_by_name() {
        for m in Modifier::ALL {
            assert_eq!(Modifier::parse(m.name()), Some(m));
        }
        for s in Shortcut::ALL {
            assert_eq!(Shortcut::parse(s.name()), Some(s));
        }
        for bad in [
            "",
            "super",
            "Shift",
            "copy; reboot",
            "alt+f4",
            "close_window",
            "shutdown",
        ] {
            assert!(
                Modifier::parse(bad).is_none() && Shortcut::parse(bad).is_none(),
                "{bad}"
            );
        }
    }

    #[test]
    fn shortcuts_follow_each_systems_conventions() {
        let (mods, code) = Shortcut::Copy.chord(System::Mac);
        assert_eq!((mods, code), (vec![Modifier::Meta], Code::Letter('c')));
        let (mods, code) = Shortcut::Copy.chord(System::Windows);
        assert_eq!((mods, code), (vec![Modifier::Ctrl], Code::Letter('c')));
        assert_eq!(
            Shortcut::Redo.chord(System::Mac).0,
            vec![Modifier::Meta, Modifier::Shift]
        );
        assert_eq!(Shortcut::Redo.chord(System::Other).1, Code::Letter('y'));
        assert_eq!(Shortcut::Back.chord(System::Mac).1, Code::LeftBracket);
        assert_eq!(Shortcut::Back.chord(System::Windows).1, Code::Left);
        assert_eq!(
            Shortcut::SwitchApp.chord(System::Mac).0,
            vec![Modifier::Meta]
        );
        assert_eq!(
            Shortcut::SwitchApp.chord(System::Other).0,
            vec![Modifier::Alt]
        );
        // Every shortcut has a key on every system.
        for s in Shortcut::ALL {
            for system in [System::Mac, System::Windows, System::Other] {
                let (_, code) = s.chord(system);
                assert!(!matches!(code, Code::Function(0)));
            }
        }
    }

    #[test]
    fn phone_numbers_are_checked() {
        for ok in ["+1 234-567-8900", "(020) 7946 0958", "*#06#", "112"] {
            assert_eq!(clean_phone_number(ok).as_deref(), Some(ok), "{ok}");
        }
        for bad in [
            "",
            "ab",
            "12",
            "tel:123",
            "123;rm",
            "1234\n5678",
            "+++",
            &"1".repeat(40),
        ] {
            assert_eq!(clean_phone_number(bad), None, "{bad:?}");
        }
        assert_eq!(
            clean_phone_number("  555 0100 ").as_deref(),
            Some("555 0100")
        );
    }

    #[test]
    fn a_verification_code_notification_is_recognised() {
        assert_eq!(
            verification_code("Verification code from GitHub", " 482910 ").as_deref(),
            Some("482910")
        );
        assert_eq!(
            verification_code("Verification code", "1234").as_deref(),
            Some("1234")
        );
        assert_eq!(verification_code("Verification code", "123"), None);
        assert_eq!(verification_code("Verification code", "12345678 9"), None);
        assert_eq!(verification_code("Mom", "482910"), None);
        assert_eq!(
            verification_code("Verification code from X", "see 482910"),
            None
        );
    }

    #[test]
    fn steps_are_clamped() {
        assert_eq!(clamp_step(1_000_000), MAX_STEP);
        assert_eq!(clamp_step(-1_000_000), -MAX_STEP);
        assert_eq!(clamp_step(15), 15);
    }
}

#[cfg(test)]
mod edge_tests {
    use super::*;

    #[test]
    fn edge_wire_round_trips_and_opposites() {
        for n in 0..4 {
            let edge = Edge::from_wire(n).unwrap();
            assert_eq!(edge.to_wire(), n);
            assert_eq!(edge.opposite().opposite(), edge);
        }
        assert_eq!(Edge::from_wire(4), None);
        assert_eq!(Edge::Right.opposite(), Edge::Left);
    }

    #[test]
    fn fractions_cover_the_whole_edge() {
        assert_eq!(to_fraction(0, 1080), 0);
        assert_eq!(to_fraction(1079, 1080), 65535);
        assert_eq!(from_fraction(0, 1080), 0);
        assert_eq!(from_fraction(65535, 1080), 1079);
        // Out of range is clamped, a one-pixel screen is not a division by zero.
        assert_eq!(to_fraction(-5, 1080), 0);
        assert_eq!(to_fraction(5000, 1080), 65535);
        assert_eq!(to_fraction(3, 1), 0);
        assert_eq!(from_fraction(40000, 1), 0);
    }

    #[test]
    fn a_pointer_arrives_on_the_opposite_side_at_the_same_height() {
        // Left the host over its right edge, halfway down: it enters from the
        // left of a 1920x1080 screen, halfway down.
        let (x, y) = entry_point(Edge::Right.opposite(), 32768, 1920, 1080);
        assert_eq!(x, 0);
        assert!((538..=541).contains(&y), "y was {y}");
        assert_eq!(entry_point(Edge::Right, 0, 1920, 1080), (1919, 0));
        assert_eq!(entry_point(Edge::Top, 65535, 1920, 1080), (1919, 0));
        assert_eq!(entry_point(Edge::Bottom, 0, 1920, 1080), (0, 1079));
    }

    #[test]
    fn only_a_push_outward_counts() {
        assert_eq!(outward_push(Edge::Left, -10, 4), 10);
        assert_eq!(outward_push(Edge::Left, 10, 0), -10);
        assert_eq!(outward_push(Edge::Right, 7, 0), 7);
        assert_eq!(outward_push(Edge::Top, 0, -3), 3);
        assert_eq!(outward_push(Edge::Bottom, 0, 9), 9);
    }

    #[test]
    fn on_edge_says_where_along_the_edge() {
        assert_eq!(on_edge(Edge::Left, 0, 0, 1920, 1080), Some(0));
        assert_eq!(on_edge(Edge::Left, 1, 500, 1920, 1080), None);
        assert_eq!(on_edge(Edge::Right, 1919, 1079, 1920, 1080), Some(65535));
        assert!(on_edge(Edge::Bottom, 960, 1079, 1920, 1080).is_some());
        assert_eq!(on_edge(Edge::Top, 960, 5, 1920, 1080), None);
    }
}

#[cfg(test)]
mod keymap_tests {
    use super::*;

    #[test]
    fn plain_keys_keep_their_number_on_linux() {
        // Escape, A, Enter, Space, F12.
        for sc in [0x01, 0x1E, 0x1C, 0x39, 0x58] {
            assert_eq!(scancode_to_evdev(sc), Some(sc));
        }
        assert_eq!(scancode_to_evdev(0xE048), Some(103), "up arrow");
        assert_eq!(scancode_to_evdev(0xE053), Some(111), "delete");
        assert_eq!(scancode_to_evdev(0xE0FF), None);
        assert_eq!(scancode_to_evdev(0x59), None);
    }

    #[test]
    fn linux_codes_round_trip_through_scan_codes() {
        for sc in (0x01u16..=0x58).chain(0xE000..=0xE0FF) {
            if let Some(evdev) = scancode_to_evdev(sc) {
                assert_eq!(evdev_to_scancode(evdev), Some(sc), "{sc:#x}");
            }
        }
        assert_eq!(evdev_to_scancode(0), None);
    }

    #[test]
    fn letters_land_on_the_mac_letters() {
        // A S D F on the home row, Q W E R on the one above.
        assert_eq!(
            [0x1E, 0x1F, 0x20, 0x21].map(|s| scancode_to_mac(s).unwrap()),
            [0, 1, 2, 3]
        );
        assert_eq!(
            [0x10, 0x11, 0x12, 0x13].map(|s| scancode_to_mac(s).unwrap()),
            [12, 13, 14, 15]
        );
        assert_eq!(scancode_to_mac(0x1C), Some(36), "enter");
        assert_eq!(scancode_to_mac(0xE04B), Some(123), "left arrow");
    }

    #[test]
    fn control_acts_as_command_on_a_mac() {
        let ctrl = scancode_to_mac(0x1D).unwrap();
        assert_eq!(mac_modifier_flag(ctrl), 0x0010_0000, "command");
        let win = scancode_to_mac(0xE05B).unwrap();
        assert_eq!(mac_modifier_flag(win), 0x0004_0000, "control");
        let alt = scancode_to_mac(0x38).unwrap();
        assert_eq!(mac_modifier_flag(alt), 0x0008_0000, "option");
        assert_eq!(
            mac_modifier_flag(scancode_to_mac(0x2A).unwrap()),
            0x0002_0000,
            "shift"
        );
        assert_eq!(mac_modifier_flag(0), 0, "a letter sets no flag");
    }

    #[test]
    fn no_two_keys_share_a_mac_code() {
        let mut seen = std::collections::HashMap::new();
        let all = (0x01u16..=0x58).chain(0xE000..=0xE0FF);
        for sc in all {
            if let Some(code) = scancode_to_mac(sc) {
                if let Some(other) = seen.insert(code, sc) {
                    panic!("{sc:#x} and {other:#x} both map to mac key {code}");
                }
            }
        }
    }
}

#[cfg(all(test, unix, not(target_os = "macos"), not(target_os = "android")))]
mod x11_tests {
    use super::*;

    /// Runs against a real X server (Xvfb will do); skipped when there is none.
    #[test]
    fn the_pointer_can_be_placed_and_read_back() {
        if std::env::var_os("DISPLAY").is_none() {
            eprintln!("no DISPLAY: skipping");
            return;
        }
        let (w, h) = screen_size().expect("an X screen");
        assert!(w > 0 && h > 0);
        set_cursor_position(120, 80).expect("placed");
        assert_eq!(cursor_position(), Some((120, 80)));
        let (x, y) = entry_point(Edge::Right, 32768, w, h);
        set_cursor_position(x, y).expect("placed on the edge");
        assert_eq!(cursor_position(), Some((w - 1, y)));
        assert!(on_edge(Edge::Right, w - 1, y, w, h).is_some());
    }
}
