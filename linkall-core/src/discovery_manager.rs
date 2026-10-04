//! Unified Discovery Manager — merges all discovery layers into a single stream.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
//! │   mDNS/NSD   │  │ UDP Broadcast│  │ UDP Multicast│  │ Peer Cache   │
//! │   Layer 1    │  │   Layer 2    │  │   Layer 3    │  │ Probe L4/L5  │
//! └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘
//!        │                 │                 │                  │
//!        ▼                 ▼                 ▼                  ▼
//!   ┌─────────────────────────────────────────────────────────────┐
//!   │                   DiscoveryManager                          │
//!   │  • Dedup by device_id (not IP)                             │
//!   │  • Merge multi-source peer records                         │
//!   │  • Track freshness per source                              │
//!   │  • Evict stale peers                                       │
//!   └────────────────────────┬────────────────────────────────────┘
//!                            │
//!                            ▼
//!                    DiscoveryEvent stream → Engine
//! ```

use crate::peer_manager::DiscoverySource;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;

// ── Configuration ─────────────────────────────────────────────────────────────

/// How long a peer stays "fresh" per discovery source before being considered stale.
pub fn staleness_timeout(source: DiscoverySource) -> Duration {
    match source {
        DiscoverySource::Mdns => Duration::from_secs(30),
        DiscoverySource::UdpBeacon => Duration::from_secs(10),
        DiscoverySource::UdpMulticast => Duration::from_secs(10),
        // Hotspot probes are short-lived — we re-probe frequently because
        // the hotspot host may change IPs or go away without notice.
        DiscoverySource::HotspotProbe => Duration::from_secs(6),
        // LAN sweeps run on a slower cadence than hotspot probes (see
        // `lan_probe.rs`), so give results a longer runway before eviction.
        DiscoverySource::LanProbe => Duration::from_secs(45),
        DiscoverySource::Manual => Duration::from_secs(u64::MAX), // never expires
        DiscoverySource::Unknown => Duration::from_secs(15),
    }
}

// ── Public types ──────────────────────────────────────────────────────────────

/// A peer discovered by any discovery layer.
#[derive(Debug, Clone)]
pub struct DiscoveredPeer {
    pub device_id: Uuid,
    pub device_name: String,
    pub addrs: Vec<IpAddr>,
    pub port: u16,
    pub source: DiscoverySource,
    pub protocol_version: Option<u16>,
    /// First 8 bytes of the identity key fingerprint for quick matching.
    /// Full fingerprint is exchanged during the TCP handshake.
    pub identity_fingerprint_prefix: Option<[u8; 8]>,
}

/// Events emitted by the DiscoveryManager to the engine.
#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    /// A new peer appeared that was not previously known.
    PeerAppeared(DiscoveredPeer),
    /// A known peer's addresses or discovery source changed.
    PeerUpdated(DiscoveredPeer),
    /// A peer disappeared from all discovery layers.
    PeerDisappeared {
        device_id: Uuid,
        source: DiscoverySource,
    },
}

// ── Internal state ────────────────────────────────────────────────────────────

/// Per-source freshness record for a discovered peer.
#[derive(Debug, Clone)]
struct SourceRecord {
    source: DiscoverySource,
    addrs: Vec<IpAddr>,
    port: u16,
    last_seen: Instant,
    device_name: String,
}

/// Merged record for a single peer across all discovery sources.
#[derive(Debug, Clone)]
struct MergedPeer {
    device_id: Uuid,
    sources: Vec<SourceRecord>,
    /// Protocol version reported by the peer (latest seen).
    protocol_version: Option<u16>,
    /// Identity fingerprint prefix (latest seen).
    identity_fingerprint_prefix: Option<[u8; 8]>,
    /// Whether we have emitted a PeerAppeared event for this peer.
    announced: bool,
}

