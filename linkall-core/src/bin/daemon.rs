use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use linkall_core::{
    engine::{Engine, EngineConfig, EngineEvent, SyncDispatchReport, SyncTarget},
    history::{History, HistoryEntry, HistoryFilter},
    ipc::{IpcRequest, IpcResponse},
    peer_manager::PeerConnectionState,
    protocol::ClipboardContent,
    settings::{default_history_path, default_settings_path, ClipboardTemplate, SettingsStore},
    trust::format_fingerprint,
};
use serde::Serialize;
use serde_json::json;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{mpsc, Mutex, Notify};
use uuid::Uuid;

const MAX_FEEDBACK_EVENTS: usize = 200;
const MAX_INCOMING_CLIPBOARDS: usize = 128;

#[derive(Debug, Clone, Serialize)]
struct FeedbackEvent {
    timestamp: u64,
    kind: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    clipboard_id: Option<u64>,
}

#[derive(Clone)]
struct DaemonState {
    engine: Arc<Engine>,
    settings: Arc<Mutex<SettingsStore>>,
    history: Arc<Mutex<History>>,
    feedback: Arc<Mutex<VecDeque<FeedbackEvent>>>,
    incoming_clipboards: Arc<Mutex<HashMap<u64, serde_json::Value>>>,
    incoming_order: Arc<Mutex<VecDeque<u64>>>,
    started_at: Instant,
    shutdown: Arc<Notify>,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("Link All daemon failed: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let env_filter = tracing_subscriber::EnvFilter::try_from_env("LINKALL_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    if std::env::var("LINKALL_LOG_JSON").is_ok() {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    }

    let settings_store =
        SettingsStore::load(default_settings_path()).context("loading settings")?;
    let initial_settings = settings_store.get().clone();

    let config = EngineConfig {
        device_name: initial_settings.resolved_device_name(),
        port: initial_settings.port,
        ..Default::default()
    };

    let (event_tx, mut event_rx) = mpsc::channel(256);
    let engine = Arc::new(Engine::start(config, event_tx).await?);
    engine.apply_settings(initial_settings.clone()).await;

    let history = History::load_with_limit(
        default_history_path(),
        initial_settings.effective_history_limit(),
    )
    .context("loading history")?;

    let state = DaemonState {
        engine: engine.clone(),
        settings: Arc::new(Mutex::new(settings_store)),
        history: Arc::new(Mutex::new(history)),
        feedback: Arc::new(Mutex::new(VecDeque::new())),
        incoming_clipboards: Arc::new(Mutex::new(HashMap::new())),
        incoming_order: Arc::new(Mutex::new(VecDeque::new())),
        started_at: Instant::now(),
        shutdown: Arc::new(Notify::new()),
    };

    {
        let history_state = state.history.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if let Err(error) = history_state.lock().await.purge_expired_sensitive_entries() {
                    tracing::warn!("daemon sensitive-history pruning failed: {error:#}");
                }
            }
        });
    }

    let event_state = state.clone();
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if let Err(error) = handle_event(event_state.clone(), event).await {
                tracing::warn!("event processing failed: {error:#}");
            }
        }
    });

    #[cfg(unix)]
    {
        let handler_state = state.clone();
        linkall_core::ipc::server::spawn(Arc::new(move |req| {
            let handler_state = handler_state.clone();
            async move { handle_request(handler_state, req).await }
        }))
        .await
        .context("starting IPC server")?;
    }

    #[cfg(windows)]
    {
        use linkall_core::ipc_windows::spawn_windows_ipc;
        let handler_state = state.clone();
        spawn_windows_ipc(Arc::new(move |req| {
            let handler_state = handler_state.clone();
            async move { handle_request(handler_state, req).await }
        }))
        .await
        .context("starting Windows named-pipe IPC server")?;
        tracing::info!("Windows IPC server started on \\\\.\\pipe\\linkall");
    }

    tracing::info!(
        "Link All daemon started. IPC socket: {:?}",
        linkall_core::ipc::socket_path()
    );

    // ── SET-06: Hot-reload settings without daemon restart ────────────────────
    //
    // Poll the settings file's modification time every second.  When it
    // changes (e.g. the Mac preferences UI or an external editor wrote it),
    // reload the store and apply the new settings to the running engine and
    // history buffer without any restart.
    {
        let reload_state = state.clone();
        let settings_path = default_settings_path();
        tokio::spawn(async move {
            let mut last_mtime: Option<std::time::SystemTime> = std::fs::metadata(&settings_path)
                .and_then(|m| m.modified())
                .ok();

            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                let current_mtime = std::fs::metadata(&settings_path)
                    .and_then(|m| m.modified())
                    .ok();

                if current_mtime != last_mtime && current_mtime.is_some() {
                    last_mtime = current_mtime;
                    tracing::info!("Settings file changed — hot-reloading");

                    match SettingsStore::load(&settings_path) {
                        Ok(new_store) => {
                            let new_settings = new_store.get().clone();
                            {
                                let mut store = reload_state.settings.lock().await;
                                *store = new_store;
                            }
                            {
                                let mut history = reload_state.history.lock().await;
                                let _ =
                                    history.set_max_entries(new_settings.effective_history_limit());
                            }
                            reload_state.engine.apply_settings(new_settings).await;
                            tracing::info!("Settings hot-reload complete");
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Settings hot-reload failed (file may be mid-write): {e:#}"
                            );
                        }
                    }
                }
            }
        });
    }

    tokio::select! {
        _ = state.shutdown.notified() => {
            tracing::info!("Shutdown requested by IPC client");
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Shutting down on SIGINT");
        }
    }

    Ok(())
}

