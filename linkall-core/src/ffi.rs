//! C-compatible FFI layer.
//!
//! Platform wrappers load the shared library and call these functions.
//! All heap-allocated strings/bytes are freed via the corresponding
//! `linkall_free_*` functions — never call the system free() on them.
//!
//! Thread safety: all functions are safe to call from any thread.
//! The engine uses Tokio internally; we create a dedicated runtime here.

#![allow(clippy::missing_safety_doc)]

use crate::engine::{Engine, EngineConfig, EngineEvent, SyncTarget};
use crate::protocol::ClipboardContent;
use serde_json::json;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::sync::OnceLock;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

// ── Tokio runtime (singleton) ─────────────────────────────────────────────────

static RT: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RT.get_or_init(|| Runtime::new().expect("Tokio runtime"))
}

// ── Engine handle ─────────────────────────────────────────────────────────────

pub struct LinkAllHandle {
    engine: Engine,
    event_rx: std::sync::Mutex<Option<mpsc::Receiver<EngineEvent>>>,
}

/// Allocate and start the engine. Returns NULL on failure.
///
/// # Parameters
/// - `device_name`: UTF-8 C string; use NULL for auto-detected hostname.
/// - `port`: 0 → use default port (47823).
#[no_mangle]
pub unsafe extern "C" fn linkall_start(
    device_name: *const c_char,
    port: u16,
) -> *mut LinkAllHandle {
    let name = if device_name.is_null() {
        EngineConfig::default().device_name
    } else {
        unsafe { CStr::from_ptr(device_name) }
            .to_string_lossy()
            .into_owned()
    };

    let port = if port == 0 {
        crate::protocol::DEFAULT_PORT
    } else {
        port
    };

    let config = EngineConfig {
        device_name: name,
        port,
        ..EngineConfig::default()
    };

    let (engine_event_tx, mut engine_event_rx) = mpsc::channel(256);
    let (event_tx, event_rx) = mpsc::channel(256);

    match runtime().block_on(Engine::start(config, engine_event_tx)) {
        Ok(engine) => {
            // Desktop hosts answer Remote File Explorer requests in Rust (see
            // local_files); every event is still forwarded to the host app.
            let serving_engine = engine.clone();
            runtime().spawn(async move {
                while let Some(ev) = engine_event_rx.recv().await {
                    #[cfg(not(target_os = "android"))]
                    crate::local_files::handle_event(&serving_engine, &ev);
                    #[cfg(target_os = "android")]
                    let _ = &serving_engine;
                    if event_tx.send(ev).await.is_err() {
                        break;
                    }
                }
            });
            #[cfg(windows)]
            {
                let e_clone = std::sync::Arc::new(engine.clone());
                runtime().spawn(async move {
                    let handler = std::sync::Arc::new(move |req: crate::ipc::IpcRequest| {
                        let eng = e_clone.clone();
                        async move { crate::ipc::handle_ipc_request(eng, req).await }
                    });
                    if let Err(e) = crate::ipc_windows::spawn_windows_ipc(handler).await {
                        eprintln!("Windows IPC server failed to start: {}", e);
                    }
                });
            }
            Box::into_raw(Box::new(LinkAllHandle {
                engine,
                event_rx: std::sync::Mutex::new(Some(event_rx)),
            }))
        }
        Err(e) => {
            eprintln!("linkall_start error: {:#}", e);
            std::ptr::null_mut()
        }
    }
}

/// Stop and free the engine.
///
/// # Safety
/// `handle` must be a pointer returned by `linkall_start` and must not be
/// used again after this call.
#[no_mangle]
pub unsafe extern "C" fn linkall_stop(handle: *mut LinkAllHandle) {
    if !handle.is_null() {
        drop(Box::from_raw(handle));
    }
}

// ── Push clipboard ────────────────────────────────────────────────────────────

/// Push UTF-8 text to all peers. Returns number of peers reached.
///
/// # Safety
/// `text` must be a valid, non-null UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn linkall_push_text(
    handle: *mut LinkAllHandle,
    text: *const c_char,
) -> c_int {
    if handle.is_null() || text.is_null() {
        return -1;
    }
    let s = CStr::from_ptr(text).to_string_lossy().into_owned();
    let h = &*handle;
    runtime().block_on(h.engine.push_clipboard(ClipboardContent::Text(s))) as c_int
}

/// Push raw image bytes to all peers.
///
/// # Safety
/// `data` must point to `len` valid bytes; `mime` must be a valid C string.
#[no_mangle]
pub unsafe extern "C" fn linkall_push_image(
    handle: *mut LinkAllHandle,
    mime: *const c_char,
    data: *const u8,
    len: usize,
) -> c_int {
    if handle.is_null() || mime.is_null() || data.is_null() {
        return -1;
    }
    let mime = CStr::from_ptr(mime).to_string_lossy().into_owned();
    let bytes = std::slice::from_raw_parts(data, len).to_vec();
    let h = &*handle;
    runtime().block_on(
        h.engine
            .push_clipboard(ClipboardContent::Image { mime, data: bytes }),
    ) as c_int
}

/// Push raw image bytes to one peer.
///
/// # Safety
/// `target` and `mime` must be valid C strings; `data` must point to `len`
/// valid bytes.
#[no_mangle]
pub unsafe extern "C" fn linkall_push_image_to(
    handle: *mut LinkAllHandle,
    target: *const c_char,
    mime: *const c_char,
    data: *const u8,
    len: usize,
) -> c_int {
    if handle.is_null() || target.is_null() || mime.is_null() || data.is_null() {
        return -1;
    }
    let Ok(id) = uuid::Uuid::parse_str(&CStr::from_ptr(target).to_string_lossy()) else {
        return -1;
    };
    let mime = CStr::from_ptr(mime).to_string_lossy().into_owned();
    let bytes = std::slice::from_raw_parts(data, len).to_vec();
    let h = &*handle;
    let report = runtime().block_on(h.engine.push_clipboard_to(
        ClipboardContent::Image { mime, data: bytes },
        SyncTarget::Device(id),
    ));
    report.delivered_count() as c_int
}

