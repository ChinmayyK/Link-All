//! What a paired phone may ask this computer to do: media keys, volume and
//! slide keys. Nothing else is reachable: the action is checked against a
//! fixed list, no shell is involved, and the setting that allows it is off
//! until the user turns it on.

/// One thing a phone can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    PlayPause,
    NextTrack,
    PreviousTrack,
    VolumeUp,
    VolumeDown,
    Mute,
    SlideNext,
    SlidePrevious,
}

impl Action {
    pub const ALL: [Action; 8] = [
        Action::PlayPause,
        Action::NextTrack,
        Action::PreviousTrack,
        Action::VolumeUp,
        Action::VolumeDown,
        Action::Mute,
        Action::SlideNext,
        Action::SlidePrevious,
    ];

    /// The name that travels in the message.
    pub fn name(self) -> &'static str {
        match self {
            Action::PlayPause => "play_pause",
            Action::NextTrack => "next_track",
            Action::PreviousTrack => "previous_track",
            Action::VolumeUp => "volume_up",
            Action::VolumeDown => "volume_down",
            Action::Mute => "mute",
            Action::SlideNext => "slide_next",
            Action::SlidePrevious => "slide_previous",
        }
    }

    pub fn parse(name: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.name() == name)
    }
}

/// Carries out [`Action`] on this computer. Blocks briefly, so call it from a
/// blocking thread.
pub fn perform(action: Action) -> Result<(), String> {
    imp::perform(action)
}

#[cfg(target_os = "macos")]
mod imp {
    use super::Action;
    use std::process::Command;

    // Media keys are system-defined events; AppleScript's JavaScript bridge can post them.
    const MEDIA_KEY: &str = r#"
ObjC.import('Cocoa');
function key(code) {
  [true, false].forEach(function (down) {
    var flags = down ? 0xa00 : 0xb00;
    var ev = $.NSEvent.otherEventWithTypeLocationModifierFlagsTimestampWindowNumberContextSubtypeData1Data2(
      14, $.NSMakePoint(0, 0), flags, 0, 0, null, 8, (code << 16) | flags, -1);
    $.CGEventPost(0, ev.CGEvent);
  });
}
"#;

    fn run(args: &[&str]) -> Result<(), String> {
        let out = Command::new("/usr/bin/osascript")
            .args(args)
            .output()
            .map_err(|e| format!("couldn't run osascript: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }

    fn media(code: u32) -> Result<(), String> {
        run(&[
            "-l",
            "JavaScript",
            "-e",
            &format!("{MEDIA_KEY} key({code});"),
        ])
    }

    fn arrow(code: u32) -> Result<(), String> {
        run(&[
            "-l",
            "JavaScript",
            "-e",
            &format!(
                "ObjC.import('Cocoa'); [true, false].forEach(function (down) {{ \
                 $.CGEventPost($.kCGHIDEventTap, $.CGEventCreateKeyboardEvent($(), {code}, down)); }});"
            ),
        ])
    }

    fn volume(delta: i32) -> Result<(), String> {
        run(&[
            "-e",
            &format!(
                "set v to (output volume of (get volume settings)) + ({delta})\n\
                 if v > 100 then set v to 100\n\
                 if v < 0 then set v to 0\n\
                 set volume output volume v"
            ),
        ])
    }

    pub fn perform(action: Action) -> Result<(), String> {
        match action {
            Action::PlayPause => media(16),
            Action::NextTrack => media(17),
            Action::PreviousTrack => media(18),
            Action::VolumeUp => volume(6),
            Action::VolumeDown => volume(-6),
            Action::Mute => run(&[
                "-e",
                "set volume output muted (not (output muted of (get volume settings)))",
            ]),
            Action::SlideNext => arrow(124),
            Action::SlidePrevious => arrow(123),
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::Action;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_LEFT,
        VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_RIGHT, VK_VOLUME_DOWN,
        VK_VOLUME_MUTE, VK_VOLUME_UP,
    };

    fn press(vk: u16) -> Result<(), String> {
        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let inputs = [key(0), key(KEYEVENTF_KEYUP)];
        let sent = unsafe { SendInput(2, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
        if sent == 2 {
            Ok(())
        } else {
            Err("Windows didn't accept the key press".to_string())
        }
    }

    pub fn perform(action: Action) -> Result<(), String> {
        press(match action {
            Action::PlayPause => VK_MEDIA_PLAY_PAUSE,
            Action::NextTrack => VK_MEDIA_NEXT_TRACK,
            Action::PreviousTrack => VK_MEDIA_PREV_TRACK,
            Action::VolumeUp => VK_VOLUME_UP,
            Action::VolumeDown => VK_VOLUME_DOWN,
            Action::Mute => VK_VOLUME_MUTE,
            Action::SlideNext => VK_RIGHT,
            Action::SlidePrevious => VK_LEFT,
        })
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
mod imp {
    use super::Action;
    use std::process::{Command, Stdio};

    /// Runs the first of `candidates` that is installed and succeeds.
    fn first_that_works(candidates: &[&[&str]]) -> Result<(), String> {
        let mut missing = Vec::new();
        for c in candidates {
            match Command::new(c[0])
                .args(&c[1..])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
            {
                Ok(s) if s.success() => return Ok(()),
                Ok(_) => {}
                Err(_) => missing.push(c[0]),
            }
        }
        Err(format!("needs one of: {}", missing.join(", ")))
    }

    pub fn perform(action: Action) -> Result<(), String> {
        match action {
            Action::PlayPause => first_that_works(&[&["playerctl", "play-pause"]]),
            Action::NextTrack => first_that_works(&[&["playerctl", "next"]]),
            Action::PreviousTrack => first_that_works(&[&["playerctl", "previous"]]),
            Action::VolumeUp => first_that_works(&[
                &[
                    "wpctl",
                    "set-volume",
                    "-l",
                    "1",
                    "@DEFAULT_AUDIO_SINK@",
                    "5%+",
                ],
                &["pactl", "set-sink-volume", "@DEFAULT_SINK@", "+5%"],
            ]),
            Action::VolumeDown => first_that_works(&[
                &["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"],
                &["pactl", "set-sink-volume", "@DEFAULT_SINK@", "-5%"],
            ]),
            Action::Mute => first_that_works(&[
                &["wpctl", "set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"],
                &["pactl", "set-sink-mute", "@DEFAULT_SINK@", "toggle"],
            ]),
            Action::SlideNext => {
                first_that_works(&[&["xdotool", "key", "Right"], &["wtype", "-k", "Right"]])
            }
            Action::SlidePrevious => {
                first_that_works(&[&["xdotool", "key", "Left"], &["wtype", "-k", "Left"]])
            }
        }
    }
}

#[cfg(target_os = "android")]
mod imp {
    use super::Action;

    pub fn perform(_action: Action) -> Result<(), String> {
        Err("this device can't be controlled".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_round_trips_by_name() {
        for a in Action::ALL {
            assert_eq!(Action::parse(a.name()), Some(a));
        }
    }

    #[test]
    fn unknown_or_odd_names_are_refused() {
        for bad in [
            "",
            "shutdown",
            "play_pause; rm -rf /",
            "PLAY_PAUSE",
            "lock_screen",
        ] {
            assert_eq!(Action::parse(bad), None, "{bad}");
        }
    }
}