async fn handle_event(state: DaemonState, event: EngineEvent) -> Result<()> {
    match event {
        EngineEvent::ClipboardReceived {
            from_name,
            content,
            activity_id,
            ..
        } => {
            let settings = state.settings.lock().await.get().clone();
            {
                let mut history = state.history.lock().await;
                history.push_with_options(
                    &content,
                    from_name.clone(),
                    settings.max_history_text_bytes,
                )?;
            }

            store_incoming_clipboard(
                &state,
                activity_id,
                incoming_payload_json(activity_id, &content),
            )
            .await;
            push_feedback(
                &state,
                FeedbackEvent {
                    timestamp: now_secs(),
                    kind: "clipboard_received".into(),
                    message: format!("Clipboard received from {from_name}"),
                    device_id: None,
                    device_name: Some(from_name),
                    clipboard_id: Some(activity_id),
                },
            )
            .await;
        }
        EngineEvent::HistoryMetadataReceived {
            from_name, entry, ..
        } => {
            state.history.lock().await.push_metadata(&entry)?;
            push_feedback(
                &state,
                FeedbackEvent {
                    timestamp: now_secs(),
                    kind: "history_metadata".into(),
                    message: format!("History updated from {from_name}"),
                    device_id: None,
                    device_name: Some(from_name),
                    clipboard_id: None,
                },
            )
            .await;
        }
        EngineEvent::ClipboardSynced {
            peer_device,
            peer_name,
            ..
        } => {
            push_feedback(
                &state,
                FeedbackEvent {
                    timestamp: now_secs(),
                    kind: "clipboard_dispatch".into(),
                    message: format!("Sent clipboard to {peer_name}"),
                    device_id: Some(peer_device.to_string()),
                    device_name: Some(peer_name),
                    clipboard_id: None,
                },
            )
            .await;
        }
        EngineEvent::ClipboardSyncFailed {
            peer_device,
            peer_name,
            reason,
            ..
        } => {
            push_feedback(
                &state,
                FeedbackEvent {
                    timestamp: now_secs(),
                    kind: "clipboard_sync_failed".into(),
                    message: format!("Failed to sync with {peer_name}: {reason}"),
                    device_id: Some(peer_device.to_string()),
                    device_name: Some(peer_name),
                    clipboard_id: None,
                },
            )
            .await;
        }
        EngineEvent::SystemHealthUpdated(issues) => {
            tracing::info!(
                "[HEALTH] issues: {:?}",
                issues.iter().map(|i| i.kind).collect::<Vec<_>>()
            );
        }
        EngineEvent::ClipboardDeliveryStatus {
            activity_id,
            status,
        } => {
            tracing::info!("[DELIVERY] Activity {} status: {:?}", activity_id, status);
        }
        // The engine sets the pairing flags itself before emitting these;
        // setting them again here raced a cancel or expiry that landed while
        // the event was queued and revived the request.
        EngineEvent::OutgoingPairingWaiting {
            device_id,
            device_name,
            pin,
        } => {
            push_feedback(
                &state,
                FeedbackEvent {
                    timestamp: now_secs(),
                    kind: "trust_waiting".into(),
                    message: format!("Waiting for {device_name} to accept. PIN: {pin}"),
                    device_id: Some(device_id.to_string()),
                    device_name: Some(device_name),
                    clipboard_id: None,
                },
            )
            .await;
        }
        EngineEvent::PairingRequested {
            device_id,
            device_name,
            pin,
        } => {
            push_feedback(
                &state,
                FeedbackEvent {
                    timestamp: now_secs(),
                    kind: "trust_prompt".into(),
                    message: format!("{device_name} wants to connect. PIN: {pin}"),
                    device_id: Some(device_id.to_string()),
                    device_name: Some(device_name),
                    clipboard_id: None,
                },
            )
            .await;
        }
        EngineEvent::PairingConfirmed { device_id } => {
            tracing::info!("[PAIRING] Confirmed for device {}", device_id);
        }
        EngineEvent::PairingRejected { device_id } => {
            tracing::info!("[PAIRING] Rejected for device {}", device_id);
        }
        EngineEvent::PeerConnected {
            device_id,
            device_name,
            ..
        } => {
            push_feedback(
                &state,
                FeedbackEvent {
                    timestamp: now_secs(),
                    kind: "peer_connected".into(),
                    message: format!("{device_name} connected"),
                    device_id: Some(device_id.to_string()),
                    device_name: Some(device_name),
                    clipboard_id: None,
                },
            )
            .await;
        }
        EngineEvent::PeerDisconnected {
            device_id,
            device_name,
            reason,
        } => {
            let name = device_name.unwrap_or_else(|| "Unknown device".into());
            let detail = reason
                .as_deref()
                .map(|value| format!(" ({value})"))
                .unwrap_or_default();
            push_feedback(
                &state,
                FeedbackEvent {
                    timestamp: now_secs(),
                    kind: "peer_disconnected".into(),
                    message: format!("{name} disconnected{detail}"),
                    device_id: Some(device_id.to_string()),
                    device_name: Some(name),
                    clipboard_id: None,
                },
            )
            .await;
        }
        EngineEvent::Warning(message) => {
            push_feedback(
                &state,
                FeedbackEvent {
                    timestamp: now_secs(),
                    kind: "warning".into(),
                    message,
                    device_id: None,
                    device_name: None,
                    clipboard_id: None,
                },
            )
            .await;
        }
        EngineEvent::FileTransferIncoming {
            transfer_id: _,
            from_name,
            file_name,
            file_bytes,
            ..
        } => {
            push_feedback(
                &state,
                FeedbackEvent {
                    timestamp: now_secs(),
                    kind: "file_transfer_incoming".into(),
                    message: format!(
                        "{from_name} wants to send {file_name} ({} bytes)",
                        file_bytes
                    ),
                    device_id: None,
                    device_name: Some(from_name),
                    clipboard_id: None,
                },
            )
            .await;
        }
        EngineEvent::FileTransferProgress {
            file_name,
            percent,
            bytes_received,
            total_bytes,
            speed_bps,
            ..
        } => {
            let speed_str = match speed_bps {
                Some(bps) if bps > 1_000_000 => format!("{:.1} MB/s", bps as f64 / 1_000_000.0),
                Some(bps) if bps > 1_000 => format!("{:.0} KB/s", bps as f64 / 1_000.0),
                Some(bps) => format!("{bps} B/s"),
                None => "calculating...".into(),
            };
            push_feedback(
                &state,
                FeedbackEvent {
                    timestamp: now_secs(),
                    kind: "file_transfer_progress".into(),
                    message: format!(
                        "Receiving {file_name}: {percent}% ({bytes_received}/{total_bytes} bytes, {speed_str})"
                    ),
                    device_id: None,
                    device_name: None,
                    clipboard_id: None,
                },
            )
            .await;
        }
        EngineEvent::FileTransferComplete {
            from_name,
            file_name,
            dest_path,
            ..
        } => {
            let dest_str = dest_path.to_string_lossy().to_string();
            push_feedback(
                &state,
                FeedbackEvent {
                    timestamp: now_secs(),
                    kind: "file_transfer_complete".into(),
                    message: format!("Received {file_name} from {from_name} → {dest_str}"),
                    device_id: None,
                    device_name: Some(from_name),
                    clipboard_id: None,
                },
            )
            .await;
        }
        EngineEvent::FileTransferFailed {
            from_device,
            reason,
            ..
        } => {
            push_feedback(
                &state,
                FeedbackEvent {
                    timestamp: now_secs(),
                    kind: "file_transfer_failed".into(),
                    message: format!("File transfer failed: {reason}"),
                    device_id: Some(from_device.to_string()),
                    device_name: None,
                    clipboard_id: None,
                },
            )
            .await;
        }
        EngineEvent::FolderTransferComplete {
            peer_name,
            folder_name,
            file_count,
            failed_count,
            outbound,
            ..
        } => {
            let verb = if outbound { "Sent" } else { "Received" };
            push_feedback(
                &state,
                FeedbackEvent {
                    timestamp: now_secs(),
                    kind: "folder_transfer_complete".into(),
                    message: format!(
                        "{verb} folder {folder_name} ({} of {file_count} files) {} {peer_name}",
                        file_count.saturating_sub(failed_count),
                        if outbound { "to" } else { "from" },
                    ),
                    device_id: None,
                    device_name: Some(peer_name),
                    clipboard_id: None,
                },
            )
            .await;
        }
        EngineEvent::FileTransferPaused { .. } | EngineEvent::FileTransferResumed { .. } => {
            // These are informational only; no feedback needed.
        }
        #[cfg(unix)]
        EngineEvent::OpenUrlOnDeviceRequested {
            from_device, url, ..
        } => {
            // A trusted peer sent a link: open it here, then tell the peer.
            let engine = state.engine.clone();
            tokio::spawn(async move {
                let result = open_url_for_peer(&url).await;
                let (success, error) = match result {
                    Ok(()) => (true, None),
                    Err(e) => (false, Some(e)),
                };
                engine
                    .ack_open_url_on_device(from_device, success, error)
                    .await;
            });
        }
        EngineEvent::RemoteFilesQueryReceived { .. }
        | EngineEvent::RemoteThumbnailRequestReceived { .. }
        | EngineEvent::RemoteFilePullRequestReceived { .. } => {
            linkall_core::local_files::handle_event(&state.engine, &event);
        }
        _ => {}
    }

    Ok(())
}