/// Push a file to all peers.
///
/// # Safety
/// `name` must be a valid C string; `data` must point to `len` valid bytes.
#[no_mangle]
pub unsafe extern "C" fn linkall_push_file(
    handle: *mut LinkAllHandle,
    name: *const c_char,
    data: *const u8,
    len: usize,
) -> c_int {
    if handle.is_null() || name.is_null() || data.is_null() {
        return -1;
    }
    let name = CStr::from_ptr(name).to_string_lossy().into_owned();
    let bytes = std::slice::from_raw_parts(data, len).to_vec();
    let h = &*handle;
    runtime().block_on(
        h.engine
            .push_clipboard(ClipboardContent::File { name, data: bytes }),
    ) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn linkall_send_file_path(
    handle: *mut LinkAllHandle,
    target_device_ptr: *const c_char,
    path_ptr: *const c_char,
    file_name_ptr: *const c_char,
    mime_type_ptr: *const c_char,
) -> c_int {
    if handle.is_null() || path_ptr.is_null() || file_name_ptr.is_null() || mime_type_ptr.is_null()
    {
        return -1;
    }

    let target_device = if !target_device_ptr.is_null() {
        let s = CStr::from_ptr(target_device_ptr).to_string_lossy();
        if s.is_empty() {
            None
        } else {
            uuid::Uuid::parse_str(&s).ok()
        }
    } else {
        None
    };

    let path = std::path::PathBuf::from(CStr::from_ptr(path_ptr).to_string_lossy().into_owned());
    let file_name = CStr::from_ptr(file_name_ptr).to_string_lossy().into_owned();
    let mime_type = CStr::from_ptr(mime_type_ptr).to_string_lossy().into_owned();

    let h = &*handle;
    let res = runtime().block_on(h.engine.send_file_path(
        path,
        file_name,
        mime_type,
        target_device,
        None,
        false,
        1,
    ));
    if res.is_ok() {
        0
    } else {
        -1
    }
}

// ── Poll for events ───────────────────────────────────────────────────────────

/// Event type codes returned by `linkall_poll_event`.
pub const PB_EVENT_NONE: c_int = 0;
pub const PB_EVENT_CLIPBOARD_TEXT: c_int = 1;
pub const PB_EVENT_CLIPBOARD_IMAGE: c_int = 2;
pub const PB_EVENT_CLIPBOARD_FILE: c_int = 3;
pub const PB_EVENT_PAIRING_REQUESTED: c_int = 4;
pub const PB_EVENT_PEER_CONNECTED: c_int = 5;
pub const PB_EVENT_PEER_DISCONNECTED: c_int = 6;
pub const PB_EVENT_WARNING: c_int = 7;
pub const PB_EVENT_CLIPBOARD_SYNCED: c_int = 8;
pub const PB_EVENT_CLIPBOARD_AVAILABLE: c_int = 11; // timeline-first: not yet applied
pub const PB_EVENT_FILE_TRANSFER_INCOMING: c_int = 12;
pub const PB_EVENT_FILE_TRANSFER_PROGRESS: c_int = 13;
pub const PB_EVENT_FILE_TRANSFER_COMPLETE: c_int = 14;
pub const PB_EVENT_FILE_TRANSFER_FAILED: c_int = 15;
pub const PB_EVENT_ACTIVITY_UPDATED: c_int = 16;
pub const PB_EVENT_CALL_STATE_CHANGED: c_int = 17;
pub const PB_EVENT_CALL_ACTION: c_int = 18;
pub const PB_EVENT_BATTERY_STATE_CHANGED: c_int = 19;
pub const PB_EVENT_FILE_TRANSFER_PAUSED: c_int = 20;
pub const PB_EVENT_FILE_TRANSFER_RESUMED: c_int = 21;
pub const PB_EVENT_CAMERA_STREAM_REQUEST: c_int = 22;
pub const PB_EVENT_CAMERA_STREAM_ACCEPT: c_int = 23;
pub const PB_EVENT_CAMERA_STREAM_STOP: c_int = 24;
pub const PB_EVENT_CAMERA_FRAME: c_int = 25;
pub const PB_EVENT_SYSTEM_HEALTH_UPDATED: c_int = 26;
pub const PB_EVENT_PEER_DISCOVERED: c_int = 27;
pub const PB_EVENT_NETWORK_STATE_CHANGED: c_int = 28;
pub const PB_EVENT_OUTGOING_PAIRING_WAITING: c_int = 29;
pub const PB_EVENT_REMOTE_FILES_QUERY: c_int = 30;
pub const PB_EVENT_REMOTE_THUMBNAIL_REQUEST: c_int = 31;
pub const PB_EVENT_REMOTE_FILE_PULL_REQUEST: c_int = 32;
pub const PB_EVENT_REMOTE_FILES_RESPONSE: c_int = 33;
pub const PB_EVENT_REMOTE_THUMBNAIL_RESPONSE: c_int = 34;
pub const PB_EVENT_SPEED_TEST_PROGRESS: c_int = 35;
pub const PB_EVENT_SPEED_TEST_COMPLETE: c_int = 36;
pub const PB_EVENT_REMOTE_FILE_ACTION_REQUEST: c_int = 37;
pub const PB_EVENT_OPEN_URL_ON_DEVICE_REQUESTED: c_int = 38;
pub const PB_EVENT_OPEN_URL_ON_DEVICE_ACK: c_int = 39;
/// A notification mirrored from a phone: title via
/// `linkall_event_notification_title`, body via `linkall_event_text`.
pub const PB_EVENT_NOTIFICATION_RECEIVED: c_int = 40;
/// Every file of a folder transfer finished: folder name via
/// `linkall_event_transfer_file_name`, folder path (receiver only) via
/// `linkall_event_transfer_dest_path`, counts via
/// `linkall_event_folder_file_count` / `linkall_event_folder_failed_count`.
pub const PB_EVENT_FOLDER_TRANSFER_COMPLETE: c_int = 41;

/// Opaque event payload. Call `linkall_event_*` accessors to read fields.
/// Must be freed with `linkall_free_event`.
pub struct PbEvent {
    inner: EngineEvent,
    // Cached C-string allocations for accessors so multiple calls never invalidate earlier pointers.
    cached_strings: Vec<CString>,
}

impl PbEvent {
    fn cache_str(&mut self, s: impl Into<Vec<u8>>) -> *const c_char {
        let bytes = s.into();
        if let Some(existing) = self
            .cached_strings
            .iter()
            .find(|cs| cs.as_bytes() == bytes.as_slice())
        {
            return existing.as_ptr();
        }
        let cs = CString::new(bytes).unwrap_or_default();
        let ptr = cs.as_ptr();
        self.cached_strings.push(cs);
        ptr
    }
}

/// Non-blocking poll. Returns a heap-allocated `PbEvent*` or NULL if no event.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn linkall_poll_event(handle: *mut LinkAllHandle) -> *mut PbEvent {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    let h = &*handle;
    let mut lock = h.event_rx.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(rx) = lock.as_mut() {
        match rx.try_recv() {
            Ok(event) => Box::into_raw(Box::new(PbEvent {
                inner: event,
                cached_strings: Vec::new(),
            })),
            Err(_) => std::ptr::null_mut(),
        }
    } else {
        std::ptr::null_mut()
    }
}

