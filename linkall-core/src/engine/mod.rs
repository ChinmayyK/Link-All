use crate::activity::ActivityFeed;
use crate::dedup::hash_content;
use crate::discovery::{Discovery, PeerEvent, PeerInfo};
use crate::discovery_manager::{DiscoveryEvent, DiscoveryManager};
use crate::file_transfer::{default_save_dir, FileTransferManager};
use crate::identity::IdentityStore;
use crate::lan_probe::spawn_lan_probe;
use crate::mesh::{ClipboardApplyPolicy, MeshRouter};
use crate::network::{self, PeerSession, Server};
use crate::network_manager::{self, NetworkChangeEvent, NetworkInterfaceInfo};
use crate::peer_manager::{
    DiscoverySource, PairingOutcome, PeerConnectionState, PeerManager, PeerRecord, SessionShutdown,
};
use crate::probe::{self, ProbeResult, QualityProbe};
use crate::protocol::{
    AppMessage, ClipboardContent, FileTransferMetadata, HistoryMetadata, DEFAULT_PORT,
};
use crate::retry::Backoff;
use crate::settings::{
    default_peer_store_path, default_settings_path, default_trust_store_path, Settings,
    SettingsStore,
};
use crate::trust::{TrustRecord, TrustState, TrustStore};
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::timeout;
use tracing::{error, info, warn};
use uuid::Uuid;

// The engine is one `Engine` type whose methods are split by concern across
// these modules (each adds its own `impl Engine` block), plus the free
// functions that run sessions, connections, and discovery. Shared state
// lives in `types::EngineShared`.
mod background;
pub(crate) mod clipboard;
mod connection;
pub(crate) mod file_ops;
mod folder_ops;
mod health;
mod listener;
mod peer_discovery;
mod remote_ops;
mod session;
mod settings_ops;
pub(crate) mod telemetry;
mod transfer_ops;
mod trust_ops;
mod types;

use connection::*;
pub(crate) use file_ops::*;
pub use folder_ops::{FolderItem, FolderProgress, FolderSend};
pub use health::HealthIssue;
use listener::*;
use peer_discovery::*;
pub(crate) use remote_ops::*;
use session::*;
pub(crate) use transfer_ops::*;
use trust_ops::*;
pub use types::*;

#[derive(Clone)]
pub struct Engine {
    pub(crate) shared: EngineShared,
    pub(crate) seq: std::sync::Arc<tokio::sync::Mutex<u64>>,
}