/// Opens a web link from a peer in the user's default browser. Only http and
/// https links are opened; anything else (file:, app schemes) is refused.
#[cfg(unix)]
async fn open_url_for_peer(url: &str) -> std::result::Result<(), String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://"))
        || url.contains(char::is_whitespace)
    {
        return Err("Only web links can be opened".into());
    }
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let url = url.to_string();
    let status = tokio::task::spawn_blocking(move || {
        std::process::Command::new(opener)
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
    })
    .await
    .map_err(|e| format!("Could not start the browser: {e}"))?
    .map_err(|e| format!("Could not start the browser: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("The browser did not open the link".into())
    }
}

async fn handle_request(state: DaemonState, req: IpcRequest) -> IpcResponse {
    match handle_request_inner(state, req).await {
        Ok(response) => response,
        Err(error) => IpcResponse::error(error.to_string()),
    }
}

async fn handle_request_inner(state: DaemonState, req: IpcRequest) -> Result<IpcResponse> {
    match req {
        IpcRequest::Ping => Ok(IpcResponse::Pong {
            uptime_secs: state.started_at.elapsed().as_secs(),
        }),
        IpcRequest::Status => {
            let snapshot = state.engine.status_snapshot().await;
            let settings = state.settings.lock().await.get().clone();
            let peer_count = snapshot
                .peers
                .iter()
                .filter(|peer| peer.status == PeerConnectionState::Connected)
                .count();
            let pending_count = state.engine.pending_remote_clipboards().await.len();
            let fingerprint = state.engine.local_fingerprint();
            let active_call = state.engine.active_call().await;
            let active_transfers = state.engine.active_transfers().await;
            let peer_batteries = state.engine.peer_batteries().await;
            let peer_networks = state.engine.peer_networks().await;
            let peer_storages = state.engine.peer_storages().await;
            Ok(IpcResponse::ok(json!({
                "device_name":           settings.resolved_device_name(),
                "port":                  settings.port,
                "sync_enabled":          settings.sync_enabled,
                "peer_count":            peer_count,
                "last_sync_at":          snapshot.last_sync_at,
                "peers":                 snapshot.peers,
                "pending_clipboard_count": pending_count,
                "local_fingerprint":     fingerprint,
                "local_device_id":       state.engine.local_device_id().to_string(),
                "local_device_name":     state.engine.local_device_name(),
                "active_call":           active_call,
                "active_transfers":      active_transfers,
                "active_speed_tests":    state.engine.active_speed_tests().await,
                "peer_batteries":        peer_batteries,
                "peer_networks":         peer_networks,
                "peer_storages":         peer_storages,
                "health":                state.engine.health().await,
                "folders":               state.engine.folders().await,
            })))
        }
        // Re-trigger mDNS discovery — called by the Mac "Scan" button and
        // also by the Android NSD retry scheduler when it sends a push.
        IpcRequest::RescanPeers => {
            state.engine.rescan_peers().await;
            Ok(IpcResponse::ok(json!({ "ok": true })))
        }
        IpcRequest::Peers => {
            let snapshot = state.engine.status_snapshot().await;
            Ok(IpcResponse::ok(snapshot.peers))
        }
        IpcRequest::TrustedDevices => Ok(IpcResponse::ok(state.engine.trusted_devices().await)),
        IpcRequest::DeviceDetails { device_id } => {
            let record = state
                .engine
                .trusted_devices()
                .await
                .into_iter()
                .find(|device| device.device_id == parse_uuid(&device_id).unwrap_or_default())
                .context("device not found")?;
            Ok(IpcResponse::ok(json!({
                "device_id": record.device_id,
                "device_name": record.device_name,
                "display_name": record.display_name,
                "effective_name": record.effective_name(),
                "state": record.state,
                "fingerprint": format_fingerprint(&record.key_fingerprint),
                "first_seen": record.first_seen,
                "trusted_since": record.trusted_since,
                "last_seen": record.last_seen,
            })))
        }
        IpcRequest::ConnectPeer { ip, port } => {
            state.engine.connect_to_peer(ip, port).await?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::ReconnectPeer { device_id } => match parse_uuid(&device_id) {
            Ok(id) => match state.engine.reconnect_peer_by_id(id).await {
                Ok(_) => Ok(IpcResponse::ok_empty()),
                Err(e) => Ok(IpcResponse::error(e.to_string())),
            },
            Err(_) => Ok(IpcResponse::error("invalid device id")),
        },
        // ConnectManual: resolve hostname (may be a name, not a bare IP) then connect.
        IpcRequest::ConnectManual { host, port } => {
            use std::net::ToSocketAddrs;
            let default_port = state.settings.lock().await.get().port;
            let port = port.unwrap_or(default_port);
            let addr_str = format!("{}:{}", host, port);
            let ip = tokio::task::spawn_blocking(move || {
                addr_str
                    .to_socket_addrs()
                    .ok()
                    .and_then(|mut it| it.next())
                    .map(|a| a.ip().to_string())
            })
            .await
            .context("DNS spawn")?
            .context("could not resolve host")?;
            state.engine.connect_to_peer(ip, port).await?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::DisconnectPeer { device_id } => {
            state
                .engine
                .disconnect_peer(parse_uuid(&device_id)?)
                .await?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::TrustPeer { device_id } => {
            state.engine.trust_peer(parse_uuid(&device_id)?).await?;
            Ok(IpcResponse::ok_empty())
        }

        IpcRequest::RejectPeer { device_id } => {
            state.engine.reject_peer(parse_uuid(&device_id)?).await?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::SendPairingRequest { device_id } => {
            state
                .engine
                .send_pairing_request(parse_uuid(&device_id)?)
                .await;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::CancelPairingRequest { device_id } => {
            let _ = state
                .engine
                .set_outgoing_pairing_waiting(parse_uuid(&device_id)?, false);
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::RespondToPairing {
            device_id,
            accepted,
        } => {
            let _ = state
                .engine
                .respond_to_pairing(parse_uuid(&device_id)?, accepted)
                .await;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::GenerateQrToken => {
            let token = state.engine.generate_qr_token().await;
            Ok(IpcResponse::ok(serde_json::json!({ "token": token })))
        }
        IpcRequest::TrustPeerFromQr { device_id, token } => {
            let uuid = parse_uuid(&device_id)?;
            // SECURITY: Do NOT trust the peer here. Trust is granted only
            // after the engine's AppMessage::QrAuth handler validates the token.
            let _ = state.engine.send_qr_auth(uuid, token).await;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::RevokeTrustedDevice { device_id } => {
            state.engine.revoke_peer(parse_uuid(&device_id)?).await?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::RenameTrustedDevice {
            device_id,
            display_name,
        } => {
            state
                .engine
                .rename_trusted_device(parse_uuid(&device_id)?, display_name)
                .await?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::PauseSyncPeer { device_id } => {
            state
                .engine
                .pause_sync_peer(parse_uuid(&device_id)?)
                .await?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::ResumeSyncPeer { device_id } => {
            state
                .engine
                .resume_sync_peer(parse_uuid(&device_id)?)
                .await?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::ForgetDevice { device_id } => {
            state.engine.forget_device(parse_uuid(&device_id)?).await?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::SetAutoConnect { device_id, enabled } => {
            state
                .engine
                .set_auto_connect(parse_uuid(&device_id)?, enabled)
                .await?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::PushText { text } => Ok(IpcResponse::ok(
            dispatch_text(&state, text, SyncTarget::All).await?,
        )),
        IpcRequest::PushTextTo { text, target } => Ok(IpcResponse::ok(
            dispatch_text(&state, text, SyncTarget::Device(parse_uuid(&target)?)).await?,
        )),
        IpcRequest::PushImage { mime, data_base64 } => {
            let data = decode_base64(&data_base64)?;
            let content = ClipboardContent::Image {
                mime: mime.clone(),
                data,
            };
            remember_history(&state, &content, current_device_name(&state).await).await?;
            Ok(IpcResponse::ok(
                state
                    .engine
                    .push_clipboard_to(content, SyncTarget::All)
                    .await,
            ))
        }
        IpcRequest::PushFile { name, data_base64 } => {
            let data = decode_base64(&data_base64)?;
            let content = ClipboardContent::File {
                name: name.clone(),
                data,
            };
            remember_history(&state, &content, current_device_name(&state).await).await?;
            Ok(IpcResponse::ok(
                state
                    .engine
                    .push_clipboard_to(content, SyncTarget::All)
                    .await,
            ))
        }
        IpcRequest::RememberText { text } => {
            let content = ClipboardContent::Text(text);
            Ok(IpcResponse::ok(
                remember_history(&state, &content, current_device_name(&state).await).await?,
            ))
        }
        IpcRequest::History { last } => {
            let history = state.history.lock().await;
            Ok(IpcResponse::ok(
                history.recent(last).cloned().collect::<Vec<_>>(),
            ))
        }
        IpcRequest::HistorySearch { query, limit } => {
            let history = state.history.lock().await;
            Ok(IpcResponse::ok(
                history
                    .search(&query)
                    .take(limit)
                    .cloned()
                    .collect::<Vec<_>>(),
            ))
        }
        IpcRequest::HistorySearchFuzzy { query, limit } => {
            let history = state.history.lock().await;
            let scored = history.search_fuzzy(&query, limit);
            let json_items: Vec<_> = scored
                .into_iter()
                .map(|item| json!({ "score": item.score, "entry": item.entry }))
                .collect();
            Ok(IpcResponse::ok(json_items))
        }
        IpcRequest::HistoryPin { id, pinned } => {
            let mut history = state.history.lock().await;
            let entry = history
                .set_pinned(id, pinned)?
                .cloned()
                .context("history item not found")?;
            Ok(IpcResponse::ok(entry))
        }
        IpcRequest::HistoryDelete { id } => {
            state.history.lock().await.remove(id)?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::HistoryClear => {
            state.history.lock().await.clear()?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::HistoryRepush { id, target } => {
            let target = target
                .map(|value| parse_uuid(&value))
                .transpose()?
                .map(SyncTarget::Device)
                .unwrap_or(SyncTarget::All);
            let entry = {
                let history = state.history.lock().await;
                history.get(id).cloned().context("history item not found")?
            };
            let text = entry
                .repushable_text()
                .map(str::to_owned)
                .context("only text history items can be re-sent right now")?;
            Ok(IpcResponse::ok(
                state
                    .engine
                    .push_clipboard_to(ClipboardContent::Text(text), target)
                    .await,
            ))
        }
        IpcRequest::Feedback { last } => {
            let feedback = state.feedback.lock().await;
            Ok(IpcResponse::ok(
                feedback
                    .iter()
                    .rev()
                    .take(last)
                    .cloned()
                    .collect::<Vec<_>>(),
            ))
        }
        IpcRequest::IncomingClipboard { id } => {
            let payload = state
                .incoming_clipboards
                .lock()
                .await
                .get(&id)
                .cloned()
                .context("clipboard payload not found")?;
            Ok(IpcResponse::ok(payload))
        }
        IpcRequest::GetSettings => Ok(IpcResponse::ok(state.settings.lock().await.get().clone())),
        IpcRequest::PatchSettings { patch } => {
            let updated = {
                let mut store = state.settings.lock().await;
                store.patch(&patch)?;
                store.get().clone()
            };
            {
                let mut history = state.history.lock().await;
                history.set_max_entries(updated.effective_history_limit())?;
            }
            state.engine.apply_settings(updated).await;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::SetSyncEnabled { enabled } => {
            let updated = {
                let mut store = state.settings.lock().await;
                store.patch(&json!({ "sync_enabled": enabled }).to_string())?;
                store.get().clone()
            };
            state.engine.apply_settings(updated).await;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::ActivityRecent { limit } => {
            Ok(IpcResponse::ok(state.engine.activity_recent(limit).await))
        }
        IpcRequest::ActivitySince { since_id } => {
            Ok(IpcResponse::ok(state.engine.activity_since(since_id).await))
        }
        IpcRequest::PendingRemoteClipboards => Ok(IpcResponse::ok(
            state.engine.pending_remote_clipboards().await,
        )),
        IpcRequest::ApplyClipboard { content_hash } => {
            state.engine.apply_clipboard_by_hash(content_hash).await?;
            Ok(IpcResponse::ok_empty())
        }

        // Re-push a received clipboard item by hash (Mac "Send" button on feed row).
        IpcRequest::PushClipboardHash {
            hash,
            target_device_id,
        } => {
            let target = target_device_id
                .as_deref()
                .map(parse_uuid)
                .transpose()?
                .map(SyncTarget::Device)
                .unwrap_or(SyncTarget::All);
            state.engine.repush_clipboard_hash(hash, target).await?;
            Ok(IpcResponse::ok_empty())
        }

        // Push the current local clipboard to peers — daemon reads it from the OS.
        IpcRequest::PushClipboard { target_device_id } => {
            let target = target_device_id
                .as_deref()
                .map(parse_uuid)
                .transpose()?
                .map(SyncTarget::Device)
                .unwrap_or(SyncTarget::All);
            state.engine.push_current_clipboard(target).await?;
            Ok(IpcResponse::ok_empty())
        }

        // Persist a full settings snapshot from the Mac preferences UI.
        // Every non-None field is patched; unset fields are left unchanged.
        // Changes take effect immediately on the running engine — no restart needed.
        IpcRequest::SaveSettings {
            port,
            device_name,
            sync_enabled,
            sync_text,
            sync_images,
            sync_files,
            history_limit,
            max_history_text_bytes,
            max_payload_bytes,
            clipboard_poll_ms,
            max_pushes_per_sec,
            rate_limit_burst,
            smart_sync_duplicate_window_ms,
            smart_sync_debounce_ms,
            block_sensitive_text,
            require_tofu_confirmation,
            show_receive_notification,
            ignore_patterns,
        } => {
            let mut patch = serde_json::Map::new();
            macro_rules! maybe {
                ($key:expr, $val:expr) => {
                    if let Some(v) = $val {
                        patch.insert($key.into(), json!(v));
                    }
                };
            }
            maybe!("port", port);
            maybe!("device_name", device_name);
            maybe!("sync_enabled", sync_enabled);
            maybe!("sync_text", sync_text);
            maybe!("sync_images", sync_images);
            maybe!("sync_files", sync_files);
            maybe!("history_limit", history_limit);
            maybe!("max_history_text_bytes", max_history_text_bytes);
            maybe!("max_payload_bytes", max_payload_bytes);
            maybe!("clipboard_poll_ms", clipboard_poll_ms);
            maybe!("max_pushes_per_sec", max_pushes_per_sec);
            maybe!("rate_limit_burst", rate_limit_burst);
            maybe!(
                "smart_sync_duplicate_window_ms",
                smart_sync_duplicate_window_ms
            );
            maybe!("smart_sync_debounce_ms", smart_sync_debounce_ms);
            maybe!("block_sensitive_text", block_sensitive_text);
            maybe!("require_tofu_confirmation", require_tofu_confirmation);
            maybe!("show_receive_notification", show_receive_notification);
            maybe!("ignore_patterns", ignore_patterns);

            let patch_str = serde_json::to_string(&patch)?;
            let updated = {
                let mut store = state.settings.lock().await;
                store.patch(&patch_str)?;
                store.get().clone()
            };
            if let Some(lim) = history_limit {
                let mut history = state.history.lock().await;
                let _ = history.set_max_entries(lim);
            }
            state.engine.apply_settings(updated).await;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::SendFile {
            name,
            mime,
            data_base64,
            target_device,
        } => {
            let data = decode_base64(&data_base64)?;
            let transfer_id = state
                .engine
                .send_file(
                    data,
                    name,
                    mime,
                    target_device.as_deref().map(parse_uuid).transpose()?,
                )
                .await?;
            Ok(IpcResponse::ok(hex::encode(transfer_id)))
        }
        IpcRequest::SendFilePath {
            path,
            name,
            mime,
            target_device,
            batch_id,
            is_directory,
            item_count,
        } => {
            let transfer_id = state
                .engine
                .send_file_path(
                    std::path::PathBuf::from(path),
                    name,
                    mime,
                    target_device.as_deref().map(parse_uuid).transpose()?,
                    batch_id,
                    is_directory,
                    item_count,
                )
                .await?;
            Ok(IpcResponse::ok(hex::encode(transfer_id)))
        }
        IpcRequest::SendFolder {
            path,
            target_device,
        } => {
            let send = state
                .engine
                .send_folder(
                    std::path::PathBuf::from(path),
                    target_device.as_deref().map(parse_uuid).transpose()?,
                )
                .await?;
            Ok(IpcResponse::ok(json!(send)))
        }
        IpcRequest::CancelFolder { batch_id } => {
            state.engine.cancel_folder(&batch_id).await?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::AcceptFileTransfer { transfer_id } => {
            state
                .engine
                .accept_file_transfer(parse_transfer_id(&transfer_id)?)
                .await?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::RejectFileTransfer {
            transfer_id,
            reason,
        } => {
            state
                .engine
                .reject_file_transfer(parse_transfer_id(&transfer_id)?, reason)
                .await?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::CancelFileTransfer { transfer_id } => {
            state
                .engine
                .cancel_file_transfer(parse_transfer_id(&transfer_id)?)
                .await?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::PauseFileTransfer { transfer_id } => {
            state
                .engine
                .pause_file_transfer(parse_transfer_id(&transfer_id)?)
                .await?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::ResumeFileTransfer { transfer_id } => {
            state
                .engine
                .resume_file_transfer(parse_transfer_id(&transfer_id)?)
                .await?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::SetTimelineFirstMode { enabled } => {
            let updated = {
                let mut store = state.settings.lock().await;
                store.patch(&json!({ "timeline_first_mode": enabled }).to_string())?;
                store.get().clone()
            };
            state.engine.apply_settings(updated).await;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::SetAutoApplyClipboard { enabled } => {
            let updated = {
                let mut store = state.settings.lock().await;
                store.patch(&json!({ "auto_apply_remote_clipboard": enabled }).to_string())?;
                store.get().clone()
            };
            state.engine.apply_settings(updated).await;
            Ok(IpcResponse::ok_empty())
        }

        // ── History tag management ────────────────────────────────────────────
        IpcRequest::HistoryTag { id, tag } => {
            let added = state.history.lock().await.add_tag(id, &tag)?;
            Ok(IpcResponse::ok(serde_json::json!({ "added": added })))
        }
        IpcRequest::HistoryUntag { id, tag } => {
            let removed = state.history.lock().await.remove_tag(id, &tag)?;
            Ok(IpcResponse::ok(serde_json::json!({ "removed": removed })))
        }

        // ── History stats & JSON export ───────────────────────────────────────
        IpcRequest::HistoryStats => {
            let stats = state.history.lock().await.stats();
            Ok(IpcResponse::ok(stats))
        }
        IpcRequest::HistoryExportJson => {
            let json = state.history.lock().await.export_json()?;
            Ok(IpcResponse::Ok {
                data: serde_json::from_str(&json).ok(),
            })
        }

        // ── Filtered history ──────────────────────────────────────────────────
        IpcRequest::HistoryFilteredList {
            kind,
            device,
            from_secs,
            to_secs,
            tag,
            limit,
            pinned_only,
        } => {
            let filter = HistoryFilter {
                kind,
                device,
                from_secs,
                to_secs,
                tag,
                limit: Some(limit),
                pinned_only,
            };
            let history = state.history.lock().await;
            let entries: Vec<_> = history.filter(&filter).cloned().collect();
            Ok(IpcResponse::ok(entries))
        }

        // ── Clipboard templates ───────────────────────────────────────────────
        IpcRequest::TemplateList => {
            let templates = state
                .settings
                .lock()
                .await
                .get()
                .clipboard_templates
                .clone();
            Ok(IpcResponse::ok(templates))
        }
        IpcRequest::TemplatePush {
            name,
            target_device,
        } => {
            let templates = state
                .settings
                .lock()
                .await
                .get()
                .clipboard_templates
                .clone();
            let tmpl = templates
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(&name))
                .cloned()
                .with_context(|| format!("template '{}' not found", name))?;
            let target = target_device
                .as_deref()
                .map(parse_uuid)
                .transpose()?
                .map(SyncTarget::Device)
                .unwrap_or(SyncTarget::All);
            let content = ClipboardContent::Text(tmpl.text.clone());
            remember_history(&state, &content, current_device_name(&state).await).await?;
            Ok(IpcResponse::ok(
                state.engine.push_clipboard_to(content, target).await,
            ))
        }
        IpcRequest::TemplateSet {
            name,
            text,
            description,
        } => {
            let mut store = state.settings.lock().await;
            let settings = store.get_mut();
            if let Some(t) = settings
                .clipboard_templates
                .iter_mut()
                .find(|t| t.name.eq_ignore_ascii_case(&name))
            {
                t.text = text;
                t.description = description;
            } else {
                settings.clipboard_templates.push(ClipboardTemplate {
                    name,
                    text,
                    description,
                });
            }
            store.save()?;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::TemplateRemove { name } => {
            let mut store = state.settings.lock().await;
            let before = store.get().clipboard_templates.len();
            store
                .get_mut()
                .clipboard_templates
                .retain(|t| !t.name.eq_ignore_ascii_case(&name));
            let removed = store.get().clipboard_templates.len() != before;
            store.save()?;
            Ok(IpcResponse::ok(serde_json::json!({ "removed": removed })))
        }

        // ── Per-peer settings ─────────────────────────────────────────────────
        IpcRequest::GetPeerSettings { device_id } => {
            let store = state.settings.lock().await;
            let peer = store
                .get()
                .per_peer
                .get(&device_id)
                .cloned()
                .unwrap_or_default();
            Ok(IpcResponse::ok(peer))
        }
        IpcRequest::PatchPeerSettings { device_id, patch } => {
            let mut store = state.settings.lock().await;
            let peer = store.get_mut().per_peer.entry(device_id).or_default();
            // Apply partial JSON patch to PeerSettings.
            let mut current = serde_json::to_value(&*peer).context("serialising peer settings")?;
            let patch_val: serde_json::Value =
                serde_json::from_str(&patch).context("parsing peer settings patch")?;
            if let (Some(obj), Some(patch_obj)) = (current.as_object_mut(), patch_val.as_object()) {
                for (k, v) in patch_obj {
                    obj.insert(k.clone(), v.clone());
                }
            }
            *peer = serde_json::from_value(current).context("applying peer settings patch")?;
            store.save()?;
            Ok(IpcResponse::ok_empty())
        }

        // ── Runtime metrics (PER-07, PER-08) ─────────────────────────────────
        IpcRequest::GetMetrics => {
            let uptime_secs = state.started_at.elapsed().as_secs();
            let engine_peer_count = state.engine.connected_peer_count();
            let settings = state.settings.lock().await.get().clone();
            let history_count = state.history.lock().await.stats().total;

            let d = uptime_secs / 86400;
            let h = (uptime_secs % 86400) / 3600;
            let m = (uptime_secs % 3600) / 60;
            let s = uptime_secs % 60;
            let uptime_fmt = if d > 0 {
                format!("{}d {}h {}m {}s", d, h, m, s)
            } else if h > 0 {
                format!("{}h {}m {}s", h, m, s)
            } else {
                format!("{}m {}s", m, s)
            };

            Ok(IpcResponse::ok(json!({
                "uptime_secs": uptime_secs,
                "uptime": uptime_fmt,
                "connected_peers": engine_peer_count,
                "history_entries": history_count,
                "sync_enabled": settings.sync_enabled,
                "port": settings.port,
            })))
        }

        // ── History CSV export (HIS-06) ───────────────────────────────────────
        IpcRequest::HistoryExportCsv => {
            let csv = state.history.lock().await.export_csv();
            Ok(IpcResponse::ok(csv))
        }

        // ── Call continuity ─────────────────────────────────────────────────
        IpcRequest::CallAction {
            action,
            target_device,
        } => {
            state
                .engine
                .send_call_action(action, parse_uuid(&target_device)?)
                .await;
            Ok(IpcResponse::ok_empty())
        }
        // Android pushes its phone call state via IPC (daemon relays it to Mac).
        IpcRequest::PushCallState {
            state: call_state,
            number,
            contact_name,
        } => {
            state
                .engine
                .push_call_state(call_state, number, contact_name)
                .await;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::PushBatteryStatus { level, charging } => {
            state.engine.push_battery_status(level, charging).await;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::PushStorageStatus {
            images_bytes,
            videos_bytes,
            apps_bytes,
            free_bytes,
            total_bytes,
        } => {
            state
                .engine
                .push_storage_status(
                    images_bytes,
                    videos_bytes,
                    apps_bytes,
                    free_bytes,
                    total_bytes,
                )
                .await;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::RemoteFilesQuery {
            target_device,
            summary_only,
            category,
            source,
            search_query,
            offset,
            limit,
            timeout_secs,
        } => {
            let target_uuid = parse_uuid(&target_device)?;
            let cat = category
                .as_deref()
                .and_then(linkall_core::ipc::parse_remote_file_category);
            let src = source
                .as_deref()
                .and_then(linkall_core::ipc::parse_remote_file_source);
            let res = state
                .engine
                .query_remote_files_sync(
                    target_uuid,
                    summary_only,
                    cat,
                    src,
                    search_query,
                    offset,
                    limit,
                    timeout_secs.unwrap_or(10),
                )
                .await?;
            Ok(IpcResponse::ok(res))
        }
        IpcRequest::RemoteThumbnailRequest {
            target_device,
            file_id,
            size_px,
        } => {
            let target_uuid = parse_uuid(&target_device)?;
            let res = state
                .engine
                .request_remote_thumbnail_sync(target_uuid, file_id, size_px, 10)
                .await?;
            use base64::Engine as _;
            let base64_str = base64::engine::general_purpose::STANDARD.encode(&res.data);
            Ok(IpcResponse::ok(serde_json::json!({
                "file_id": res.file_id,
                "data_base64": base64_str,
                "error": res.error,
            })))
        }
        IpcRequest::RemoteFilePullRequest {
            target_device,
            file_id,
        } => {
            let target_uuid = parse_uuid(&target_device)?;
            let request_id = uuid::Uuid::new_v4();
            state
                .engine
                .send_remote_file_pull_request(target_uuid, request_id, file_id)
                .await;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::RemoteFileActionRequest {
            target_device,
            file_id,
            action,
            new_name,
        } => {
            let target_uuid = parse_uuid(&target_device)?;
            state
                .engine
                .send_remote_file_action_request(target_uuid, action, file_id, new_name)
                .await;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::OpenUrlOnDevice { target_device, url } => {
            let target_uuid = parse_uuid(&target_device)?;
            state.engine.open_url_on_device(target_uuid, url).await;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::AckOpenUrlOnDevice {
            requester_device,
            success,
            error,
        } => {
            let requester_uuid = parse_uuid(&requester_device)?;
            state
                .engine
                .ack_open_url_on_device(requester_uuid, success, error)
                .await;
            Ok(IpcResponse::ok_empty())
        }
        IpcRequest::StartSpeedTest {
            device_id,
            duration_secs,
        } => {
            let target_uuid = parse_uuid(&device_id)?;
            state
                .engine
                .start_speed_test(target_uuid, duration_secs)
                .await?;
            Ok(IpcResponse::ok_empty())
        }

        IpcRequest::Shutdown => {
            state.shutdown.notify_waiters();
            Ok(IpcResponse::ok_empty())
        }
    }
}

async fn dispatch_text(
    state: &DaemonState,
    text: String,
    target: SyncTarget,
) -> Result<SyncDispatchReport> {
    let content = ClipboardContent::Text(text);
    remember_history(state, &content, current_device_name(state).await).await?;
    Ok(state.engine.push_clipboard_to(content, target).await)
}

async fn remember_history(
    state: &DaemonState,
    content: &ClipboardContent,
    source_device: String,
) -> Result<HistoryEntry> {
    let settings = state.settings.lock().await.get().clone();
    let mut history = state.history.lock().await;
    history
        .push_with_options(content, source_device, settings.max_history_text_bytes)
        .cloned()
}

async fn current_device_name(state: &DaemonState) -> String {
    state.settings.lock().await.get().resolved_device_name()
}

async fn push_feedback(state: &DaemonState, event: FeedbackEvent) {
    let mut feedback = state.feedback.lock().await;
    feedback.push_back(event);
    while feedback.len() > MAX_FEEDBACK_EVENTS {
        feedback.pop_front();
    }
}

async fn store_incoming_clipboard(state: &DaemonState, id: u64, payload: serde_json::Value) {
    state.incoming_clipboards.lock().await.insert(id, payload);
    let mut order = state.incoming_order.lock().await;
    order.push_back(id);
    while order.len() > MAX_INCOMING_CLIPBOARDS {
        if let Some(oldest) = order.pop_front() {
            state.incoming_clipboards.lock().await.remove(&oldest);
        }
    }
}

fn incoming_payload_json(id: u64, content: &ClipboardContent) -> serde_json::Value {
    match content {
        ClipboardContent::Text(text) => json!({
            "id": id,
            "type": "text",
            "text": text,
        }),
        ClipboardContent::Image { mime, data } => json!({
            "id": id,
            "type": "image",
            "mime": mime,
            "data_base64": base64::engine::general_purpose::STANDARD.encode(data),
        }),
        ClipboardContent::File { name, data } => json!({
            "id": id,
            "type": "file",
            "name": name,
            "data_base64": base64::engine::general_purpose::STANDARD.encode(data),
        }),
    }
}

fn parse_uuid(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value).with_context(|| format!("invalid UUID: {value}"))
}

fn decode_base64(value: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|error| anyhow!("invalid base64 payload: {error}"))
}

fn parse_transfer_id(value: &str) -> Result<[u8; 16]> {
    let bytes = hex::decode(value).with_context(|| format!("invalid transfer id: {value}"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow!("transfer id must be 16 bytes"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
