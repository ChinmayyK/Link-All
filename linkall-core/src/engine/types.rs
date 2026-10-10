//! Public engine types: events, config, status snapshots, per-peer
//! telemetry, and the shared state every engine task holds.

use super::*;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum DeliveryStatus {
    Queued,
    Sent,
    Delivered,
    Applied,
    Failed(String),
}

#[derive(Debug)]
pub enum EngineEvent {
    /// A remote clipboard item arrived and was added to the activity feed.
    /// If `auto_applied` is true it was also written to the local clipboard.
    ClipboardReceived {
        from_device: Uuid,
        from_name: String,
        content: std::sync::Arc<ClipboardContent>,
        /// True when the engine auto-applied it to the local clipboard.
        /// False when timeline-first mode is active (user must apply manually).
        auto_applied: bool,
        /// Relay path that brought this item here.
        relay_path: Vec<String>,
        /// Activity feed entry ID for this event.
        activity_id: u64,
    },
    HistoryMetadataReceived {
        from_device: Uuid,
        from_name: String,
        entry: HistoryMetadata,
    },
    ClipboardSynced {
        peer_device: Uuid,
        peer_name: String,
        seq: u64,
    },
    ClipboardSyncFailed {
        peer_device: Uuid,
        peer_name: String,
        seq: u64,
        reason: String,
    },
    PairingRequested {
        device_id: Uuid,
        device_name: String,
        pin: String,
    },
    OutgoingPairingWaiting {
        device_id: Uuid,
        device_name: String,
        pin: String,
    },
    PairingConfirmed {
        device_id: Uuid,
    },
    PairingRejected {
        device_id: Uuid,
    },
    /// A pairing request with this peer started, ended or changed state
    /// without any other event saying so (expired, withdrawn, cancelled).
    /// Event-driven UIs re-read the peer snapshot.
    PairingChanged {
        device_id: Uuid,
    },
    /// An untrusted peer was discovered on the network (useful for UI lists).
    PeerDiscovered {
        device_id: Uuid,
        device_name: String,
        platform: Option<String>,
    },
    /// The list from `Engine::health` changed.
    SystemHealthUpdated(Vec<HealthIssue>),
    ClipboardDeliveryStatus {
        activity_id: u64,
        status: DeliveryStatus,
    },
    PeerConnected {
        device_id: Uuid,
        device_name: String,
        addr: SocketAddr,
        trusted: bool,
    },
    PeerDisconnected {
        device_id: Uuid,
        device_name: Option<String>,
        reason: Option<String>,
    },
    PeerSyncStateChanged {
        device_id: Uuid,
        enabled: bool,
    },
    /// A remote device wants to send a file — UI should prompt user to accept.
    FileTransferIncoming {
        transfer_id: [u8; 16],
        from_device: Uuid,
        from_name: String,
        file_name: String,
        file_bytes: u64,
        mime_type: String,
    },
    /// File transfer progress update.
    FileTransferProgress {
        transfer_id: [u8; 16],
        from_device: Uuid,
        file_name: String,
        percent: u8,
        bytes_received: u64,
        total_bytes: u64,
        speed_bps: Option<u64>,
        eta_secs: Option<u64>,
        /// True when this device is the sender. Frontends cannot infer direction
        /// from other events: transfers from trusted peers are auto-accepted
        /// without a `FileTransferIncoming` event.
        outbound: bool,
    },
    /// File transfer completed and is ready at `dest_path`.
    FileTransferComplete {
        transfer_id: [u8; 16],
        from_device: Uuid,
        from_name: String,
        file_name: String,
        dest_path: PathBuf,
    },
    /// File transfer failed or was cancelled.
    FileTransferFailed {
        transfer_id: [u8; 16],
        from_device: Uuid,
        reason: String,
        /// One file of a folder transfer. Set on the way to the host; see
        /// `FolderTransferComplete`.
        in_folder: bool,
    },
    /// Every file of a folder transfer has finished, in either direction.
    /// The per-file events still arrive; hosts show this one instead of a
    /// notification per file (see `is_folder_item`).
    FolderTransferComplete {
        batch_id: String,
        peer_id: Uuid,
        peer_name: String,
        folder_name: String,
        file_count: u32,
        failed_count: u32,
        /// True when this device sent the folder.
        outbound: bool,
        /// The folder on this device; `None` when we were the sender.
        dest_dir: Option<PathBuf>,
    },
    /// File transfer was paused.
    FileTransferPaused {
        transfer_id: [u8; 16],
    },
    /// File transfer was resumed.
    FileTransferResumed {
        transfer_id: [u8; 16],
    },

