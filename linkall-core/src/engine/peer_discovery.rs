//! Discovered-peer handling: which sightings to connect to, connect
//! attempt gating, and how peers are ranked for status display.

use super::*;

pub(super) fn display_peers_for_status(
    peers: Vec<PeerRecord>,
    local_device_id: Uuid,
    local_device_name: &str,
    local_ip: IpAddr,
    trust_lookup: &HashMap<Uuid, TrustRecord>,
) -> Vec<PeerRecord> {
    let mut deduped: HashMap<Uuid, PeerRecord> = HashMap::new();

    for peer in peers {
        if is_obviously_local_peer(
            peer.id,
            &peer.friendly_name,
            peer.ips.first().cloned(),
            local_device_id,
            local_device_name,
            Some(local_ip),
        ) {
            continue;
        }

        let key = peer.id;
        match deduped.get(&key) {
            Some(existing) if !peer_should_replace(existing, &peer) => {}
            _ => {
                deduped.insert(key, peer);
            }
        }
    }

    let mut peers: Vec<_> = deduped
        .into_values()
        .map(|mut p| {
            p.lifecycle_state = Some(p.lifecycle_state());
            p.pairing_expires_in_secs = p.pairing_expires_in();
            if let Some(rec) = trust_lookup.get(&p.id) {
                p.fingerprint_display =
                    Some(crate::trust::format_fingerprint(&rec.key_fingerprint));
                p.first_seen = Some(rec.first_seen);
            }
            p
        })
        .collect();
    peers.sort_by(|left, right| {
        peer_display_rank(left)
            .cmp(&peer_display_rank(right))
            .then_with(|| left.friendly_name.cmp(&right.friendly_name))
            .then_with(|| left.id.cmp(&right.id))
    });
    peers
}

pub(super) fn is_obviously_local_peer(
    peer_id: Uuid,
    peer_name: &str,
    peer_ip: Option<IpAddr>,
    local_device_id: Uuid,
    local_device_name: &str,
    local_ip: Option<IpAddr>,
) -> bool {
    if peer_id == local_device_id {
        return true;
    }

    matches!((peer_ip, local_ip), (Some(peer_ip), Some(local_ip)) if peer_ip == local_ip
            && peer_name
                .trim()
                .eq_ignore_ascii_case(local_device_name.trim()))
}

pub(super) fn peer_should_replace(current: &PeerRecord, candidate: &PeerRecord) -> bool {
    peer_display_rank(candidate) < peer_display_rank(current)
}

pub(super) fn peer_display_rank(
    peer: &PeerRecord,
) -> (u8, u8, u8, std::cmp::Reverse<u64>, std::cmp::Reverse<u64>) {
    let status_rank = match peer.status {
        PeerConnectionState::Connected => 0,
        PeerConnectionState::Connecting => 1,
        PeerConnectionState::Failed => 2,
        PeerConnectionState::Disconnected => 3,
    };
    let trust_rank = if peer.trusted { 0 } else { 1 };
    let sync_rank = if peer.sync_enabled { 0 } else { 1 };
    (
        status_rank,
        trust_rank,
        sync_rank,
        std::cmp::Reverse(peer.last_seen.unwrap_or(0)),
        std::cmp::Reverse(peer.last_sync.unwrap_or(0)),
    )
}

pub(super) async fn on_peer_found(shared: EngineShared, peer: PeerInfo) -> Result<()> {
    on_peer_found_via(
        shared,
        peer.device_id,
        peer.device_name,
        peer.addrs,
        peer.port,
        DiscoverySource::Mdns,
    )
    .await
}