/// Returns the event type code for `event`.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_type(event: *const PbEvent) -> c_int {
    if event.is_null() {
        return PB_EVENT_NONE;
    }
    match &(*event).inner {
        EngineEvent::ClipboardReceived {
            content,
            auto_applied,
            ..
        } => {
            if *auto_applied {
                match &**content {
                    ClipboardContent::Text(_) => PB_EVENT_CLIPBOARD_TEXT,
                    ClipboardContent::Image { .. } => PB_EVENT_CLIPBOARD_IMAGE,
                    ClipboardContent::File { .. } => PB_EVENT_CLIPBOARD_FILE,
                }
            } else {
                // Timeline-first: available but not auto-applied.
                PB_EVENT_CLIPBOARD_AVAILABLE
            }
        }
        EngineEvent::HistoryMetadataReceived { .. } => PB_EVENT_WARNING,
        EngineEvent::SystemHealthUpdated(_) => PB_EVENT_SYSTEM_HEALTH_UPDATED,
        EngineEvent::ClipboardDeliveryStatus { .. } => PB_EVENT_WARNING,
        EngineEvent::PairingRequested { .. } => PB_EVENT_PAIRING_REQUESTED,
        EngineEvent::PairingConfirmed { .. } => PB_EVENT_WARNING,
        EngineEvent::PairingRejected { .. } => PB_EVENT_WARNING,
        EngineEvent::PairingRequest { .. } => PB_EVENT_WARNING,
        EngineEvent::PairingResponse { .. } => PB_EVENT_WARNING,
        EngineEvent::ClipboardSynced { .. } => PB_EVENT_CLIPBOARD_SYNCED,
        EngineEvent::ClipboardSyncFailed { .. } => PB_EVENT_WARNING,
        EngineEvent::PeerConnected { .. } => PB_EVENT_PEER_CONNECTED,
        EngineEvent::PeerDisconnected { .. } => PB_EVENT_PEER_DISCONNECTED,
        EngineEvent::FileTransferIncoming { .. } => PB_EVENT_FILE_TRANSFER_INCOMING,
        EngineEvent::FileTransferProgress { .. } => PB_EVENT_FILE_TRANSFER_PROGRESS,
        EngineEvent::FileTransferComplete { .. } => PB_EVENT_FILE_TRANSFER_COMPLETE,
        EngineEvent::FileTransferFailed { .. } => PB_EVENT_FILE_TRANSFER_FAILED,
        EngineEvent::FolderTransferComplete { .. } => PB_EVENT_FOLDER_TRANSFER_COMPLETE,
        EngineEvent::FileTransferPaused { .. } => PB_EVENT_FILE_TRANSFER_PAUSED,
        EngineEvent::FileTransferResumed { .. } => PB_EVENT_FILE_TRANSFER_RESUMED,
        EngineEvent::ActivityFeedUpdated { .. } => PB_EVENT_ACTIVITY_UPDATED,
        EngineEvent::CallStateChanged { .. } => PB_EVENT_CALL_STATE_CHANGED,
        EngineEvent::CallActionRequest { .. } => PB_EVENT_CALL_ACTION,
        EngineEvent::BatteryStateChanged { .. } => PB_EVENT_BATTERY_STATE_CHANGED,
        EngineEvent::NetworkStateChanged { .. } => PB_EVENT_NETWORK_STATE_CHANGED,
        EngineEvent::NotificationReceived { .. } => PB_EVENT_NOTIFICATION_RECEIVED,
        EngineEvent::CameraStreamRequest { .. } => PB_EVENT_CAMERA_STREAM_REQUEST,
        EngineEvent::CameraStreamAccept { .. } => PB_EVENT_CAMERA_STREAM_ACCEPT,
        EngineEvent::CameraStreamStop { .. } => PB_EVENT_CAMERA_STREAM_STOP,
        EngineEvent::CameraFrameReceived { .. } => PB_EVENT_CAMERA_FRAME,
        EngineEvent::PeerDiscovered { .. } => PB_EVENT_PEER_DISCOVERED,
        // A refresh hint: re-read the peer list.
        EngineEvent::PairingChanged { .. } => PB_EVENT_PEER_DISCOVERED,
        EngineEvent::OutgoingPairingWaiting { .. } => PB_EVENT_OUTGOING_PAIRING_WAITING,
        EngineEvent::RemoteFilesQueryReceived { .. } => PB_EVENT_REMOTE_FILES_QUERY,
        EngineEvent::RemoteThumbnailRequestReceived { .. } => PB_EVENT_REMOTE_THUMBNAIL_REQUEST,
        EngineEvent::RemoteFilePullRequestReceived { .. } => PB_EVENT_REMOTE_FILE_PULL_REQUEST,
        EngineEvent::RemoteFileActionRequestReceived { .. } => PB_EVENT_REMOTE_FILE_ACTION_REQUEST,
        EngineEvent::OpenUrlOnDeviceRequested { .. } => PB_EVENT_OPEN_URL_ON_DEVICE_REQUESTED,
        EngineEvent::OpenUrlOnDeviceAckReceived { .. } => PB_EVENT_OPEN_URL_ON_DEVICE_ACK,
        EngineEvent::RemoteFilesResponseReceived { .. } => PB_EVENT_REMOTE_FILES_RESPONSE,
        EngineEvent::RemoteThumbnailResponseReceived { .. } => PB_EVENT_REMOTE_THUMBNAIL_RESPONSE,
        EngineEvent::SpeedTestProgress { .. } => PB_EVENT_SPEED_TEST_PROGRESS,
        EngineEvent::SpeedTestComplete { .. } => PB_EVENT_SPEED_TEST_COMPLETE,
        EngineEvent::Warning(_) => PB_EVENT_WARNING,
        EngineEvent::PeerSyncStateChanged { .. } => PB_EVENT_SYSTEM_HEALTH_UPDATED,
    }
}

/// Get the text payload (for TEXT events). Lifetime: until `linkall_free_event`.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_text(event: *mut PbEvent) -> *const c_char {
    let e = &mut *event;

    let text: Option<String> = match &e.inner {
        EngineEvent::ClipboardReceived { content, .. } => {
            if let ClipboardContent::Text(ref s) = **content {
                Some(s.clone())
            } else {
                None
            }
        }
        EngineEvent::Warning(s) => Some(s.clone()),
        EngineEvent::CallStateChanged { state, .. } => Some(state.clone()),
        EngineEvent::NotificationReceived { text, .. } => Some(text.clone()),
        EngineEvent::NetworkStateChanged { network_type, .. } => Some(network_type.clone()),
        EngineEvent::ActivityFeedUpdated { entries, .. } => serde_json::to_string(entries).ok(),
        EngineEvent::RemoteFilesResponseReceived {
            summary,
            files,
            total_matching,
            error,
            ..
        } => serde_json::to_string(&json!({
            "summary": summary,
            "files": files,
            "total_matching": total_matching,
            "error": error,
        }))
        .ok(),
        EngineEvent::RemoteThumbnailResponseReceived {
            file_id,
            data,
            error,
            ..
        } => {
            use base64::Engine as _;
            let base64_str = base64::engine::general_purpose::STANDARD.encode(data);
            serde_json::to_string(&json!({
                "file_id": file_id,
                "data_base64": base64_str,
                "error": error,
            }))
            .ok()
        }
        EngineEvent::OpenUrlOnDeviceRequested { url, .. } => Some(url.clone()),
        _ => None,
    };

    if let Some(s) = text {
        return e.cache_str(s);
    }

    std::ptr::null()
}

/// Get the device name associated with the event.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_device_name(event: *mut PbEvent) -> *const c_char {
    let e = &mut *event;
    let name: Option<String> = match &e.inner {
        EngineEvent::ClipboardReceived { from_name, .. } => Some(from_name.clone()),
        EngineEvent::HistoryMetadataReceived { from_name, .. } => Some(from_name.clone()),
        EngineEvent::ClipboardSynced { peer_name, .. } => Some(peer_name.clone()),
        EngineEvent::ClipboardSyncFailed { peer_name, .. } => Some(peer_name.clone()),
        EngineEvent::PairingRequested { device_name, .. } => Some(device_name.clone()),
        EngineEvent::OutgoingPairingWaiting { device_name, .. } => Some(device_name.clone()),
        EngineEvent::PeerConnected { device_name, .. } => Some(device_name.clone()),
        EngineEvent::FileTransferIncoming { from_name, .. } => Some(from_name.clone()),
        EngineEvent::FileTransferComplete { from_name, .. } => Some(from_name.clone()),
        EngineEvent::BatteryStateChanged { from_name, .. } => Some(from_name.clone()),
        EngineEvent::NetworkStateChanged { from_name, .. } => Some(from_name.clone()),
        EngineEvent::OpenUrlOnDeviceRequested { from_name, .. } => Some(from_name.clone()),
        EngineEvent::CallStateChanged { from_name, .. } => Some(from_name.clone()),
        EngineEvent::NotificationReceived { from_name, .. } => Some(from_name.clone()),
        EngineEvent::FolderTransferComplete { peer_name, .. } => Some(peer_name.clone()),
        _ => None,
    };
    if let Some(n) = name {
        e.cache_str(n)
    } else {
        std::ptr::null()
    }
}

/// Returns 1 if this ClipboardReceived was auto-applied; 0 if timeline-first.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_auto_applied(event: *const PbEvent) -> c_int {
    if event.is_null() {
        return 0;
    }
    if let EngineEvent::ClipboardReceived { auto_applied, .. } = &(*event).inner {
        if *auto_applied {
            1
        } else {
            0
        }
    } else {
        0
    }
}

/// Returns the activity feed entry ID for a ClipboardReceived event (-1 if not applicable).
#[no_mangle]
pub unsafe extern "C" fn linkall_event_activity_id(event: *const PbEvent) -> i64 {
    if event.is_null() {
        return -1;
    }
    if let EngineEvent::ClipboardReceived { activity_id, .. } = &(*event).inner {
        *activity_id as i64
    } else {
        -1
    }
}

/// Get the transfer ID (hex string) for file transfer events.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_transfer_id(event: *mut PbEvent) -> *const c_char {
    if event.is_null() {
        return std::ptr::null();
    }
    let e = &mut *event;
    let tid = match &e.inner {
        EngineEvent::FileTransferIncoming { transfer_id, .. }
        | EngineEvent::FileTransferProgress { transfer_id, .. }
        | EngineEvent::FileTransferComplete { transfer_id, .. }
        | EngineEvent::FileTransferPaused { transfer_id, .. }
        | EngineEvent::FileTransferResumed { transfer_id, .. }
        | EngineEvent::FileTransferFailed { transfer_id, .. } => Some(hex::encode(transfer_id)),
        _ => None,
    };
    if let Some(s) = tid {
        e.cache_str(s)
    } else {
        std::ptr::null()
    }
}

