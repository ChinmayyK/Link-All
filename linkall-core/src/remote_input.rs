//! Mouse and keyboard input a paired phone sends to this computer: the phone
//! as a trackpad and keyboard. Like [`crate::remote_control`], it is only
//! reachable when the user has turned "Remote control" on, and the keys a
//! phone can press are a fixed list; typed text is sent as characters, never
//! run as a command.

/// A mouse button, as numbered on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

impl Button {
    pub fn from_wire(n: u8) -> Option<Button> {
        match n {
            0 => Some(Button::Left),
            1 => Some(Button::Right),
            2 => Some(Button::Middle),
            _ => None,
        }
    }
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
}

impl Key {
    pub const ALL: [Key; 8] = [
        Key::Backspace,
        Key::Enter,
        Key::Escape,
        Key::Tab,
        Key::Left,
        Key::Right,
        Key::Up,
        Key::Down,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Key::Backspace => "backspace",
            Key::Enter => "enter",
            Key::Escape => "escape",
            Key::Tab => "tab",
            Key::Left => "left",
            Key::Right => "right",
            Key::Up => "up",
            Key::Down => "down",
        }
    }

    pub fn parse(name: &str) -> Option<Key> {
        Key::ALL.into_iter().find(|k| k.name() == name)
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

/// Moves the pointer by (`dx`, `dy`) from where it is.
pub fn pointer_move(dx: i32, dy: i32) -> Result<(), String> {
    imp::pointer_move(clamp_step(dx), clamp_step(dy))
}

/// Presses or releases a mouse button.
pub fn pointer_button(button: Button, down: bool) -> Result<(), String> {
    imp::pointer_button(button, down)
}

/// Scrolls by (`dx`, `dy`) in pixel-like units; positive `dy` moves the
/// content up, as with a trackpad's natural direction off.
pub fn scroll(dx: i32, dy: i32) -> Result<(), String> {
    imp::scroll(clamp_step(dx), clamp_step(dy))
}

/// Types `text` as characters.
pub fn type_text(text: &str) -> Result<(), String> {
    let text: String = text.chars().take(MAX_TEXT_CHARS).collect();
    if text.is_empty() {
        return Ok(());
    }
    imp::type_text(&text)
}

/// Presses and releases one of the fixed keys.
pub fn press_key(key: Key) -> Result<(), String> {
    imp::press_key(key)
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{Button, Key};
    use std::ffi::c_void;
    use std::sync::Mutex;
    use std::time::Instant;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    #[link(name = "CoreFoundation", kind = "framework")]
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
        fn CGEventPost(tap: u32, event: *mut c_void);
        fn CFRelease(obj: *const c_void);
    }

    const HID_TAP: u32 = 0;
    const CLICK_STATE: u32 = 1;
    const MOVED: u32 = 5;

    struct State {
        down: [bool; 3],
        last_click: Option<(Instant, u8)>,
    }

    static STATE: Mutex<State> = Mutex::new(State {
        down: [false; 3],
        last_click: None,
    });

    fn post(event: *mut c_void) -> Result<(), String> {
        if event.is_null() {
            return Err("macOS didn't create the event".into());
        }
        unsafe {
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

    pub fn press_key(key: Key) -> Result<(), String> {
        let code: u16 = match key {
            Key::Backspace => 51,
            Key::Enter => 36,
            Key::Escape => 53,
            Key::Tab => 48,
            Key::Left => 123,
            Key::Right => 124,
            Key::Down => 125,
            Key::Up => 126,
        };
        for down in [true, false] {
            post(unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), code, down) })?;
        }
        Ok(())
    }
}

#[cfg(windows)]
mod imp {
    use super::{Button, Key};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
        MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, VK_BACK, VK_DOWN, VK_ESCAPE, VK_LEFT,
        VK_RETURN, VK_RIGHT, VK_TAB, VK_UP,
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

    fn key(vk: u16, scan: u16, flags: u32) -> INPUT {
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
        };
        send(&[mouse(0, 0, 0, flags)])
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
            inputs.push(key(0, unit, KEYEVENTF_UNICODE));
            inputs.push(key(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
        }
        send(&inputs)
    }

    pub fn press_key(k: Key) -> Result<(), String> {
        let vk = match k {
            Key::Backspace => VK_BACK,
            Key::Enter => VK_RETURN,
            Key::Escape => VK_ESCAPE,
            Key::Tab => VK_TAB,
            Key::Left => VK_LEFT,
            Key::Right => VK_RIGHT,
            Key::Up => VK_UP,
            Key::Down => VK_DOWN,
        };
        send(&[key(vk, 0, 0), key(vk, 0, KEYEVENTF_KEYUP)])
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
mod imp {
    use super::{Button, Key};
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
        for b in [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE] {
            ioctl(&file, UI_SET_KEYBIT, b as i32)?;
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

    pub fn pointer_button(button: Button, down: bool) -> Result<(), String> {
        let code = match button {
            Button::Left => BTN_LEFT,
            Button::Right => BTN_RIGHT,
            Button::Middle => BTN_MIDDLE,
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

    pub fn press_key(key: Key) -> Result<(), String> {
        let (xdo, wtype) = match key {
            Key::Backspace => ("BackSpace", "BackSpace"),
            Key::Enter => ("Return", "Return"),
            Key::Escape => ("Escape", "Escape"),
            Key::Tab => ("Tab", "Tab"),
            Key::Left => ("Left", "Left"),
            Key::Right => ("Right", "Right"),
            Key::Up => ("Up", "Up"),
            Key::Down => ("Down", "Down"),
        };
        first_that_works(&[
            vec!["xdotool".into(), "key".into(), xdo.into()],
            vec!["wtype".into(), "-k".into(), wtype.into()],
        ])
    }
}

#[cfg(target_os = "android")]
mod imp {
    use super::{Button, Key};

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
    pub fn press_key(_: Key) -> Result<(), String> {
        Err(NO.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_round_trips_by_name() {
        for k in Key::ALL {
            assert_eq!(Key::parse(k.name()), Some(k));
        }
    }

    #[test]
    fn odd_key_names_are_refused() {
        for bad in ["", "ctrl", "Enter", "delete", "f4", "enter; reboot"] {
            assert_eq!(Key::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn only_three_buttons_exist() {
        assert_eq!(Button::from_wire(0), Some(Button::Left));
        assert_eq!(Button::from_wire(1), Some(Button::Right));
        assert_eq!(Button::from_wire(2), Some(Button::Middle));
        assert_eq!(Button::from_wire(3), None);
    }

    #[test]
    fn steps_are_clamped() {
        assert_eq!(clamp_step(1_000_000), MAX_STEP);
        assert_eq!(clamp_step(-1_000_000), -MAX_STEP);
        assert_eq!(clamp_step(15), 15);
    }
}
