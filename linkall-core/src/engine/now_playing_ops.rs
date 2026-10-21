//! Now Playing: telling paired devices what this device is playing, and
//! keeping what they play so the apps can show it and send controls back.

use super::*;
use crate::now_playing::{MediaAction, NowPlaying};
use base64::Engine as _;

/// How often a playing track is repeated, so the position on the other side stays right.
const HEARTBEAT: Duration = Duration::from_secs(8);
/// How far the position may drift from where the last message said it would be before it is resent.
const DRIFT_MS: i128 = 2500;

/// What an app shows for a track on another device.
#[derive(Debug, Clone, Serialize)]
pub struct NowPlayingView {
    pub device_id: String,
    pub device_name: String,
    pub platform: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub app: String,
    pub playing: bool,
    /// Where it is now, carried forward from the last message while it plays.
    pub position_ms: u64,
    pub duration_ms: u64,
    pub art_base64: Option<String>,
}

pub(crate) struct Sent {
    pub info: NowPlaying,
    pub at: Instant,
}

/// The last track told to the paired devices.
pub(crate) type SentSlot = Arc<std::sync::Mutex<Option<Sent>>>;

fn message_of(info: &NowPlaying, with_art: bool) -> AppMessage {
    let mut info = info.clone();
    if !with_art {
        info.art = None;
    }
    AppMessage::NowPlaying { info }
}

impl Engine {
    /// This device's player changed or ticked: tell the paired devices when it matters
    /// (a new track, play or pause, a jump in position, or a regular heartbeat).
    /// `None` means nothing is playing any more.
    pub fn set_now_playing(&self, info: Option<NowPlaying>) {
        let info = info.map(NowPlaying::sanitized).filter(|i| !i.is_empty());
        let mut sent = self.shared.now_playing_sent.lock().unwrap();
        let (message, remember) = match (&info, sent.as_ref()) {
            (None, None) => return,
            (None, Some(last)) if last.info.is_empty() => return,
            (None, Some(_)) => {
                let cleared = NowPlaying::default();
                (message_of(&cleared, false), cleared)
            }
            (Some(now), None) => (message_of(now, true), now.clone()),
            (Some(now), Some(last)) => {
                let elapsed = last.at.elapsed();
                let expected = if last.info.playing {
                    last.info.position_ms as i128 + elapsed.as_millis() as i128
                } else {
                    last.info.position_ms as i128
                };
                let changed_track = !now.same_track(&last.info);
                let due = changed_track
                    || now.playing != last.info.playing
                    || (now.position_ms as i128 - expected).abs() > DRIFT_MS
                    || elapsed
                        >= if now.playing {
                            HEARTBEAT
                        } else {
                            Duration::from_secs(60)
                        };
                if !due {
                    return;
                }
                // An app that sends the cover once per track keeps it for the
                // devices that connect later.
                let mut remember = now.clone();
                if remember.art.is_none() && !changed_track {
                    remember.art = last.info.art.clone();
                }
                (
                    message_of(
                        now,
                        changed_track || last.info.art.is_none() && now.art.is_some(),
                    ),
                    remember,
                )
            }
        };
        *sent = Some(Sent {
            info: remember,
            at: Instant::now(),
        });
        drop(sent);
        self.tell_peers(message);
    }

    fn tell_peers(&self, message: AppMessage) {
        for (peer_id, tx) in self.shared.peer_manager.all_connected_senders() {
            let trusted = self
                .shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false);
            if trusted {
                let _ = tx.try_send(message.clone());
            }
        }
    }

    /// What the other devices are playing now. A track that stopped sending, while
    /// it was playing, is dropped after a while; a paused one stays for ten minutes.
    pub fn now_playing_list(&self) -> Vec<NowPlayingView> {
        let mut out = Vec::new();
        for entry in self.shared.now_playing_remote.iter() {
            let (info, at) = entry.value();
            let age = at.elapsed();
            let fresh = if info.playing {
                age < Duration::from_secs(25)
            } else {
                age < Duration::from_secs(600)
            };
            let Some(peer) = self.shared.peer_manager.get(*entry.key()) else {
                continue;
            };
            if !fresh || !peer.trusted || peer.status != PeerConnectionState::Connected {
                continue;
            }
            let position = if info.playing {
                (info.position_ms + age.as_millis() as u64)
                    .min(info.duration_ms.max(info.position_ms))
            } else {
                info.position_ms
            };
            out.push(NowPlayingView {
                device_id: entry.key().to_string(),
                device_name: peer.friendly_name.clone(),
                platform: peer.platform.clone(),
                title: info.title.clone(),
                artist: info.artist.clone(),
                album: info.album.clone(),
                app: info.app.clone(),
                playing: info.playing,
                position_ms: position,
                duration_ms: info.duration_ms,
                art_base64: info
                    .art
                    .as_ref()
                    .map(|a| base64::engine::general_purpose::STANDARD.encode(a)),
            });
        }
        out.sort_by(|a, b| {
            b.playing
                .cmp(&a.playing)
                .then(a.device_name.cmp(&b.device_name))
        });
        out
    }

    /// The cover art of what `device` is playing, as sent.
    pub fn now_playing_art(&self, device: Uuid) -> Option<Vec<u8>> {
        self.shared
            .now_playing_remote
            .get(&device)
            .and_then(|e| e.value().0.art.clone())
    }

    /// Play, pause, skip or seek the player on a connected, trusted device on 2.1 or later.
    pub async fn send_media_command(
        &self,
        target: Uuid,
        action: &str,
        value: i64,
    ) -> Result<(), String> {
        if MediaAction::parse(action, value).is_none() {
            return Err("That isn't a media command.".to_string());
        }
        let tx = self.remote_control_sender(target)?;
        tx.send(AppMessage::MediaCommand {
            action: action.to_string(),
            value,
        })
        .await
        .map_err(|_| "That device isn't connected.".to_string())
    }

    /// Starts reading this computer's own player. Windows and Android read theirs in
    /// their apps and call `set_now_playing`.
    pub(super) fn spawn_now_playing_source(&self) {
        if !cfg!(any(
            target_os = "macos",
            all(unix, not(target_os = "android"))
        )) {
            return;
        }
        let engine = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(2));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                // Nobody to tell: don't poll the player.
                let anyone = engine
                    .shared
                    .peer_manager
                    .list()
                    .iter()
                    .any(|p| p.trusted && p.status == PeerConnectionState::Connected);
                if !anyone {
                    continue;
                }
                let info = tokio::task::spawn_blocking(crate::now_playing::current)
                    .await
                    .ok()
                    .flatten();
                engine.set_now_playing(info);
            }
        });
    }
}

/// Sends what this device is playing to a device that has just connected.
pub(super) fn send_current_to(shared: &EngineShared, outbox: mpsc::Sender<AppMessage>) {
    let info = shared
        .now_playing_sent
        .lock()
        .unwrap()
        .as_ref()
        .map(|s| s.info.clone())
        .filter(|i| !i.is_empty());
    if let Some(info) = info {
        let _ = outbox.try_send(message_of(&info, true));
    }
}