impl Engine {
    pub async fn start(config: EngineConfig, event_tx: mpsc::Sender<EngineEvent>) -> Result<Self> {
        let mut config = config;
        ensure_parent(&config.trust_store_path)?;
        ensure_parent(&config.peer_store_path)?;
        ensure_parent(&config.identity_path)?;
        if config.data_dir.parent().is_some() {
            std::fs::create_dir_all(&config.data_dir).ok();
        }

        let (identity, trust, peer_manager, history) = tokio::task::spawn_blocking({
            let config = config.clone();
            move || {
                let identity = IdentityStore::new(&config.identity_path)
                    .load_or_create()
                    .context("loading identity key")?;
                let mut trust =
                    TrustStore::load(&config.trust_store_path).context("loading trust store")?;
                let peer_manager =
                    PeerManager::load(&config.peer_store_path).context("loading peer store")?;
                match trust.clear_old_installs_once() {
                    Ok(retired) => {
                        for id in retired {
                            let _ = peer_manager.forget_device(id);
                        }
                    }
                    Err(e) => tracing::warn!("could not clear old installs: {e:#}"),
                }
                let history_path = config.data_dir.join("history.json");
                let limit = config.history_limit.unwrap_or(500);
                let history = crate::history::History::load_with_limit(&history_path, limit)
                    .unwrap_or_else(|_| {
                        let tmp = std::env::temp_dir().join("linkall_history_fallback.json");
                        crate::history::History::load_with_limit(&tmp, limit)
                            .expect("cannot create fallback history store")
                    });
                Ok::<_, anyhow::Error>((identity, trust, peer_manager, history))
            }
        })
        .await
        .unwrap()?;

        if config.device_id.is_nil() {
            config.device_id = stable_device_id(identity.public_bytes);
        }
        let trust = Arc::new(Mutex::new(trust));
        let peer_manager = Arc::new(peer_manager);

        let (active_interface, bind_addr) = resolve_bind_address(&config)?;
        let (listener_tx, listener_rx) = mpsc::channel(8);
        let discovery_pair = if config.enable_discovery {
            let (tx, rx) = mpsc::channel(8);
            Some((tx, rx))
        } else {
            None
        };

        // Engine events pass through the folder tap on their way to the host.
        let host_event_tx = event_tx;
        let (event_tx, tap_rx) = mpsc::channel(1024);
        let shared = EngineShared {
            config: config.clone(),
            trust,
            peer_manager,
            event_tx: event_tx.clone(),
            identity_key: Arc::new(std::sync::RwLock::new(identity)),
            network_state: Arc::new(Mutex::new(RuntimeNetworkState {
                bind_addr,
                active_interface,
                listener_error: None,
            })),
            listener_tx: listener_tx.clone(),
            discovery_tx: discovery_pair.as_ref().map(|(tx, _)| tx.clone()),
            network_reconcile: Arc::new(Mutex::new(())),
            mesh_router: Arc::new(Mutex::new(MeshRouter::new(
                config.device_id,
                config.device_name.clone(),
            ))),
            activity: Arc::new(Mutex::new(ActivityFeed::new(200))),
            folders: Arc::default(),
            file_transfers: Arc::new(Mutex::new(FileTransferManager::new(
                config
                    .file_save_dir
                    .clone()
                    .unwrap_or_else(default_save_dir),
            ))),
            speed_tests: Arc::new(Mutex::new(std::collections::HashMap::new())),
            apply_policy: Arc::new(Mutex::new(ClipboardApplyPolicy::default())),
            settings: Arc::new(std::sync::Mutex::new(Settings::default())),
            quality_probes: Arc::new(dashmap::DashMap::new()),
            clipboard_store: Arc::new(Mutex::new(crate::engine_support::ClipboardStore::default())),
            local_clipboard: Arc::new(Mutex::new(crate::engine_support::LocalClipboard::new())),
            history: Arc::new(Mutex::new(history)),
            feedback: Arc::new(Mutex::new(crate::engine_support::FeedbackLog::new(200))),
            local_last_wake: Arc::new(std::sync::atomic::AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            )),
            local_sleeping: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            device_status: DeviceStatus::default(),

            dedup: Arc::new(Mutex::new(crate::dedup::Deduplicator::new())),
            qr_auth_token: Arc::new(Mutex::new(None)),
            remote_waiters: RemoteWaiters::default(),
            started_at: Instant::now(),
            network_hint: Arc::default(),
        };

        let engine = Self {
            shared: shared.clone(),
            seq: Arc::new(Mutex::new(0)),
        };
        folder_ops::spawn_folder_event_tap(
            shared.folders.clone(),
            shared.activity.clone(),
            tap_rx,
            host_event_tx,
        );

        spawn_listener_supervisor(shared.clone(), listener_rx);
        if let Some((_, discovery_rx)) = discovery_pair {
            spawn_discovery_supervisor(shared.clone(), discovery_rx);
            spawn_firewall_free_discovery(shared.clone());
        }

        let initial_bind = {
            let state = engine.shared.network_state.lock().await;
            state.bind_addr
        };
        send_listener_rebind(&engine.shared, initial_bind).await?;
        let discovery_ip = {
            let state = engine.shared.network_state.lock().await;
            state
                .active_interface
                .as_ref()
                .map(|i| i.ip)
                .unwrap_or(initial_bind.ip())
        };
        let _identity_pubkey = shared
            .identity_key
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .public_bytes;

