//! What stands between this device and working sync, worked out from the
//! engine's own state. Every app shows the first issue as a banner; the
//! `kind` tells it which fix to offer. Checks that only the host OS can
//! make (battery limits, notification permission) stay in the apps.

use super::*;

/// How long paired devices may go unseen before they count as not found.
const UNSEEN_GRACE_SECS: u64 = 120;
/// How often the list is re-checked for the change event.
const CHECK_EVERY: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HealthIssue {
    /// "no_network", "listener_down", "sync_paused", "devices_not_found"
    /// or "connection_blocked".
    pub kind: &'static str,
    pub title: String,
    pub detail: String,
    /// The paired device the issue is about, if it is about one.
    pub device_id: Option<Uuid>,
}

impl HealthIssue {
    fn new(kind: &'static str, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            detail: detail.into(),
            device_id: None,
        }
    }
}

impl Engine {
    /// Current issues, most fundamental first; empty when all is well.
    pub async fn health(&self) -> Vec<HealthIssue> {
        evaluate(&self.shared).await
    }

    /// Emits `SystemHealthUpdated` whenever the issue list changes, so apps
    /// that redraw on events see an issue appear or clear without polling.
    pub(super) fn spawn_health_monitor(&self) {
        let shared = self.shared.clone();
        tokio::spawn(async move {
            let mut last: Vec<HealthIssue> = Vec::new();
            let mut interval = tokio::time::interval(CHECK_EVERY);
            loop {
                interval.tick().await;
                let now = evaluate(&shared).await;
                if now != last {
                    if shared
                        .event_tx
                        .send(EngineEvent::SystemHealthUpdated(now.clone()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    last = now;
                }
            }
        });
    }
}

async fn evaluate(shared: &EngineShared) -> Vec<HealthIssue> {
    let mut issues = Vec::new();
    let peers = shared.peer_manager.list();
    let any_connected = peers
        .iter()
        .any(|p| p.status == crate::peer_manager::PeerConnectionState::Connected);
    let (no_interface, listener_error) = {
        let net = shared.network_state.lock().await;
        (net.active_interface.is_none(), net.listener_error.clone())
    };
    // A connected device proves there is a network, whatever interface
    // detection says; an address the host chose to bind is taken as given.
    let no_network = no_interface && shared.config.bind_ip.is_none() && !any_connected;

    if no_network {
        issues.push(HealthIssue::new(
            "no_network",
            "Not connected to a network",
            "Connect to Wi-Fi or Ethernet. Link All works between devices on the same network.",
        ));
    } else if listener_error.is_some() {
        issues.push(HealthIssue::new(
            "listener_down",
            "Other devices can't reach this one",
            "Link All couldn't start listening after the network changed. It tries again on the \
             next network change; restarting Link All fixes it now.",
        ));
    }

    if !shared.settings.lock().unwrap().sync_enabled {
        issues.push(HealthIssue::new(
            "sync_paused",
            "Sync is paused",
            "Clipboard and files aren't shared until you turn sync back on.",
        ));
    }

    if no_network {
        return issues;
    }
    // Devices the user disconnected on purpose are not a problem to report.
    let paired: Vec<_> = peers
        .into_iter()
        .filter(|p| p.trusted && !p.explicit_disconnect)
        .collect();
    if paired.is_empty() || any_connected {
        return issues;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let recently = |t: Option<u64>| t.is_some_and(|t| now.saturating_sub(t) < UNSEEN_GRACE_SECS);
    let seen: Vec<_> = paired
        .iter()
        .filter(|p| recently(p.last_discovery_at) || recently(p.last_seen))
        .collect();

    // Discovery only hears a device while Link All runs on it, so one heard
    // again after a connection to it failed is running but being blocked on
    // the way. (Heard before the failure proves nothing: it may have quit.)
    let heard_after_failure = |p: &&&crate::peer_manager::PeerRecord| matches!((p.last_failure_at, p.last_discovery_at), (Some(failed), Some(heard)) if heard > failed);
    if let Some(peer) = seen.iter().find(heard_after_failure) {
        issues.push(HealthIssue {
            device_id: Some(peer.id),
            ..HealthIssue::new(
                "connection_blocked",
                format!("{} is nearby but can't connect", peer.friendly_name),
                "A firewall or the network may be blocking Link All. Allow Link All through \
                 the firewall on both devices.",
            )
        });
    } else if seen.is_empty() && shared.started_at.elapsed().as_secs() >= UNSEEN_GRACE_SECS {
        issues.push(HealthIssue::new(
            "devices_not_found",
            "Can't find your devices",
            "Open Link All on them and check they're on the same Wi-Fi. Guest and office \
             networks often stop devices from seeing each other.",
        ));
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer_manager::DiscoverySource;
    use std::net::Ipv4Addr;

    async fn engine(tmp: &tempfile::TempDir) -> Engine {
        let (tx, mut rx) = mpsc::channel(64);
        tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let cfg = EngineConfig {
            device_id: Uuid::new_v4(),
            device_name: "Health".into(),
            port: 0,
            trust_store_path: tmp.path().join("trust.json"),
            peer_store_path: tmp.path().join("peers.json"),
            identity_path: tmp.path().join("identity.key"),
            data_dir: tmp.path().join("data"),
            file_save_dir: Some(tmp.path().join("received")),
            bind_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            enable_discovery: false,
            ..EngineConfig::default()
        };
        let engine = Engine::start(cfg, tx).await.expect("engine start");
        // Test engines load this machine's real settings; pin what matters.
        let mut settings = engine.current_settings().await;
        settings.sync_enabled = true;
        engine.apply_settings(settings).await;
        engine
    }

    fn kinds(issues: &[HealthIssue]) -> Vec<&'static str> {
        issues.iter().map(|i| i.kind).collect()
    }

    fn add_paired(shared: &EngineShared, name: &str) -> (Uuid, SocketAddr) {
        let id = Uuid::new_v4();
        let addr = SocketAddr::from(([127, 0, 0, 1], 9));
        shared
            .peer_manager
            .upsert_peer(id, name.into(), addr, true, DiscoverySource::Mdns)
            .unwrap();
        (id, addr)
    }

    #[tokio::test]
    async fn paused_sync_is_reported_and_clears() {
        let tmp = tempfile::TempDir::new().unwrap();
        let engine = engine(&tmp).await;
        let mut settings = engine.current_settings().await;
        settings.sync_enabled = false;
        engine.apply_settings(settings.clone()).await;
        assert!(kinds(&engine.health().await).contains(&"sync_paused"));

        settings.sync_enabled = true;
        engine.apply_settings(settings).await;
        assert!(!kinds(&engine.health().await).contains(&"sync_paused"));
    }

    /// Heard on the network after a connection to it failed: running, blocked.
    /// Heard only before the failure: it may simply have quit.
    #[tokio::test]
    async fn blocked_only_when_heard_after_a_failure() {
        let tmp = tempfile::TempDir::new().unwrap();
        let engine = engine(&tmp).await;
        let shared = &engine.shared;
        let (id, addr) = add_paired(shared, "Laptop");

        shared
            .peer_manager
            .mark_failed(id, addr, "connection timed out".into())
            .unwrap();
        assert!(!kinds(&engine.health().await).contains(&"connection_blocked"));

        // Discovery times are whole seconds; hear it in a later one.
        tokio::time::sleep(Duration::from_millis(1100)).await;
        shared
            .peer_manager
            .upsert_peer(id, "Laptop".into(), addr, true, DiscoverySource::Mdns)
            .unwrap();
        let issues = engine.health().await;
        let blocked = issues.iter().find(|i| i.kind == "connection_blocked");
        assert_eq!(blocked.and_then(|i| i.device_id), Some(id), "{issues:?}");
    }

    #[tokio::test]
    async fn paired_devices_unseen_for_two_minutes_are_not_found() {
        let tmp = tempfile::TempDir::new().unwrap();
        let engine = engine(&tmp).await;
        let mut shared = engine.shared.clone();
        add_paired(&shared, "Phone");
        // Just discovered: not missing yet.
        assert!(!kinds(&evaluate(&shared).await).contains(&"devices_not_found"));

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut peer = shared.peer_manager.list().pop().unwrap();
        peer.last_seen = Some(now - 600);
        peer.last_discovery_at = Some(now - 600);
        shared.peer_manager.replace_for_test(peer);
        // An engine that only just started has not had time to find anyone.
        assert!(!kinds(&evaluate(&shared).await).contains(&"devices_not_found"));
        shared.started_at = Instant::now() - Duration::from_secs(UNSEEN_GRACE_SECS + 1);
        assert_eq!(kinds(&evaluate(&shared).await), vec!["devices_not_found"]);
    }
}
