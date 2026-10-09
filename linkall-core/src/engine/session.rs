//! A live peer session: registration, the reader/writer loop, and
//! dispatch of every inbound AppMessage.

use super::*;

// Inbound message handlers, one module per area. `dispatch_inbound` routes
// each message to its area; the match there is exhaustive, so a new
// AppMessage variant fails to compile until it is routed somewhere.
mod clipboard;
mod device;
mod link;
mod pairing;
mod remote;
mod speed_test;
mod transfers;

/// Work handed from the reader to the session's disk writer, so decrypting
/// the next chunk never waits on a file write.
pub(super) enum DiskTaskMsg {
    Chunk {
        transfer_id: [u8; 16],
        chunk_index: u32,
        offset: u64,
        padding: usize,
        data: Vec<u8>,
    },
    Complete {
        transfer_id: [u8; 16],
        sha256_checksum: String,
    },
}

/// What an inbound message handler can see of its session.
pub(super) struct InboundCtx {
    pub(super) shared: EngineShared,
    pub(super) peer_id: Uuid,
    pub(super) peer_name: String,
    /// Queue for replies to this peer (the session writer drains it).
    pub(super) outbox_tx: mpsc::Sender<AppMessage>,
    pub(super) session_pin: Option<String>,
    pub(super) ping_sent_at: Arc<std::sync::Mutex<Option<Instant>>>,
    pub(super) peer_sleeping: Arc<std::sync::atomic::AtomicBool>,
    pub(super) disk_tx: mpsc::Sender<DiskTaskMsg>,
    pub(super) endpoint: SocketAddr,
    /// This connection's id in the peer manager.
    pub(super) session_id: u64,
    last_seen: Arc<std::sync::atomic::AtomicU64>,
}

