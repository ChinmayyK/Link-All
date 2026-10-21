//! Now Playing: what is playing on one device, shown on the others with its
//! cover art and controls. Each device reads its own player and tells the
//! paired ones; this module is the data, the commands that may come back, and
//! how this computer reads its own player (Spotify and Music on a Mac, any
//! MPRIS player on Linux). Windows and Android read theirs in their own apps
//! and hand it to the engine.

use serde::{Deserialize, Serialize};

/// The largest cover art sent, in bytes.
pub const MAX_ART_BYTES: usize = 120 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NowPlaying {
    /// Empty when nothing is playing any more.
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    /// The app playing it, "Spotify" or "YouTube".
    pub app: String,
    /// A small JPEG or PNG of the cover, sent once per track.
    pub art: Option<Vec<u8>>,
}

impl NowPlaying {
    /// Whether this is the same track as `other`, whatever the position.
    pub fn same_track(&self, other: &NowPlaying) -> bool {
        self.title == other.title && self.artist == other.artist && self.album == other.album
    }

    pub fn is_empty(&self) -> bool {
        self.title.is_empty()
    }

    /// The same, with limits applied: text lengths, and cover art dropped if it's too big.
    pub fn sanitized(mut self) -> NowPlaying {
        let clip = |s: &mut String, max: usize| {
            if s.chars().count() > max {
                *s = s.chars().take(max).collect();
            }
        };
        clip(&mut self.title, 200);
        clip(&mut self.artist, 200);
        clip(&mut self.album, 200);
        clip(&mut self.app, 60);
        if self.art.as_ref().is_some_and(|a| a.len() > MAX_ART_BYTES) {
            self.art = None;
        }
        self
    }
}

/// A transport command for a device's player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAction {
    Play,
    Pause,
    PlayPause,
    Next,
    Previous,
    /// Move to a position, in milliseconds.
    Seek(u64),
}

impl MediaAction {
    pub fn parse(name: &str, value: i64) -> Option<MediaAction> {
        match name {
            "play" => Some(MediaAction::Play),
            "pause" => Some(MediaAction::Pause),
            "play_pause" => Some(MediaAction::PlayPause),
            "next" => Some(MediaAction::Next),
            "previous" => Some(MediaAction::Previous),
            "seek" => u64::try_from(value).ok().map(MediaAction::Seek),
            _ => None,
        }
    }
}

// ── Reading and controlling this computer's player ──────────────────────────

/// What this computer is playing now, or None. Blocks briefly: run it off the async threads.
pub fn current() -> Option<NowPlaying> {
    imp::current()
}

/// Carries out a command on this computer's player. Returns why it couldn't.
pub fn control(action: MediaAction) -> Result<(), String> {
    imp::control(action)
}

/// One line of `key|state|title|artist|album|position_s|duration|art` from the
/// Mac's AppleScript reader, as a track. Spotify's duration is in milliseconds,
/// Music's in seconds, so the app says which.
pub fn parse_mac_line(line: &str) -> Option<(NowPlaying, Option<String>)> {
    let parts: Vec<&str> = line.trim_end().splitn(8, '|').collect();
    if parts.len() < 7 {
        return None;
    }
    let app = parts[0];
    let playing = match parts[1] {
        "playing" => true,
        "paused" => false,
        _ => return None,
    };
    if parts[2].trim().is_empty() {
        return None;
    }
    // AppleScript prints numbers with the system's decimal mark.
    let number = |s: &str| s.trim().replace(',', ".").parse::<f64>().ok();
    let position_ms = (number(parts[5]).unwrap_or(0.0).max(0.0) * 1000.0) as u64;
    let duration = number(parts[6]).unwrap_or(0.0).max(0.0);
    let duration_ms = if app == "Spotify" {
        duration as u64
    } else {
        (duration * 1000.0) as u64
    };
    let art_url = parts
        .get(7)
        .map(|s| s.trim())
        .filter(|s| s.starts_with("http"))
        .map(str::to_string);
    Some((
        NowPlaying {
            title: parts[2].trim().to_string(),
            artist: parts[3].trim().to_string(),
            album: parts[4].trim().to_string(),
            playing,
            position_ms,
            duration_ms,
            app: app.to_string(),
            art: None,
        },
        art_url,
    ))
}

