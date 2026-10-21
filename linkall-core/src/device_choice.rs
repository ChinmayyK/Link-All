//! Which devices an action can go to, and why the others can't.
//!
//! Every feature that asks "which device?" (send, remote, browse, folder sync)
//! asks the same question with a different rule, and the apps used to answer it
//! each their own way. This is the one answer: the devices that can be used,
//! best first, then the ones that can't, each with the reason. It holds no
//! state: the app passes in the device the user chose last.

use crate::peer_manager::{PeerConnectionState, PeerRecord};
use serde::Serialize;

/// What the user is about to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Send files or a folder: any connected paired device.
    Send,
    /// Press media, volume and slide keys: a connected paired computer on 2.1 or later.
    Remote,
    /// Browse a device's files: any connected paired device.
    Browse,
    /// Keep a folder synced to it: a paired computer, connected or not.
    SyncTarget,
    /// Make it ring so it can be found: a connected paired phone on 2.1 or later.
    Find,
}

impl Purpose {
    pub fn parse(name: &str) -> Option<Purpose> {
        match name {
            "send" => Some(Purpose::Send),
            "remote" => Some(Purpose::Remote),
            "browse" => Some(Purpose::Browse),
            "sync_target" => Some(Purpose::SyncTarget),
            "find" => Some(Purpose::Find),
            _ => None,
        }
    }
}

/// One device in a chooser.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub name: String,
    pub platform: Option<String>,
    /// Whether it can be picked right now.
    pub available: bool,
    /// Why not, as a short sentence; None when available.
    pub reason: Option<String>,
    /// The device the user chose last for this purpose.
    pub last_used: bool,
}

/// The first Link All version that understands remote control.
pub const REMOTE_CONTROL_MIN: (u32, u32) = (2, 1);

/// True for a peer on 2.1.0 or later. `None` (it never reported a version) is too old.
pub fn understands_remote_control(app_version: Option<&str>) -> bool {
    let Some(v) = app_version else { return false };
    let mut parts = v.split(['.', '-', '+']).map(|p| {
        p.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse::<u32>()
            .unwrap_or(0)
    });
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0)) >= REMOTE_CONTROL_MIN
}

fn is_phone(peer: &PeerRecord) -> bool {
    peer.platform.as_deref() == Some("Android")
}

/// Why `peer` can't be used for `purpose` right now, or None when it can.
fn unavailable_reason(peer: &PeerRecord, purpose: Purpose, now: u64) -> Option<String> {
    let connected = peer.status == PeerConnectionState::Connected;
    match purpose {
        Purpose::Remote if is_phone(peer) => return Some("A phone can't be controlled".into()),
        Purpose::SyncTarget if is_phone(peer) => return Some("Folders sync to computers".into()),
        Purpose::Find if !is_phone(peer) => return Some("Only a phone can ring".into()),
        _ => {}
    }
    if purpose != Purpose::SyncTarget && !connected {
        return Some(match peer.last_seen {
            Some(t) if t <= now => format!("Offline · seen {}", ago(now - t)),
            _ => "Offline".into(),
        });
    }
    if matches!(purpose, Purpose::Remote | Purpose::Find)
        && !understands_remote_control(peer.app_version.as_deref())
    {
        return Some("Needs Link All 2.1 or later".into());
    }
    if !peer.remote_sync_enabled && connected && purpose == Purpose::Send {
        return Some("Sync is paused on that device".into());
    }
    if connected
        && peer.background_restricted == Some(true)
        && peer.last_seen.is_some_and(|t| now.saturating_sub(t) > 600)
    {
        return Some("Paused by the phone".into());
    }
    None
}

