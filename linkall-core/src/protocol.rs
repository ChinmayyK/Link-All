//! Link All Protocol — wire format definitions
//!
//! All messages are length-prefixed (u32 LE) + postcard-encoded.
//! After the handshake, every frame is AEAD-encrypted with a
//! per-session AES-256-GCM key derived via X25519 ECDH + HKDF.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024; // 4 MB
pub const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024; // 32 MB
pub const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024 * 1024; // 1 TB (chunked / file-backed)

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ClipboardContent {
    Text(String),
    Image {
        mime: String,
        #[serde(skip)]
        data: Vec<u8>,
    },
    /// File payload — delivered as clipboard and also saved to Downloads/LinkAll.
    File {
        name: String,
        #[serde(skip)]
        data: Vec<u8>,
    },
}

impl ClipboardContent {
    pub fn byte_len(&self) -> usize {
        match self {
            ClipboardContent::Text(s) => s.len(),
            ClipboardContent::Image { data, .. } => data.len(),
            ClipboardContent::File { data, .. } => data.len(),
        }
    }

    /// Fix 16: Returns true if the content carries no actual data.
    ///
    /// Previously every call site re-implemented this guard:
    /// ```ignore
    /// if matches!(&content, ClipboardContent::Text(s) if s.is_empty()) { ... }
    /// ```
    /// Now the engine can do a single `if content.is_empty() { return; }` check
    /// before broadcasting, preventing empty clipboard events from propagating.
    pub fn is_empty(&self) -> bool {
        match self {
            ClipboardContent::Text(s) => s.is_empty(),
            ClipboardContent::Image { data, .. } => data.is_empty(),
            ClipboardContent::File { data, .. } => data.is_empty(),
        }
    }

    pub fn kind_str(&self) -> &'static str {
        match self {
            ClipboardContent::Text(_) => "text",
            ClipboardContent::Image { .. } => "image",
            ClipboardContent::File { .. } => "file",
        }
    }

    /// Convenience wrapper: `truncated_preview(80)`.
    /// Used by Linux notifications and Windows balloon tips.
    pub fn preview_string(&self) -> String {
        self.truncated_preview(80)
    }

    /// A short human-readable preview suitable for notifications and timeline entries.
    ///
    /// Text is truncated to `max_chars` with an ellipsis; images and files show
    /// their type and size. The preview is always a single line.
    pub fn truncated_preview(&self, max_chars: usize) -> String {
        match self {
            ClipboardContent::Text(s) => {
                // Collapse to first non-empty line, then truncate.
                let first = s
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .unwrap_or("(empty)");
                if first.len() <= max_chars {
                    first.to_string()
                } else {
                    // Truncate at a char boundary.
                    let mut end = max_chars.saturating_sub(1);
                    while end > 0 && !first.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}…", &first[..end])
                }
            }
            ClipboardContent::Image { mime, data } => {
                let kb = data.len() as f64 / 1024.0;
                if kb >= 1024.0 {
                    format!("[Image {} {:.1} MB]", mime, kb / 1024.0)
                } else {
                    format!("[Image {} {:.0} KB]", mime, kb)
                }
            }
            ClipboardContent::File { name, data } => {
                let kb = data.len() as f64 / 1024.0;
                if kb >= 1024.0 {
                    format!("[File '{}' {:.1} MB]", name, kb / 1024.0)
                } else {
                    format!("[File '{}' {:.0} KB]", name, kb)
                }
            }
        }
    }

    /// Word count for text content; 0 for images and files.
    pub fn word_count(&self) -> usize {
        match self {
            ClipboardContent::Text(s) => s.split_whitespace().count(),
            _ => 0,
        }
    }

    /// Line count for text content; 0 for images and files.
    pub fn line_count(&self) -> usize {
        match self {
            ClipboardContent::Text(s) => s.lines().count(),
            _ => 0,
        }
    }
}

// ── History metadata ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryMetadata {
    pub hash: String,
    pub timestamp: u64,
    /// Human-readable device name — NEVER a raw UUID.
    /// Example: "Chinmay's Pixel 8", "MacBook Pro"
    pub source_device: String,
    pub kind: String,
    pub bytes: u64,
    pub pinned: bool,
}