/// One line of `status|title|artist|album|length_us|artUrl|player` from playerctl.
pub fn parse_playerctl_line(line: &str) -> Option<(NowPlaying, Option<String>)> {
    let parts: Vec<&str> = line.trim_end().splitn(7, '|').collect();
    if parts.len() < 7 {
        return None;
    }
    let playing = match parts[0] {
        "Playing" => true,
        "Paused" => false,
        _ => return None,
    };
    if parts[1].trim().is_empty() {
        return None;
    }
    let length_us = parts[4].trim().parse::<u64>().unwrap_or(0);
    let art = Some(parts[5].trim().to_string()).filter(|s| !s.is_empty());
    let player = parts[6].trim();
    let mut app = player.split('.').next().unwrap_or(player).to_string();
    if let Some(first) = app.get(..1) {
        app = first.to_uppercase() + &app[1..];
    }
    Some((
        NowPlaying {
            title: parts[1].trim().to_string(),
            artist: parts[2].trim().to_string(),
            album: parts[3].trim().to_string(),
            playing,
            position_ms: 0,
            duration_ms: length_us / 1000,
            app,
            art: None,
        },
        art,
    ))
}

/// Cover art from a `file://` path or an http(s) address, small enough to send.
#[cfg(not(target_os = "android"))]
fn fetch_art(location: &str) -> Option<Vec<u8>> {
    let bytes = if let Some(path) = location.strip_prefix("file://") {
        let path = percent_decode(path);
        std::fs::read(path).ok()?
    } else if location.starts_with("http://") || location.starts_with("https://") {
        let out = std::process::Command::new("curl")
            .args(["-s", "-m", "4", "-L", "--max-filesize", "400000", location])
            .output()
            .ok()?;
        out.status.success().then_some(out.stdout)?
    } else {
        return None;
    };
    shrink_art(bytes)
}

/// Scales cover art down to at most 300 pixels and re-encodes it as a JPEG, so it
/// stays small to send.
#[cfg(not(target_os = "android"))]
fn shrink_art(bytes: Vec<u8>) -> Option<Vec<u8>> {
    #[cfg(feature = "image")]
    {
        let img = image::load_from_memory(&bytes).ok()?;
        let img = img.thumbnail(300, 300).to_rgb8();
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Jpeg).ok()?;
        let out = out.into_inner();
        return (out.len() <= MAX_ART_BYTES).then_some(out);
    }
    #[cfg(not(feature = "image"))]
    {
        (bytes.len() <= MAX_ART_BYTES).then_some(bytes)
    }
}

#[cfg(not(target_os = "android"))]
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use std::process::Command;
    use std::sync::Mutex;

    const READER: &str = r#"
on reading(appName)
  if application appName is running then
    tell application appName
      if player state is playing or player state is paused then
        set t to current track
        set s to "paused"
        if player state is playing then set s to "playing"
        set artUrl to ""
        if appName is "Spotify" then set artUrl to artwork url of t
        return appName & "|" & s & "|" & (name of t) & "|" & (artist of t) & "|" & (album of t) & "|" & (player position as string) & "|" & (duration of t as string) & "|" & artUrl
      end if
    end tell
  end if
  return ""