/// Get file name for file transfer events.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_transfer_file_name(event: *mut PbEvent) -> *const c_char {
    if event.is_null() {
        return std::ptr::null();
    }
    let e = &mut *event;
    let name: Option<String> = match &e.inner {
        EngineEvent::FileTransferIncoming { file_name, .. } => Some(file_name.clone()),
        EngineEvent::FileTransferProgress { file_name, .. } => Some(file_name.clone()),
        EngineEvent::FileTransferComplete { file_name, .. } => Some(file_name.clone()),
        EngineEvent::FolderTransferComplete { folder_name, .. } => Some(folder_name.clone()),
        _ => None,
    };
    if let Some(n) = name {
        e.cache_str(n)
    } else {
        std::ptr::null()
    }
}

/// Get progress percentage (0-100) for FileTransferProgress events; -1 otherwise.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_transfer_percent(event: *const PbEvent) -> c_int {
    if event.is_null() {
        return -1;
    }
    if let EngineEvent::FileTransferProgress { percent, .. } = &(*event).inner {
        *percent as c_int
    } else {
        -1
    }
}

/// Get total bytes for FileTransferIncoming/Progress events; -1 otherwise.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_transfer_total_bytes(event: *const PbEvent) -> i64 {
    if event.is_null() {
        return -1;
    }
    match &(*event).inner {
        EngineEvent::FileTransferIncoming { file_bytes, .. } => *file_bytes as i64,
        EngineEvent::FileTransferProgress { total_bytes, .. } => *total_bytes as i64,
        _ => -1,
    }
}

/// Get bytes received for FileTransferProgress events; -1 otherwise.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_transfer_bytes_received(event: *const PbEvent) -> i64 {
    if event.is_null() {
        return -1;
    }
    if let EngineEvent::FileTransferProgress { bytes_received, .. } = &(*event).inner {
        *bytes_received as i64
    } else {
        -1
    }
}

/// Get the destination path for FileTransferComplete events.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_transfer_dest_path(event: *mut PbEvent) -> *const c_char {
    if event.is_null() {
        return std::ptr::null();
    }
    let e = &mut *event;
    let path = match &e.inner {
        EngineEvent::FileTransferComplete { dest_path, .. } => Some(dest_path.clone()),
        EngineEvent::FolderTransferComplete { dest_dir, .. } => {
            Some(dest_dir.clone().unwrap_or_default())
        }
        _ => None,
    };
    match path {
        Some(p) => e.cache_str(p.to_string_lossy().into_owned()),
        None => std::ptr::null(),
    }
}

/// Files in a FOLDER_TRANSFER_COMPLETE event's folder; 0 otherwise.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_folder_file_count(event: *const PbEvent) -> c_int {
    if event.is_null() {
        return 0;
    }
    match &(*event).inner {
        EngineEvent::FolderTransferComplete { file_count, .. } => *file_count as c_int,
        _ => 0,
    }
}

/// Files of a FOLDER_TRANSFER_COMPLETE event's folder that did not arrive.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_folder_failed_count(event: *const PbEvent) -> c_int {
    if event.is_null() {
        return 0;
    }
    match &(*event).inner {
        EngineEvent::FolderTransferComplete { failed_count, .. } => *failed_count as c_int,
        _ => 0,
    }
}

/// 1 when a file transfer event is for one file of a folder transfer.
/// Hosts skip their per-file notifications for these and show the folder's
/// FOLDER_TRANSFER_COMPLETE instead.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_transfer_in_folder(event: *const PbEvent) -> c_int {
    if event.is_null() {
        return 0;
    }
    let name = match &(*event).inner {
        EngineEvent::FileTransferIncoming { file_name, .. }
        | EngineEvent::FileTransferProgress { file_name, .. }
        | EngineEvent::FileTransferComplete { file_name, .. } => file_name.as_str(),
        EngineEvent::FileTransferFailed { in_folder, .. } => return *in_folder as c_int,
        _ => return 0,
    };
    crate::file_transfer::is_folder_item(name) as c_int
}

/// Get the fingerprint display string for TOFU_PROMPT events.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_fingerprint(event: *mut PbEvent) -> *const c_char {
    let e = &mut *event;
    let pin_str: Option<Vec<u8>> = match &e.inner {
        EngineEvent::PairingRequested { pin, .. } => Some(pin.as_bytes().to_vec()),
        EngineEvent::OutgoingPairingWaiting { pin, .. } => Some(pin.as_bytes().to_vec()),
        _ => None,
    };
    if let Some(bytes) = pin_str {
        e.cache_str(bytes)
    } else {
        std::ptr::null()
    }
}

/// Get the device ID string for TOFU_PROMPT and CALL_STATE_CHANGED events.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_device_id(event: *mut PbEvent) -> *const c_char {
    let e = &mut *event;
    let id_str = match &e.inner {
        EngineEvent::PairingRequested { device_id, .. } => Some(device_id.to_string()),
        EngineEvent::OutgoingPairingWaiting { device_id, .. } => Some(device_id.to_string()),
        EngineEvent::PairingChanged { device_id } => Some(device_id.to_string()),
        EngineEvent::CallStateChanged { from_device, .. } => Some(from_device.to_string()),
        EngineEvent::BatteryStateChanged { from_device, .. } => Some(from_device.to_string()),
        EngineEvent::NetworkStateChanged { from_device, .. } => Some(from_device.to_string()),
        EngineEvent::RemoteFilesQueryReceived { from_device, .. } => Some(from_device.to_string()),
        EngineEvent::RemoteThumbnailRequestReceived { from_device, .. } => {
            Some(from_device.to_string())
        }
        EngineEvent::RemoteFilePullRequestReceived { from_device, .. } => {
            Some(from_device.to_string())
        }
        EngineEvent::RemoteFilesResponseReceived { from_device, .. } => {
            Some(from_device.to_string())
        }
        EngineEvent::RemoteThumbnailResponseReceived { from_device, .. } => {
            Some(from_device.to_string())
        }
        EngineEvent::SpeedTestProgress { peer_id, .. } => Some(peer_id.to_string()),
        EngineEvent::SpeedTestComplete { peer_id, .. } => Some(peer_id.to_string()),
        EngineEvent::CameraStreamRequest { from_device, .. } => Some(from_device.to_string()),
        EngineEvent::CameraStreamAccept { from_device, .. } => Some(from_device.to_string()),
        EngineEvent::CameraStreamStop { from_device, .. } => Some(from_device.to_string()),
        EngineEvent::OpenUrlOnDeviceRequested { from_device, .. } => Some(from_device.to_string()),
        EngineEvent::FolderTransferComplete { peer_id, .. } => Some(peer_id.to_string()),
        EngineEvent::OpenUrlOnDeviceAckReceived { from_device, .. } => {
            Some(from_device.to_string())
        }
        _ => None,
    };
    if let Some(s) = id_str {
        e.cache_str(s)
    } else {
        std::ptr::null()
    }
}

/// Get bytes transferred for SpeedTestProgress events; -1 otherwise.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_speed_test_bytes(event: *const PbEvent) -> i64 {
    if event.is_null() {
        return -1;
    }
    if let EngineEvent::SpeedTestProgress {
        bytes_transferred, ..
    } = &(*event).inner
    {
        *bytes_transferred as i64
    } else {
        -1
    }
}

/// Get duration for SpeedTestProgress events; -1 otherwise.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_speed_test_duration(event: *const PbEvent) -> c_int {
    if event.is_null() {
        return -1;
    }
    if let EngineEvent::SpeedTestProgress { duration_secs, .. } = &(*event).inner {
        *duration_secs as c_int
    } else {
        -1
    }
}

