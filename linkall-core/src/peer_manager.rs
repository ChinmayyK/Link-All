//! Link All peer manager — device lifecycle + session registry.
//!
//! Device state model (layered):
//!
//! ```text
//! Layer          Meaning
//! ─────────────────────────────────────────────────────
//! trusted        Is this device cryptographically allowed?
//! remembered     Is the pairing persisted across restarts?
//! connected      Is there an active TCP session right now?
//! sync_enabled   Should clipboard data flow to/from this peer?
//! auto_connect   Reconnect automatically on startup / network restore?
//! ```

use crate::protocol::AppMessage;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

/// How stale a persisted `last_seen` may get before a sighting alone is
/// worth a peers.json write.
const LAST_SEEN_PERSIST_SECS: u64 = 300;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PeerConnectionState {
    Connected,
    #[default]
    Disconnected,
    Connecting,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySource {
    Mdns,
    Manual,
    UdpBeacon,
    UdpMulticast,
    /// Discovered via hotspot gateway probing (Android/iPhone hotspot).
    HotspotProbe,
    /// Discovered via active outbound TCP connect sweep of the local subnet.
    /// Unlike mDNS/UDP beacons, this never listens for unsolicited inbound
    /// traffic, so it keeps working even when a firewall policy blocks that
    /// (e.g. locked-down corporate networks) without requiring any firewall
    /// exception or admin rights.
    LanProbe,
    #[default]
    Unknown,
}

/// How the last pairing attempt with a peer ended, kept so the UI can say
/// why a request disappeared instead of silently dropping back to "Pair".
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PairingOutcome {
    Accepted,
    /// The other device declined our request.
    Declined,
    /// The other device withdrew the request it sent us.
    Cancelled,
    /// Nobody answered within `pairing::PAIRING_TIMEOUT`.
    Expired,
    /// The other device runs a Link All from before this pairing flow. Those
    /// decline on their own after 30 s, so the answer says nothing about its
    /// user; updating Link All there fixes it.
    UpdateNeeded,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceLifecycleState {
    Discovered,
    PendingApproval,
    PairingInProgress,
    Paired,
    Trusted,
    Connecting,
    Reconnecting,
    Connected,
    AutoConnected,
}

/// Historical address record for a peer, enabling peer cache probe
/// to try old addresses when a peer moves between IPs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddrRecord {
    pub addr: std::net::SocketAddr,
    pub last_seen_at: u64,
    pub success_count: u32,
}

/// Full device record persisted in the peer store.
///
/// Internal `id` (UUID) is NEVER shown in primary UI.
/// Use `friendly_name` for all user-facing display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PeerRecord {
    pub id: Uuid,
    pub friendly_name: String,
    pub platform: Option<String>,
    pub ips: Vec<IpAddr>,
    pub port: u16,

    // ── Lifecycle layers ──────────────────────────────────────────────────────
    pub trusted: bool,
    pub remembered: bool,
    pub sync_enabled: bool,
    /// Indicates if the remote peer has globally paused their sync.
    /// Default true (active). If false, UI should show "Sync Paused".
    pub remote_sync_enabled: bool,
    pub auto_connect: bool,

    // ── Runtime state ─────────────────────────────────────────────────────────
    pub status: PeerConnectionState,
    pub last_seen: Option<u64>,
    pub last_sync: Option<u64>,
    pub discovery: DiscoverySource,
    pub last_error: Option<String>,
    /// User manually disconnected this peer and auto-reconnect must stay off
    /// until a fresh, explicit reconnect action is initiated.
    pub explicit_disconnect: bool,
    /// This peer asked to pair and our user has not answered yet.
    pub pairing_requested: bool,
    /// We asked this peer to pair and are waiting for its answer.
    pub outgoing_pairing_waiting: bool,
    /// Security code of the live session, shown on both devices while pairing.
    pub pairing_pin: Option<String>,
    /// Unix seconds when the current pairing request (either direction) began.
    #[serde(default)]
    pub pairing_started_at: Option<u64>,
    /// How the last pairing attempt ended; cleared when a new one starts.
    #[serde(default)]
    pub pairing_outcome: Option<PairingOutcome>,
    /// Unix seconds when our user last declined this peer, for the re-ask cooldown.
    #[serde(default)]
    pub pairing_declined_at: Option<u64>,
    /// Link All version the peer sent in its last handshake. Empty when it
    /// connected without one: builds before the version exchange.
    #[serde(default)]
    pub app_version: Option<String>,
    /// The peer said its OS may pause it in the background (a phone with
    /// battery limits on), so its drops while asleep are expected. None when
    /// it didn't say: a computer, or an older build.
    #[serde(default)]
    pub background_restricted: Option<bool>,
    /// The peer's network card address, kept from its last handshake so it
    /// can be woken with Wake-on-LAN while it sleeps and is not connected.
    #[serde(default)]
    pub mac_address: Option<String>,

    // ── Multi-layer discovery state ──────────────────────────────────────────
    /// When this peer was last seen via any discovery layer (separate from
    /// `last_seen` which tracks TCP session activity).
    #[serde(default)]
    pub last_discovery_at: Option<u64>,
    /// Which discovery layers have seen this peer.
    #[serde(default)]
    pub discovery_sources: Vec<DiscoverySource>,
    /// Historical IP+port records — enables peer cache probe to try old addresses.
    #[serde(default)]
    pub addr_history: Vec<AddrRecord>,
    /// When this peer last disconnected (enables adaptive probe scheduling).
    #[serde(default)]
    pub last_disconnect_at: Option<u64>,
    /// When a connection to this peer last failed (with `last_error`).
    #[serde(default)]
    pub last_failure_at: Option<u64>,

    // ── Computed fields (for UI serialization) ───────────────────────────────
    #[serde(default)]
    pub lifecycle_state: Option<DeviceLifecycleState>,
    /// Trust-store key fingerprint, formatted for display. Populated at
    /// status-serialization time from `TrustStore` (a separate store from
    /// this record) — not meaningfully persisted here.
    #[serde(default)]
    pub fingerprint_display: Option<String>,
    /// When this device's trust record was first observed (from
    /// `TrustRecord::first_seen`). Populated at status-serialization time,
    /// same as `fingerprint_display`.
    #[serde(default)]
    pub first_seen: Option<u64>,
    /// Seconds left before the current pairing request expires. Populated at
    /// status-serialization time from `pairing_started_at`.
    #[serde(default)]
    pub pairing_expires_in_secs: Option<u64>,
}

impl Default for PeerRecord {
    fn default() -> Self {
        Self {
            id: Uuid::nil(),
            friendly_name: String::new(),
            platform: None,
            ips: Vec::new(),
            port: crate::protocol::DEFAULT_PORT,
            trusted: false,
            remembered: true,
            sync_enabled: true,
            remote_sync_enabled: true,
            auto_connect: true,
            status: PeerConnectionState::Disconnected,
            last_seen: None,
            last_sync: None,
            discovery: DiscoverySource::Unknown,
            last_error: None,
            explicit_disconnect: false,
            pairing_requested: false,
            outgoing_pairing_waiting: false,
            pairing_pin: None,
            pairing_started_at: None,
            pairing_outcome: None,
            pairing_declined_at: None,
            app_version: None,
            background_restricted: None,
            mac_address: None,
            last_discovery_at: None,
            discovery_sources: Vec::new(),
            addr_history: Vec::new(),
            last_disconnect_at: None,
            last_failure_at: None,
            lifecycle_state: None,
            fingerprint_display: None,
            first_seen: None,
            pairing_expires_in_secs: None,
        }
    }
}

impl PeerRecord {
    pub fn lifecycle_state(&self) -> DeviceLifecycleState {
        if self.trusted {
            match self.status {
                PeerConnectionState::Connected => {
                    if self.auto_connect {
                        DeviceLifecycleState::AutoConnected
                    } else {
                        DeviceLifecycleState::Connected
                    }
                }
                PeerConnectionState::Connecting => {
                    if self.explicit_disconnect {
                        DeviceLifecycleState::Connecting
                    } else {
                        DeviceLifecycleState::Reconnecting
                    }
                }
                _ => DeviceLifecycleState::Trusted,
            }
        } else if self.pairing_requested {
            DeviceLifecycleState::PendingApproval
        } else if self.status == PeerConnectionState::Connecting
            || self.status == PeerConnectionState::Connected
            || self.outgoing_pairing_waiting
        {
            DeviceLifecycleState::PairingInProgress
        } else {
            DeviceLifecycleState::Discovered
        }
    }

    /// Seconds left on the current pairing request, if one is in flight.
    pub fn pairing_expires_in(&self) -> Option<u64> {
        if !self.pairing_requested && !self.outgoing_pairing_waiting {
            return None;
        }
        let started = self.pairing_started_at?;
        let timeout = crate::pairing::PAIRING_TIMEOUT.as_secs();
        Some((started + timeout).saturating_sub(now_secs()))
    }

    /// Whether our user declined this peer recently enough to refuse a re-ask.
    pub fn in_decline_cooldown(&self) -> bool {
        self.pairing_declined_at.is_some_and(|at| {
            now_secs().saturating_sub(at) < crate::pairing::DECLINE_COOLDOWN.as_secs()
        })
    }