impl MergedPeer {
    fn new(peer: &DiscoveredPeer) -> Self {
        Self {
            device_id: peer.device_id,
            sources: vec![SourceRecord {
                source: peer.source,
                addrs: peer.addrs.clone(),
                port: peer.port,
                last_seen: Instant::now(),
                device_name: peer.device_name.clone(),
            }],
            protocol_version: peer.protocol_version,
            identity_fingerprint_prefix: peer.identity_fingerprint_prefix,
            announced: false,
        }
    }

    /// Update this merged record with a new discovery event.
    /// Returns true if the peer's effective addresses changed.
    fn update(&mut self, peer: &DiscoveredPeer) -> bool {
        if let Some(v) = peer.protocol_version {
            self.protocol_version = Some(v);
        }
        if let Some(fp) = peer.identity_fingerprint_prefix {
            self.identity_fingerprint_prefix = Some(fp);
        }

        // Find existing source record or create new.
        let existing = self.sources.iter_mut().find(|s| s.source == peer.source);
        let addrs_changed;
        match existing {
            Some(record) => {
                addrs_changed = record.addrs != peer.addrs || record.port != peer.port;
                record.addrs = peer.addrs.clone();
                record.port = peer.port;
                record.last_seen = Instant::now();
                record.device_name = peer.device_name.clone();
            }
            None => {
                addrs_changed = true;
                self.sources.push(SourceRecord {
                    source: peer.source,
                    addrs: peer.addrs.clone(),
                    port: peer.port,
                    last_seen: Instant::now(),
                    device_name: peer.device_name.clone(),
                });
            }
        }
        addrs_changed
    }

    /// Remove stale source records. Returns list of removed sources.
    fn evict_stale(&mut self) -> Vec<DiscoverySource> {
        let mut removed = Vec::new();
        self.sources.retain(|s| {
            let timeout = staleness_timeout(s.source);
            if s.last_seen.elapsed() > timeout {
                removed.push(s.source);
                false
            } else {
                true
            }
        });
        removed
    }

    /// True if this peer has no remaining discovery sources.
    fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Best available device name (prefer mDNS > manual > beacon).
    fn best_name(&self) -> String {
        // Priority: Manual > Mdns > UdpBeacon > UdpMulticast > Unknown
        let priority = |s: &DiscoverySource| -> u8 {
            match s {
                DiscoverySource::Manual => 0,
                DiscoverySource::Mdns => 1,
                DiscoverySource::HotspotProbe => 2,
                DiscoverySource::LanProbe => 3,
                DiscoverySource::UdpBeacon => 4,
                DiscoverySource::UdpMulticast => 5,
                DiscoverySource::Unknown => 6,
            }
        };
        let mut sorted = self.sources.clone();
        sorted.sort_by_key(|s| priority(&s.source));
        sorted
            .first()
            .map(|s| s.device_name.clone())
            .unwrap_or_else(|| format!("device-{}", &self.device_id.to_string()[..8]))
    }

    /// All unique addresses across all sources, sorted IPv4-first.
    fn merged_addrs(&self) -> Vec<IpAddr> {
        let mut addrs: Vec<IpAddr> = self
            .sources
            .iter()
            .flat_map(|s| s.addrs.iter().copied())
            .collect();
        addrs.sort_unstable();
        addrs.dedup();
        // IPv4 first for reliability.
        addrs.sort_by_key(|a| if a.is_ipv4() { 0u8 } else { 1 });
        addrs
    }

    /// Best port (prefer mDNS source, then most recent).
    fn best_port(&self) -> u16 {
        self.sources
            .iter()
            .find(|s| s.source == DiscoverySource::Mdns)
            .or_else(|| self.sources.iter().max_by_key(|s| s.last_seen))
            .map(|s| s.port)
            .unwrap_or(crate::protocol::DEFAULT_PORT)
    }