impl HistoryMetadata {
    pub fn from_content(content: &ClipboardContent, source_device: String, pinned: bool) -> Self {
        let hash = hex::encode(crate::dedup::hash_content(content));
        let (kind, bytes) = match content {
            ClipboardContent::Text(text) => ("text".to_string(), text.len() as u64),
            ClipboardContent::Image { data, .. } => ("image".to_string(), data.len() as u64),
            ClipboardContent::File { data, .. } => ("file".to_string(), data.len() as u64),
        };
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            hash,
            timestamp,
            source_device,
            kind,
            bytes,
            pinned,
        }
    }

    /// Human-readable summary shown in timeline.
    /// Uses friendly device name, NOT internal ID.
    pub fn summary(&self) -> String {
        match self.kind.as_str() {
            "text" => format!("[{}] copied text", self.source_device),
            "image" => format!("[{}] copied image", self.source_device),
            "file" => format!("[{}] received file", self.source_device),
            _ => format!("[{}] clipboard item", self.source_device),
        }
    }
}

// ── Device metadata (exchanged during handshake / discovery) ─────────────────

/// Sent in the Hello `device_name` extension slot so peers know
/// the platform and can format notifications like "Copied from Chinmay's Pixel 8".
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceMetadata {
    /// User-visible name: "Chinmay's Pixel 8", "MacBook Pro", "DESKTOP-ABC123"
    pub device_name: String,
    /// "Android", "macOS", "Windows", "Linux"
    pub platform: String,
    /// OS version string (best-effort)
    pub platform_version: String,
    /// Link All app version
    pub app_version: String,
    /// Indicates if the initiator is manually trying to reconnect.
    #[serde(default)]
    pub is_manual_reconnect: Option<bool>,
}

// ── File transfer metadata ────────────────────────────────────────────────────

/// Announced before a chunked file transfer begins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTransferMetadata {
    pub transfer_id: [u8; 16],
    pub file_name: String,
    pub size_bytes: u64,
    pub mime_type: String,
    #[serde(default)]
    pub is_directory: bool,
    #[serde(default)]
    pub item_count: u32,
    #[serde(default)]
    pub batch_id: Option<String>,
}