    // ── Speed Test Events ────────────────────────────────────────────────────
    SpeedTestProgress {
        test_id: Uuid,
        peer_id: Uuid,
        direction: String, // "upload" or "download"
        bytes_transferred: u64,
        duration_secs: u32,
    },
    SpeedTestComplete {
        test_id: Uuid,
        peer_id: Uuid,
    },
    /// Activity feed snapshot (full or incremental). Used to update the UI.
    ActivityFeedUpdated {
        entries: Vec<crate::activity::ActivityEntry>,
    },
    /// A connected Android device reported a phone call state change.
    /// Used by macOS to show an incoming-call banner; by Android to update UI.
    CallStateChanged {
        from_device: Uuid,
        from_name: String,
        /// "ringing", "offhook", "idle"
        state: String,
        number: String,
        contact_name: String,
    },
    /// A remote peer requested a call action (accept/decline).
    /// Consumed by the Android JNI layer to invoke TelecomManager APIs.
    CallActionRequest {
        action: String,
        from_device: Uuid,
    },
    /// A connected peer device reported a battery status change (F20).
    BatteryStateChanged {
        from_device: Uuid,
        from_name: String,
        level: u8,
        charging: bool,
    },
    /// A connected peer device reported a network status change.
    NetworkStateChanged {
        from_device: Uuid,
        from_name: String,
        network_type: String,
    },
    /// A connected Android device relayed a push notification.
    NotificationReceived {
        id: String,
        package: String,
        title: String,
        text: String,
        from_device: Uuid,
        from_name: String,
    },
    RemoteFilesQueryReceived {
        request_id: Uuid,
        from_device: Uuid,
        summary_only: bool,
        category: Option<crate::protocol::RemoteFileCategory>,
        source: Option<crate::protocol::RemoteFileSource>,
        search_query: Option<String>,
        offset: u32,
        limit: u32,
    },
    RemoteFilesResponseReceived {
        request_id: Uuid,
        from_device: Uuid,
        summary: Option<crate::protocol::RemoteFilesSummary>,
        files: Vec<crate::protocol::RemoteFileEntry>,
        total_matching: u32,
        error: Option<String>,
    },
    RemoteThumbnailRequestReceived {
        request_id: Uuid,
        from_device: Uuid,
        file_id: u64,
        size_px: u32,
    },
    RemoteThumbnailResponseReceived {
        request_id: Uuid,
        from_device: Uuid,
        file_id: u64,
        data: Vec<u8>,
        error: Option<String>,
    },
    RemoteFilePullRequestReceived {
        request_id: Uuid,
        from_device: Uuid,
        file_id: u64,
    },
    RemoteFileActionRequestReceived {
        from_device: Uuid,
        action: String,
        file_id: u64,
        new_name: Option<String>,
    },
    /// A trusted, connected peer asked us to open a URL — act on it
    /// immediately (auto-open, no local confirmation gate).
    OpenUrlOnDeviceRequested {
        from_device: Uuid,
        from_name: String,
        url: String,
    },
    /// Delivery feedback for a URL-open request we sent to another device.
    OpenUrlOnDeviceAckReceived {
        from_device: Uuid,
        success: bool,
        error: Option<String>,
    },
    /// An untrusted peer has requested to pair with this device.
    PairingRequest {
        device_id: Uuid,
        device_name: String,
    },
    /// A peer responded to our pairing request.
    PairingResponse {
        device_id: Uuid,
        accepted: bool,
    },
    Warning(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncTarget {
    All,
    Device(Uuid),
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncDispatchPeer {
    pub device_id: Uuid,
    pub device_name: String,
    pub delivered: bool,
    pub metadata_only: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncDispatchReport {
    pub seq: u64,
    pub target: SyncTarget,
    pub peers: Vec<SyncDispatchPeer>,
}

impl SyncDispatchReport {
    pub fn delivered_count(&self) -> usize {
        self.peers
            .iter()
            .filter(|peer| peer.delivered && !peer.metadata_only)
            .count()
    }
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub device_id: Uuid,
    pub device_name: String,
    pub port: u16,
    pub trust_store_path: PathBuf,
    pub peer_store_path: PathBuf,
    pub identity_path: PathBuf,
    /// Where settings.json is read and written. Embedded hosts point it into
    /// their own data folder; tests point it at a temp file.
    pub settings_path: PathBuf,
    pub connect_timeout: Duration,
    pub heartbeat_interval: Duration,
    pub heartbeat_timeout: Duration,
    pub bind_ip: Option<IpAddr>,
    pub enable_discovery: bool,
    pub network_poll_interval: Duration,
    /// Root directory for daemon-managed data files (history, feedback, etc.).
    pub data_dir: PathBuf,
    /// Optional override for dedicated file transfer saves.
    pub file_save_dir: Option<PathBuf>,
    /// Maximum number of history entries to keep in memory and on disk.
    pub history_limit: Option<usize>,
}

pub fn default_device_name() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("scutil")
            .args(["--get", "ComputerName"])
            .output()
        {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !name.is_empty() {
                return name;
            }
        }
    }
    whoami::devicename()
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            device_id: Uuid::nil(),
            device_name: default_device_name(),
            port: DEFAULT_PORT,
            trust_store_path: default_trust_store_path(),
            peer_store_path: default_peer_store_path(),
            identity_path: IdentityStore::default_path(),
            settings_path: default_settings_path(),
            connect_timeout: Duration::from_secs(3),
            heartbeat_interval: Duration::from_secs(5),
            heartbeat_timeout: Duration::from_secs(12),
            bind_ip: None,
            enable_discovery: true,
            // Android has no netlink hint path (see network_manager) and the
            // app already reports connectivity changes through
            // notifyNetworkRestored, so a 1 s interface poll only kept the
            // phone's CPU ticking.
            network_poll_interval: if cfg!(target_os = "android") {
                Duration::from_secs(15)
            } else {
                Duration::from_secs(1)
            },
            data_dir: default_peer_store_path()
                .parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".")),
            file_save_dir: None,
            history_limit: Some(500),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EngineStatus {
    pub active_interface: Option<NetworkInterfaceInfo>,
    pub bind_address: SocketAddr,
    pub peers: Vec<PeerRecord>,
    pub last_sync_at: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeNetworkState {
    pub(crate) bind_addr: SocketAddr,
    pub(crate) active_interface: Option<NetworkInterfaceInfo>,
    /// Set while the listener is down after a failed rebind.
    pub(crate) listener_error: Option<String>,
}

#[derive(Debug)]
pub(crate) enum ListenerCommand {
    Rebind(SocketAddr),
}

#[derive(Debug)]
pub(crate) enum DiscoveryCommand {
    Restart { bind_ip: IpAddr, port: u16 },
}

/// Active phone call state tracked by the engine.
/// Updated when a connected Android device reports call state changes.
/// Exposed in the IPC status response so macOS can poll it.
#[derive(Debug, Clone, Serialize)]
pub struct ActiveCallState {
    pub device_id: Uuid,
    pub device_name: String,
    pub state: String,
    pub number: String,
    pub contact_name: String,
    /// When the phone last reported this call. A phone repeats a live call's
    /// state every [`CALL_REFRESH`]; a call not heard of for [`CALL_LEASE`]
    /// is over, whether or not its "idle" ever arrived.
    #[serde(skip)]
    pub(crate) heard_at: std::time::Instant,
}

/// How often a phone repeats the state of a call that is still going.
pub const CALL_REFRESH: Duration = Duration::from_secs(10);
/// How long a call stays shown without being heard of again.
pub const CALL_LEASE: Duration = Duration::from_secs(35);

impl ActiveCallState {
    pub(crate) fn expired(&self) -> bool {
        self.heard_at.elapsed() > CALL_LEASE
    }
}

/// Battery level from a connected peer device (F20).
/// Updated when a BatteryStatus message is received.
#[derive(Debug, Clone, Serialize)]
pub struct PeerBatteryState {
    pub device_id: Uuid,
    pub device_name: String,
    pub level: u8,
    pub charging: bool,
}

/// Network connection state from a connected peer device.
#[derive(Debug, Clone, Serialize)]
pub struct PeerNetworkState {
    pub device_id: Uuid,
    pub device_name: String,
    pub network_type: String,
}

/// Storage state from a connected peer device.
#[derive(Debug, Clone, Serialize)]
pub struct PeerStorageState {
    pub device_id: Uuid,
    pub device_name: String,
    pub images_bytes: u64,
    pub videos_bytes: u64,
    pub apps_bytes: u64,
    pub free_bytes: u64,
    pub total_bytes: u64,
}

/// Result of a remote files query (`query_remote_files_sync`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RemoteFilesResult {
    pub summary: Option<crate::protocol::RemoteFilesSummary>,
    pub files: Vec<crate::protocol::RemoteFileEntry>,
    pub total_matching: u32,
    pub error: Option<String>,
}

/// Result of a remote thumbnail request (`request_remote_thumbnail_sync`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RemoteThumbnailResult {
    pub file_id: u64,
    pub data: Vec<u8>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct QrAuthToken {
    pub token: String,
    pub expires_at: std::time::Instant,
}

#[derive(Clone)]
pub(crate) struct EngineShared {
    pub(crate) config: EngineConfig,
    pub(crate) trust: Arc<Mutex<TrustStore>>,
    pub(crate) peer_manager: Arc<PeerManager>,
    pub(crate) event_tx: mpsc::Sender<EngineEvent>,
    pub(crate) identity_key: Arc<std::sync::RwLock<crate::identity::IdentityKey>>,
    pub(crate) network_state: Arc<Mutex<RuntimeNetworkState>>,
    pub(crate) listener_tx: mpsc::Sender<ListenerCommand>,
    pub(crate) discovery_tx: Option<mpsc::Sender<DiscoveryCommand>>,
    pub(crate) network_reconcile: Arc<Mutex<()>>,
    // ── New: mesh-aware shared state ─────────────────────────────────────────
    /// Mesh fanout router + relay dedup (shared, lock-protected).
    pub(crate) mesh_router: Arc<Mutex<MeshRouter>>,
    /// Cross-device activity feed.
    pub(crate) activity: Arc<Mutex<ActivityFeed>>,
    /// File transfer manager.
    pub(crate) file_transfers: Arc<Mutex<FileTransferManager>>,
    /// Per-folder tallies for folder transfers.
    pub(crate) folders: Arc<std::sync::Mutex<super::folder_ops::FolderTallies>>,
    /// Speed tests manager.
    pub(crate) speed_tests:
        Arc<Mutex<std::collections::HashMap<uuid::Uuid, crate::speed_test::SpeedTestState>>>,
    /// Clipboard apply policy (timeline-first vs auto-apply).
    pub(crate) apply_policy: Arc<Mutex<ClipboardApplyPolicy>>,
    /// Settings snapshot for policy decisions (updated lazily).
    pub(crate) settings: Arc<std::sync::Mutex<Settings>>,
    /// Per-peer link-quality probes — drives adaptive chunk sizing (HIGH-03).
    /// Keyed by peer device UUID; populated on first Pong receipt.
    pub(crate) quality_probes: Arc<dashmap::DashMap<uuid::Uuid, QualityProbe>>,
    /// Clipboard content store — maps content hash → text payload for repush.
    pub(crate) clipboard_store: Arc<Mutex<crate::engine_support::ClipboardStore>>,
    /// Local clipboard reader (platform abstraction for push_current_clipboard).
    pub(crate) local_clipboard: Arc<Mutex<crate::engine_support::LocalClipboard>>,
    /// Persistent history store.
    pub(crate) history: Arc<Mutex<crate::history::History>>,
    /// In-memory feedback event log (most-recent N events).
    pub(crate) feedback: Arc<Mutex<crate::engine_support::FeedbackLog>>,
    /// Tracks when the local device woke up from sleep. Prevents immediate disconnects when waking from deep sleep.
    pub(crate) local_last_wake: Arc<std::sync::atomic::AtomicU64>,
    /// Set while the host says this device is asleep (phone screen off).
    /// Heartbeats and reconnect attempts slow down so the radio can idle.
    pub(crate) local_sleeping: Arc<std::sync::atomic::AtomicBool>,
    /// Battery, storage, network and call state, ours and each peer's.
    pub(crate) device_status: DeviceStatus,
    /// Rate limit for pairing UI spam from untrusted peers.