    /// Primary discovery source (highest priority active source).
    fn primary_source(&self) -> DiscoverySource {
        if self
            .sources
            .iter()
            .any(|s| s.source == DiscoverySource::Mdns)
        {
            DiscoverySource::Mdns
        } else if self
            .sources
            .iter()
            .any(|s| s.source == DiscoverySource::Manual)
        {
            DiscoverySource::Manual
        } else if self
            .sources
            .iter()
            .any(|s| s.source == DiscoverySource::HotspotProbe)
        {
            DiscoverySource::HotspotProbe
        } else if self
            .sources
            .iter()
            .any(|s| s.source == DiscoverySource::LanProbe)
        {
            DiscoverySource::LanProbe
        } else if self
            .sources
            .iter()
            .any(|s| s.source == DiscoverySource::UdpBeacon)
        {
            DiscoverySource::UdpBeacon
        } else if self
            .sources
            .iter()
            .any(|s| s.source == DiscoverySource::UdpMulticast)
        {
            DiscoverySource::UdpMulticast
        } else {
            DiscoverySource::Unknown
        }
    }

    /// Convert to DiscoveredPeer for event emission.
    fn to_discovered_peer(&self) -> DiscoveredPeer {
        DiscoveredPeer {
            device_id: self.device_id,
            device_name: self.best_name(),
            addrs: self.merged_addrs(),
            port: self.best_port(),
            source: self.primary_source(),
            protocol_version: self.protocol_version,
            identity_fingerprint_prefix: self.identity_fingerprint_prefix,
        }
    }

    /// List of all discovery sources currently tracking this peer.
    fn active_sources(&self) -> Vec<DiscoverySource> {
        self.sources.iter().map(|s| s.source).collect()
    }
}

// ── DiscoveryManager ──────────────────────────────────────────────────────────

/// Accepts discovery events from all layers, deduplicates, and emits a unified stream.
pub struct DiscoveryManager {
    /// Our own device ID (filter out self-discovery).
    my_device_id: Uuid,
    /// Merged peer records keyed by device_id.
    peers: HashMap<Uuid, MergedPeer>,
    /// Channel to receive raw discovery events from all layers.
    input_rx: mpsc::Receiver<DiscoveryInput>,
    /// Channel to send unified events to the engine.
    output_tx: mpsc::Sender<DiscoveryEvent>,
    /// Staleness check interval.
    eviction_interval: Duration,
}

/// Input event from any discovery layer.
#[derive(Debug, Clone)]
pub enum DiscoveryInput {
    /// A peer was found or refreshed by a discovery layer.
    Found(DiscoveredPeer),
    /// A peer was explicitly lost by a specific layer (e.g., mDNS ServiceRemoved).
    Lost {
        device_id: Uuid,
        source: DiscoverySource,
    },
    /// Trigger a refresh of all discovery layers (e.g., after network change).
    RefreshAll,
}

/// Handle for discovery layers to submit events.
#[derive(Clone)]
pub struct DiscoveryInputHandle {
    pub(crate) tx: mpsc::Sender<DiscoveryInput>,
}

impl DiscoveryInputHandle {
    /// Submit a peer discovery event.
    pub async fn found(&self, peer: DiscoveredPeer) {
        if self.tx.send(DiscoveryInput::Found(peer)).await.is_err() {
            warn!("discovery manager input channel closed");
        }
    }

    /// Submit a peer lost event.
    pub async fn lost(&self, device_id: Uuid, source: DiscoverySource) {
        if self
            .tx
            .send(DiscoveryInput::Lost { device_id, source })
            .await
            .is_err()
        {
            warn!("discovery manager input channel closed");
        }
    }

    /// Request a refresh of all discovery layers.
    pub async fn refresh(&self) {
        let _ = self.tx.send(DiscoveryInput::RefreshAll).await;
    }

    /// Non-async version for use in synchronous contexts.
    pub fn found_blocking(&self, peer: DiscoveredPeer) {
        let _ = self.tx.try_send(DiscoveryInput::Found(peer));
    }
}

impl DiscoveryManager {
    /// Create a new DiscoveryManager.
    ///
    /// Returns:
    /// - The manager (must be spawned via `run()`)
    /// - An input handle for discovery layers to submit events
    /// - An output receiver for the engine to consume unified events
    pub fn new(my_device_id: Uuid) -> (Self, DiscoveryInputHandle, mpsc::Receiver<DiscoveryEvent>) {
        let (input_tx, input_rx) = mpsc::channel(256);
        let (output_tx, output_rx) = mpsc::channel(64);

        let manager = Self {
            my_device_id,
            peers: HashMap::new(),
            input_rx,
            output_tx,
            eviction_interval: Duration::from_secs(10),
        };

        let handle = DiscoveryInputHandle { tx: input_tx };

        (manager, handle, output_rx)
    }