        if let Some(discovery_tx) = &engine.shared.discovery_tx {
            if discovery_ip.is_unspecified() {
                // Network interface not ready yet at startup — spawn a background
                // task that retries until a real IP is available, then kicks mDNS.
                // Without this, discovery silently never starts and the user has
                // to manually click "Scan" to trigger it.
                let retry_shared = engine.shared.clone();
                let retry_tx = discovery_tx.clone();
                tokio::spawn(async move {
                    for attempt in 1..=20 {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        let ip = {
                            let state = retry_shared.network_state.lock().await;
                            state
                                .active_interface
                                .as_ref()
                                .map(|i| i.ip)
                                .unwrap_or(state.bind_addr.ip())
                        };
                        if !ip.is_unspecified() {
                            tracing::info!(
                                "discovery retry #{}: network ready at {}, starting mDNS",
                                attempt,
                                ip
                            );
                            let _ = retry_tx
                                .send(DiscoveryCommand::Restart {
                                    bind_ip: ip,
                                    port: retry_shared.config.port,
                                })
                                .await;
                            break;
                        }
                        tracing::debug!(
                            "discovery retry #{}: network still not ready, waiting...",
                            attempt
                        );
                    }
                });
            } else {
                let _ = discovery_tx
                    .send(DiscoveryCommand::Restart {
                        bind_ip: discovery_ip,
                        port: engine.shared.config.port,
                    })
                    .await;
            }
        }

        // Embedded hosts (Windows FFI, Android JNI) never loaded the saved
        // settings, so every launch ran on defaults - "Share clipboard" came
        // back on after each restart, and the next save overwrote the file
        // with those defaults.
        if let Ok(store) = SettingsStore::load(engine.settings_path()) {
            engine.apply_settings(store.get().clone()).await;
        }

        engine.spawn_network_monitor().await?;
        engine.spawn_peer_pruner();
        engine.spawn_call_lease_watch();
        engine.spawn_sensitive_history_pruner();
        engine.spawn_auto_reconnector();
        engine.spawn_health_monitor();

        // Spawn UDP broadcast beacon and listener for resilient discovery.
        // Off with discovery: test engines beaconed onto the real LAN, and
        // every running app listed them as phantom "device-xxxxxxxx" peers.
        if engine.shared.config.enable_discovery {
            engine.spawn_udp_beacon();
            engine.spawn_udp_listener();
        }