fn ago(secs: u64) -> String {
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

/// The devices for `purpose`: usable ones first (the last-used on top, then
/// the most recently seen, then by name), then the unusable ones in the same order.
pub fn choices(
    peers: &[PeerRecord],
    purpose: Purpose,
    last_used: Option<&str>,
    now: u64,
) -> Vec<Choice> {
    let mut out: Vec<(Choice, u64)> = peers
        .iter()
        // Only paired devices are choices: pairing happens on the Devices screen,
        // and listing every device on the network would bury the real ones.
        .filter(|p| p.trusted)
        .map(|p| {
            let reason = unavailable_reason(p, purpose, now);
            let id = p.id.to_string();
            (
                Choice {
                    last_used: last_used == Some(id.as_str()),
                    id,
                    name: p.friendly_name.clone(),
                    platform: p.platform.clone(),
                    available: reason.is_none(),
                    reason,
                },
                p.last_seen.unwrap_or(0),
            )
        })
        .collect();
    out.sort_by(|(a, seen_a), (b, seen_b)| {
        b.available
            .cmp(&a.available)
            .then(b.last_used.cmp(&a.last_used))
            .then(seen_b.cmp(seen_a))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out.into_iter().map(|(c, _)| c).collect()
}

/// Whether the chooser should offer "All devices": only for sending, and only
/// when there is more than one device to send to.
pub fn offers_all(purpose: Purpose, choices: &[Choice]) -> bool {
    purpose == Purpose::Send && choices.iter().filter(|c| c.available).count() > 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    const NOW: u64 = 1_000_000;

    fn peer(name: &str, platform: &str, connected: bool, seen_ago: u64) -> PeerRecord {
        PeerRecord {
            id: Uuid::new_v4(),
            friendly_name: name.into(),
            platform: Some(platform.into()),
            trusted: true,
            remote_sync_enabled: true,
            status: if connected {
                PeerConnectionState::Connected
            } else {
                PeerConnectionState::Disconnected
            },
            last_seen: Some(NOW - seen_ago),
            app_version: Some("2.1.0".into()),
            ..Default::default()
        }
    }

    fn names(c: &[Choice]) -> Vec<&str> {
        c.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn the_last_used_device_comes_first_among_the_usable_ones() {
        let (a, b) = (
            peer("Desk", "Windows", true, 5),
            peer("Mac", "macOS", true, 1),
        );
        let last = a.id.to_string();
        let got = choices(&[b, a], Purpose::Send, Some(&last), NOW);
        assert_eq!(names(&got), ["Desk", "Mac"]);
        assert!(got[0].last_used && !got[1].last_used);
    }

    #[test]
    fn without_a_last_used_the_most_recently_seen_comes_first() {
        let (a, b) = (
            peer("Old", "Linux", true, 500),
            peer("New", "macOS", true, 1),
        );
        assert_eq!(
            names(&choices(&[a, b], Purpose::Send, None, NOW)),
            ["New", "Old"]
        );
    }

    #[test]
    fn an_offline_device_is_listed_last_with_when_it_was_seen() {
        let on = peer("On", "macOS", true, 1);
        let off = peer("Off", "Windows", false, 600);
        let got = choices(&[off, on], Purpose::Send, None, NOW);
        assert_eq!(names(&got), ["On", "Off"]);
        assert!(!got[1].available);
        assert_eq!(got[1].reason.as_deref(), Some("Offline · seen 10 min ago"));
    }

    #[test]
    fn remote_skips_phones_and_old_computers_with_a_reason() {
        let phone = peer("Phone", "Android", true, 1);
        let mut old = peer("Old PC", "Windows", true, 1);
        old.app_version = Some("2.0.2".into());
        let ok = peer("Mac", "macOS", true, 1);
        let got = choices(&[phone, old, ok], Purpose::Remote, None, NOW);
        assert_eq!(names(&got), ["Mac", "Old PC", "Phone"]);
        assert_eq!(
            got[1].reason.as_deref(),
            Some("Needs Link All 2.1 or later")
        );
        assert_eq!(
            got[2].reason.as_deref(),
            Some("A phone can't be controlled")
        );
    }

    #[test]
    fn a_computer_that_never_reported_a_version_counts_as_too_old_for_remote() {
        let mut p = peer("PC", "Windows", true, 1);
        p.app_version = None;
        assert!(!choices(&[p], Purpose::Remote, None, NOW)[0].available);
    }

    #[test]
    fn a_sync_target_may_be_offline_but_not_a_phone() {
        let off = peer("Laptop", "macOS", false, 100);
        let phone = peer("Phone", "Android", true, 1);
        let got = choices(&[off, phone], Purpose::SyncTarget, None, NOW);
        assert!(got.iter().find(|c| c.name == "Laptop").unwrap().available);
        assert!(!got.iter().find(|c| c.name == "Phone").unwrap().available);
    }

    #[test]
    fn a_device_that_is_not_paired_is_not_listed() {
        let mut stranger = peer("Stranger", "Windows", true, 1);
        stranger.trusted = false;
        let got = choices(
            &[stranger, peer("Mac", "macOS", true, 1)],
            Purpose::Send,
            None,
            NOW,
        );
        assert_eq!(names(&got), ["Mac"]);
    }

    #[test]
    fn a_device_that_paused_sync_cannot_be_sent_to() {
        let mut p = peer("Paused", "Windows", true, 1);
        p.remote_sync_enabled = false;
        assert_eq!(
            choices(&[p], Purpose::Send, None, NOW)[0].reason.as_deref(),
            Some("Sync is paused on that device")
        );
    }

    #[test]
    fn all_devices_is_offered_only_for_send_with_two_or_more_usable() {
        let two = choices(
            &[peer("A", "macOS", true, 1), peer("B", "Windows", true, 1)],
            Purpose::Send,
            None,
            NOW,
        );
        assert!(offers_all(Purpose::Send, &two));
        assert!(!offers_all(Purpose::Remote, &two));
        let one_usable = choices(
            &[peer("A", "macOS", true, 1), peer("B", "Windows", false, 1)],
            Purpose::Send,
            None,
            NOW,
        );
        assert!(!offers_all(Purpose::Send, &one_usable));
    }

    #[test]
    fn remote_version_check() {
        assert!(understands_remote_control(Some("2.1.0")));
        assert!(understands_remote_control(Some("3.0.0")));
        assert!(!understands_remote_control(Some("2.0.2")));
        assert!(!understands_remote_control(None));
    }

    #[test]
    fn only_connected_phones_on_2_1_can_be_asked_to_ring() {
        let mut old = peer("Old phone", "Android", true, 5);
        old.app_version = Some("2.0.2".into());
        let got = choices(
            &[
                peer("Phone", "Android", true, 5),
                peer("Laptop", "macOS", true, 5),
                peer("Away", "Android", false, 5),
                old,
            ],
            Purpose::Find,
            None,
            NOW,
        );
        let by = |n: &str| got.iter().find(|c| c.name == n).unwrap().clone();
        assert!(by("Phone").available);
        assert_eq!(
            by("Laptop").reason.as_deref(),
            Some("Only a phone can ring")
        );
        assert!(by("Away").reason.unwrap().starts_with("Offline"));
        assert_eq!(
            by("Old phone").reason.as_deref(),
            Some("Needs Link All 2.1 or later")
        );
        assert!(!offers_all(Purpose::Find, &got));
    }
}