/// Get phase for SpeedTestProgress events.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_speed_test_phase(event: *mut PbEvent) -> *const c_char {
    if event.is_null() {
        return std::ptr::null();
    }
    let e = &mut *event;
    let phase_str: Option<String> = match &e.inner {
        EngineEvent::SpeedTestProgress { direction, .. } => Some(direction.as_str().to_string()),
        _ => None,
    };
    if let Some(s) = phase_str {
        e.cache_str(s)
    } else {
        std::ptr::null()
    }
}

/// Apply a remote clipboard item by its content hash. Returns 1 on success.
/// The Swift layer calls this when the user clicks "Apply" in the timeline view.
///
/// # Safety
/// `handle` and `hash_ptr` must be valid.
#[no_mangle]
pub unsafe extern "C" fn linkall_apply_clipboard(
    handle: *mut LinkAllHandle,
    hash_ptr: *const c_char,
) -> c_int {
    if handle.is_null() || hash_ptr.is_null() {
        return 0;
    }
    let hash = match std::ffi::CStr::from_ptr(hash_ptr).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return 0,
    };
    let h = &*handle;
    match runtime().block_on(h.engine.apply_clipboard_by_hash(hash)) {
        Ok(true) => 1,
        _ => 0,
    }
}

/// Accept an incoming file transfer. Returns 1 on success.
///
/// # Safety
/// `handle` and `transfer_id_hex` must be valid.
#[no_mangle]
pub unsafe extern "C" fn linkall_accept_file_transfer(
    handle: *mut LinkAllHandle,
    transfer_id_hex: *const c_char,
) -> c_int {
    if handle.is_null() || transfer_id_hex.is_null() {
        return 0;
    }
    let hex_str = match std::ffi::CStr::from_ptr(transfer_id_hex).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return 0,
    };
    let Ok(bytes) = hex::decode(&hex_str) else {
        return 0;
    };
    let Ok(tid): Result<[u8; 16], _> = bytes.try_into() else {
        return 0;
    };
    let h = &*handle;
    match runtime().block_on(h.engine.accept_file_transfer(tid)) {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

/// Reject an incoming file transfer.
///
/// # Safety
#[no_mangle]
pub unsafe extern "C" fn linkall_reject_file_transfer(
    handle: *mut LinkAllHandle,
    transfer_id_hex: *const c_char,
) -> c_int {
    if handle.is_null() || transfer_id_hex.is_null() {
        return 0;
    }
    let hex_str = match std::ffi::CStr::from_ptr(transfer_id_hex).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return 0,
    };
    let Ok(bytes) = hex::decode(&hex_str) else {
        return 0;
    };
    let Ok(tid): Result<[u8; 16], _> = bytes.try_into() else {
        return 0;
    };
    let h = &*handle;
    match runtime().block_on(h.engine.reject_file_transfer(tid, "user rejected".into())) {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

/// Cancel an active file transfer.
///
/// # Safety
#[no_mangle]
pub unsafe extern "C" fn linkall_cancel_file_transfer(
    handle: *mut LinkAllHandle,
    transfer_id_hex: *const c_char,
) -> c_int {
    if handle.is_null() || transfer_id_hex.is_null() {
        return 0;
    }
    let tid_str = match CStr::from_ptr(transfer_id_hex).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let tid = if let Ok(parsed) = crate::ipc::parse_transfer_id(tid_str) {
        parsed
    } else {
        return 0;
    };
    let h = &*handle;
    match runtime().block_on(h.engine.cancel_file_transfer(tid)) {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

/// Pause an active file transfer.
///
/// # Safety
#[no_mangle]
pub unsafe extern "C" fn linkall_pause_file_transfer(
    handle: *mut LinkAllHandle,
    transfer_id_hex: *const c_char,
) -> c_int {
    if handle.is_null() || transfer_id_hex.is_null() {
        return 0;
    }
    let tid_str = match CStr::from_ptr(transfer_id_hex).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let tid = if let Ok(parsed) = crate::ipc::parse_transfer_id(tid_str) {
        parsed
    } else {
        return 0;
    };
    let h = &*handle;
    match runtime().block_on(h.engine.pause_file_transfer(tid)) {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

/// Resume a paused file transfer.
///
/// # Safety
#[no_mangle]
pub unsafe extern "C" fn linkall_resume_file_transfer(
    handle: *mut LinkAllHandle,
    transfer_id_hex: *const c_char,
) -> c_int {
    if handle.is_null() || transfer_id_hex.is_null() {
        return 0;
    }
    let tid_str = match CStr::from_ptr(transfer_id_hex).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let tid = if let Ok(parsed) = crate::ipc::parse_transfer_id(tid_str) {
        parsed
    } else {
        return 0;
    };
    let h = &*handle;
    match runtime().block_on(h.engine.resume_file_transfer(tid)) {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

/// Free an event returned by `linkall_poll_event`.
///
/// # Safety
/// `event` must be a pointer returned by `linkall_poll_event`.
#[no_mangle]
pub unsafe extern "C" fn linkall_free_event(event: *mut PbEvent) {
    if !event.is_null() {
        drop(Box::from_raw(event));
    }
}

// ── Camera accessors ─────────────────────────────────────────────────────────

/// Get the data buffer for a PB_EVENT_CAMERA_FRAME event.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_camera_frame_data(_event: *mut PbEvent) -> *const u8 {
    // Camera frames are no longer sent via event bus to avoid OOM.
    // They must be fetched directly from the engine state via linkall_engine_get_camera_frame.
    std::ptr::null()
}

/// Get the data length for a PB_EVENT_CAMERA_FRAME event.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_camera_frame_len(_event: *const PbEvent) -> usize {
    0
}

/// Fetch the latest camera frame for a specific peer directly from the engine.
/// Copies up to `max_len` bytes into `out_buffer`. Returns actual length, or 0 if none/error.
#[no_mangle]
pub unsafe extern "C" fn linkall_engine_get_camera_frame(
    engine: *mut crate::engine::Engine,
    peer_id_bytes: *const u8,
    out_buffer: *mut u8,
    max_len: usize,
) -> usize {
    if engine.is_null() || peer_id_bytes.is_null() || out_buffer.is_null() {
        return 0;
    }
    let engine = &*engine;
    let peer_id = match uuid::Uuid::from_slice(std::slice::from_raw_parts(peer_id_bytes, 16)) {
        Ok(id) => id,
        Err(_) => return 0,
    };

    // Fetch frame from engine using public method
    if let Some(frame_data) = engine.get_latest_camera_frame(peer_id) {
        let len = std::cmp::min(frame_data.len(), max_len);
        std::ptr::copy_nonoverlapping(frame_data.as_ptr(), out_buffer, len);
        return len;
    }
    0
}

/// Push a camera frame to all peers.
#[no_mangle]
pub unsafe extern "C" fn linkall_push_video_frame(
    handle: *mut LinkAllHandle,
    data: *const u8,
    len: usize,
) -> c_int {
    if handle.is_null() || data.is_null() || len == 0 {
        return -1;
    }
    let bytes = std::slice::from_raw_parts(data, len).to_vec();
    let h = &*handle;
    runtime().block_on(h.engine.push_camera_frame(bytes));
    0
}

/// Stop the camera stream and broadcast stop message to all peers.
#[no_mangle]
pub unsafe extern "C" fn linkall_stop_camera_stream(handle: *mut LinkAllHandle) -> c_int {
    if handle.is_null() {
        return -1;
    }
    let h = &*handle;
    runtime().block_on(h.engine.stop_camera_stream());
    0
}

/// Ask a specific connected peer to start streaming its camera. Returns 0
/// if the request was sent, -1 on invalid args or if that peer isn't
/// currently connected.
///
/// # Safety
/// `handle` and `target_device_ptr` must be valid.
#[no_mangle]
pub unsafe extern "C" fn linkall_request_camera_stream(
    handle: *mut LinkAllHandle,
    target_device_ptr: *const c_char,
) -> c_int {
    if handle.is_null() || target_device_ptr.is_null() {
        return -1;
    }
    let id_str = match CStr::from_ptr(target_device_ptr).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return -1,
    };
    let target_device = match uuid::Uuid::parse_str(&id_str) {
        Ok(id) => id,
        Err(_) => return -1,
    };
    let h = &*handle;
    let sent = runtime().block_on(h.engine.request_camera_stream(target_device));
    if sent {
        0
    } else {
        -1
    }
}

/// Returns 1 if a PB_EVENT_CAMERA_STREAM_ACCEPT event's request was
/// accepted, 0 if rejected or not applicable.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_camera_stream_accepted(event: *const PbEvent) -> c_int {
    if event.is_null() {
        return 0;
    }
    if let EngineEvent::CameraStreamAccept { accepted, .. } = &(*event).inner {
        if *accepted {
            1
        } else {
            0
        }
    } else {
        0
    }
}

// ── Windows P/Invoke helpers ──────────────────────────────────────────────────

/// Respond to a TOFU prompt.  `trust` = 1 to accept, 0 to reject.
/// Returns 0 on success.
///
/// # Safety
/// `handle` and `device_id_ptr` must be valid.
#[no_mangle]
pub unsafe extern "C" fn linkall_trust_peer(
    handle: *mut LinkAllHandle,
    device_id_ptr: *const std::ffi::c_char,
    trust: std::ffi::c_int,
) -> std::ffi::c_int {
    if handle.is_null() || device_id_ptr.is_null() {
        return -1;
    }
    let id_str = match std::ffi::CStr::from_ptr(device_id_ptr).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return -1,
    };
    let device_id = match uuid::Uuid::parse_str(&id_str) {
        Ok(id) => id,
        Err(_) => return -1,
    };
    let h = &*handle;
    if trust != 0 {
        runtime().block_on(async {
            let _ = h.engine.trust_peer(device_id).await;
        });
    } else {
        runtime().block_on(async {
            let _ = h.engine.reject_peer(device_id).await;
        });
    }
    0
}

/// Alias for `linkall_apply_clipboard` for Windows P/Invoke compatibility.
///
/// # Safety
/// `handle` and `hash_ptr` must be valid.
#[no_mangle]
pub unsafe extern "C" fn linkall_apply_by_hash(
    handle: *mut LinkAllHandle,
    hash_ptr: *const std::ffi::c_char,
) -> std::ffi::c_int {
    linkall_apply_clipboard(handle, hash_ptr)
}

/// Send a call action to a peer.
///
/// # Safety
/// `handle`, `action_ptr` and `target_device_ptr` must be valid.
#[no_mangle]
pub unsafe extern "C" fn linkall_send_call_action(
    handle: *mut LinkAllHandle,
    action_ptr: *const std::ffi::c_char,
    target_device_ptr: *const std::ffi::c_char,
) -> std::ffi::c_int {
    if handle.is_null() || action_ptr.is_null() || target_device_ptr.is_null() {
        return -1;
    }

    let action = match std::ffi::CStr::from_ptr(action_ptr).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return -1,
    };

    let id_str = match std::ffi::CStr::from_ptr(target_device_ptr).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return -1,
    };

    let device_id = match uuid::Uuid::parse_str(&id_str) {
        Ok(id) => id,
        Err(_) => return -1,
    };

    let h = &*handle;
    runtime().block_on(async {
        h.engine.send_call_action(action, device_id).await;
    });

    0
}

pub type LinkAllEventCallback =
    extern "C" fn(event: *mut PbEvent, user_data: *mut std::ffi::c_void);

/// Register a callback to be invoked on a background thread when events occur.
/// This consumes the internal event receiver; `linkall_poll_event` will subsequently return NULL.
/// The callback must be thread-safe (e.g. JNI AttachCurrentThread or dispatch_async).
#[no_mangle]
pub unsafe extern "C" fn linkall_register_event_callback(
    handle: *mut LinkAllHandle,
    callback: LinkAllEventCallback,
    user_data: *mut std::ffi::c_void,
) {
    if handle.is_null() {
        return;
    }
    let h = &mut *handle;

    let mut rx_opt = h.event_rx.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut rx) = rx_opt.take() {
        let ud_addr = user_data as usize;
        let cb_addr = callback as usize; // Cast fn ptr to usize to force Send
        runtime().spawn(async move {
            while let Some(event) = rx.recv().await {
                let cb: LinkAllEventCallback = std::mem::transmute(cb_addr);
                let pb_event = Box::into_raw(Box::new(PbEvent {
                    inner: event,
                    cached_strings: Vec::new(),
                }));
                cb(pb_event, ud_addr as *mut std::ffi::c_void);
            }
        });
    }
}