    /// Records an address to dial this peer at. True when it was new.
    /// An IPv4 address seen through a dual-stack socket (`::ffff:a.b.c.d`)
    /// is stored as plain IPv4, or every reconnect dialled the host twice.
    fn add_ip(&mut self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        if self.ips.contains(&ip) {
            return false;
        }
        self.ips.push(ip);
        // Every stored IP is dialled on reconnect; a phone that
        // roams DHCP leases would otherwise grow this forever.
        if self.ips.len() > 8 {
            self.ips.remove(0);
        }
        true
    }

    pub fn socket_addrs(&self) -> Vec<SocketAddr> {
        self.ips
            .iter()
            .map(|ip| SocketAddr::new(*ip, self.port))
            .collect()
    }

    /// Whether this peer should receive clipboard payloads right now.
    pub fn is_sync_eligible(&self) -> bool {
        self.trusted && self.sync_enabled
    }

    /// Whether this peer should reconnect automatically.
    pub fn should_auto_reconnect(&self) -> bool {
        self.trusted && self.remembered && self.auto_connect && !self.explicit_disconnect
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PeerStoreData {
    peers: HashMap<Uuid, PeerRecord>,
}

struct LivePeerSession {
    session_id: u64,
    endpoint: SocketAddr,
    pub sender: mpsc::Sender<AppMessage>,
    pub file_sender: mpsc::Sender<AppMessage>,
    pub shutdown_tx: Option<oneshot::Sender<SessionShutdown>>,
    pub is_outbound: bool,
    pub connected_at: u64,
}

#[derive(Debug)]
pub struct SessionShutdown {
    pub reason: String,
    pub send_bye: bool,
    pub explicit_disconnect: bool,
}

#[derive(Debug)]
pub struct ReplacedSession {
    pub session_id: u64,
    pub endpoint: SocketAddr,
    pub shutdown_tx: Option<oneshot::Sender<SessionShutdown>>,
}

pub struct PeerManager {
    path: PathBuf,
    store: dashmap::DashMap<Uuid, PeerRecord>,
    live: dashmap::DashMap<Uuid, LivePeerSession>,
    // `RwLock` instead of `Mutex`: manual_targets is read-heavy (checked on
    // every reconnect cycle) and never held across an `.await` point.  Using
    // `std::sync::Mutex` in an async context risks blocking a Tokio worker
    // thread for the full duration of a lock contention window (HIGH-02).
    manual_targets: dashmap::DashMap<SocketAddr, u32>,
    next_session_id: AtomicU64,
    // Saves are fire-and-forget blocking tasks that may run out of order, so
    // each carries a sequence number (assigned with its snapshot) and a writer
    // holding `last_written_seq` skips a snapshot older than one on disk.
    save_seq: std::sync::Mutex<u64>,
    last_written_seq: std::sync::Arc<std::sync::Mutex<u64>>,
}

impl PeerManager {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut store: PeerStoreData = if path.exists() {
            let bytes = std::fs::read(&path).context("reading peer store")?;
            if bytes.is_empty() {
                PeerStoreData::default()
            } else {
                serde_json::from_slice(&bytes)
                    .unwrap_or_else(|err| crate::settings::set_aside_corrupt(&path, &err))
            }
        } else {
            PeerStoreData::default()
        };

        // Connections do not persist across restarts.
        for peer in store.peers.values_mut() {
            peer.status = PeerConnectionState::Disconnected;
        }

        // Stores written before add_ip hold IPv4 hosts twice, once as
        // `::ffff:a.b.c.d`.
        for peer in store.peers.values_mut() {
            for ip in std::mem::take(&mut peer.ips) {
                peer.add_ip(ip);
            }
        }

        // One-time migration: a prior bug (see upsert_peer_ext) hard-coded
        // `remembered: true` on every bare mDNS/UDP sighting, so peer stores
        // written before this fix are full of phantom "known" devices that
        // were never actually paired. Correct that on load: `remembered`
        // should only be true if the peer has an actual relationship with
        // the user (trusted, or pairing is currently in flight) - the same
        // invariant `upsert_peer_ext` now maintains for newly-discovered
        // peers going forward.
        for peer in store.peers.values_mut() {
            if !peer.trusted && !peer.pairing_requested && !peer.outgoing_pairing_waiting {
                peer.remembered = false;
            }
            // A paired device is a remembered one. Pairing used to set only
            // `trusted`, leaving devices first seen unpaired (the usual way)
            // unremembered, so the reconnector never dialled them.
            if peer.trusted {
                peer.remembered = true;
            }
        }

        // Pairing requests ride on a session and its code, and neither
        // survives a restart; their expiry timers are gone too. Cleared after
        // the migration above, which reads the flags.
        for peer in store.peers.values_mut() {
            peer.pairing_requested = false;
            peer.outgoing_pairing_waiting = false;
            peer.pairing_pin = None;
            peer.pairing_started_at = None;
            peer.pairing_outcome = None; // "declined" days later is noise
        }

        let store_dashmap = dashmap::DashMap::new();
        for (k, v) in store.peers {
            store_dashmap.insert(k, v);
        }

        Ok(Self {
            path,
            store: store_dashmap,
            live: dashmap::DashMap::new(),
            manual_targets: dashmap::DashMap::new(),
            next_session_id: AtomicU64::new(1),
            save_seq: std::sync::Mutex::new(0),
            last_written_seq: std::sync::Arc::new(std::sync::Mutex::new(0)),
        })
    }

    pub fn save(&self) -> Result<()> {
        let path = self.path.clone();

        // Snapshot and sequence number are taken together so a higher
        // sequence always means a newer snapshot.
        let (seq, bytes) = {
            let mut seq = self.save_seq.lock().unwrap_or_else(|e| e.into_inner());
            let store_to_save = PeerStoreData {
                peers: self
                    .store
                    .iter()
                    .filter(|p| p.value().trusted || p.value().remembered)
                    .map(|p| (*p.key(), p.value().clone()))
                    .collect(),
            };
            // Compact, not pretty — internal cache, not hand-edited, and this
            // fires on every connect/reconnect/disconnect (so, repeatedly
            // during a flaky hotspot/Wi-Fi session), same reasoning as
            // history.rs's persist().
            let bytes = serde_json::to_vec(&store_to_save)?;
            *seq += 1;
            (*seq, bytes)
        };

        let last_written = self.last_written_seq.clone();
        let save_fn = move || -> Result<()> {
            use rand::Rng;
            use std::io::Write;
            let mut last = last_written.lock().unwrap_or_else(|e| e.into_inner());
            if seq <= *last {
                return Ok(()); // a newer snapshot already landed
            }
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let rng_suffix: u32 = rand::thread_rng().gen();
            let tmp = path.with_extension(format!("tmp.{}", rng_suffix));
            let result = (|| -> Result<()> {
                let mut file =
                    std::fs::File::create(&tmp).context("creating peer store temp file")?;
                file.write_all(&bytes).context("writing peer store")?;
                // Without this a power loss can leave a zero-length file
                // after the rename, which load() treats as "no peers".
                file.sync_all().context("syncing peer store")?;
                drop(file);
                std::fs::rename(&tmp, &path).context("renaming peer store")?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    *last = seq;
                    Ok(())
                }
                Err(err) => {
                    let _ = std::fs::remove_file(&tmp);
                    Err(err)
                }
            }
        };

        if tokio::runtime::Handle::try_current().is_ok() {
            // A runtime worker must not block on disk I/O, so failures here
            // can only be logged; the sync path below propagates them.
            tokio::task::spawn_blocking(move || {
                if let Err(err) = save_fn() {
                    tracing::warn!(error = %err, "failed to persist peer store");
                }
            });
            Ok(())
        } else {
            save_fn()
        }
    }

    pub fn list(&self) -> Vec<PeerRecord> {
        self.store.iter().map(|p| p.value().clone()).collect()
    }

    #[cfg(test)]
    pub(crate) fn replace_for_test(&self, record: PeerRecord) {
        self.store.insert(record.id, record);
    }

    pub fn get(&self, device_id: Uuid) -> Option<PeerRecord> {
        self.store.get(&device_id).map(|p| p.value().clone())
    }

    pub fn upsert_peer(
        &self,
        device_id: Uuid,
        friendly_name: String,
        endpoint: SocketAddr,
        trusted: bool,
        discovery: DiscoverySource,
    ) -> Result<PeerRecord> {
        self.upsert_peer_ext(device_id, friendly_name, endpoint, trusted, discovery, None)
    }

    pub fn upsert_peer_ext(
        &self,
        device_id: Uuid,
        friendly_name: String,
        endpoint: SocketAddr,
        trusted: bool,
        discovery: DiscoverySource,
        platform: Option<String>,
    ) -> Result<PeerRecord> {
        if self.store.len() > 1000 {
            self.prune_stale_peers();
            if self.store.len() > 1000 {
                return Err(anyhow::anyhow!("Peer limit reached"));
            }
        }
        let now = now_secs();
        // Beacons repeat every few seconds for every peer on the LAN, so
        // rewriting peers.json on each sighting kept the disk (and, on
        // phones, the CPU) busy for nothing. Persist only when something
        // durable changed, or to refresh a stale last_seen.
        let mut changed = !self.store.contains_key(&device_id);
        let record = {
            let is_placeholder = |name: &str| {
                // "Deskdrop Device" is what peers sent before the rename.
                name.starts_with("device-")
                    || name.eq_ignore_ascii_case("Link All Device")
                    || name.eq_ignore_ascii_case("Deskdrop Device")
            };

            // Deduplicate old peers if they have the exact same non-placeholder name
            // DO NOT deduplicate if the old peer is trusted to prevent unauthenticated
            // attackers from spoofing a name and deleting a trusted pairing (Unauthenticated DoS on Trust).
            if !is_placeholder(&friendly_name) {
                let mut duplicates = Vec::new();
                for p in self.store.iter() {
                    if *p.key() != device_id
                        && p.value().friendly_name == friendly_name
                        && p.value().status == PeerConnectionState::Disconnected
                        && !p.value().trusted
                    {
                        duplicates.push(*p.key());
                    }
                }
                for id in duplicates {
                    self.store.remove(&id);
                    changed = true;
                }
            }

            let mut record = self.store.entry(device_id).or_insert_with(|| PeerRecord {
                id: device_id,
                friendly_name: friendly_name.clone(),
                platform: platform.clone(),
                ips: vec![endpoint.ip().to_canonical()],
                port: endpoint.port(),
                trusted,
                // A bare discovery sighting (mDNS/UDP beacon) is not a
                // relationship with the user yet — only mark it "remembered"
                // (and thus shown as a known device, see PeerViewModel.IsKnown
                // on the Windows client) if it's already trusted. Otherwise it
                // surfaces as a transient "Nearby" entry until the user pairs.
                remembered: trusted,
                sync_enabled: true,
                remote_sync_enabled: true,
                auto_connect: true,
                last_seen: Some(now),
                last_sync: None,
                status: PeerConnectionState::Disconnected,
                discovery,
                last_error: None,
                explicit_disconnect: false,
                pairing_requested: false,
                outgoing_pairing_waiting: false,
                pairing_pin: None,
                last_discovery_at: Some(now),
                discovery_sources: vec![discovery],
                addr_history: vec![AddrRecord {
                    addr: endpoint,
                    last_seen_at: now,
                    success_count: 1,
                }],
                last_disconnect_at: None,
                last_failure_at: None,
                lifecycle_state: None,
                fingerprint_display: None,
                first_seen: None,
                ..PeerRecord::default()
            });

            // Do not overwrite a real name with a placeholder name.
            if (!is_placeholder(&friendly_name)
                || is_placeholder(&record.friendly_name)
                || record.friendly_name.is_empty())
                && record.friendly_name != friendly_name
            {
                record.friendly_name = friendly_name;
                changed = true;
            }
            if platform.is_some() && record.platform != platform {
                record.platform = platform;
                changed = true;
            }
            if record.add_ip(endpoint.ip()) {
                changed = true;
            }
            if record.port != endpoint.port() || record.trusted != trusted {
                record.port = endpoint.port();
                record.trusted = trusted;
                changed = true;
            }
            if trusted && !record.remembered {
                record.remembered = true;
                changed = true;
            }
            if record
                .last_seen
                .is_none_or(|t| now.saturating_sub(t) >= LAST_SEEN_PERSIST_SECS)
            {
                changed = true;
            }
            record.last_seen = Some(now);
            if record.discovery == DiscoverySource::Unknown {
                record.discovery = discovery;
                changed = true;
            }

            // Track multi-layer discovery metadata.
            record.last_discovery_at = Some(now);
            if !record.discovery_sources.contains(&discovery) {
                record.discovery_sources.push(discovery);
                changed = true;
            }
            // Maintain address history (cap at 10 entries).
            let addr = endpoint;
            if let Some(existing) = record.addr_history.iter_mut().find(|r| r.addr == addr) {
                existing.last_seen_at = now;
                existing.success_count = existing.success_count.saturating_add(1);
            } else {
                changed = true;
                record.addr_history.push(AddrRecord {
                    addr,
                    last_seen_at: now,
                    success_count: 1,
                });
                // Keep only the 10 most recently seen.
                if record.addr_history.len() > 10 {
                    record
                        .addr_history
                        .sort_by_key(|b| std::cmp::Reverse(b.last_seen_at));
                    record.addr_history.truncate(10);
                }
            }

            record.clone()
        };

        if changed {
            self.save()?;
        }
        Ok(record)
    }

    pub fn mark_connecting(&self, device_id: Uuid, endpoint: Option<SocketAddr>) -> Result<bool> {
        if let Some(endpoint) = endpoint {
            if self.live_endpoint(device_id) == Some(endpoint) {
                return Ok(false);
            }
        }

        {
            let mut entry = self.store.entry(device_id).or_insert_with(|| PeerRecord {
                id: device_id,
                port: endpoint
                    .map(|addr| addr.port())
                    .unwrap_or(crate::protocol::DEFAULT_PORT),
                ips: endpoint
                    .map(|addr| vec![addr.ip().to_canonical()])
                    .unwrap_or_default(),
                status: PeerConnectionState::Connecting,
                ..PeerRecord::default()
            });
            if entry.status == PeerConnectionState::Connecting
                || entry.status == PeerConnectionState::Connected
            {
                return Ok(false);
            }
            if let Some(endpoint) = endpoint {
                entry.add_ip(endpoint.ip());
                entry.port = endpoint.port();
            }
            entry.status = PeerConnectionState::Connecting;
            entry.last_error = None;
        }
        self.save()?;
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn replace_live_session(
        &self,
        local_device_id: Uuid,
        device_id: Uuid,
        is_outbound: bool,
        endpoint: SocketAddr,
        sender: mpsc::Sender<AppMessage>,
        file_sender: mpsc::Sender<AppMessage>,
        shutdown_tx: oneshot::Sender<SessionShutdown>,
    ) -> Result<(u64, Option<ReplacedSession>, bool)> {
        let we_are_initiator = local_device_id < device_id;
        let incoming_is_winner = we_are_initiator == is_outbound;

        let session_id = self.next_session_id.fetch_add(1, Ordering::Relaxed);
        let new_session = LivePeerSession {
            session_id,
            endpoint,
            sender,
            file_sender,
            shutdown_tx: Some(shutdown_tx),
            is_outbound,
            connected_at: now_secs(),
        };

        // Atomic check-and-replace under a single shard lock (dashmap's
        // Entry holds the shard guard for the whole match) - closes a race
        // where two concurrent sessions for the same peer could each read
        // the same stale `existing` value via a separate get(), both pass
        // the tie-break check below, and then both insert - with whichever
        // insert() lands last silently winning the `live` slot regardless
        // of which one the tie-break rule says should win.
        let replaced = match self.live.entry(device_id) {
            dashmap::mapref::entry::Entry::Occupied(mut occupied) => {
                let existing_is_winner = we_are_initiator == occupied.get().is_outbound;
                if !incoming_is_winner && existing_is_winner {
                    return Ok((0, None, true)); // rejected_new = true
                }
                Some(occupied.insert(new_session))
            }
            dashmap::mapref::entry::Entry::Vacant(vacant) => {
                vacant.insert(new_session);
                None
            }
        };

        {
            // An incoming session's endpoint is the peer's outgoing port,
            // which can't be dialled; the caller has already recorded where
            // the peer listens.
            let dial = is_outbound.then_some(endpoint);
            let mut entry = self.store.entry(device_id).or_insert_with(|| PeerRecord {
                id: device_id,
                port: dial.map_or(crate::protocol::DEFAULT_PORT, |addr| addr.port()),
                ips: vec![endpoint.ip().to_canonical()],
                ..PeerRecord::default()
            });
            if let Some(dial) = dial {
                entry.add_ip(dial.ip());
                entry.port = dial.port();
            }
            entry.last_seen = Some(now_secs());
            entry.status = PeerConnectionState::Connected;
            entry.last_error = None;
        }
        self.save()?;

        Ok((
            session_id,
            replaced.map(|session| ReplacedSession {
                session_id: session.session_id,
                endpoint: session.endpoint,
                shutdown_tx: session.shutdown_tx,
            }),
            false,
        ))
    }

    pub fn mark_disconnected(&self, device_id: Uuid, reason: Option<String>) -> Result<()> {
        self.live.remove(&device_id);
        {
            if let Some(mut entry) = self.store.get_mut(&device_id) {
                entry.status = PeerConnectionState::Disconnected;
                entry.last_error = reason;
                entry.last_disconnect_at = Some(now_secs());
            }
        }
        self.save()
    }

    pub fn mark_disconnected_if_current(
        &self,
        device_id: Uuid,
        session_id: u64,
        reason: Option<String>,
    ) -> Result<Option<u64>> {
        let connected_at = {
            if let Some(current) = self.live.get(&device_id) {
                if current.session_id != session_id {
                    return Ok(None);
                }
                current.connected_at
            } else {
                return Ok(None);
            }
        };

        self.mark_disconnected(device_id, reason)?;
        Ok(Some(connected_at))
    }

    pub fn mark_failed_all(&self, device_id: Uuid, reason: String) -> Result<()> {
        if self.live.contains_key(&device_id) {
            return Ok(());
        }
        self.live.remove(&device_id);
        {
            if let Some(mut entry) = self.store.get_mut(&device_id) {
                entry.status = PeerConnectionState::Failed;
                entry.last_error = Some(reason);
                entry.last_failure_at = Some(now_secs());
            }
        }
        self.save()
    }

    pub fn mark_failed(&self, device_id: Uuid, endpoint: SocketAddr, reason: String) -> Result<()> {
        let live = self
            .live
            .get(&device_id)
            .map(|session| (session.endpoint, session.is_outbound));
        if let Some((live_endpoint, live_is_outbound)) = live {
            if live_endpoint != endpoint {
                {
                    if let Some(mut entry) = self.store.get_mut(&device_id) {
                        // Only an outgoing session's endpoint can be dialled again.
                        if live_is_outbound {
                            entry.add_ip(live_endpoint.ip());
                            entry.port = live_endpoint.port();
                        }
                        entry.status = PeerConnectionState::Connected;
                        entry.last_error = Some(reason);
                        entry.last_failure_at = Some(now_secs());
                    }
                }
                return self.save();
            }
            // live_endpoint == endpoint: falls through to the "mark Failed"
            // code below, unchanged.
        } else {
            // No live session was visible above. Re-check immediately
            // before writing Failed: closes the race where a concurrent
            // replace_live_session() call registers a live session in
            // between - without this, a stale failure report for a losing
            // multi-address attempt could clobber a peer's status to Failed
            // right after it successfully connected. Mirrors the existing
            // guard in mark_failed_all().
            if self.live.contains_key(&device_id) {
                return Ok(());
            }
        }

        self.live.remove(&device_id);
        {
            if let Some(mut entry) = self.store.get_mut(&device_id) {
                entry.status = PeerConnectionState::Failed;
                entry.last_error = Some(reason);
                entry.last_failure_at = Some(now_secs());
            }
        }
        self.save()
    }

    pub fn update_trust(&self, device_id: Uuid, trusted: bool) -> Result<()> {
        {
            if let Some(mut entry) = self.store.get_mut(&device_id) {
                entry.trusted = trusted;
                // Pairing makes a device one to reconnect to by itself.
                if trusted {
                    entry.remembered = true;
                }
            }
        }
        self.save()
    }

    pub fn update_last_sync(&self, device_id: Uuid) -> Result<()> {
        {
            if let Some(mut entry) = self.store.get_mut(&device_id) {
                entry.last_sync = Some(now_secs());
                entry.last_seen = Some(now_secs());
            }
        }
        self.save()
    }

    // ── Device lifecycle controls ─────────────────────────────────────────────

    pub fn set_sync_enabled(&self, device_id: Uuid, enabled: bool) -> Result<bool> {
        let found = {
            if let Some(mut entry) = self.store.get_mut(&device_id) {
                entry.sync_enabled = enabled;
                true
            } else {
                false
            }
        };
        if found {
            self.save()?;
        }
        Ok(found)
    }

    pub fn set_remote_sync_enabled(&self, device_id: Uuid, enabled: bool) -> Result<bool> {
        let found = {
            if let Some(mut entry) = self.store.get_mut(&device_id) {
                entry.remote_sync_enabled = enabled;
                true
            } else {
                false
            }
        };
        if found {
            self.save()?;
        }
        Ok(found)
    }

    pub fn set_auto_connect(&self, device_id: Uuid, auto_connect: bool) -> Result<bool> {
        let found = {
            if let Some(mut entry) = self.store.get_mut(&device_id) {
                entry.auto_connect = auto_connect;
                true
            } else {
                false
            }
        };
        if found {
            self.save()?;
        }
        Ok(found)
    }

    /// Sets whether this peer has an active pairing request pending.
    /// Raising it (re)starts the request clock.
    pub fn set_pairing_requested(&self, device_id: Uuid, requested: bool) -> Result<bool> {
        self.update_pairing_flags(device_id, |entry| entry.pairing_requested = requested)
    }

    /// Sets whether we are waiting on this peer to answer our request.
    /// Raising it (re)starts the request clock.
    pub fn set_outgoing_pairing_waiting(&self, device_id: Uuid, waiting: bool) -> Result<bool> {
        self.update_pairing_flags(device_id, |entry| entry.outgoing_pairing_waiting = waiting)
    }

    fn update_pairing_flags(
        &self,
        device_id: Uuid,
        apply: impl FnOnce(&mut PeerRecord),
    ) -> Result<bool> {
        let changed = {
            if let Some(mut entry) = self.store.get_mut(&device_id) {
                let was_pending = (entry.pairing_requested, entry.outgoing_pairing_waiting);
                apply(&mut entry);
                let now_pending = (entry.pairing_requested, entry.outgoing_pairing_waiting);
                let raised = (now_pending.0 && !was_pending.0) || (now_pending.1 && !was_pending.1);
                if raised {
                    entry.pairing_started_at = Some(now_secs());
                    entry.pairing_outcome = None;
                } else if !now_pending.0 && !now_pending.1 {
                    entry.pairing_started_at = None;
                }
                true
            } else {
                false
            }
        };
        if changed {
            self.save()?;
        }
        Ok(changed)
    }

    /// Closes any pairing request with this peer, in both directions, and
    /// records how it ended. `None` means the local user ended it, which the
    /// UI needs no message for.
    pub fn end_pairing(&self, device_id: Uuid, outcome: Option<PairingOutcome>) -> Result<bool> {
        let found = {
            if let Some(mut entry) = self.store.get_mut(&device_id) {
                let we_asked = entry.outgoing_pairing_waiting;
                entry.pairing_requested = false;
                entry.outgoing_pairing_waiting = false;
                entry.pairing_started_at = None;
                entry.pairing_outcome = match outcome {
                    Some(PairingOutcome::Declined | PairingOutcome::Expired)
                        if we_asked && entry.app_version.as_deref() == Some("") =>
                    {
                        Some(PairingOutcome::UpdateNeeded)
                    }
                    other => other,
                };
                true
            } else {
                false
            }
        };
        if found {
            self.save()?;
        }
        Ok(found)
    }

    /// Records what the peer said about itself in its handshake. An empty
    /// platform (older peers) keeps whatever was known before.
    pub fn set_handshake_info(
        &self,
        device_id: Uuid,
        version: String,
        platform: String,
        background_restricted: Option<bool>,
        mac_address: Option<String>,
    ) {
        let changed = match self.store.get_mut(&device_id) {
            Some(mut entry) => {
                entry.app_version = Some(version);
                let mut changed = entry.background_restricted != background_restricted;
                entry.background_restricted = background_restricted;
                // A peer that did not say (older build, or a card we cannot
                // read) keeps the address already known.
                if mac_address.is_some() && entry.mac_address != mac_address {
                    entry.mac_address = mac_address;
                    changed = true;
                }
                if !platform.is_empty() && entry.platform.as_deref() != Some(platform.as_str()) {
                    entry.platform = Some(platform);
                    changed = true;
                }
                changed
            }
            None => false,
        };
        if changed {
            let _ = self.save();
        }
    }

    /// Records that our user declined this peer, starting the re-ask cooldown.
    pub fn mark_pairing_declined(&self, device_id: Uuid) -> Result<bool> {
        let found = {
            if let Some(mut entry) = self.store.get_mut(&device_id) {
                entry.pairing_declined_at = Some(now_secs());
                true
            } else {
                false
            }
        };
        if found {
            self.save()?;
        }
        Ok(found)
    }

    /// Sets the pairing PIN for this peer.
    pub fn set_pairing_pin(&self, device_id: Uuid, pin: Option<String>) -> Result<bool> {
        let found = {
            if let Some(mut entry) = self.store.get_mut(&device_id) {
                entry.pairing_pin = pin;
                true
            } else {
                false
            }
        };
        if found {
            self.save()?;
        }
        Ok(found)
    }

    pub fn set_explicit_disconnect(&self, device_id: Uuid, explicit: bool) -> Result<bool> {
        let found = {
            if let Some(mut entry) = self.store.get_mut(&device_id) {
                entry.explicit_disconnect = explicit;
                if explicit {
                    entry.status = PeerConnectionState::Disconnected;
                    entry.last_error = Some("manually disconnected".to_string());
                }
                true
            } else {
                false
            }
        };
        if found {
            self.save()?;
        }
        Ok(found)
    }

    pub fn is_explicitly_disconnected(&self, device_id: Uuid) -> bool {
        self.store
            .get(&device_id)
            .map(|entry| entry.explicit_disconnect)
            .unwrap_or(false)
    }

    pub fn forget_device(&self, device_id: Uuid) -> Result<bool> {
        let found = {
            if let Some(mut entry) = self.store.get_mut(&device_id) {
                entry.trusted = false;
                entry.remembered = false;
                entry.auto_connect = false;
                entry.explicit_disconnect = true;
                entry.pairing_requested = false;
                entry.outgoing_pairing_waiting = false;
                entry.pairing_started_at = None;
                entry.pairing_pin = None;
                true
            } else {
                false
            }
        };
        if found {
            self.save()?;
        }
        Ok(found)
    }

    /// Drop a device we have no relationship with (not trusted, not
    /// remembered, no pairing in flight, not connected) once discovery stops
    /// seeing it. It is only a "Nearby" entry; the next sighting adds it again.
    /// `forget_device` is for the user's own disconnect and marks the device
    /// explicitly disconnected, which refuses every automatic connection, so
    /// using it here blocked a nearby device (a freshly reinstalled phone, say)
    /// from ever connecting or asking to pair after one missed announcement.
    pub fn drop_stranger(&self, device_id: Uuid) -> Result<bool> {
        if self.live.contains_key(&device_id) {
            return Ok(false);
        }
        let removed = self
            .store
            .remove_if(&device_id, |_, p| {
                !p.trusted && !p.remembered && !p.pairing_requested && !p.outgoing_pairing_waiting
            })
            .is_some();
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    /// Stop dialling this peer at any address it was last seen at: one of
    /// them now answers as another device, so that host (and its other
    /// addresses, IPv4 and IPv6 alike) is someone else now, most often the
    /// same phone after a reinstall. Dropping only the address that answered
    /// left the others to be redialled one by one, and every redial replaced
    /// the new device's live session and changed its pairing code mid-request.
    /// If this peer comes back, discovery records its address again.
    pub fn forget_addresses(&self, device_id: Uuid) -> Result<bool> {
        let removed = match self.store.get_mut(&device_id) {
            Some(mut entry) => {
                let had = !entry.ips.is_empty() || !entry.addr_history.is_empty();
                entry.ips.clear();
                entry.addr_history.clear();
                had
            }
            None => false,
        };
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    /// Stop dialling this peer at one address, because that address leads
    /// back to this device: a loopback address learnt through a VM's port
    /// forward, or a LAN address this device was given after the peer left.
    pub fn forget_address(&self, device_id: Uuid, ip: IpAddr) -> Result<bool> {
        let ip = ip.to_canonical();
        let removed = match self.store.get_mut(&device_id) {
            Some(mut entry) => {
                let before = entry.ips.len() + entry.addr_history.len();
                entry.ips.retain(|known| *known != ip);
                entry
                    .addr_history
                    .retain(|record| record.addr.ip().to_canonical() != ip);
                before != entry.ips.len() + entry.addr_history.len()
            }
            None => false,
        };
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    // ── Sender views ──────────────────────────────────────────────────────────

    /// Connected + trusted + sync_enabled peers — receives clipboard payloads.
    pub fn active_senders(&self) -> Vec<(Uuid, mpsc::Sender<AppMessage>)> {
        self.live
            .iter()
            .filter(|p| {
                self.store
                    .get(p.key())
                    .map(|p| p.value().is_sync_eligible())
                    .unwrap_or(false)
            })
            .map(|p| (*p.key(), p.value().sender.clone()))
            .collect()
    }

    /// Id of the peer's live session, if it has one.
    pub fn live_session_id(&self, device_id: Uuid) -> Option<u64> {
        self.live.get(&device_id).map(|s| s.session_id)
    }

    pub fn sender(&self, device_id: Uuid) -> Option<mpsc::Sender<AppMessage>> {
        self.live.get(&device_id).map(|s| s.sender.clone())
    }

    pub fn file_sender(&self, device_id: Uuid) -> Option<mpsc::Sender<AppMessage>> {
        self.live.get(&device_id).map(|s| s.file_sender.clone())
    }

    /// All connected peers regardless of sync state (for heartbeats / control).
    pub fn all_connected_senders(&self) -> Vec<(Uuid, mpsc::Sender<AppMessage>)> {
        self.live
            .iter()
            .map(|p| (*p.key(), p.value().sender.clone()))
            .collect()
    }

    /// All connected AND trusted peers (for manual file transfers, ignoring sync_enabled).
    pub fn all_trusted_senders(&self) -> Vec<(Uuid, mpsc::Sender<AppMessage>)> {
        self.live
            .iter()
            .filter(|p| {
                self.store
                    .get(p.key())
                    .map(|r| r.value().trusted)
                    .unwrap_or(false)
            })
            .map(|p| (*p.key(), p.value().sender.clone()))
            .collect()
    }

    /// Returns trusted, remembered, disconnected peers that are eligible for
    /// active probing by the peer cache probe module.
    ///
    /// A peer is eligible if:
    /// - It is trusted and remembered
    /// - It is not currently connected
    /// - It has at least one known address (from addr_history or current IPs)
    /// - It was not explicitly disconnected by the user
    pub fn peers_needing_probe(&self) -> Vec<PeerRecord> {
        self.store
            .iter()
            .map(|p| p.value().clone())
            .filter(|p| {
                p.trusted
                    && p.remembered
                    && !p.explicit_disconnect
                    && !self.live.contains_key(&p.id)
                    && (p.status == PeerConnectionState::Disconnected
                        || p.status == PeerConnectionState::Failed)
                    && (!p.addr_history.is_empty() || !p.ips.is_empty())
            })
            .collect()
    }

    /// Open incoming pairing requests from devices other than `except`: how
    /// many in all, and how many of those are connected from `host`.
    pub fn open_pairing_requests(&self, except: Uuid, host: Option<IpAddr>) -> (usize, usize) {
        let mut all = 0;
        let mut from_host = 0;
        for p in self.store.iter() {
            let open = p.pairing_requested && p.pairing_expires_in().is_some_and(|left| left > 0);
            if *p.key() == except || !open {
                continue;
            }
            all += 1;
            let ip = self.live_endpoint(*p.key()).map(|e| e.ip().to_canonical());
            if host.is_some() && ip == host {
                from_host += 1;
            }
        }
        (all, from_host)
    }

    pub fn is_connected(&self, device_id: Uuid) -> bool {
        self.live.contains_key(&device_id)
    }

    pub fn live_endpoint(&self, device_id: Uuid) -> Option<SocketAddr> {
        self.live.get(&device_id).map(|s| s.endpoint)
    }

    /// Whether a connected device is linked from this host, either way round.
    pub fn has_live_session_at(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        self.live
            .iter()
            .any(|s| s.endpoint.ip().to_canonical() == ip)
    }

    pub fn endpoint_for(&self, device_id: Uuid) -> Option<SocketAddr> {
        self.get(device_id)
            .and_then(|record| record.socket_addrs().first().copied())
    }

    pub fn note_manual_target(&self, endpoint: SocketAddr) {
        self.manual_targets.entry(endpoint).or_insert(0);
    }

    pub fn record_manual_failure(&self, endpoint: SocketAddr) {
        *self.manual_targets.entry(endpoint).or_insert(0) += 1;
    }

    pub fn clear_manual_target(&self, endpoint: SocketAddr) {
        self.manual_targets.remove(&endpoint);
    }

    pub fn manual_targets(&self) -> Vec<SocketAddr> {
        self.manual_targets.iter().map(|r| *r.key()).collect()
    }

    pub fn shutdown_all_sessions(&self, reason: &str) -> Result<Vec<ReplacedSession>> {
        let keys: Vec<_> = self.live.iter().map(|x| *x.key()).collect();
        let mut sessions = Vec::new();
        for k in keys {
            if let Some(session) = self.live.remove(&k) {
                sessions.push(session);
            }
        }
        {
            for mut entry in self.store.iter_mut() {
                entry.status = PeerConnectionState::Disconnected;
                entry.last_error = Some(reason.to_string());
            }
        }
        self.save()?;
        Ok(sessions
            .into_iter()
            .map(|s| ReplacedSession {
                session_id: s.1.session_id,
                endpoint: s.1.endpoint,
                shutdown_tx: s.1.shutdown_tx,
            })
            .collect())
    }

    pub fn shutdown_peer_session(&self, device_id: Uuid) -> Result<Option<ReplacedSession>> {
        let removed = self.live.remove(&device_id);
        {
            if let Some(mut entry) = self.store.get_mut(&device_id) {
                entry.status = PeerConnectionState::Disconnected;
                entry.last_error = Some("manually disconnected".to_string());
            }
        }
        self.save()?;
        Ok(removed.map(|s| ReplacedSession {
            session_id: s.1.session_id,
            endpoint: s.1.endpoint,
            shutdown_tx: s.1.shutdown_tx,
        }))
    }

    pub fn last_sync_at(&self) -> Option<u64> {
        self.store
            .iter()
            .map(|p| p.value().clone())
            .filter_map(|p| p.last_sync)
            .max()
    }

    pub fn connected_count(&self) -> usize {
        self.live.len()
    }

    /// Prune in-memory peer records for devices that are:
    ///   1. Not currently connected (not in the live session map), AND
    ///   2. Not persisted across restarts (`remembered = false`), AND
    ///   3. Not trusted (pruning trusted-but-forgotten peers would lose their
    ///      TOFU key, requiring re-verification on next connect).
    ///
    /// The daemon is designed to run for months; without this, every
    /// transiently-seen device accumulates an entry in the peer store (MED-05).
    ///
    /// Call periodically (e.g. every 5 minutes from a background task).
    pub fn prune_stale_peers(&self) -> usize {
        let live_ids: std::collections::HashSet<Uuid> =
            self.live.iter().map(|r| *r.key()).collect();
        let now = now_secs();
        // Discovery refreshes a present peer every few seconds, so an unpaired
        // device silent this long has left (e.g. devices seen on another network).
        const STALE_THRESHOLD_SECS: u64 = 10 * 60;
        let pruned = {
            let before = self.store.len();
            self.store.retain(|id, record| {
                // Always keep live connections
                if live_ids.contains(id) {
                    return true;
                }
                // Always keep trusted peers
                if record.trusted {
                    return true;
                }
                // Always keep remembered peers (see doc comment above).
                if record.remembered {
                    return true;
                }
                if record.pairing_requested || record.outgoing_pairing_waiting {
                    return true;
                }
                // Remove untrusted, unremembered, disconnected peers gone quiet
                let last_activity = record.last_seen.or(record.last_discovery_at).unwrap_or(0);
                if now.saturating_sub(last_activity) > STALE_THRESHOLD_SECS {
                    return false;
                }
                // Keep recent untrusted peers (they may be mid-pairing)
                true
            });
            before - self.store.len()
        };
        if pruned > 0 {
            tracing::info!(
                pruned,
                "pruned stale peer records (untrusted, not seen in 24h)"
            );
            let _ = self.save();
        }
        pruned
    }

    /// O(1) count of peers that are connected AND sync-eligible.
    ///
    /// Reads only the `live` session map (connected peers) and cross-checks
    /// `sync_enabled` from the persisted record, avoiding a full table scan.
    ///
    /// Uses `unwrap_or_else(|p| p.into_inner())` on both locks so a panicking
    /// task elsewhere cannot permanently poison the count (LOW-08).
    pub fn sync_eligible_count(&self) -> usize {
        let live_ids: Vec<Uuid> = self.live.iter().map(|r| *r.key()).collect();

        live_ids
            .iter()
            .filter(|id| {
                self.store
                    .get(id)
                    .map(|r| r.value().sync_enabled && r.value().trusted)
                    .unwrap_or(false)
            })
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use tempfile::NamedTempFile;
    use tokio::sync::oneshot;

    #[test]
    fn open_pairing_requests_counts_only_open_ones_from_others() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
        for (i, id) in ids.iter().enumerate() {
            manager
                .upsert_peer(
                    *id,
                    format!("Phone {i}"),
                    SocketAddr::from(([10, 0, 0, i as u8 + 1], 47823)),
                    false,
                    DiscoverySource::Mdns,
                )
                .unwrap();
        }
        manager.set_pairing_requested(ids[0], true).unwrap();
        manager.set_pairing_requested(ids[1], true).unwrap();

        assert_eq!(manager.open_pairing_requests(ids[2], None).0, 2);
        assert_eq!(manager.open_pairing_requests(ids[0], None).0, 1);
        manager.end_pairing(ids[1], None).unwrap();
        assert_eq!(manager.open_pairing_requests(ids[2], None).0, 1);
    }

    #[test]
    fn unreadable_store_is_set_aside_and_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("peers.json");
        std::fs::write(&path, b"\0\0\0garbage").unwrap();
        let manager = PeerManager::load(&path).expect("load must not fail");
        assert!(manager.list().is_empty());
        assert!(std::fs::read_dir(dir.path()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("peers.json.corrupt")));
    }

    #[test]
    fn persists_peer_records() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let id = Uuid::new_v4();
        manager
            .upsert_peer(
                id,
                "Desk".into(),
                SocketAddr::from(([192, 168, 1, 8], 47823)),
                true,
                DiscoverySource::Manual,
            )
            .unwrap();
        let manager2 = PeerManager::load(file.path()).unwrap();
        let peers = manager2.list();
        assert_eq!(peers.len(), 1);
        assert!(peers[0].trusted);
    }

    #[test]
    fn pause_sync_suppresses_senders() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let id = Uuid::new_v4();
        manager
            .upsert_peer(
                id,
                "Phone".into(),
                SocketAddr::from(([192, 168, 1, 10], 47823)),
                true,
                DiscoverySource::Mdns,
            )
            .unwrap();
        let (tx, _rx) = mpsc::channel(1);
        let (stop, _stop_rx) = oneshot::channel();
        manager
            .replace_live_session(
                Uuid::nil(),
                id,
                true,
                SocketAddr::from(([192, 168, 1, 10], 47823)),
                tx.clone(),
                tx,
                stop,
            )
            .unwrap();
        assert_eq!(manager.active_senders().len(), 1);
        manager.set_sync_enabled(id, false).unwrap();
        assert_eq!(manager.active_senders().len(), 0);
        manager.set_sync_enabled(id, true).unwrap();
        assert_eq!(manager.active_senders().len(), 1);
    }

    #[test]
    fn untrusted_discovery_sighting_is_not_remembered() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let id = Uuid::new_v4();
        let peer = manager
            .upsert_peer(
                id,
                "device-abcd1234".into(),
                SocketAddr::from(([192, 168, 1, 30], 47823)),
                false,
                DiscoverySource::UdpBeacon,
            )
            .unwrap();
        assert!(
            !peer.remembered,
            "bare discovery sighting must not be treated as a known/paired device"
        );
        assert!(!peer.trusted);
    }

    #[test]
    fn trusted_upsert_is_remembered() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let id = Uuid::new_v4();
        let peer = manager
            .upsert_peer(
                id,
                "Laptop".into(),
                SocketAddr::from(([192, 168, 1, 31], 47823)),
                true,
                DiscoverySource::Mdns,
            )
            .unwrap();
        assert!(peer.remembered);
    }