// ── Remote File & Media Explorer definitions ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RemoteFileCategory {
    All,
    Images,
    Videos,
    Audio,
    Documents,
    Apks,
    Archives,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RemoteFileSource {
    All,
    WhatsApp,
    Downloads,
    Camera,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteFileEntry {
    pub file_id: u64,         // MediaStore _ID
    pub display_name: String, // _DISPLAY_NAME
    pub size_bytes: u64,      // _SIZE
    pub mime_type: String,    // MIME_TYPE
    pub date_modified: u64,   // DATE_MODIFIED (epoch seconds)
    pub category: RemoteFileCategory,
    pub source: RemoteFileSource,
    pub content_uri: String, // Content URI (e.g. "content://media/external/file/1234")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteFileCategoryCounts {
    pub images: u32,
    pub videos: u32,
    pub audio: u32,
    pub documents: u32,
    pub apks: u32,
    pub archives: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteFileSourceCounts {
    pub whatsapp: u32,
    pub downloads: u32,
    pub camera: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteFilesSummary {
    pub type_counts: RemoteFileCategoryCounts,
    pub source_counts: RemoteFileSourceCounts,
}

// ── Wire messages ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcdhFrame {
    pub version: u16,
    pub ecdh_pubkey: [u8; 32],
    pub nonce: [u8; 16],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppMessage {
    Hello {
        device_id: Uuid,
        device_name: String,
        identity_pubkey: [u8; 32],
        identity_proof: [u8; 32],
        metadata_json: Option<String>,
    },
    HelloAck {
        device_id: Uuid,
        device_name: String,
        identity_pubkey: [u8; 32],
        nonce_response: [u8; 16],
        identity_proof: [u8; 32],
        trusted: bool,
        metadata_json: Option<String>,
    },
    /// Broadcast when a device generates a new identity key.
    KeyRotated {
        new_pubkey_bytes: [u8; 32],
    },
    QrAuth {
        token: String,
    },
    ClipboardPush {
        seq: u64,
        content: std::sync::Arc<ClipboardContent>,
        origin_device: Uuid,
        /// Friendly name of the originating device for UI display.
        /// Never a raw UUID.
        origin_device_name: String,
        /// Relay path for mesh tracing: list of device names that forwarded this.
        #[serde(default)]
        relay_path: Vec<String>,
    },
    HistoryMetadata {
        entry: HistoryMetadata,
    },
    DeviceSleepState {
        is_asleep: bool,
    },
    DeviceSyncState {
        enabled: bool,
    },
    ClipboardAck {
        seq: u64,
    },
    /// A request to trust this device for future auto-connections.
    PairingRequest {
        origin_device: Uuid,
        origin_device_name: String,
        #[serde(default)]
        pin: Option<String>,
    },
    /// Response to a pairing request (accepted or declined).
    PairingResponse {
        origin_device: Uuid,
        accepted: bool,
    },
    /// Announce a file transfer before sending chunks (dedicated pipeline).
    FileTransferAnnounce {
        meta: FileTransferMetadata,
    },
    /// Receiver accepts, rejects, or resumes a transfer.
    FileTransferAccept {
        transfer_id: [u8; 16],
        accepted: bool,
        /// Resume from this chunk index (0 = fresh start).
        #[serde(default)]
        resume_from_chunk: u32,
        reject_reason: Option<String>,
    },
    /// One chunk of file data (dedicated file transfer channel).
    FileChunk {
        transfer_id: [u8; 16],
        chunk_index: u32,
        total_chunks: u32,
        #[serde(skip)]
        data: Vec<u8>,
        #[serde(default)]
        compressed: bool,
    },
    /// Periodic acknowledgement from receiver → sender.
    FileChunkAck {
        transfer_id: [u8; 16],
        last_confirmed_chunk: u32,
    },
    /// Sender signals all chunks sent; receiver should verify.
    FileTransferComplete {
        transfer_id: [u8; 16],
        sha256_checksum: String,
    },
    /// Receiver confirms finalization (success or error).
    FileTransferCompleteAck {
        transfer_id: [u8; 16],
        success: bool,
        error: Option<String>,
    },
    /// Either side cancels a transfer in progress.
    FileTransferCancel {
        transfer_id: [u8; 16],
        reason: String,
    },
    /// Either side pauses a transfer in progress.
    FileTransferPause {
        transfer_id: [u8; 16],
    },
    FileTransferResume {
        transfer_id: [u8; 16],
    },

    // ── Speed Test Messages ──────────────────────────────────────────────────
    /// Request to begin a speed test.
    SpeedTestRequest {
        test_id: Uuid,
        duration_secs: u32,
    },
    /// Response to a speed test request.
    SpeedTestResponse {
        test_id: Uuid,
        accepted: bool,
        reason: Option<String>,
    },
    /// A chunk of raw dummy data for the speed test.
    SpeedTestData {
        test_id: Uuid,
        seq: u32,
        #[serde(skip)]
        data: Vec<u8>,
    },
    /// Periodic stats sent from receiver back to the sender.
    SpeedTestStats {
        test_id: Uuid,
        received_bytes: u64,
    },
    /// Finalize the speed test (or transition to upload phase).
    SpeedTestComplete {
        test_id: Uuid,
    },
    /// Phone call state propagated from an Android device to connected peers.
    /// Enables call continuity: ringing/offhook/idle states are relayed so
    /// macOS (or other peers) can show an incoming-call banner and trigger
    /// remote accept/decline actions.
    CallStateUpdate {
        /// "ringing", "offhook", "idle"
        state: String,
        /// Phone number (may be empty if blocked/unknown)
        number: String,
        /// Contact name resolved on Android (empty if not in contacts)
        contact_name: String,
        /// Device that originated this event
        origin_device: Uuid,
        origin_device_name: String,
    },
    /// Remote call action request (accept/decline) sent from a peer
    /// back to the Android device that reported a ringing call.
    CallAction {
        /// "accept" or "decline"
        action: String,
        origin_device: Uuid,
    },
    /// Battery status from a connected device. Pushed periodically
    /// (every 5 min or on ≥5% change) for passive display on peers.
    BatteryStatus {
        /// Battery level 0–100
        level: u8,
        /// Whether the device is currently charging
        charging: bool,
        origin_device: Uuid,
        origin_device_name: String,
    },
    NetworkStatus {
        /// "wifi", "cellular", or "offline"
        network_type: String,
        origin_device: Uuid,
        origin_device_name: String,
    },
    /// Storage stats from a connected device. Pushed periodically.
    StorageStatus {
        images_bytes: u64,
        videos_bytes: u64,
        apps_bytes: u64,
        free_bytes: u64,
        total_bytes: u64,
        origin_device: Uuid,
        origin_device_name: String,
    },
    /// Relay a push notification from an Android device to connected peers.
    NotificationRelay {
        /// Unique ID or tag for this notification
        id: String,
        /// Package name of the app that posted the notification
        package: String,
        /// Notification title (e.g. sender name)
        title: String,
        /// Notification text/body
        text: String,
        /// Device that originated this event
        origin_device: Uuid,
        origin_device_name: String,
    },
    /// A generic permission error relayed from a peer to display a UI warning.
    PermissionError {
        feature: String,
        message: String,
        origin_device: Uuid,
        origin_device_name: String,
    },
    Ping {
        timestamp_ms: u64,
    },
    Pong {
        timestamp_ms: u64,
    },
    /// Request to start a virtual camera stream.
    CameraStreamRequest {
        origin_device: Uuid,
    },
    /// Accept a virtual camera stream request.
    CameraStreamAccept {
        origin_device: Uuid,
        accepted: bool,
    },
    /// Stop a virtual camera stream.
    CameraStreamStop {
        origin_device: Uuid,
    },
    /// A single encoded video frame (NAL unit) for the virtual camera.
    CameraFrame {
        origin_device: Uuid,
        data: Vec<u8>,
    },
    /// Desktop -> Android: Query summary counts (`Type` + `Sources`) or paginated file lists.
    RemoteFilesQuery {
        request_id: Uuid,
        origin_device: Uuid,
        summary_only: bool,
        category: Option<RemoteFileCategory>,
        source: Option<RemoteFileSource>,
        search_query: Option<String>,
        offset: u32,
        limit: u32,
    },
    /// Android -> Desktop: Response containing summary counts or paginated file entries.
    RemoteFilesResponse {
        request_id: Uuid,
        summary: Option<RemoteFilesSummary>,
        files: Vec<RemoteFileEntry>,
        total_matching: u32,
        error: Option<String>,
    },
    /// Desktop -> Android: Request thumbnail bytes for a MediaStore file ID.
    RemoteThumbnailRequest {
        request_id: Uuid,
        origin_device: Uuid,
        file_id: u64,
        size_px: u32,
    },
    /// Android -> Desktop: Response containing compressed thumbnail bytes (JPEG).
    RemoteThumbnailResponse {
        request_id: Uuid,
        file_id: u64,
        #[serde(skip)]
        data: Vec<u8>,
        error: Option<String>,
    },
    /// Desktop -> Android: Request to pull/download a full file.
    RemoteFilePullRequest {
        request_id: Uuid,
        origin_device: Uuid,
        file_id: u64,
    },
    /// Desktop -> Android: Request to perform an action on a remote file.
    RemoteFileActionRequest {
        action: String, // "delete", "rename"
        file_id: u64,
        new_name: Option<String>,
    },
    /// Cross-device link handoff: ask a trusted, connected peer to open a
    /// URL immediately. Receiver restricts this to http/https schemes.
    OpenUrlOnDevice {
        url: String,
        origin_device: Uuid,
        origin_device_name: String,
    },
    /// Delivery feedback for `OpenUrlOnDevice`, sent back to the origin.
    OpenUrlOnDeviceAck {
        success: bool,
        error: Option<String>,
    },
    Bye,
}

impl AppMessage {
    pub fn take_raw_payload(&mut self) -> Option<Vec<u8>> {
        match self {
            AppMessage::FileChunk { data, .. } => Some(std::mem::take(data)),
            AppMessage::ClipboardPush { content, .. } => {
                if let Some(c) = std::sync::Arc::get_mut(content) {
                    match c {
                        ClipboardContent::Image { data, .. } => Some(std::mem::take(data)),
                        ClipboardContent::File { data, .. } => Some(std::mem::take(data)),
                        _ => None,
                    }
                } else {
                    // Fallback to explicitly cloning the inner data if we can't get mut
                    match &**content {
                        ClipboardContent::Image { data, .. } => Some(data.clone()),
                        ClipboardContent::File { data, .. } => Some(data.clone()),
                        _ => None,
                    }
                }
            }
            AppMessage::SpeedTestData { data, .. } => Some(std::mem::take(data)),
            AppMessage::RemoteThumbnailResponse { data, .. } => Some(std::mem::take(data)),
            _ => None,
        }
    }

    pub fn expects_raw_payload(&self) -> bool {
        match self {
            AppMessage::FileChunk { .. } => true,
            AppMessage::ClipboardPush { content, .. } => matches!(
                **content,
                ClipboardContent::Image { .. } | ClipboardContent::File { .. }
            ),
            AppMessage::SpeedTestData { .. } => true,
            AppMessage::RemoteThumbnailResponse { .. } => true,
            _ => false,
        }
    }

    pub fn set_raw_payload(&mut self, payload: Vec<u8>) {
        match self {
            AppMessage::FileChunk { data, .. } => *data = payload,
            AppMessage::ClipboardPush { content, .. } => {
                if let Some(c) = std::sync::Arc::get_mut(content) {
                    match c {
                        ClipboardContent::Image { data, .. } => *data = payload,
                        ClipboardContent::File { data, .. } => *data = payload,
                        _ => {}
                    }
                }
            }
            AppMessage::SpeedTestData { data, .. } => *data = payload,
            AppMessage::RemoteThumbnailResponse { data, .. } => *data = payload,
            _ => {}
        }
    }
}

// ── mDNS / defaults ──────────────────────────────────────────────────────────

// Wire protocol names (this service type, the UDP beacon and connect-back
// prefixes, and the HKDF/HMAC labels in crypto, identity, pairing and
// lan_probe) keep the app's original name, Deskdrop. They are protocol
// identifiers, never shown to users; changing them would stop this version
// from finding, pairing with or talking to devices on older builds.
pub const MDNS_SERVICE_TYPE: &str = "_deskdrop._tcp.local.";
// Bumped 4 -> 5: session key derivation now produces two directional keys
// (see crypto.rs derive_session_key) instead of one shared key, fixing an
// AES-GCM nonce-reuse vulnerability. This is a deliberate breaking change —
// old and new builds must not silently interoperate with mismatched key
// derivation, so a version bump forces the existing handshake version
// check to reject the pairing cleanly instead.
pub const PROTOCOL_VERSION: u16 = 5;
pub const DEFAULT_PORT: u16 = 47823;
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Fix 16: ClipboardContent::is_empty ───────────────────────────────────

    #[test]
    fn empty_text_is_empty() {
        assert!(ClipboardContent::Text(String::new()).is_empty());
    }

    #[test]
    fn nonempty_text_is_not_empty() {
        assert!(!ClipboardContent::Text("hello".into()).is_empty());
    }

    #[test]
    fn empty_image_is_empty() {
        let c = ClipboardContent::Image {
            mime: "image/png".into(),
            data: vec![],
        };
        assert!(c.is_empty());
    }

    #[test]
    fn nonempty_image_is_not_empty() {
        let c = ClipboardContent::Image {
            mime: "image/png".into(),
            data: vec![0xFF; 8],
        };
        assert!(!c.is_empty());
    }

    #[test]
    fn empty_file_is_empty() {
        let c = ClipboardContent::File {
            name: "doc.pdf".into(),
            data: vec![],
        };
        assert!(c.is_empty());
    }

    #[test]
    fn nonempty_file_is_not_empty() {
        let c = ClipboardContent::File {
            name: "doc.pdf".into(),
            data: vec![1, 2, 3],
        };
        assert!(!c.is_empty());
    }

    #[test]
    fn is_empty_consistent_with_byte_len() {
        let items: Vec<ClipboardContent> = vec![
            ClipboardContent::Text(String::new()),
            ClipboardContent::Text("x".into()),
            ClipboardContent::Image {
                mime: "image/png".into(),
                data: vec![],
            },
            ClipboardContent::Image {
                mime: "image/png".into(),
                data: vec![0],
            },
        ];
        for item in &items {
            assert_eq!(item.is_empty(), item.byte_len() == 0);
        }
    }

    #[test]
    fn optional_wire_fields_round_trip_through_bincode() {
        let hello = EcdhFrame {
            version: PROTOCOL_VERSION,
            ecdh_pubkey: [2u8; 32],
            nonce: [3u8; 16],
        };
        let ack = AppMessage::HelloAck {
            device_id: Uuid::nil(),
            device_name: "PeerB".into(),
            identity_pubkey: [4u8; 32],
            nonce_response: [6u8; 16],
            identity_proof: [5u8; 32],
            trusted: false,
            metadata_json: None,
        };
        let file_ack = AppMessage::FileTransferCompleteAck {
            transfer_id: [7u8; 16],
            success: true,
            error: None,
        };

        let _decoded_hello: EcdhFrame =
            postcard::from_bytes(&postcard::to_stdvec(&hello).unwrap()).unwrap();
        let decoded_ack: AppMessage =
            postcard::from_bytes(&postcard::to_stdvec(&ack).unwrap()).unwrap();
        let decoded_file_ack: AppMessage =
            postcard::from_bytes(&postcard::to_stdvec(&file_ack).unwrap()).unwrap();

        match decoded_ack {
            AppMessage::HelloAck { metadata_json, .. } => assert!(metadata_json.is_none()),
            _ => panic!("Expected HelloAck"),
        }
        match decoded_file_ack {
            AppMessage::FileTransferCompleteAck {
                transfer_id,
                success,
                error,
            } => {
                assert_eq!(transfer_id, [7u8; 16]);
                assert!(success);
                assert!(error.is_none());
            }
            other => panic!("unexpected decoded message: {other:?}"),
        }
    }
}