// ── Remote Explorer C API ─────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn linkall_send_remote_files_query(
    handle: *mut LinkAllHandle,
    target_device_id: *const c_char,
    request_id: *const c_char,
    summary_only: c_int,
    category: *const c_char,
    source: *const c_char,
    search_query: *const c_char,
    offset: u32,
    limit: u32,
) -> c_int {
    if handle.is_null() || target_device_id.is_null() || request_id.is_null() {
        return 0;
    }
    let h = &*handle;
    let target_raw = match CStr::from_ptr(target_device_id).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let target_uuid = match uuid::Uuid::parse_str(target_raw) {
        Ok(u) => u,
        Err(_) => return 0,
    };
    let req_raw = match CStr::from_ptr(request_id).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let req_uuid = match uuid::Uuid::parse_str(req_raw) {
        Ok(u) => u,
        Err(_) => return 0,
    };

    let cat_opt = if category.is_null() {
        None
    } else {
        match CStr::from_ptr(category).to_str() {
            Ok("Images") | Ok("images") | Ok("image") => {
                Some(crate::protocol::RemoteFileCategory::Images)
            }
            Ok("Videos") | Ok("videos") | Ok("video") => {
                Some(crate::protocol::RemoteFileCategory::Videos)
            }
            Ok("Audio") | Ok("audio") => Some(crate::protocol::RemoteFileCategory::Audio),
            Ok("Documents") | Ok("documents") | Ok("document") => {
                Some(crate::protocol::RemoteFileCategory::Documents)
            }
            Ok("APKs") | Ok("Apks") | Ok("apks") | Ok("apk") => {
                Some(crate::protocol::RemoteFileCategory::Apks)
            }
            Ok("Archives") | Ok("archives") | Ok("archive") => {
                Some(crate::protocol::RemoteFileCategory::Archives)
            }
            Ok("Other") | Ok("other") => Some(crate::protocol::RemoteFileCategory::Other),
            _ => None,
        }
    };

    let src_opt = if source.is_null() {
        None
    } else {
        match CStr::from_ptr(source).to_str() {
            Ok("WhatsApp") | Ok("whatsapp") => Some(crate::protocol::RemoteFileSource::WhatsApp),
            Ok("Downloads") | Ok("downloads") => Some(crate::protocol::RemoteFileSource::Downloads),
            Ok("Camera") | Ok("camera") => Some(crate::protocol::RemoteFileSource::Camera),
            Ok("Other") | Ok("other") => Some(crate::protocol::RemoteFileSource::Other),
            _ => None,
        }
    };

    let query_opt = if search_query.is_null() {
        None
    } else {
        CStr::from_ptr(search_query)
            .to_str()
            .ok()
            .map(|s| s.to_string())
    };

    runtime().block_on(h.engine.send_remote_files_query(
        target_uuid,
        req_uuid,
        summary_only != 0,
        cat_opt,
        src_opt,
        query_opt,
        offset,
        limit,
    ));
    1
}