impl InboundCtx {
    /// Any traffic proves the peer is alive; the heartbeat reads this.
    pub(super) fn touch_last_seen(&self) {
        self.last_seen.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
}

/// Whether the session keeps reading after a message.
pub(super) enum Flow {
    Continue,
    Disconnect(String),
}

async fn dispatch_inbound(ctx: &InboundCtx, msg: AppMessage) -> Flow {
    use AppMessage as M;
    match msg {
        m @ (M::ClipboardPush { .. } | M::HistoryMetadata { .. } | M::ClipboardAck { .. }) => {
            clipboard::handle(ctx, m).await
        }
        m @ (M::FileTransferAnnounce { .. }
        | M::FileTransferAccept { .. }
        | M::FileChunk { .. }
        | M::FileChunkAck { .. }
        | M::FileTransferComplete { .. }
        | M::FileTransferCompleteAck { .. }
        | M::FileTransferCancel { .. }
        | M::FileTransferPause { .. }
        | M::FileTransferResume { .. }) => transfers::handle(ctx, m).await,
        m @ (M::SpeedTestRequest { .. }
        | M::SpeedTestResponse { .. }
        | M::SpeedTestData { .. }
        | M::SpeedTestStats { .. }
        | M::SpeedTestComplete { .. }) => speed_test::handle(ctx, m).await,
        m @ (M::DeviceSleepState { .. }
        | M::DeviceSyncState { .. }
        | M::KeyRotated { .. }
        | M::Ping { .. }
        | M::Pong { .. }
        | M::Bye
        | M::PermissionError { .. }) => link::handle(ctx, m).await,
        m @ (M::PairingRequest { .. } | M::PairingResponse { .. } | M::QrAuth { .. }) => {
            pairing::handle(ctx, m).await
        }
        m @ (M::CallStateUpdate { .. }
        | M::BatteryStatus { .. }
        | M::NetworkStatus { .. }
        | M::StorageStatus { .. }
        | M::CallAction { .. }
        | M::NotificationRelay { .. }) => device::handle(ctx, m).await,
        m @ (M::RemoteFilesQuery { .. }
        | M::RemoteFilesResponse { .. }
        | M::RemoteThumbnailRequest { .. }
        | M::RemoteThumbnailResponse { .. }
        | M::RemoteFilePullRequest { .. }
        | M::RemoteFileActionRequest { .. }
        | M::OpenUrlOnDevice { .. }
        | M::OpenUrlOnDeviceAck { .. }) => remote::handle(ctx, m).await,
        // Handshake messages; once the session is up they carry nothing.
        M::Hello { .. } | M::HelloAck { .. } => Flow::Continue,
        // Sent only by older builds that had the camera stream.
        M::RetiredCameraStreamRequest { .. }
        | M::RetiredCameraStreamAccept { .. }
        | M::RetiredCameraStreamStop { .. }
        | M::RetiredCameraFrame { .. } => Flow::Continue,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn register_session(
    shared: EngineShared,
    stream: TcpStream,
    endpoint: SocketAddr,
    peer_id: Uuid,
    peer_name: String,
    session: crate::crypto::SessionKey,
    trusted: bool,
    peer_trusts_us: Option<bool>,
    discovery: DiscoverySource,
    session_pin: Option<String>,
    is_outbound: bool,
) -> Result<()> {
    // File chunks get their own small queue: 8 × 4 MB = 32 MB waiting to be
    // encrypted, on top of the kernel socket buffer. That is enough to keep
    // any LAN link busy, and the backpressure stops the reader from racing
    // ahead of the network and pinning hundreds of MB on a phone.
    let (outbox_tx, mut outbox_rx) = mpsc::channel::<AppMessage>(64);
    let (file_outbox_tx, mut file_outbox_rx) = mpsc::channel::<AppMessage>(8);
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<SessionShutdown>();
    match shared
        .peer_manager
        .upsert_peer(peer_id, peer_name.clone(), endpoint, trusted, discovery)
    {
        Ok(_) => {}
        Err(e) => {
            warn!("peer discovery connect failed error={:?}", e);
            return Err(e);
        }
    }
    let (session_id, replaced, rejected_new) = shared.peer_manager.replace_live_session(
        shared.config.device_id,
        peer_id,
        is_outbound,
        endpoint,
        outbox_tx.clone(),
        file_outbox_tx.clone(),
        shutdown_tx,
    )?;

    if rejected_new {
        tracing::debug!(
            "Session with {} rejected by dedup (we already have the winning session)",
            peer_id
        );
        return Ok(());
    }

    // The PIN is derived per handshake, and a simultaneous dial runs two
    // handshakes with different PINs. Recording it only for the session
    // that won the tie-break keeps both devices showing the same code;
    // writing it before dedup let each side keep a different loser's PIN.
    if let Some(pin) = session_pin.clone() {
        let _ = shared
            .peer_manager
            .set_pairing_pin(peer_id, Some(pin.clone()));
        if let Some(they_trust_us) = peer_trusts_us {
            reconcile_one_sided_trust(&shared, peer_id, &peer_name, trusted, they_trust_us, pin);
        }
    }

    let was_connected = replaced.is_some();
    if let Some(replaced) = replaced {
        if let Some(old_shutdown) = replaced.shutdown_tx {
            let _ = old_shutdown.send(SessionShutdown {
                reason: format!("session migrated to {}", endpoint),
                send_bye: false,
                explicit_disconnect: false,
            });
        }
    }

    // A session swap (duplicate-dial tie-break, or a fresh stream replacing
    // a stale one) is invisible to the user: the peer never went offline.
    if !was_connected {
        let _ = shared.event_tx.try_send(EngineEvent::PeerConnected {
            device_id: peer_id,
            device_name: peer_name.clone(),
            addr: endpoint,
            trusted,
        });

        let feed = shared.activity.clone();
        let name = peer_name.clone();
        tokio::spawn(async move {
            feed.lock().await.record_peer_connected(peer_id, name);
        });
    }

    // The user asked to pair before this session existed (or on a session
    // that has since been replaced), or reconcile_one_sided_trust found the
    // peer no longer trusts us: deliver the request now.
    // The code is per session, so tell the UI the one this request carries:
    // if the request was sent before any session existed, the code it showed
    // was a placeholder.
    if let Some(peer) = shared
        .peer_manager
        .get(peer_id)
        .filter(|p| p.outgoing_pairing_waiting)
    {
        let _ = outbox_tx.try_send(pairing_request_message(&shared, peer_id));
        if let Some(pin) = peer.pairing_pin {
            let _ = shared
                .event_tx
                .try_send(EngineEvent::OutgoingPairingWaiting {
                    device_id: peer_id,
                    device_name: peer_name.clone(),
                    pin,
                });
        }
    }

    // A peer's "clipboard sharing off" notice only lives as long as the
    // session it arrived on: it isn't resent on its own, so a stale flag from
    // an old session would block broadcasts to it forever. Reset here; a
    // peer that has sharing off re-announces it right below.
    let _ = shared.peer_manager.set_remote_sync_enabled(peer_id, true);

    // Push local battery and network status to the newly connected peer if trusted.
    if trusted {
        let outbox = outbox_tx.clone();
        let sh = shared.clone();
        tokio::spawn(async move {
            let sharing_on = sh.settings.lock().unwrap().sync_enabled;
            if !sharing_on {
                let _ = outbox
                    .send(AppMessage::DeviceSyncState { enabled: false })
                    .await;
            }
            let battery_val = *sh.device_status.local_battery.lock().unwrap();
            if let Some((level, charging)) = battery_val {
                let _ = outbox
                    .send(AppMessage::BatteryStatus {
                        level,
                        charging,
                        origin_device: sh.config.device_id,
                        origin_device_name: sh.config.device_name.clone(),
                    })
                    .await;
            }
            let net_val = sh.device_status.local_network.lock().unwrap().clone();
            if let Some(net) = net_val {
                let _ = outbox
                    .send(AppMessage::NetworkStatus {
                        network_type: net,
                        origin_device: sh.config.device_id,
                        origin_device_name: sh.config.device_name.clone(),
                    })
                    .await;
            }
            let storage_val = *sh.device_status.local_storage.lock().unwrap();
            if let Some((images, videos, apps, free, total)) = storage_val {
                let _ = outbox
                    .send(AppMessage::StorageStatus {
                        images_bytes: images,
                        videos_bytes: videos,
                        apps_bytes: apps,
                        free_bytes: free,
                        total_bytes: total,
                        origin_device: sh.config.device_id,
                        origin_device_name: sh.config.device_name.clone(),
                    })
                    .await;
            }
        });
    }

    // If this peer is reconnecting, re-announce any unfinished outbound
    // transfers so the receiver can respond with resume_from_chunk.
    {
        let pending_shared = shared.clone();
        let pending_outbox = outbox_tx.clone();
        tokio::spawn(async move {
            let pending = pending_shared
                .file_transfers
                .lock()
                .await
                .pending_outbound_announcements_for(peer_id);
            for meta in pending {
                if pending_outbox
                    .send(AppMessage::FileTransferAnnounce { meta })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    // MED-02: wrap the session task in a JoinHandle watcher so that panics
    // are logged rather than silently swallowed by the Tokio runtime.
    let panic_peer_name = peer_name.clone();
    let session_outbox_tx = outbox_tx.clone();
    let session_handle = tokio::spawn(async move {
        let (mut sess_tx, mut sess_rx) = PeerSession {
            stream,
            session,
            peer_device_id: peer_id,
            peer_device_name: peer_name.clone(),
        }
        .split();
        let mut heartbeat = tokio::time::interval(shared.config.heartbeat_interval);
        // After a suspend, the default Burst behaviour fires every missed
        // tick back to back - a pointless ping storm on wake.
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let last_seen = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        ));
        let ping_sent_at = std::sync::Arc::new(std::sync::Mutex::new(None::<std::time::Instant>));
        let peer_sleeping = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let rx_last_seen = last_seen.clone();
        let rx_ping_sent_at = ping_sent_at.clone();
        let rx_peer_sleeping = peer_sleeping.clone();
        let rx_shared = shared.clone();
        let rx_peer_name = peer_name.clone();

        let rx_peer_id = peer_id;
        let rx_session_pin = session_pin.clone();

        let (disk_tx, mut disk_rx) = tokio::sync::mpsc::channel::<DiskTaskMsg>(8);
        let mut last_disk_prog_emit: std::collections::HashMap<[u8; 16], std::time::Instant> =
            std::collections::HashMap::new();

        let dw_shared = shared.clone();
        let dw_event_tx = shared.event_tx.clone();
        let dw_outbox_tx = session_outbox_tx.clone();
        let dw_peer_id = peer_id;
        let dw_peer_name = peer_name.clone();
        let disk_writer_task = tokio::spawn(async move {
            while let Some(msg) = disk_rx.recv().await {
                match msg {
                    DiskTaskMsg::Chunk {
                        transfer_id,
                        chunk_index,
                        offset,
                        padding,
                        data,
                    } => {
                        let io_ctx = {
                            let mut mgr = dw_shared.file_transfers.lock().await;
                            if let Some(t) = mgr.get_inbound_mut(&transfer_id) {
                                t.take_io_context()
                            } else {
                                None
                            }
                        };

                        if let Some((mut file, mut hasher, last_offset)) = io_ctx {
                            let data_len = data.len();
                            let res = tokio::task::spawn_blocking(move || {
                                use sha2::Digest;
                                use std::io::{Seek, SeekFrom, Write};

                                if last_offset != offset {
                                    if let Err(e) = file.seek(SeekFrom::Start(offset)) {
                                        return Err(anyhow::anyhow!("seek error: {}", e));
                                    }
                                }
                                if let Err(e) = file.write_all(&data) {
                                    return Err(anyhow::anyhow!("write error: {}", e));
                                }
                                hasher.update(&data);
                                if padding > 0 {
                                    hasher.update(vec![0u8; padding]);
                                }
                                let new_offset = offset + data.len() as u64;
                                Ok::<_, anyhow::Error>((file, hasher, new_offset))
                            })
                            .await
                            .unwrap();

                            match res {
                                Ok((file, hasher, new_offset)) => {
                                    let mut mgr = dw_shared.file_transfers.lock().await;
                                    if let Some(t) = mgr.get_inbound_mut(&transfer_id) {
                                        if t.status
                                            != crate::file_transfer::TransferStatus::Transferring
                                        {
                                            tracing::debug!(
                                                "discarding disk write for non-active transfer {:?} (status: {:?})",
                                                transfer_id,
                                                t.status
                                            );
                                            continue;
                                        }
                                        t.restore_io_context(file, hasher, new_offset);
                                        let prog = t.commit_chunk(chunk_index, data_len);
                                        let should_ack = t.should_ack();
                                        let file_name = t.meta.file_name.clone();
                                        drop(mgr);

                                        let now = std::time::Instant::now();
                                        let last = last_disk_prog_emit
                                            .get(&transfer_id)
                                            .copied()
                                            .unwrap_or_else(|| {
                                                now.checked_sub(std::time::Duration::from_secs(1))
                                                    .unwrap()
                                            });
                                        if now.duration_since(last).as_millis() >= 100
                                            || prog.percent == 100
                                        {
                                            last_disk_prog_emit.insert(transfer_id, now);
                                            let _ = dw_event_tx.try_send(
                                                EngineEvent::FileTransferProgress {
                                                    transfer_id,
                                                    from_device: dw_peer_id,
                                                    file_name,
                                                    percent: prog.percent,
                                                    bytes_received: prog.bytes_received,
                                                    total_bytes: prog.total_bytes,
                                                    speed_bps: prog.speed_bps,
                                                    eta_secs: prog.eta_secs,
                                                    outbound: false,
                                                },
                                            );
                                        }

                                        if should_ack {
                                            let _ = dw_outbox_tx
                                                .send(AppMessage::FileChunkAck {
                                                    transfer_id,
                                                    last_confirmed_chunk: chunk_index,
                                                })
                                                .await;
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Disk I/O error: {}", e);
                                    let mut mgr = dw_shared.file_transfers.lock().await;
                                    mgr.cancel_inbound(&transfer_id, "disk i/o error");
                                }
                            }
                        } else {
                            tracing::error!(
                                "Missing io_ctx for chunk {} of transfer {:?}",
                                chunk_index,
                                transfer_id
                            );
                        }
                    }
                    DiskTaskMsg::Complete {
                        transfer_id,
                        sha256_checksum,
                    } => {
                        let file_handle = {
                            let mut mgr = dw_shared.file_transfers.lock().await;
                            mgr.get_inbound_mut(&transfer_id)
                                .and_then(|t| t.file_handle.take())
                        };

                        if let Some(mut file) = file_handle {
                            let _ = tokio::task::spawn_blocking(move || {
                                use std::io::Write;
                                let _ = file.flush();
                                let _ = file.get_ref().sync_all();
                            })
                            .await;
                        }

                        let result = {
                            let mut mgr = dw_shared.file_transfers.lock().await;
                            if let Some(transfer) = mgr.get_inbound_mut(&transfer_id) {
                                let file_name = transfer.meta.file_name.clone();
                                let file_bytes = transfer.meta.size_bytes;
                                match transfer.finalize(sha256_checksum.clone()) {
                                    Ok(dest) => Ok((dest, file_name, file_bytes)),
                                    Err(e) => Err((e.to_string(), Some(file_name))),
                                }
                            } else {
                                Err(("transfer not found".to_string(), None))
                            }
                        };
                        match result {
                            Ok((dest, file_name, file_bytes)) => {
                                dw_shared
                                    .file_transfers
                                    .lock()
                                    .await
                                    .remove_inbound(&transfer_id);
                                let hex_tid = hex::encode(transfer_id);
                                let dest_path_str = dest.to_string_lossy().to_string();
                                dw_shared
                                    .activity
                                    .lock()
                                    .await
                                    .record_file_transfer_complete(
                                        dw_peer_id,
                                        dw_peer_name.clone(),
                                        file_name.clone(),
                                        file_bytes,
                                        hex_tid,
                                        Some(dest_path_str),
                                    );
                                let _ = dw_outbox_tx
                                    .send(AppMessage::FileTransferCompleteAck {
                                        transfer_id,
                                        success: true,
                                        error: None,
                                    })
                                    .await;
                                let _ = dw_event_tx
                                    .send(EngineEvent::FileTransferComplete {
                                        transfer_id,
                                        from_device: dw_peer_id,
                                        from_name: dw_peer_name.clone(),
                                        file_name,
                                        dest_path: dest,
                                    })
                                    .await;
                            }
                            Err((e, failed_name)) => {
                                // Drop it and its partial file. Marking it failed
                                // and keeping it left it in the active list, and
                                // the desktop apps showed it in progress for good.
                                dw_shared
                                    .file_transfers
                                    .lock()
                                    .await
                                    .cancel_inbound(&transfer_id, &e);
                                let hex_tid = hex::encode(transfer_id);
                                dw_shared.activity.lock().await.record_file_transfer_failed(
                                    dw_peer_id,
                                    dw_peer_name.clone(),
                                    failed_name,
                                    hex_tid,
                                    e.clone(),
                                );
                                let _ = dw_outbox_tx
                                    .send(AppMessage::FileTransferCompleteAck {
                                        transfer_id,
                                        success: false,
                                        error: Some(e.clone()),
                                    })
                                    .await;
                                let _ = dw_event_tx
                                    .send(EngineEvent::FileTransferFailed {
                                        in_folder: false,
                                        transfer_id,
                                        from_device: dw_peer_id,
                                        reason: e,
                                    })
                                    .await;
                            }
                        }
                        pump_transfer_queue(&dw_shared).await;
                    }
                }
            }
        });

        let ctx = InboundCtx {
            shared: rx_shared,
            peer_id: rx_peer_id,
            peer_name: rx_peer_name,
            outbox_tx: session_outbox_tx,
            endpoint,
            session_id,
            session_pin: rx_session_pin,
            ping_sent_at: rx_ping_sent_at,
            peer_sleeping: rx_peer_sleeping,
            disk_tx,
            last_seen: rx_last_seen,
        };
        let mut rx_task = tokio::spawn(async move {
            loop {
                let msg = match sess_rx.recv().await {
                    Ok(msg) => msg,
                    Err(err) => break err.to_string(),
                };
                if let Flow::Disconnect(reason) = dispatch_inbound(&ctx, msg).await {
                    break reason;
                }
            }
        });

        let mut last_tick_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let disconnect_reason = loop {
            let now_millis = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let last_seen_millis = last_seen.load(std::sync::atomic::Ordering::Relaxed);

            tokio::select! {
                biased;
                shutdown = &mut shutdown_rx => {
                    match shutdown {
                        Ok(cmd) => {
                            if cmd.explicit_disconnect {
                                let _ = sess_tx.send(&mut AppMessage::CallAction {
                                    action: "system:explicit_disconnect".to_string(),
                                    origin_device: shared.config.device_id,
                                }).await;
                            }
                            if cmd.send_bye {
                                let _ = sess_tx.send(&mut AppMessage::Bye).await;
                            }
                            break cmd.reason;
                        }
                        Err(_) => {
                            break "session shutdown channel dropped".to_string();
                        }
                    }
                }
                _ = heartbeat.tick() => {
                    let tick_delta = now_millis.saturating_sub(last_tick_millis);
                    last_tick_millis = now_millis;

                    // If this tokio interval tick took significantly longer than expected (e.g. > 20s),
                    // the OS almost certainly suspended our CPU (Doze mode or sleep).
                    // We implicitly grant ourselves a fresh 15-second grace period.
                    if tick_delta > (shared.config.heartbeat_interval.as_millis() as u64) + 5000 {
                        shared.local_last_wake.store(now_millis, std::sync::atomic::Ordering::Relaxed);
                    }

                    // Either side being asleep relaxes the heartbeat: the
                    // sleeping side pings rarely, and the awake side was told
                    // (DeviceSleepState) to do the same, so neither may time
                    // the other out on silence. Dead links still surface as
                    // TCP errors via keepalive.
                    let is_sleeping = peer_sleeping.load(std::sync::atomic::Ordering::Relaxed)
                        || shared.local_sleeping.load(std::sync::atomic::Ordering::Relaxed);
                    let timeout = if is_sleeping {
                        // 24 hours timeout if peer is sleeping
                        24 * 60 * 60 * 1000
                    } else {
                        shared.config.heartbeat_timeout.as_millis() as u64
                    };

                    let time_since_seen = now_millis.saturating_sub(last_seen_millis);
                    let time_since_wake = now_millis.saturating_sub(shared.local_last_wake.load(std::sync::atomic::Ordering::Relaxed));

                    // Only timeout if we haven't seen a heartbeat in `timeout` ms AND
                    // we've been awake for at least `timeout` ms.
                    // This prevents us from disconnecting immediately when our own CPU wakes from deep sleep.
                    if time_since_seen > timeout && time_since_wake > timeout {
                        break format!("heartbeat timeout (sleeping: {is_sleeping}, time_since_seen: {time_since_seen}, time_since_wake: {time_since_wake})");
                    }

                    shared.file_transfers.lock().await.prune_stale_transfers();

                    // Only send a ping if awake, OR if asleep and we haven't seen them for 5 minutes
                    let should_ping = if !is_sleeping {
                        true
                    } else {
                        let last_ping_elapsed = ping_sent_at.lock().unwrap_or_else(|e| e.into_inner()).map(|i| i.elapsed().as_millis() as u64).unwrap_or(u64::MAX);
                        last_ping_elapsed > 5 * 60 * 1000
                    };

                    if should_ping {
                        let mut ping = probe::make_ping();
                        *ping_sent_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
                        if let Err(err) = sess_tx.send(&mut ping).await {
                            break format!("heartbeat send failed: {err}");
                        }
                    }
                }
                Some(mut msg) = outbox_rx.recv() => {
                    if let Err(err) = sess_tx.send(&mut msg).await {
                        break format!("send failed: {err}");
                    }
                }
                Some(mut msg) = file_outbox_rx.recv() => {
                    if let Err(err) = sess_tx.send_no_flush(&mut msg).await {
                        break format!("send failed: {err}");
                    }
                    // Write a few queued chunks per flush, then go back to the
                    // select so pings, acks and clipboard never wait behind
                    // a long run of file data.
                    for _ in 0..3 {
                        match file_outbox_rx.try_recv() {
                            Ok(mut next_msg) => {
                                if let Err(_err) = sess_tx.send_no_flush(&mut next_msg).await {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    if let Err(err) = sess_tx.flush().await {
                        break format!("flush failed: {err}");
                    }
                }
                rx_res = &mut rx_task => {
                    match rx_res {
                        Ok(reason) => break reason,
                        Err(_) => break "rx task panicked".to_string(),
                    }
                }
            }
        };

        rx_task.abort();
        disk_writer_task.abort();
        let reason = Some(disconnect_reason);
        match shared
            .peer_manager
            .mark_disconnected_if_current(peer_id, session_id, reason.clone())
        {
            Ok(Some(connected_at)) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let duration = now.saturating_sub(connected_at);
                if duration < 15 {
                    let _ = shared.event_tx.send(EngineEvent::Warning(
                        format!("Device '{}' disconnected rapidly ({}s). If this is an Android device, please ensure 'Ignore Battery Optimizations' (Background Execution) is enabled in its settings.", peer_name, duration)
                    )).await;
                }

                tracing::warn!(
                    "peer disconnected: peer_id={}, reason={:?}",
                    peer_id,
                    reason
                );
                let _ = shared
                    .event_tx
                    .send(EngineEvent::PeerDisconnected {
                        device_id: peer_id,
                        device_name: Some(peer_name.clone()),
                        reason: reason.clone(),
                    })
                    .await;

                shared.dedup.lock().await.remove_peer(peer_id);
                // A call this peer was relaying ends with it: its "idle" can
                // no longer arrive.
                super::telemetry::clear_call_from(&shared, peer_id).await;

                {
                    // A reconnect can register a new session and resume the
                    // transfers in the awaits since mark_disconnected_if_current;
                    // pausing then would strand the resumed transfer. Checking
                    // under the transfers lock orders this against the resume,
                    // which takes the same lock.
                    let mut transfers = shared.file_transfers.lock().await;
                    if shared.peer_manager.sender(peer_id).is_none() {
                        transfers.pause_all_for_device(peer_id);
                    }
                }
                pump_transfer_queue(&shared).await;

                // Drain pending remote file waiters and notify oneshot receivers with error fast-path
                drain_remote_waiters(&shared, peer_id).await;

                // FIX: Phantom Pairing Prompts. The code shown in a prompt
                // belongs to this session, so the prompt closes with it; the
                // requester resends (with the new session's code) once it is
                // back. Our own outgoing request survives and is resent then.
                // Skipped if a new session already registered in the awaits
                // above: its code and any request it carried are current.
                if shared.peer_manager.sender(peer_id).is_none() {
                    let was_asked = shared
                        .peer_manager
                        .get(peer_id)
                        .is_some_and(|p| p.pairing_requested);
                    let _ = shared.peer_manager.set_pairing_requested(peer_id, false);
                    let _ = shared.peer_manager.set_pairing_pin(peer_id, None);
                    if was_asked {
                        notify_pairing_changed(&shared, peer_id).await;
                    }
                }

                // Record in activity feed.
                let feed = shared.activity.clone();
                let name = peer_name.clone();
                let disc_reason = reason.clone();
                tokio::spawn(async move {
                    feed.lock()
                        .await
                        .record_peer_disconnected(peer_id, name, disc_reason);
                });

                if shared
                    .peer_manager
                    .get(peer_id)
                    .map(|peer| peer.should_auto_reconnect())
                    .unwrap_or(false)
                {
                    // ── AirDrop-style immediate reconnect ────────────────────
                    // Instead of waiting for the 3s auto-reconnector tick,
                    // spawn an immediate reconnect attempt after a short
                    // anti-loop delay. This cuts reconnect latency from ~3s
                    // to ~500ms for trusted peers.
                    let shared_reconnect = shared.clone();
                    let peer_endpoints = shared
                        .peer_manager
                        .get(peer_id)
                        .map(|p| p.socket_addrs())
                        .unwrap_or_default();
                    let peer_discovery = shared
                        .peer_manager
                        .get(peer_id)
                        .map(|p| p.discovery)
                        .unwrap_or(DiscoverySource::Unknown);
                    if !peer_endpoints.is_empty() {
                        tokio::spawn(async move {
                            // Small delay to prevent 0-delay infinite loops
                            // if the remote immediately resets.
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            // Only attempt if still disconnected (auto-reconnector
                            // may have already picked it up).
                            let still_offline = shared_reconnect
                                .peer_manager
                                .get(peer_id)
                                .map(|p| {
                                    p.status
                                        == crate::peer_manager::PeerConnectionState::Disconnected
                                        || p.status
                                            == crate::peer_manager::PeerConnectionState::Failed
                                })
                                .unwrap_or(false);
                            if still_offline {
                                tracing::debug!(
                                    peer_id = %peer_id,
                                    "immediate reconnect: attempting fast recovery"
                                );
                                let _ = connect_once(
                                    shared_reconnect,
                                    peer_endpoints,
                                    Some(peer_id),
                                    peer_discovery,
                                    false,
                                )
                                .await;
                            }
                        });
                    }
                }
            }
            Ok(None) => {
                drain_remote_waiters(&shared, peer_id).await;
            }
            Err(err) => {
                warn!(peer_id = %peer_id, error = %err, "failed to mark peer disconnected");
                drain_remote_waiters(&shared, peer_id).await;
            }
        }
    });

    // MED-02: observe the session task handle so panics surface as log errors
    // instead of being silently discarded by the Tokio runtime.
    tokio::spawn(async move {
        if let Err(panic) = session_handle.await {
            error!(
                peer_id = %peer_id,
                peer_name = %panic_peer_name,
                error = ?panic,
                "peer session task panicked — peer will appear disconnected"
            );
        }
    });

    Ok(())
}