/// Shared handling for any discovery layer: upsert the peer record and,
/// if appropriate, kick off a connection attempt. `source` records which
/// layer produced this sighting (mDNS, UDP beacon, active LAN probe, ...).
pub(super) async fn on_peer_found_via(
    shared: EngineShared,
    device_id: Uuid,
    device_name: String,
    addrs: Vec<IpAddr>,
    port: u16,
    source: DiscoverySource,
) -> Result<()> {
    // Probe-based layers (LanProbe, HotspotProbe) only confirm that *something*
    // accepts a TCP connection on the port — the real device identity isn't
    // known until the handshake completes, so `device_id` here is a synthetic
    // placeholder derived from the IP. `connect_once` enforces that the
    // handshake's peer ID matches `expected_device_id`, so passing the
    // placeholder would make it reject every single connection as an
    // "identity changed" mismatch. Treat these exactly like a manual
    // "connect to this address, whoever answers" attempt instead.
    if matches!(
        source,
        DiscoverySource::LanProbe | DiscoverySource::HotspotProbe
    ) {
        for ip in addrs {
            let addr = SocketAddr::new(ip, port);
            if !mark_probe_connect_inflight(addr) {
                continue; // already attempting this address
            }
            let shared_clone = shared.clone();
            tokio::spawn(async move {
                if let Err(err) = connect_loop(shared_clone, vec![addr], None, source).await {
                    warn!(addr = %addr, error = %err, "probe-discovered peer connection failed");
                }
                clear_probe_connect_inflight(addr);
            });
        }
        return Ok(());
    }

    let trusted = shared.trust.lock().await.is_trusted(device_id);

    for ip in addrs {
        let addr = SocketAddr::new(ip, port);

        let record = shared
            .peer_manager
            .upsert_peer(device_id, device_name.clone(), addr, trusted, source)
            .ok();

        if !should_initiate_session(&shared.peer_manager, device_id, source) {
            continue;
        }

        // A device we've never paired with (not trusted, not remembered, no
        // pairing in flight) re-announces itself via mDNS/UDP every few
        // seconds, re-entering this function each time. Without a cooldown
        // that spawns an unbounded stream of connect_loop tasks/sockets for
        // every stranger on the network. Devices we actually have a
        // relationship with keep the existing eager-retry behavior.
        let has_relationship = trusted
            || record
                .as_ref()
                .map(|r| r.remembered || r.pairing_requested || r.outgoing_pairing_waiting)
                .unwrap_or(false);
        if !has_relationship && !allow_discovery_connect_attempt(device_id, addr) {
            continue;
        }

        if shared.peer_manager.live_endpoint(device_id) == Some(addr) {
            continue;
        }

        if matches!(
            shared.peer_manager.get(device_id),
            Some(record)
                if record.status == PeerConnectionState::Connecting
                    && record.socket_addrs().contains(&addr)
        ) {
            continue;
        }

        let shared_clone = shared.clone();
        tokio::spawn(async move {
            if let Err(err) = connect_loop(shared_clone, vec![addr], Some(device_id), source).await
            {
                warn!(peer_id = %device_id, error = %err, "discovered peer connection failed");
            }
        });
    }

    Ok(())
}

/// Addresses currently being connected to via a probe-discovered (unconfirmed
/// identity) sighting. Prevents a slow handshake from overlapping with the
/// next sweep's rediscovery of the same address before it resolves.
pub(super) fn probe_connect_inflight() -> &'static dashmap::DashSet<SocketAddr> {
    static SET: std::sync::OnceLock<dashmap::DashSet<SocketAddr>> = std::sync::OnceLock::new();
    SET.get_or_init(dashmap::DashSet::new)
}

pub(super) fn mark_probe_connect_inflight(addr: SocketAddr) -> bool {
    probe_connect_inflight().insert(addr)
}

pub(super) fn clear_probe_connect_inflight(addr: SocketAddr) {
    probe_connect_inflight().remove(&addr);
}

/// Cooldown before re-attempting a connection to a discovery-sighted device
/// we have no relationship with (see `on_peer_found_via`). Bounds how often
/// we open a socket to a stranger who just happens to be re-announcing
/// itself on the network.
pub(super) const DISCOVERY_CONNECT_COOLDOWN: Duration = Duration::from_secs(60);

pub(super) fn discovery_connect_attempts() -> &'static dashmap::DashMap<(Uuid, SocketAddr), Instant>
{
    static MAP: std::sync::OnceLock<dashmap::DashMap<(Uuid, SocketAddr), Instant>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(dashmap::DashMap::new)
}

pub(super) fn allow_discovery_connect_attempt(device_id: Uuid, addr: SocketAddr) -> bool {
    allow_discovery_connect_attempt_at(device_id, addr, Instant::now())
}