    #[test]
    fn pairing_a_seen_device_remembers_it_and_load_repairs_old_stores() {
        let file = NamedTempFile::new().unwrap();
        let id = Uuid::new_v4();
        let addr = SocketAddr::from(([192, 168, 1, 42], 47823));
        {
            let manager = PeerManager::load(file.path()).unwrap();
            manager
                .upsert_peer(id, "Pixel".into(), addr, false, DiscoverySource::Mdns)
                .unwrap();
            assert!(!manager.get(id).unwrap().remembered);
            manager.update_trust(id, true).unwrap();
            assert!(manager.get(id).unwrap().remembered, "pairing remembers it");
            // A store written before the fix: trusted, never remembered.
            if let Some(mut entry) = manager.store.get_mut(&id) {
                entry.remembered = false;
            }
            manager.save().unwrap();
        }
        let reloaded = PeerManager::load(file.path()).unwrap();
        assert!(reloaded.get(id).unwrap().should_auto_reconnect());
    }

    #[test]
    fn load_migrates_legacy_remembered_phantom_peers() {
        let file = NamedTempFile::new().unwrap();
        let id = Uuid::new_v4();
        {
            let manager = PeerManager::load(file.path()).unwrap();
            manager
                .upsert_peer(
                    id,
                    "device-abcd1234".into(),
                    SocketAddr::from(([192, 168, 1, 40], 47823)),
                    false,
                    DiscoverySource::UdpBeacon,
                )
                .unwrap();
            // Simulate a peer store written by the pre-fix code, which
            // hard-coded `remembered: true` on every bare discovery sighting.
            if let Some(mut entry) = manager.store.get_mut(&id) {
                entry.remembered = true;
            }
            manager.save().unwrap();
        }
        let reloaded = PeerManager::load(file.path()).unwrap();
        let peer = reloaded.get(id).unwrap();
        assert!(
            !peer.remembered,
            "legacy phantom peer must be demoted to Nearby on load"
        );
        assert!(!peer.trusted);
    }