#[no_mangle]
pub unsafe extern "C" fn linkall_send_remote_thumbnail_request(
    handle: *mut LinkAllHandle,
    target_device_id: *const c_char,
    request_id: *const c_char,
    file_id: u64,
    size_px: u32,
) -> c_int {
    if handle.is_null() || target_device_id.is_null() || request_id.is_null() {
        return 0;
    }
    let h = &*handle;
    let target_raw = match CStr::from_ptr(target_device_id).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let target_uuid = match uuid::Uuid::parse_str(target_raw) {
        Ok(u) => u,
        Err(_) => return 0,
    };
    let req_raw = match CStr::from_ptr(request_id).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let req_uuid = match uuid::Uuid::parse_str(req_raw) {
        Ok(u) => u,
        Err(_) => return 0,
    };

    runtime().block_on(h.engine.send_remote_thumbnail_request(
        target_uuid,
        req_uuid,
        file_id,
        size_px,
    ));
    1
}

#[no_mangle]
pub unsafe extern "C" fn linkall_send_remote_file_pull_request(
    handle: *mut LinkAllHandle,
    target_device_id: *const c_char,
    request_id: *const c_char,
    file_id: u64,
) -> c_int {
    if handle.is_null() || target_device_id.is_null() || request_id.is_null() {
        return 0;
    }
    let h = &*handle;
    let target_raw = match CStr::from_ptr(target_device_id).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let target_uuid = match uuid::Uuid::parse_str(target_raw) {
        Ok(u) => u,
        Err(_) => return 0,
    };
    let req_raw = match CStr::from_ptr(request_id).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let req_uuid = match uuid::Uuid::parse_str(req_raw) {
        Ok(u) => u,
        Err(_) => return 0,
    };

    runtime().block_on(
        h.engine
            .send_remote_file_pull_request(target_uuid, req_uuid, file_id),
    );
    1
}

#[no_mangle]
pub unsafe extern "C" fn linkall_send_remote_files_response(
    handle: *mut LinkAllHandle,
    request_id: *const c_char,
    target_device_id: *const c_char,
    summary_json: *const c_char,
    files_json: *const c_char,
    total_matching: u32,
    error_str: *const c_char,
) -> c_int {
    if handle.is_null() || request_id.is_null() || target_device_id.is_null() {
        return 0;
    }
    let req_raw = match CStr::from_ptr(request_id).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let req_uuid = match uuid::Uuid::parse_str(req_raw) {
        Ok(u) => u,
        Err(_) => return 0,
    };
    let target_raw = match CStr::from_ptr(target_device_id).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let target_uuid = match uuid::Uuid::parse_str(target_raw) {
        Ok(u) => u,
        Err(_) => return 0,
    };

    let summary: Option<crate::protocol::RemoteFilesSummary> = if summary_json.is_null() {
        None
    } else {
        match CStr::from_ptr(summary_json).to_str() {
            Ok(s) if !s.is_empty() => serde_json::from_str(s).ok(),
            _ => None,
        }
    };

    let files: Vec<crate::protocol::RemoteFileEntry> = if files_json.is_null() {
        Vec::new()
    } else {
        match CStr::from_ptr(files_json).to_str() {
            Ok(s) if !s.is_empty() => serde_json::from_str(s).unwrap_or_default(),
            _ => Vec::new(),
        }
    };

    let err_opt = if error_str.is_null() {
        None
    } else {
        match CStr::from_ptr(error_str).to_str() {
            Ok(s) if !s.is_empty() => Some(s.to_string()),
            _ => None,
        }
    };

    let h = &*handle;
    runtime().block_on(h.engine.send_remote_files_response(
        target_uuid,
        req_uuid,
        summary,
        files,
        total_matching,
        err_opt,
    ));
    1
}

#[no_mangle]
pub unsafe extern "C" fn linkall_event_remote_request_id(event: *mut PbEvent) -> *const c_char {
    let e = &mut *event;
    let s = match &e.inner {
        EngineEvent::RemoteFilesQueryReceived { request_id, .. } => Some(request_id.to_string()),
        EngineEvent::RemoteFilesResponseReceived { request_id, .. } => Some(request_id.to_string()),
        EngineEvent::RemoteThumbnailRequestReceived { request_id, .. } => {
            Some(request_id.to_string())
        }
        EngineEvent::RemoteThumbnailResponseReceived { request_id, .. } => {
            Some(request_id.to_string())
        }
        EngineEvent::RemoteFilePullRequestReceived { request_id, .. } => {
            Some(request_id.to_string())
        }
        _ => None,
    };
    if let Some(str_val) = s {
        e.cache_str(str_val)
    } else {
        std::ptr::null()
    }
}

#[no_mangle]
pub unsafe extern "C" fn linkall_event_remote_summary_json(event: *mut PbEvent) -> *const c_char {
    let e = &mut *event;
    if let EngineEvent::RemoteFilesResponseReceived {
        summary: Some(s), ..
    } = &e.inner
    {
        if let Ok(json) = serde_json::to_string(s) {
            return e.cache_str(json);
        }
    }
    std::ptr::null()
}

#[no_mangle]
pub unsafe extern "C" fn linkall_event_remote_files_json(event: *mut PbEvent) -> *const c_char {
    let e = &mut *event;
    if let EngineEvent::RemoteFilesResponseReceived { files, .. } = &e.inner {
        if let Ok(json) = serde_json::to_string(files) {
            return e.cache_str(json);
        }
    }
    std::ptr::null()
}