    /// Run the discovery manager event loop. Spawned as a tokio task.
    pub async fn run(mut self) {
        let mut evict_timer = tokio::time::interval(self.eviction_interval);
        evict_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                input = self.input_rx.recv() => {
                    match input {
                        Some(DiscoveryInput::Found(peer)) => {
                            self.handle_found(peer).await;
                        }
                        Some(DiscoveryInput::Lost { device_id, source }) => {
                            self.handle_lost(device_id, source).await;
                        }
                        Some(DiscoveryInput::RefreshAll) => {
                            debug!("discovery manager: refresh requested");
                            // The engine handles restarting individual layers.
                            // We just evict stale peers to get a clean slate.
                            self.evict_all_stale().await;
                        }
                        None => {
                            info!("discovery manager: input channel closed, shutting down");
                            break;
                        }
                    }
                }
                _ = evict_timer.tick() => {
                    self.evict_all_stale().await;
                }
            }
        }
    }

    #[tracing::instrument(skip_all, fields(device_id = %peer.device_id, source = ?peer.source))]
    async fn handle_found(&mut self, peer: DiscoveredPeer) {
        // Ignore our own advertisements.
        if peer.device_id == self.my_device_id {
            return;
        }

        // Filter incompatible protocol versions.
        if let Some(v) = peer.protocol_version {
            if v != crate::protocol::PROTOCOL_VERSION {
                debug!(
                    "discovery manager: ignoring peer {} with protocol v{} (we speak v{})",
                    peer.device_id,
                    v,
                    crate::protocol::PROTOCOL_VERSION
                );
                return;
            }
        }

        if let Some(merged) = self.peers.get_mut(&peer.device_id) {
            let addrs_before = merged.merged_addrs();
            let port_before = merged.best_port();
            merged.update(&peer);
            let addrs_after = merged.merged_addrs();
            let port_after = merged.best_port();
            let effective_changed = addrs_before != addrs_after || port_before != port_after;

            if effective_changed && merged.announced {
                let event = DiscoveryEvent::PeerUpdated(merged.to_discovered_peer());
                let _ = self.output_tx.send(event).await;
            } else if !merged.announced {
                merged.announced = true;
                let event = DiscoveryEvent::PeerAppeared(merged.to_discovered_peer());
                info!(
                    "discovery manager: peer appeared {} '{}' via {:?} at {:?}:{}",
                    peer.device_id,
                    merged.best_name(),
                    merged.active_sources(),
                    merged.merged_addrs(),
                    merged.best_port()
                );
                let _ = self.output_tx.send(event).await;
            }
        } else {
            let mut merged = MergedPeer::new(&peer);
            merged.announced = true;
            let event = DiscoveryEvent::PeerAppeared(merged.to_discovered_peer());
            info!(
                "discovery manager: new peer {} '{}' via {:?} at {:?}:{}",
                peer.device_id,
                merged.best_name(),
                merged.active_sources(),
                merged.merged_addrs(),
                merged.best_port()
            );
            self.peers.insert(peer.device_id, merged);
            let _ = self.output_tx.send(event).await;
        }
    }

    #[tracing::instrument(skip_all, fields(device_id = %device_id, source = ?source))]
    async fn handle_lost(&mut self, device_id: Uuid, source: DiscoverySource) {
        if let Some(merged) = self.peers.get_mut(&device_id) {
            merged.sources.retain(|s| s.source != source);
            if merged.is_empty() {
                let _ = self
                    .output_tx
                    .send(DiscoveryEvent::PeerDisappeared { device_id, source })
                    .await;
                self.peers.remove(&device_id);
                debug!(
                    "discovery manager: peer {} removed (last source {:?} lost)",
                    device_id, source
                );
            } else {
                debug!(
                    "discovery manager: peer {} lost source {:?}, still has {:?}",
                    device_id,
                    source,
                    merged.active_sources()
                );
            }
        }
    }

    async fn evict_all_stale(&mut self) {
        let mut to_remove = Vec::new();
        for (device_id, merged) in self.peers.iter_mut() {
            let removed_sources = merged.evict_stale();
            if !removed_sources.is_empty() {
                debug!(
                    "discovery manager: peer {} evicted stale sources {:?}",
                    device_id, removed_sources
                );
            }
            if merged.is_empty() {
                to_remove.push(*device_id);
            }
        }
        for device_id in to_remove {
            let _ = self
                .output_tx
                .send(DiscoveryEvent::PeerDisappeared {
                    device_id,
                    source: DiscoverySource::Unknown,
                })
                .await;
            self.peers.remove(&device_id);
            debug!(
                "discovery manager: peer {} fully evicted (all sources stale)",
                device_id
            );
        }
    }

    /// Snapshot of all currently discovered peers. Used for diagnostics.
    #[allow(dead_code)]
    pub fn discovered_peers(&self) -> Vec<DiscoveredPeer> {
        self.peers
            .values()
            .filter(|m| m.announced)
            .map(|m| m.to_discovered_peer())
            .collect()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn make_peer(id: Uuid, source: DiscoverySource) -> DiscoveredPeer {
        DiscoveredPeer {
            device_id: id,
            device_name: format!("test-{}", &id.to_string()[..8]),
            addrs: vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))],
            port: 47823,
            source,
            protocol_version: Some(crate::protocol::PROTOCOL_VERSION),
            identity_fingerprint_prefix: None,
        }
    }

    #[tokio::test]
    async fn ignores_self_discovery() {
        let my_id = Uuid::new_v4();
        let (manager, handle, mut output) = DiscoveryManager::new(my_id);

        let manager_task = tokio::spawn(manager.run());

        // Submit self-discovery — should be ignored.
        handle.found(make_peer(my_id, DiscoverySource::Mdns)).await;

        // Submit a real peer.
        let peer_id = Uuid::new_v4();
        handle
            .found(make_peer(peer_id, DiscoverySource::Mdns))
            .await;

        // Should only get the real peer.
        let event = tokio::time::timeout(Duration::from_millis(100), output.recv())
            .await
            .expect("timeout waiting for event")
            .expect("channel closed");

        match event {
            DiscoveryEvent::PeerAppeared(p) => assert_eq!(p.device_id, peer_id),
            other => panic!("expected PeerAppeared, got {:?}", other),
        }

        drop(handle);
        let _ = manager_task.await;
    }

    #[tokio::test]
    async fn deduplicates_multi_source() {
        let my_id = Uuid::new_v4();
        let peer_id = Uuid::new_v4();
        let (manager, handle, mut output) = DiscoveryManager::new(my_id);

        let manager_task = tokio::spawn(manager.run());

        // Peer appears via mDNS.
        handle
            .found(make_peer(peer_id, DiscoverySource::Mdns))
            .await;
        let event = tokio::time::timeout(Duration::from_millis(100), output.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(event, DiscoveryEvent::PeerAppeared(_)));

        // Same peer appears via UDP beacon with same address — no duplicate event.
        handle
            .found(make_peer(peer_id, DiscoverySource::UdpBeacon))
            .await;

        // Give the manager time to process.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Should not get another PeerAppeared (might get PeerUpdated if addrs differ).
        let result = tokio::time::timeout(Duration::from_millis(50), output.recv()).await;
        // Since addrs are the same, no event should be emitted.
        assert!(
            result.is_err(),
            "should not emit duplicate event for same addrs"
        );

        drop(handle);
        let _ = manager_task.await;
    }

    #[tokio::test]
    async fn emits_update_on_new_address() {
        let my_id = Uuid::new_v4();
        let peer_id = Uuid::new_v4();
        let (manager, handle, mut output) = DiscoveryManager::new(my_id);

        let manager_task = tokio::spawn(manager.run());

        // Initial discovery.
        handle
            .found(make_peer(peer_id, DiscoverySource::Mdns))
            .await;
        let _ = tokio::time::timeout(Duration::from_millis(100), output.recv())
            .await
            .unwrap()
            .unwrap();

        // Same peer, different address via UDP.
        let mut peer2 = make_peer(peer_id, DiscoverySource::UdpBeacon);
        peer2.addrs = vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20))];
        handle.found(peer2).await;

        let event = tokio::time::timeout(Duration::from_millis(100), output.recv())
            .await
            .unwrap()
            .unwrap();
        match event {
            DiscoveryEvent::PeerUpdated(p) => {
                assert_eq!(p.device_id, peer_id);
                assert!(p.addrs.len() >= 2, "should have merged addrs");
            }
            other => panic!("expected PeerUpdated, got {:?}", other),
        }

        drop(handle);
        let _ = manager_task.await;
    }

    #[tokio::test]
    async fn explicit_lost_removes_source() {
        let my_id = Uuid::new_v4();
        let peer_id = Uuid::new_v4();
        let (manager, handle, mut output) = DiscoveryManager::new(my_id);

        let manager_task = tokio::spawn(manager.run());

        // Peer seen via two sources.
        handle
            .found(make_peer(peer_id, DiscoverySource::Mdns))
            .await;
        let _ = output.recv().await;

        let mut beacon_peer = make_peer(peer_id, DiscoverySource::UdpBeacon);
        beacon_peer.addrs = vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20))];
        handle.found(beacon_peer).await;
        let _ = output.recv().await; // PeerUpdated

        // mDNS source lost — peer should still be alive via UDP.
        handle.lost(peer_id, DiscoverySource::Mdns).await;
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Should NOT get PeerDisappeared since UDP source still active.
        let result = tokio::time::timeout(Duration::from_millis(50), output.recv()).await;
        assert!(
            result.is_err(),
            "peer should not disappear while UDP source active"
        );

        // Now lose UDP too.
        handle.lost(peer_id, DiscoverySource::UdpBeacon).await;
        let event = tokio::time::timeout(Duration::from_millis(100), output.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(event, DiscoveryEvent::PeerDisappeared { .. }));

        drop(handle);
        let _ = manager_task.await;
    }

    #[tokio::test]
    async fn filters_incompatible_protocol() {
        let my_id = Uuid::new_v4();
        let peer_id = Uuid::new_v4();
        let (manager, handle, mut output) = DiscoveryManager::new(my_id);

        let manager_task = tokio::spawn(manager.run());

        let mut peer = make_peer(peer_id, DiscoverySource::Mdns);
        peer.protocol_version = Some(999); // Incompatible
        handle.found(peer).await;

        // Should not appear.
        let result = tokio::time::timeout(Duration::from_millis(100), output.recv()).await;
        assert!(result.is_err(), "incompatible peer should be filtered");

        drop(handle);
        let _ = manager_task.await;
    }

    #[test]
    fn merged_peer_sorts_ipv4_first() {
        let id = Uuid::new_v4();
        let mut merged = MergedPeer::new(&DiscoveredPeer {
            device_id: id,
            device_name: "test".into(),
            addrs: vec![
                IpAddr::V6("fe80::1".parse().unwrap()),
                IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            ],
            port: 47823,
            source: DiscoverySource::Mdns,
            protocol_version: None,
            identity_fingerprint_prefix: None,
        });
        merged.announced = true;
        let addrs = merged.merged_addrs();
        assert!(addrs[0].is_ipv4(), "IPv4 should come first");
    }

    #[test]
    fn staleness_timeout_values_are_reasonable() {
        assert!(staleness_timeout(DiscoverySource::Mdns) >= Duration::from_secs(30));
        assert!(staleness_timeout(DiscoverySource::UdpBeacon) >= Duration::from_secs(10));
        assert!(staleness_timeout(DiscoverySource::UdpMulticast) >= Duration::from_secs(10));
    }
}