    /// Cross-device duplicate prevention (mesh echo suppression).
    pub dedup: Arc<Mutex<crate::dedup::Deduplicator>>,
    /// Active QR authentication token (short-lived)
    pub qr_auth_token: Arc<Mutex<Option<QrAuthToken>>>,
    /// Callers blocked on a remote files or thumbnail reply.
    pub(crate) remote_waiters: RemoteWaiters,
    pub(crate) started_at: Instant,
    /// Asks the network monitor to re-check now (see `Engine::refresh_network`).
    pub(crate) network_hint: Arc<std::sync::OnceLock<mpsc::Sender<()>>>,
}

/// Status reported between devices. The local half caches what this device
/// last reported so it can be pushed to peers as they connect; the peer half
/// holds what each peer last reported to us, keyed by device UUID.
#[derive(Clone, Default)]
pub(crate) struct DeviceStatus {
    /// Active phone call state (set on ringing/offhook, cleared on idle).
    pub(crate) active_call: Arc<Mutex<Option<ActiveCallState>>>,
    /// Per-peer battery levels (F20).
    pub(crate) peer_batteries: Arc<dashmap::DashMap<uuid::Uuid, PeerBatteryState>>,
    pub(crate) peer_storage: Arc<dashmap::DashMap<uuid::Uuid, PeerStorageState>>,
    pub(crate) peer_networks: Arc<dashmap::DashMap<uuid::Uuid, PeerNetworkState>>,
    pub(crate) local_battery: Arc<std::sync::Mutex<Option<(u8, bool)>>>,
    #[allow(clippy::type_complexity)]
    pub(crate) local_storage: Arc<std::sync::Mutex<Option<(u64, u64, u64, u64, u64)>>>,
    pub(crate) local_network: Arc<std::sync::Mutex<Option<String>>>,
}

/// A pending synchronous remote request: the peer it was sent to, and where
/// to deliver the reply.
pub(crate) type RemoteWaiter<T> = (uuid::Uuid, oneshot::Sender<T>);

/// Callers of `query_remote_files_sync` / `request_remote_thumbnail_sync`
/// waiting for a reply, keyed by `request_id`.
#[derive(Clone, Default)]
pub(crate) struct RemoteWaiters {
    pub(crate) files: Arc<Mutex<HashMap<uuid::Uuid, RemoteWaiter<RemoteFilesResult>>>>,
    pub(crate) thumbnails: Arc<Mutex<HashMap<uuid::Uuid, RemoteWaiter<RemoteThumbnailResult>>>>,
}