pub(super) fn allow_discovery_connect_attempt_at(
    device_id: Uuid,
    addr: SocketAddr,
    now: Instant,
) -> bool {
    let map = discovery_connect_attempts();
    if let Some(last) = map.get(&(device_id, addr)) {
        if now.duration_since(*last) < DISCOVERY_CONNECT_COOLDOWN {
            return false;
        }
    }
    // Entries only matter within the cooldown window; opportunistically drop
    // everything older so this map can't grow unbounded over a long-running
    // daemon (mirrors the peer_manager 1000-entry cap's intent).
    if map.len() > 2000 {
        map.retain(|_, last| now.duration_since(*last) < DISCOVERY_CONNECT_COOLDOWN);
    }
    map.insert((device_id, addr), now);
    true
}

pub(super) fn should_initiate_session(
    peer_manager: &PeerManager,
    peer_id: Uuid,
    discovery: DiscoverySource,
) -> bool {
    if peer_manager.is_explicitly_disconnected(peer_id) {
        return false;
    }
    if peer_manager.is_connected(peer_id) && discovery != DiscoverySource::Manual {
        return false;
    }
    match discovery {
        DiscoverySource::Manual => true,
        DiscoverySource::Mdns
        | DiscoverySource::Unknown
        | DiscoverySource::UdpBeacon
        | DiscoverySource::UdpMulticast
        | DiscoverySource::HotspotProbe
        | DiscoverySource::LanProbe => {
            // Both sides attempt connection eagerly. If both succeed, the
            // lower-ID peer's session wins via replace_live_session (the
            // existing dedup logic). This is critical for asymmetric routing
            // (e.g. Android hotspot AP isolation) where only one direction
            // may be routable.
            true
        }
    }
}

#[cfg(test)]
mod discovery_connect_gate_tests {
    use super::*;
    use tempfile::NamedTempFile;

    // ---- allow_discovery_connect_attempt_at: guards the c95da46 connect-storm bug ----
    // Distinct synthetic (device_id, addr) keys per test avoid collisions in the
    // shared static DashMap under parallel `cargo test`.

    #[test]
    fn allow_discovery_connect_attempt_blocks_repeat_within_cooldown_window() {
        let device_id = Uuid::new_v4();
        let addr: SocketAddr = "203.0.113.1:47823".parse().unwrap();
        let t0 = Instant::now();

        assert!(allow_discovery_connect_attempt_at(device_id, addr, t0));
        assert!(!allow_discovery_connect_attempt_at(
            device_id,
            addr,
            t0 + Duration::from_secs(30)
        ));
    }

    #[test]
    fn allow_discovery_connect_attempt_allows_after_cooldown_elapses() {
        let device_id = Uuid::new_v4();
        let addr: SocketAddr = "203.0.113.2:47823".parse().unwrap();
        let t0 = Instant::now();

        assert!(allow_discovery_connect_attempt_at(device_id, addr, t0));
        assert!(allow_discovery_connect_attempt_at(
            device_id,
            addr,
            t0 + Duration::from_secs(61)
        ));
    }

    #[test]
    fn allow_discovery_connect_attempt_keys_by_device_and_addr_independently() {
        let device_a = Uuid::new_v4();
        let device_b = Uuid::new_v4();
        let addr_x: SocketAddr = "203.0.113.3:47823".parse().unwrap();
        let addr_y: SocketAddr = "203.0.113.4:47823".parse().unwrap();
        let t0 = Instant::now();

        assert!(allow_discovery_connect_attempt_at(device_a, addr_x, t0));
        assert!(allow_discovery_connect_attempt_at(device_a, addr_y, t0));
        assert!(allow_discovery_connect_attempt_at(device_b, addr_x, t0));
    }

    // ---- probe_connect_inflight family: guards the d3acf2d unconfirmed-peer path ----

    #[test]
    fn probe_connect_inflight_dedups_concurrent_same_address_probe() {
        let addr: SocketAddr = "203.0.113.5:47823".parse().unwrap();

        assert!(mark_probe_connect_inflight(addr));
        assert!(!mark_probe_connect_inflight(addr));
    }

    #[test]
    fn probe_connect_inflight_reattempt_allowed_after_clear() {
        let addr: SocketAddr = "203.0.113.6:47823".parse().unwrap();

        assert!(mark_probe_connect_inflight(addr));
        clear_probe_connect_inflight(addr);
        assert!(mark_probe_connect_inflight(addr));
    }