        Ok(engine)
    }

    pub async fn start_speed_test(&self, device_id: Uuid, duration_secs: u32) -> Result<()> {
        let test_id = Uuid::new_v4();
        if let Some(tx) = self.shared.peer_manager.file_sender(device_id) {
            {
                let mut tests = self.shared.speed_tests.lock().await;
                let entry = tests
                    .entry(device_id)
                    .or_insert_with(|| crate::speed_test::SpeedTestState::new(tx.clone()));
                // We'll mark it as Idle but store the test_id and duration so the response matches
                entry.test_id = Some(test_id);
                entry.duration_secs = duration_secs;
            }
            let req = AppMessage::SpeedTestRequest {
                test_id,
                duration_secs,
            };
            tx.send(req)
                .await
                .map_err(|_| anyhow::anyhow!("Failed to send speed test request"))?;
            Ok(())
        } else {
            anyhow::bail!("Peer not connected");
        }
    }

    // ── Feedback ──────────────────────────────────────────────────────────────

    pub fn set_pairing_requested(&self, device_id: Uuid, requested: bool) -> Result<()> {
        let _ = self
            .shared
            .peer_manager
            .set_pairing_requested(device_id, requested)?;
        Ok(())
    }

    pub fn set_outgoing_pairing_waiting(&self, device_id: Uuid, waiting: bool) -> Result<()> {
        let _ = self
            .shared
            .peer_manager
            .set_outgoing_pairing_waiting(device_id, waiting)?;
        Ok(())
    }

    pub async fn feedback_recent(&self, n: usize) -> Vec<crate::engine_support::FeedbackEvent> {
        self.shared.feedback.lock().await.recent(n)
    }

    /// Trigger a fresh mDNS browse query and restart the advertisement.
    /// Called by the Mac "Scan" button — surfaces peers that came online
    /// since the last browse.
    /// The host saw the network change (Wi-Fi to Ethernet, another band,
    /// a hotspot): re-read the interfaces now. A change rebinds the
    /// listener, restarts discovery and reconnects peers at once instead of
    /// at the next poll, which is 15 s on Android.
    pub fn refresh_network(&self) {
        if let Some(hint) = self.shared.network_hint.get() {
            let _ = hint.try_send(());
        }
    }

    pub async fn rescan_peers(&self) {
        if let Some(tx) = &self.shared.discovery_tx {
            let state = self.shared.network_state.lock().await;
            let bind_ip = state
                .active_interface
                .as_ref()
                .map(|i| i.ip)
                .unwrap_or(state.bind_addr.ip());
            let _ = tx
                .send(DiscoveryCommand::Restart {
                    bind_ip,
                    port: self.shared.config.port,
                })
                .await;
        }
    }

    /// Re-push a received clipboard item (by content hash) to connected peers.
    /// Used when the user taps "Send" on a feed row on the Mac.
    pub async fn repush_clipboard_hash(&self, hash: String, target: SyncTarget) -> Result<()> {
        // Look up the text from the clipboard store by hash.
        let text = self
            .shared
            .clipboard_store
            .lock()
            .await
            .get_text_by_hash(&hash)
            .context("clipboard item not found by hash")?;
        self.push_clipboard_to(ClipboardContent::Text(text), target)
            .await;
        Ok(())
    }

    /// Returns this engine's stable device UUID.
    /// Used by the Android JNI bridge to filter out self-connections during NSD.
    pub fn device_id(&self) -> Uuid {
        self.shared.config.device_id
    }

    pub fn local_device_id(&self) -> Uuid {
        self.shared.config.device_id
    }

    pub async fn bound_port(&self) -> u16 {
        loop {
            let port = self.shared.network_state.lock().await.bind_addr.port();
            if port != 0 {
                return port;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    pub fn local_device_name(&self) -> String {
        self.shared.config.device_name.clone()
    }

    /// Returns this device's Noise public-key fingerprint as a lowercase hex string.
    /// Displayed in the Mac Security pane and Android pairing screen for manual verification.
    pub fn local_fingerprint(&self) -> String {
        self.shared
            .identity_key
            .read()
            .unwrap()
            .public_bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":")
    }

    /// Atomically update sync-filter flags from a live settings change.
    /// The router checks `settings` on every clipboard event — no restart needed.
    pub async fn apply_sync_settings(
        &self,
        sync_enabled: bool,
        sync_text: bool,
        sync_images: bool,
        sync_files: bool,
    ) {
        let was_on = {
            let mut settings = self.shared.settings.lock().unwrap();
            let was_on = settings.sync_enabled;
            settings.sync_enabled = sync_enabled;
            settings.sync_text = sync_text;
            settings.sync_images = sync_images;
            settings.sync_files = sync_files;
            was_on
        };
        // Android's Sync switch arrives here, not via set_sync_enabled, so
        // peers were never told it changed.
        if was_on != sync_enabled {
            self.announce_sync_state(sync_enabled).await;
        }
        tracing::info!(
            sync_enabled,
            sync_text,
            sync_images,
            sync_files,
            "sync settings updated live"
        );
    }

    /// Called by the Android JNI layer when the OS reports network restored
    /// (e.g., Wi-Fi comes back after Doze). Immediately attempts to reconnect
    /// to all known trusted peers.
    pub async fn reconnect_all_peers(&self) {
        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        self.shared
            .local_last_wake
            .store(now_millis, std::sync::atomic::Ordering::Relaxed);

        reconnect_known_peers(self.shared.clone()).await;
    }

    /// Broadcast sleep state to all connected peers.
    pub async fn notify_sleep_state(&self, is_asleep: bool) {
        self.shared
            .local_sleeping
            .store(is_asleep, std::sync::atomic::Ordering::Relaxed);
        if !is_asleep {
            let now_millis = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            self.shared
                .local_last_wake
                .store(now_millis, std::sync::atomic::Ordering::Relaxed);
        }
        let msg = AppMessage::DeviceSleepState { is_asleep };
        let peers = self.shared.peer_manager.all_trusted_senders();
        for (peer_id, tx) in peers {
            if self.is_trusted(peer_id).await {
                let _ = tx.send(msg.clone()).await;
            }
        }
    }

    pub async fn send_message(&self, target_device: Uuid, msg: AppMessage) -> Result<()> {
        let peers = self.shared.peer_manager.all_connected_senders();
        if let Some(tx) = peers
            .into_iter()
            .find(|(id, _)| *id == target_device)
            .map(|(_, tx)| tx)
        {
            let _ = tx.send(msg).await;
            Ok(())
        } else {
            anyhow::bail!("Peer not connected")
        }
    }

    /// Returns the number of currently connected peers.
    pub fn connected_peer_count(&self) -> usize {
        self.shared.peer_manager.connected_count()
    }

    pub async fn status_snapshot(&self) -> EngineStatus {
        let state = self.shared.network_state.lock().await.clone();
        let trust_lookup: HashMap<Uuid, TrustRecord> = self
            .shared
            .trust
            .lock()
            .await
            .all_devices()
            .map(|rec| (rec.device_id, rec.clone()))
            .collect();
        EngineStatus {
            active_interface: state.active_interface,
            bind_address: state.bind_addr,
            peers: display_peers_for_status(
                self.shared.peer_manager.list(),
                self.shared.config.device_id,
                &self.shared.config.device_name,
                state.bind_addr.ip(),
                &trust_lookup,
            ),
            last_sync_at: self.shared.peer_manager.last_sync_at(),
        }
    }

    pub async fn active_transfers(&self) -> Vec<serde_json::Value> {
        let mut transfers = self.shared.file_transfers.lock().await.active_transfers();
        // Outbound rows name the device receiving the file, so apps can say who it goes to.
        for t in &mut transfers {
            let name = t["to_device_id"]
                .as_str()
                .and_then(|id| id.parse().ok())
                .and_then(|id| self.shared.peer_manager.get(id))
                .map(|peer| peer.friendly_name);
            if let (Some(name), Some(obj)) = (name, t.as_object_mut()) {
                obj.insert("from_device".into(), name.into());
            }
        }
        transfers
    }

    pub async fn active_speed_tests(&self) -> Vec<serde_json::Value> {
        let tests = self.shared.speed_tests.lock().await;
        tests.iter().map(|(peer_id, s)| {
            serde_json::json!({
                "test_id": s.test_id.map(|u| u.to_string()),
                "peer_id": peer_id.to_string(),
                "phase": match s.phase {
                    crate::speed_test::SpeedTestPhase::Idle => "Idle",
                    crate::speed_test::SpeedTestPhase::Sending => "Sending",
                    crate::speed_test::SpeedTestPhase::Receiving => "Receiving",
                },
                "bytes_transferred": s.bytes_transferred.load(std::sync::atomic::Ordering::Relaxed),
                "duration_secs": s.duration_secs,
            })
        }).collect()
    }
}

fn stable_device_id(public_key: [u8; 32]) -> Uuid {
    let digest = Sha256::digest(public_key);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {:?}", parent))?;
    }
    Ok(())
}