#[no_mangle]
pub unsafe extern "C" fn linkall_event_remote_total_matching(event: *const PbEvent) -> u32 {
    if event.is_null() {
        return 0;
    }
    match &(*event).inner {
        EngineEvent::RemoteFilesResponseReceived { total_matching, .. } => *total_matching,
        _ => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn linkall_event_remote_file_id(event: *const PbEvent) -> u64 {
    if event.is_null() {
        return 0;
    }
    match &(*event).inner {
        EngineEvent::RemoteThumbnailRequestReceived { file_id, .. } => *file_id,
        EngineEvent::RemoteThumbnailResponseReceived { file_id, .. } => *file_id,
        EngineEvent::RemoteFilePullRequestReceived { file_id, .. } => *file_id,
        _ => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn linkall_event_remote_thumbnail_data(event: *mut PbEvent) -> *const u8 {
    let e = &mut *event;
    if let EngineEvent::RemoteThumbnailResponseReceived { data, .. } = &e.inner {
        if !data.is_empty() {
            return data.as_ptr();
        }
    }
    std::ptr::null()
}

#[no_mangle]
pub unsafe extern "C" fn linkall_event_remote_thumbnail_len(event: *const PbEvent) -> usize {
    if event.is_null() {
        return 0;
    }
    if let EngineEvent::RemoteThumbnailResponseReceived { data, .. } = &(*event).inner {
        return data.len();
    }
    0
}

/// Image bytes of a CLIPBOARD_IMAGE event. Lifetime: until
/// `linkall_free_event`. Length via `linkall_event_image_len`.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_image_data(event: *const PbEvent) -> *const u8 {
    if event.is_null() {
        return std::ptr::null();
    }
    if let EngineEvent::ClipboardReceived { content, .. } = &(*event).inner {
        if let ClipboardContent::Image { data, .. } = &**content {
            if !data.is_empty() {
                return data.as_ptr();
            }
        }
    }
    std::ptr::null()
}

#[no_mangle]
pub unsafe extern "C" fn linkall_event_image_len(event: *const PbEvent) -> usize {
    if event.is_null() {
        return 0;
    }
    if let EngineEvent::ClipboardReceived { content, .. } = &(*event).inner {
        if let ClipboardContent::Image { data, .. } = &**content {
            return data.len();
        }
    }
    0
}

/// MIME type of a CLIPBOARD_IMAGE event, e.g. "image/png".
#[no_mangle]
pub unsafe extern "C" fn linkall_event_image_mime(event: *mut PbEvent) -> *const c_char {
    if event.is_null() {
        return std::ptr::null();
    }
    let e = &mut *event;
    let mime = match &e.inner {
        EngineEvent::ClipboardReceived { content, .. } => match &**content {
            ClipboardContent::Image { mime, .. } => Some(mime.clone()),
            _ => None,
        },
        _ => None,
    };
    match mime {
        Some(m) => e.cache_str(m),
        None => std::ptr::null(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn linkall_event_remote_error(event: *mut PbEvent) -> *const c_char {
    let e = &mut *event;
    let err_opt = match &e.inner {
        EngineEvent::RemoteFilesResponseReceived { error, .. } => error.clone(),
        EngineEvent::RemoteThumbnailResponseReceived { error, .. } => error.clone(),
        EngineEvent::OpenUrlOnDeviceAckReceived { error, .. } => error.clone(),
        _ => None,
    };
    if let Some(s) = err_opt {
        e.cache_str(s)
    } else {
        std::ptr::null()
    }
}

/// Returns 1 if an OPEN_URL_ON_DEVICE_ACK event reports success, 0 otherwise
/// (including for any other event type).
#[no_mangle]
pub unsafe extern "C" fn linkall_event_open_url_ack_success(event: *const PbEvent) -> c_int {
    if event.is_null() {
        return 0;
    }
    match (*event).inner {
        EngineEvent::OpenUrlOnDeviceAckReceived { success, .. } => success as c_int,
        _ => 0,
    }
}

/// The caller's phone number from a CALL_STATE_CHANGED event, or NULL.
/// Empty when the phone hides it (no call-log permission, or a private number).
#[no_mangle]
pub unsafe extern "C" fn linkall_event_call_number(event: *mut PbEvent) -> *const c_char {
    if event.is_null() {
        return std::ptr::null();
    }
    let e = &mut *event;
    match &e.inner {
        EngineEvent::CallStateChanged { number, .. } => {
            let number = number.clone();
            e.cache_str(number)
        }
        _ => std::ptr::null(),
    }
}

/// The caller's contact name from a CALL_STATE_CHANGED event, or NULL.
/// Empty when the number isn't in the phone's contacts.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_call_contact_name(event: *mut PbEvent) -> *const c_char {
    if event.is_null() {
        return std::ptr::null();
    }
    let e = &mut *event;
    match &e.inner {
        EngineEvent::CallStateChanged { contact_name, .. } => {
            let name = contact_name.clone();
            e.cache_str(name)
        }
        _ => std::ptr::null(),
    }
}

/// The title of a NOTIFICATION_RECEIVED event, or NULL.
#[no_mangle]
pub unsafe extern "C" fn linkall_event_notification_title(event: *mut PbEvent) -> *const c_char {
    if event.is_null() {
        return std::ptr::null();
    }
    let e = &mut *event;
    match &e.inner {
        EngineEvent::NotificationReceived { title, .. } => {
            let title = title.clone();
            e.cache_str(title)
        }
        _ => std::ptr::null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn create_test_handle() -> (*mut LinkAllHandle, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = EngineConfig {
            device_name: "TestDevice".into(),
            port: 0,
            trust_store_path: temp_dir.path().join("trust.json"),
            peer_store_path: temp_dir.path().join("peers.json"),
            identity_path: temp_dir.path().join("identity.bin"),
            data_dir: temp_dir.path().join("data"),
            enable_discovery: false,
            ..EngineConfig::default()
        };
        let (event_tx, event_rx) = mpsc::channel(256);
        let engine = runtime().block_on(Engine::start(config, event_tx)).unwrap();
        let handle = Box::into_raw(Box::new(LinkAllHandle {
            engine,
            event_rx: std::sync::Mutex::new(Some(event_rx)),
        }));
        (handle, temp_dir)
    }

    #[test]
    fn test_send_remote_files_response_null_inputs() {
        unsafe {
            let req = CString::new(uuid::Uuid::new_v4().to_string()).unwrap();
            let target = CString::new(uuid::Uuid::new_v4().to_string()).unwrap();

            // Null handle
            assert_eq!(
                linkall_send_remote_files_response(
                    std::ptr::null_mut(),
                    req.as_ptr(),
                    target.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    std::ptr::null()
                ),
                0
            );

            let (handle, _dir) = create_test_handle();
            assert!(!handle.is_null());

            // Null request_id
            assert_eq!(
                linkall_send_remote_files_response(
                    handle,
                    std::ptr::null(),
                    target.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    std::ptr::null()
                ),
                0
            );

            // Null target_device_id
            assert_eq!(
                linkall_send_remote_files_response(
                    handle,
                    req.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    std::ptr::null()
                ),
                0
            );

            linkall_stop(handle);
        }
    }

    #[test]
    fn test_send_remote_files_response_invalid_uuid() {
        unsafe {
            let (handle, _dir) = create_test_handle();
            assert!(!handle.is_null());

            let valid_uuid = CString::new(uuid::Uuid::new_v4().to_string()).unwrap();
            let invalid_uuid = CString::new("invalid-uuid-string").unwrap();

            // Invalid request_id
            assert_eq!(
                linkall_send_remote_files_response(
                    handle,
                    invalid_uuid.as_ptr(),
                    valid_uuid.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    std::ptr::null()
                ),
                0
            );

            // Invalid target_device_id
            assert_eq!(
                linkall_send_remote_files_response(
                    handle,
                    valid_uuid.as_ptr(),
                    invalid_uuid.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    std::ptr::null()
                ),
                0
            );

            linkall_stop(handle);
        }
    }

    #[test]
    fn test_send_remote_files_response_valid() {
        unsafe {
            let (handle, _dir) = create_test_handle();
            assert!(!handle.is_null());

            let req_uuid = CString::new(uuid::Uuid::new_v4().to_string()).unwrap();
            let target_uuid = CString::new(uuid::Uuid::new_v4().to_string()).unwrap();

            // Call with optional null JSON and error strings
            let res1 = linkall_send_remote_files_response(
                handle,
                req_uuid.as_ptr(),
                target_uuid.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                5,
                std::ptr::null(),
            );
            assert_eq!(res1, 1);

            // Call with valid JSON payloads and error string
            let summary_json = CString::new(
                serde_json::to_string(&crate::protocol::RemoteFilesSummary::default()).unwrap(),
            )
            .unwrap();
            let files_json = CString::new("[]").unwrap();
            let err_str = CString::new("no error").unwrap();

            let res2 = linkall_send_remote_files_response(
                handle,
                req_uuid.as_ptr(),
                target_uuid.as_ptr(),
                summary_json.as_ptr(),
                files_json.as_ptr(),
                0,
                err_str.as_ptr(),
            );
            assert_eq!(res2, 1);

            linkall_stop(handle);
        }
    }

    #[test]
    fn image_event_exposes_bytes_and_mime() {
        let data = vec![0x89, b'P', b'N', b'G', 1, 2, 3];
        let mut event = PbEvent {
            inner: EngineEvent::ClipboardReceived {
                from_device: uuid::Uuid::new_v4(),
                from_name: "Phone".into(),
                content: std::sync::Arc::new(ClipboardContent::Image {
                    mime: "image/png".into(),
                    data: data.clone(),
                }),
                auto_applied: true,
                relay_path: Vec::new(),
                activity_id: 1,
            },
            cached_strings: Vec::new(),
        };
        unsafe {
            let ev = &mut event as *mut PbEvent;
            assert_eq!(linkall_event_type(ev), PB_EVENT_CLIPBOARD_IMAGE);
            let len = linkall_event_image_len(ev);
            let ptr = linkall_event_image_data(ev);
            assert_eq!(std::slice::from_raw_parts(ptr, len), data.as_slice());
            let mime = CStr::from_ptr(linkall_event_image_mime(ev));
            assert_eq!(mime.to_str().unwrap(), "image/png");
            assert!(linkall_event_text(ev).is_null());
        }
    }

    #[test]
    fn image_accessors_are_empty_for_text_events() {
        let mut event = PbEvent {
            inner: EngineEvent::Warning("x".into()),
            cached_strings: Vec::new(),
        };
        unsafe {
            let ev = &mut event as *mut PbEvent;
            assert!(linkall_event_image_data(ev).is_null());
            assert_eq!(linkall_event_image_len(ev), 0);
            assert!(linkall_event_image_mime(ev).is_null());
        }
    }
}