    #[test]
    fn a_no_from_an_old_build_asks_for_an_update() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let (old, new) = (Uuid::new_v4(), Uuid::new_v4());
        for (id, version, name) in [(old, "", "Old phone"), (new, "1.3.4", "New phone")] {
            manager
                .upsert_peer(
                    id,
                    name.into(),
                    SocketAddr::from(([192, 168, 1, 41], 47823)),
                    false,
                    DiscoverySource::Mdns,
                )
                .unwrap();
            manager.set_handshake_info(id, version.into(), String::new(), None, None);
            manager.set_outgoing_pairing_waiting(id, true).unwrap();
            manager
                .end_pairing(id, Some(PairingOutcome::Declined))
                .unwrap();
        }
        assert_eq!(
            manager.get(old).unwrap().pairing_outcome,
            Some(PairingOutcome::UpdateNeeded)
        );
        assert_eq!(
            manager.get(new).unwrap().pairing_outcome,
            Some(PairingOutcome::Declined)
        );
    }

    #[test]
    fn pairing_clock_starts_on_request_and_records_outcome() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let id = Uuid::new_v4();
        manager
            .upsert_peer(
                id,
                "Laptop".into(),
                SocketAddr::from(([192, 168, 1, 41], 47823)),
                false,
                DiscoverySource::Mdns,
            )
            .unwrap();

        manager
            .end_pairing(id, Some(PairingOutcome::Declined))
            .unwrap();
        manager.set_outgoing_pairing_waiting(id, true).unwrap();
        let peer = manager.get(id).unwrap();
        assert!(peer.pairing_started_at.is_some());
        assert_eq!(
            peer.pairing_outcome, None,
            "a new request clears the old outcome"
        );
        // The clock may tick over a second between starting and reading it.
        let timeout = crate::pairing::PAIRING_TIMEOUT.as_secs();
        let left = peer.pairing_expires_in().unwrap();
        assert!(
            (timeout - 1..=timeout).contains(&left),
            "expected about {timeout}s left, got {left}s"
        );

        manager
            .end_pairing(id, Some(PairingOutcome::Expired))
            .unwrap();
        let peer = manager.get(id).unwrap();
        assert!(!peer.outgoing_pairing_waiting && peer.pairing_started_at.is_none());
        assert_eq!(peer.pairing_outcome, Some(PairingOutcome::Expired));
        assert_eq!(peer.pairing_expires_in(), None);
    }

    #[test]
    fn pairing_requests_do_not_survive_a_restart() {
        let file = NamedTempFile::new().unwrap();
        let id = Uuid::new_v4();
        {
            let manager = PeerManager::load(file.path()).unwrap();
            manager
                .upsert_peer(
                    id,
                    "Laptop".into(),
                    SocketAddr::from(([192, 168, 1, 41], 47823)),
                    false,
                    DiscoverySource::Mdns,
                )
                .unwrap();
            if let Some(mut entry) = manager.store.get_mut(&id) {
                entry.remembered = true; // kept on load, like any in-flight pairing
            }
            manager.set_outgoing_pairing_waiting(id, true).unwrap();
            manager.set_pairing_pin(id, Some("123 456".into())).unwrap();
        }
        let peer = PeerManager::load(file.path()).unwrap().get(id).unwrap();
        assert!(!peer.outgoing_pairing_waiting && !peer.pairing_requested);
        assert!(peer.pairing_pin.is_none() && peer.pairing_started_at.is_none());
    }

    #[test]
    fn load_keeps_remembered_for_in_flight_pairing() {
        let file = NamedTempFile::new().unwrap();
        let id = Uuid::new_v4();
        {
            let manager = PeerManager::load(file.path()).unwrap();
            manager
                .upsert_peer(
                    id,
                    "Nearby Laptop".into(),
                    SocketAddr::from(([192, 168, 1, 41], 47823)),
                    false,
                    DiscoverySource::Mdns,
                )
                .unwrap();
            if let Some(mut entry) = manager.store.get_mut(&id) {
                entry.remembered = true;
                entry.pairing_requested = true;
            }
            manager.save().unwrap();
        }
        let reloaded = PeerManager::load(file.path()).unwrap();
        let peer = reloaded.get(id).unwrap();
        assert!(
            peer.remembered,
            "a peer with pairing genuinely in flight must not be demoted"
        );
    }

    #[test]
    fn prune_keeps_remembered_untrusted_peers_past_staleness_window() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let id = Uuid::new_v4();
        manager
            .upsert_peer(
                id,
                "Mid-pairing device".into(),
                SocketAddr::from(([192, 168, 1, 32], 47823)),
                false,
                DiscoverySource::Manual,
            )
            .unwrap();
        // Simulate a peer the user actually cares about (e.g. mid-pairing)
        // that just hasn't been seen recently.
        if let Some(mut entry) = manager.store.get_mut(&id) {
            entry.remembered = true;
            entry.last_seen = Some(0);
            entry.last_discovery_at = Some(0);
        }
        let pruned = manager.prune_stale_peers();
        assert_eq!(pruned, 0);
        assert!(manager.get(id).is_some());
    }

    #[test]
    fn prune_drops_unpaired_peers_not_seen_recently() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let gone = Uuid::new_v4();
        let here = Uuid::new_v4();
        for id in [gone, here] {
            manager
                .upsert_peer(
                    id,
                    "device-0000".into(),
                    SocketAddr::from(([172, 16, 18, 11], 47823)),
                    false,
                    DiscoverySource::UdpBeacon,
                )
                .unwrap();
        }
        // Seen an hour ago on another network; the other peer is still beaconing.
        if let Some(mut entry) = manager.store.get_mut(&gone) {
            entry.last_seen = Some(now_secs() - 3600);
            entry.last_discovery_at = Some(now_secs() - 3600);
        }
        assert_eq!(manager.prune_stale_peers(), 1);
        assert!(manager.get(gone).is_none());
        assert!(manager.get(here).is_some());
    }

    #[test]
    fn forget_device_disables_auto_reconnect() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let id = Uuid::new_v4();
        manager
            .upsert_peer(
                id,
                "Tablet".into(),
                SocketAddr::from(([192, 168, 1, 20], 47823)),
                true,
                DiscoverySource::Mdns,
            )
            .unwrap();
        manager.forget_device(id).unwrap();
        let peer = manager.get(id).unwrap();
        assert!(!peer.trusted);
        assert!(!peer.remembered);
    }

    #[test]
    fn a_moved_device_is_not_redialled_at_any_of_its_old_addresses() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let old = Uuid::new_v4();
        for addr in [
            "192.168.1.21:47823",
            "[fe80::1]:47823",
            "[2405:201::1]:47823",
        ] {
            manager
                .upsert_peer(
                    old,
                    "Phone".into(),
                    addr.parse().unwrap(),
                    true,
                    DiscoverySource::Mdns,
                )
                .unwrap();
        }
        assert_eq!(manager.get(old).unwrap().ips.len(), 3);

        assert!(manager.forget_addresses(old).unwrap());
        let record = manager.get(old).unwrap();
        assert!(record.ips.is_empty() && record.addr_history.is_empty());
        assert!(record.trusted, "the pairing itself is kept");
    }

    #[test]
    fn an_ipv4_host_seen_through_a_dual_stack_socket_is_stored_once() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let id = Uuid::new_v4();
        for addr in ["192.168.1.21:47823", "[::ffff:192.168.1.21]:47823"] {
            manager
                .upsert_peer(
                    id,
                    "Phone".into(),
                    addr.parse().unwrap(),
                    true,
                    DiscoverySource::Mdns,
                )
                .unwrap();
        }
        let ip: IpAddr = "192.168.1.21".parse().unwrap();
        assert_eq!(manager.get(id).unwrap().ips, vec![ip]);
    }

    #[test]
    fn load_merges_ipv4_hosts_stored_twice() {
        let file = NamedTempFile::new().unwrap();
        let id = Uuid::new_v4();
        let record = PeerRecord {
            id,
            ips: vec![
                "192.168.1.21".parse().unwrap(),
                "::ffff:192.168.1.21".parse().unwrap(),
                "::ffff:192.168.1.22".parse().unwrap(),
            ],
            trusted: true,
            ..PeerRecord::default()
        };
        let data = PeerStoreData {
            peers: HashMap::from([(id, record)]),
        };
        std::fs::write(file.path(), serde_json::to_vec(&data).unwrap()).unwrap();

        let manager = PeerManager::load(file.path()).unwrap();
        let ips: Vec<IpAddr> = vec![
            "192.168.1.21".parse().unwrap(),
            "192.168.1.22".parse().unwrap(),
        ];
        assert_eq!(manager.get(id).unwrap().ips, ips);
    }

    #[test]
    fn a_host_with_an_incoming_session_counts_as_linked() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let (tx, _rx) = mpsc::channel(1);
        let (stop, _stop_rx) = oneshot::channel();
        manager
            .replace_live_session(
                Uuid::nil(),
                Uuid::new_v4(),
                false,
                "[::ffff:192.168.1.21]:53122".parse().unwrap(),
                tx.clone(),
                tx,
                stop,
            )
            .unwrap();
        assert!(manager.has_live_session_at("192.168.1.21".parse().unwrap()));
        assert!(!manager.has_live_session_at("192.168.1.22".parse().unwrap()));
    }

    #[test]
    fn an_address_that_leads_back_here_is_forgotten_alone() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let id = Uuid::new_v4();
        for addr in ["127.0.0.1:47823", "192.168.1.21:47823"] {
            manager
                .upsert_peer(
                    id,
                    "Ubuntu".into(),
                    addr.parse().unwrap(),
                    true,
                    DiscoverySource::Mdns,
                )
                .unwrap();
        }

        let loopback: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        assert!(manager.forget_address(id, loopback).unwrap());
        let record = manager.get(id).unwrap();
        let lan: IpAddr = "192.168.1.21".parse().unwrap();
        assert_eq!(record.ips, vec![lan]);
        assert!(record.addr_history.iter().all(|r| r.addr.ip() == lan));
        assert!(!manager.forget_address(id, loopback).unwrap());
    }

    #[test]
    fn a_stranger_leaving_discovery_can_still_connect_later() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let (stranger, paired) = (Uuid::new_v4(), Uuid::new_v4());
        let addr = SocketAddr::from(([192, 168, 1, 21], 47823));
        manager
            .upsert_peer(stranger, "Phone".into(), addr, false, DiscoverySource::Mdns)
            .unwrap();
        manager
            .upsert_peer(paired, "Laptop".into(), addr, true, DiscoverySource::Mdns)
            .unwrap();

        assert!(manager.drop_stranger(stranger).unwrap());
        assert!(manager.get(stranger).is_none());
        assert!(
            !manager.is_explicitly_disconnected(stranger),
            "a stranger that left mDNS must not be refused when it comes back"
        );

        assert!(
            !manager.drop_stranger(paired).unwrap(),
            "a paired device is kept"
        );
        assert!(manager.get(paired).is_some());
    }

    #[test]
    fn replacing_live_session_keeps_single_identity_record() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let id = Uuid::new_v4();
        manager
            .upsert_peer(
                id,
                "Desk".into(),
                SocketAddr::from(([192, 168, 1, 8], 47823)),
                true,
                DiscoverySource::Mdns,
            )
            .unwrap();
        let (tx1, _rx1) = mpsc::channel(1);
        let (stop1, _stop1_rx) = oneshot::channel();
        let (first_session_id, _, _) = manager
            .replace_live_session(
                Uuid::nil(),
                id,
                true,
                SocketAddr::from(([192, 168, 1, 8], 47823)),
                tx1.clone(),
                tx1,
                stop1,
            )
            .unwrap();
        let (tx2, _rx2) = mpsc::channel(1);
        let (stop2, _stop_rx2) = oneshot::channel();
        let (second_session_id, replaced, _) = manager
            .replace_live_session(
                Uuid::nil(),
                id,
                true,
                SocketAddr::from(([172, 20, 10, 4], 47823)),
                tx2.clone(),
                tx2,
                stop2,
            )
            .unwrap();
        let replaced = replaced.unwrap();
        assert_eq!(replaced.session_id, first_session_id);
        assert!(manager
            .mark_disconnected_if_current(id, first_session_id, Some("stale".into()))
            .unwrap()
            .is_none());
        assert!(manager
            .mark_disconnected_if_current(id, second_session_id, Some("closed".into()))
            .unwrap()
            .is_some());
        assert_eq!(manager.list().len(), 1);
        assert!(manager
            .get(id)
            .unwrap()
            .ips
            .contains(&IpAddr::V4(Ipv4Addr::new(172, 20, 10, 4))));
    }

    #[test]
    fn simultaneous_dial_both_sides_keep_same_stream() {
        // Stream X is dialed by A, stream Y by B. Each side sees the two
        // streams finish in opposite orders - the worst case for any
        // "keep the first one" rule.
        let stream_x = SocketAddr::from(([10, 0, 0, 1], 47823));
        let stream_y = SocketAddr::from(([10, 0, 0, 2], 47823));
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();

        let register = |mgr: &PeerManager, local: Uuid, peer: Uuid, outbound: bool, ep| {
            let (tx, _rx) = mpsc::channel(1);
            let (stop, _stop_rx) = oneshot::channel();
            mgr.replace_live_session(local, peer, outbound, ep, tx.clone(), tx, stop)
                .unwrap();
        };

        let file_a = NamedTempFile::new().unwrap();
        let mgr_a = PeerManager::load(file_a.path()).unwrap();
        register(&mgr_a, a, b, false, stream_y);
        register(&mgr_a, a, b, true, stream_x);

        let file_b = NamedTempFile::new().unwrap();
        let mgr_b = PeerManager::load(file_b.path()).unwrap();
        register(&mgr_b, b, a, false, stream_x);
        register(&mgr_b, b, a, true, stream_y);

        let kept_a = mgr_a.live_endpoint(b).unwrap();
        let kept_b = mgr_b.live_endpoint(a).unwrap();
        assert_eq!(kept_a, kept_b);
        let expected = if a < b { stream_x } else { stream_y };
        assert_eq!(kept_a, expected);
    }

    // ── Fix 14: connected_count and sync_eligible_count ───────────────────────

    #[test]
    fn connected_count_zero_when_no_sessions() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        assert_eq!(manager.connected_count(), 0);
    }

    #[test]
    fn connected_count_increments_with_live_sessions() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();

        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();

        for (id, name, ip) in [
            (id_a, "Alpha", [192, 168, 1, 10u8]),
            (id_b, "Beta", [192, 168, 1, 11]),
        ] {
            manager
                .upsert_peer(
                    id,
                    name.into(),
                    SocketAddr::from((ip, 47823)),
                    true,
                    DiscoverySource::Mdns,
                )
                .unwrap();
        }

        assert_eq!(manager.connected_count(), 0, "no live sessions yet");

        let (tx, _rx) = mpsc::channel(1);
        let (stop, _stop_rx) = oneshot::channel();
        manager
            .replace_live_session(
                Uuid::nil(),
                id_a,
                true,
                SocketAddr::from(([192, 168, 1, 10], 47823)),
                tx.clone(),
                tx,
                stop,
            )
            .unwrap();

        assert_eq!(manager.connected_count(), 1);
    }

    #[test]
    fn sync_eligible_count_excludes_untrusted_and_sync_disabled() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();

        let id_trusted = Uuid::new_v4();
        let id_untrusted = Uuid::new_v4();
        let id_nosync = Uuid::new_v4();

        // Trusted + sync enabled.
        manager
            .upsert_peer(
                id_trusted,
                "Trusted".into(),
                SocketAddr::from(([10, 0, 0, 1], 47823)),
                true,
                DiscoverySource::Mdns,
            )
            .unwrap();
        // Untrusted.
        manager
            .upsert_peer(
                id_untrusted,
                "Stranger".into(),
                SocketAddr::from(([10, 0, 0, 2], 47823)),
                false,
                DiscoverySource::Mdns,
            )
            .unwrap();
        // Trusted but sync disabled.
        manager
            .upsert_peer(
                id_nosync,
                "NoSync".into(),
                SocketAddr::from(([10, 0, 0, 3], 47823)),
                true,
                DiscoverySource::Mdns,
            )
            .unwrap();
        manager.set_sync_enabled(id_nosync, false).unwrap();

        // Give all three a live session.
        for (id, ip) in [
            (id_trusted, [10, 0, 0, 1u8]),
            (id_untrusted, [10, 0, 0, 2]),
            (id_nosync, [10, 0, 0, 3]),
        ] {
            let (tx, _rx) = mpsc::channel(1);
            let (stop, _) = oneshot::channel();
            manager
                .replace_live_session(
                    Uuid::nil(),
                    id,
                    true,
                    SocketAddr::from((ip, 47823)),
                    tx.clone(),
                    tx,
                    stop,
                )
                .unwrap();
        }

        assert_eq!(manager.connected_count(), 3, "all three connected");
        // Only id_trusted passes both trusted AND sync_enabled.
        assert_eq!(manager.sync_eligible_count(), 1);
    }

    #[test]
    fn does_not_overwrite_real_name_with_placeholder() {
        let file = NamedTempFile::new().unwrap();
        let manager = PeerManager::load(file.path()).unwrap();
        let id = Uuid::new_v4();

        // 1. Initial real name.
        manager
            .upsert_peer(
                id,
                "My Macbook".into(),
                SocketAddr::from(([10, 0, 0, 1], 47823)),
                true,
                DiscoverySource::Mdns,
            )
            .unwrap();
        assert_eq!(manager.get(id).unwrap().friendly_name, "My Macbook");

        // 2. Try overwriting with "Link All Device" (case insensitive).
        manager
            .upsert_peer(
                id,
                "Link All Device".into(),
                SocketAddr::from(([10, 0, 0, 1], 47823)),
                true,
                DiscoverySource::Mdns,
            )
            .unwrap();
        assert_eq!(manager.get(id).unwrap().friendly_name, "My Macbook");

        manager
            .upsert_peer(
                id,
                "deskdrop device".into(),
                SocketAddr::from(([10, 0, 0, 1], 47823)),
                true,
                DiscoverySource::Mdns,
            )
            .unwrap();
        assert_eq!(manager.get(id).unwrap().friendly_name, "My Macbook");

        // 3. Try overwriting with "device-" prefix.
        manager
            .upsert_peer(
                id,
                "device-12345678".into(),
                SocketAddr::from(([10, 0, 0, 1], 47823)),
                true,
                DiscoverySource::Mdns,
            )
            .unwrap();
        assert_eq!(manager.get(id).unwrap().friendly_name, "My Macbook");

        // 4. Overwrite placeholder with real name.
        let id2 = Uuid::new_v4();
        manager
            .upsert_peer(
                id2,
                "Link All Device".into(),
                SocketAddr::from(([10, 0, 0, 2], 47823)),
                true,
                DiscoverySource::Mdns,
            )
            .unwrap();
        assert_eq!(manager.get(id2).unwrap().friendly_name, "Link All Device");

        manager
            .upsert_peer(
                id2,
                "My Macbook".into(),
                SocketAddr::from(([10, 0, 0, 2], 47823)),
                true,
                DiscoverySource::Mdns,
            )
            .unwrap();
        assert_eq!(manager.get(id2).unwrap().friendly_name, "My Macbook");
    }

    // Regression test for the replace_live_session/mark_failed TOCTOU races:
    // hammers both from several real OS threads for the same peer and
    // asserts the invariant that must hold regardless of scheduling - if a
    // session is live, the store must agree it's Connected, never Failed.
    // Doesn't assert which attempt wins (that's scheduling-dependent); it
    // asserts the fix's actual guarantee, so it's a real regression check
    // rather than a flaky race-order assertion.
    #[test]
    fn concurrent_connect_and_failure_reports_never_leave_a_live_session_marked_failed() {
        let file = NamedTempFile::new().unwrap();
        let manager = std::sync::Arc::new(PeerManager::load(file.path()).unwrap());
        let id = Uuid::new_v4();
        let local_id = Uuid::new_v4();
        manager
            .upsert_peer(
                id,
                "Contested Device".into(),
                SocketAddr::from(([192, 168, 1, 200], 47823)),
                true,
                DiscoverySource::Mdns,
            )
            .unwrap();

        let mut handles = Vec::new();
        for i in 0..8u8 {
            let mgr = manager.clone();
            handles.push(std::thread::spawn(move || {
                let endpoint = SocketAddr::from(([192, 168, 1, 100 + i], 47823));
                let (tx, _rx) = mpsc::channel(1);
                let (stop, _stop_rx) = oneshot::channel();
                let is_outbound = i % 2 == 0;
                let result = mgr.replace_live_session(
                    local_id,
                    id,
                    is_outbound,
                    endpoint,
                    tx.clone(),
                    tx,
                    stop,
                );
                if let Ok((_, _, rejected_new)) = result {
                    if rejected_new {
                        let _ = mgr.mark_failed(id, endpoint, "lost tie-break".into());
                    }
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        if manager.live_endpoint(id).is_some() {
            let record = manager.get(id).unwrap();
            assert_eq!(record.status, PeerConnectionState::Connected);
        }
    }
}