    // ---- should_initiate_session ----

    fn peer_manager_with_peer(trusted: bool) -> (PeerManager, NamedTempFile, Uuid) {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let id = Uuid::new_v4();
        manager
            .upsert_peer(
                id,
                "Test Device".into(),
                "192.168.1.50:47823".parse().unwrap(),
                trusted,
                DiscoverySource::Mdns,
            )
            .unwrap();
        (manager, file, id)
    }

    #[test]
    fn should_initiate_session_false_when_explicitly_disconnected() {
        let (manager, _file, id) = peer_manager_with_peer(true);
        manager.set_explicit_disconnect(id, true).unwrap();

        assert!(!should_initiate_session(
            &manager,
            id,
            DiscoverySource::Mdns
        ));
    }

    #[test]
    fn should_initiate_session_false_when_already_connected_via_discovery() {
        let (manager, _file, id) = peer_manager_with_peer(true);
        let (tx, _rx) = mpsc::channel(1);
        let (stop, _stop_rx) = oneshot::channel();
        manager
            .replace_live_session(
                Uuid::nil(),
                id,
                true,
                "192.168.1.50:47823".parse().unwrap(),
                tx.clone(),
                tx,
                stop,
            )
            .unwrap();

        assert!(!should_initiate_session(
            &manager,
            id,
            DiscoverySource::Mdns
        ));
    }

    #[test]
    fn should_initiate_session_true_when_connected_but_source_is_manual() {
        let (manager, _file, id) = peer_manager_with_peer(true);
        let (tx, _rx) = mpsc::channel(1);
        let (stop, _stop_rx) = oneshot::channel();
        manager
            .replace_live_session(
                Uuid::nil(),
                id,
                true,
                "192.168.1.50:47823".parse().unwrap(),
                tx.clone(),
                tx,
                stop,
            )
            .unwrap();

        assert!(should_initiate_session(
            &manager,
            id,
            DiscoverySource::Manual
        ));
    }

    #[test]
    fn should_initiate_session_true_for_untouched_discovered_peer() {
        let (manager, _file, id) = peer_manager_with_peer(true);

        assert!(should_initiate_session(&manager, id, DiscoverySource::Mdns));
    }
}

#[cfg(test)]
mod display_peers_for_status_tests {
    use super::*;
    use crate::peer_manager::PeerConnectionState;

    fn bare_peer(id: Uuid) -> PeerRecord {
        PeerRecord {
            id,
            friendly_name: "Some Device".into(),
            ips: vec!["192.168.1.50".parse().unwrap()],
            status: PeerConnectionState::Disconnected,
            ..PeerRecord::default()
        }
    }

    #[test]
    fn fills_fingerprint_and_first_seen_from_trust_record() {
        let id = Uuid::new_v4();
        let trust_record = TrustRecord {
            device_id: id,
            key_fingerprint: [7u8; 32],
            first_seen: 1_700_000_000,
            ..TrustRecord::default()
        };
        let mut trust_lookup = HashMap::new();
        trust_lookup.insert(id, trust_record);

        let result = display_peers_for_status(
            vec![bare_peer(id)],
            Uuid::new_v4(),
            "Local Device",
            "10.0.0.1".parse().unwrap(),
            &trust_lookup,
        );

        let peer = result.into_iter().find(|p| p.id == id).unwrap();
        assert_eq!(peer.first_seen, Some(1_700_000_000));
        assert_eq!(
            peer.fingerprint_display,
            Some(crate::trust::format_fingerprint(&[7u8; 32]))
        );
    }

    #[test]
    fn leaves_fingerprint_and_first_seen_none_without_trust_record() {
        let id = Uuid::new_v4();
        let trust_lookup: HashMap<Uuid, TrustRecord> = HashMap::new();

        let result = display_peers_for_status(
            vec![bare_peer(id)],
            Uuid::new_v4(),
            "Local Device",
            "10.0.0.1".parse().unwrap(),
            &trust_lookup,
        );

        let peer = result.into_iter().find(|p| p.id == id).unwrap();
        assert_eq!(peer.first_seen, None);
        assert_eq!(peer.fingerprint_display, None);
    }
}