end reading
set a to reading("Spotify")
if a is "" then set a to reading("Music")
return a
"#;

    /// The cover for the last Spotify track, so it is fetched once per track.
    static ART: Mutex<Option<(String, Option<Vec<u8>>)>> = Mutex::new(None);

    pub fn current() -> Option<NowPlaying> {
        let out = Command::new("/usr/bin/osascript")
            .args(["-e", READER])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let line = String::from_utf8_lossy(&out.stdout).to_string();
        let (mut info, art_url) = parse_mac_line(&line)?;
        if let Some(url) = art_url {
            let mut cache = ART.lock().unwrap();
            if cache.as_ref().map(|(u, _)| u != &url).unwrap_or(true) {
                *cache = Some((url.clone(), fetch_art(&url)));
            }
            info.art = cache.as_ref().and_then(|(_, a)| a.clone());
        }
        Some(info.sanitized())
    }

    fn tell(app: &str, what: &str) -> Result<(), String> {
        let out = Command::new("/usr/bin/osascript")
            .args([
                "-e",
                &format!(
                    "if application \"{app}\" is running then tell application \"{app}\" to {what}"
                ),
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }

    pub fn control(action: MediaAction) -> Result<(), String> {
        let (app, _) = match current() {
            Some(info) => (info.app, ()),
            None => {
                return crate::remote_control::perform(match action {
                    MediaAction::Next => crate::remote_control::Action::NextTrack,
                    MediaAction::Previous => crate::remote_control::Action::PreviousTrack,
                    _ => crate::remote_control::Action::PlayPause,
                })
            }
        };
        let command = match action {
            MediaAction::Play => "play".to_string(),
            MediaAction::Pause => "pause".to_string(),
            MediaAction::PlayPause => "playpause".to_string(),
            MediaAction::Next => "next track".to_string(),
            MediaAction::Previous => "previous track".to_string(),
            MediaAction::Seek(ms) => format!("set player position to {}", ms as f64 / 1000.0),
        };
        tell(&app, &command)
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
mod imp {
    use super::*;
    use std::process::Command;

    pub fn current() -> Option<NowPlaying> {
        let out = Command::new("playerctl")
            .args([
                "metadata",
                "--format",
                "{{status}}|{{title}}|{{artist}}|{{album}}|{{mpris:length}}|{{mpris:artUrl}}|{{playerName}}",
            ])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let (mut info, art) = parse_playerctl_line(&String::from_utf8_lossy(&out.stdout))?;
        if let Ok(pos) = Command::new("playerctl").arg("position").output() {
            let secs = String::from_utf8_lossy(&pos.stdout)
                .trim()
                .parse::<f64>()
                .unwrap_or(0.0);
            info.position_ms = (secs.max(0.0) * 1000.0) as u64;
        }
        info.art = art.and_then(|a| fetch_art(&a));
        Some(info.sanitized())
    }

    pub fn control(action: MediaAction) -> Result<(), String> {
        let args: Vec<String> = match action {
            MediaAction::Play => vec!["play".into()],
            MediaAction::Pause => vec!["pause".into()],
            MediaAction::PlayPause => vec!["play-pause".into()],
            MediaAction::Next => vec!["next".into()],
            MediaAction::Previous => vec!["previous".into()],
            MediaAction::Seek(ms) => vec!["position".into(), format!("{}", ms as f64 / 1000.0)],
        };
        let status = Command::new("playerctl")
            .args(&args)
            .status()
            .map_err(|_| "needs playerctl".to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err("no player answered".into())
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::*;

    /// The Windows app reads the system media session itself and hands it to the engine.
    pub fn current() -> Option<NowPlaying> {
        None
    }

    pub fn control(action: MediaAction) -> Result<(), String> {
        use crate::remote_control::{perform, Action};
        match action {
            MediaAction::Play | MediaAction::Pause | MediaAction::PlayPause => {
                perform(Action::PlayPause)
            }
            MediaAction::Next => perform(Action::NextTrack),
            MediaAction::Previous => perform(Action::PreviousTrack),
            MediaAction::Seek(_) => Err("can't seek here".into()),
        }
    }
}

#[cfg(target_os = "android")]
mod imp {
    use super::*;

    /// The Android app reads its media sessions itself.
    pub fn current() -> Option<NowPlaying> {
        None
    }

    pub fn control(_: MediaAction) -> Result<(), String> {
        Err("the phone's app handles this".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spotify_line_in_milliseconds() {
        let (info, art) = parse_mac_line("Spotify|playing|Weightless|Marconi Union|Weightless|12.5|480000|https://i.scdn.co/image/abc").unwrap();
        assert_eq!(info.title, "Weightless");
        assert!(info.playing);
        assert_eq!(info.position_ms, 12_500);
        assert_eq!(info.duration_ms, 480_000);
        assert_eq!(art.as_deref(), Some("https://i.scdn.co/image/abc"));
    }

    #[test]
    fn music_line_in_seconds_with_a_comma_decimal() {
        let (info, art) = parse_mac_line("Music|paused|Song|Band|Album|61,5|215,25|").unwrap();
        assert!(!info.playing);
        assert_eq!(info.position_ms, 61_500);
        assert_eq!(info.duration_ms, 215_250);
        assert_eq!(art, None);
    }

    #[test]
    fn nothing_playing_gives_nothing() {
        assert!(parse_mac_line("").is_none());
        assert!(parse_mac_line("Music|stopped|x|y|z|0|0|").is_none());
        assert!(parse_mac_line("Spotify|playing||a|b|0|0|").is_none());
    }

    #[test]
    fn playerctl_line() {
        let (info, art) = parse_playerctl_line(
            "Playing|Title|Artist|Album|215000000|file:///tmp/cover.jpg|spotify.instance123",
        )
        .unwrap();
        assert_eq!(info.duration_ms, 215_000);
        assert_eq!(info.app, "Spotify");
        assert_eq!(art.as_deref(), Some("file:///tmp/cover.jpg"));
        assert!(parse_playerctl_line("Stopped|T|A|B|0||vlc").is_none());
        assert!(parse_playerctl_line("Playing||A|B|0||vlc").is_none());
    }

    #[test]
    fn only_listed_commands_exist() {
        assert_eq!(
            MediaAction::parse("seek", 5000),
            Some(MediaAction::Seek(5000))
        );
        assert_eq!(MediaAction::parse("seek", -1), None);
        assert_eq!(
            MediaAction::parse("play_pause", 0),
            Some(MediaAction::PlayPause)
        );
        for bad in ["", "stop; reboot", "volume", "Play"] {
            assert_eq!(MediaAction::parse(bad, 0), None, "{bad}");
        }
    }

    #[test]
    fn limits_are_applied() {
        let big = NowPlaying {
            title: "x".repeat(500),
            art: Some(vec![0; MAX_ART_BYTES + 1]),
            ..Default::default()
        }
        .sanitized();
        assert_eq!(big.title.chars().count(), 200);
        assert!(big.art.is_none());
    }
}
