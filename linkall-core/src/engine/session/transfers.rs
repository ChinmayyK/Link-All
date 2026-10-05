//! Inbound file-transfer messages: announce, accept, chunks and acks,
//! completion, cancel, pause, and resume.

use super::*;

pub(super) async fn handle(ctx: &InboundCtx, msg: AppMessage) -> Flow {
    let shared = &ctx.shared;
    let peer_id = ctx.peer_id;
    let peer_name = &ctx.peer_name;
    match msg {
        AppMessage::FileTransferAnnounce { meta } => {
            ctx.touch_last_seen();
            if !shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false)
            {
                let _ = ctx
                    .outbox_tx
                    .send(AppMessage::FileTransferCancel {
                        transfer_id: meta.transfer_id,
                        reason: "Device not trusted (Accept pairing request first)".to_string(),
                    })
                    .await;
                let _ = shared
                    .event_tx
                    .send(EngineEvent::Warning(format!(
                        "ignoring file transfer from untrusted peer {}",
                        peer_name
                    )))
                    .await;
                return Flow::Continue;
            }
            let transfer_id = meta.transfer_id;
            let file_name = meta.file_name.clone();
            let file_bytes = meta.size_bytes;
            let mime_type = meta.mime_type.clone();
            let folder = crate::engine::folder_ops::incoming_folder(&meta);

            // Register inbound transfer.
            let reg_result = shared
                .file_transfers
                .lock()
                .await
                .register_inbound(meta, peer_id, peer_name.clone())
                .map(|t| t.dest_path.is_some());
            // Already accepted once: this is the sender resuming
            // after a reconnect, so continue without asking again.
            let resuming = matches!(reg_result, Ok(true));
            if let Err(e) = reg_result {
                tracing::warn!(error = %e, "rejected file transfer announce");
                let _ = ctx
                    .outbox_tx
                    .send(AppMessage::FileTransferCancel {
                        transfer_id,
                        reason: e.to_string(),
                    })
                    .await;
                return Flow::Continue;
            }

            // Check auto-accept policy: trusted/paired devices auto-accept without requiring manual approval.
            let is_trusted = shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false);
            let settings = shared.settings.lock().unwrap().clone();
            let mut auto_accept = (is_trusted || settings.auto_accept_file_transfers)
                && (settings.auto_accept_max_bytes == 0
                    || file_bytes <= settings.auto_accept_max_bytes);

            // One answer covers a whole folder: once any file of it is
            // accepted the rest are too, and once declined the rest are
            // declined without asking again.
            if let Some((batch_id, folder_name, total)) = &folder {
                let decision = {
                    let mut folders = shared.folders.lock().unwrap();
                    let decision = folders.decision(batch_id);
                    if decision != Some(false) {
                        folders.track(
                            transfer_id,
                            batch_id,
                            folder_name,
                            *total,
                            peer_id,
                            peer_name,
                            false,
                        );
                    }
                    decision
                };
                match decision {
                    Some(false) if !resuming => {
                        shared
                            .file_transfers
                            .lock()
                            .await
                            .reject_inbound(&transfer_id);
                        let _ = ctx
                            .outbox_tx
                            .send(AppMessage::FileTransferAccept {
                                transfer_id,
                                accepted: false,
                                resume_from_chunk: 0,
                                reject_reason: Some("folder declined".into()),
                            })
                            .await;
                        return Flow::Continue;
                    }
                    Some(true) => auto_accept = true,
                    _ => {}
                }
            }

            if resuming {
                let _ = shared
                    .file_transfers
                    .lock()
                    .await
                    .queue_inbound(&transfer_id);
                let bg_shared = shared.clone();
                tokio::spawn(async move {
                    pump_transfer_queue(&bg_shared).await;
                });
            } else if auto_accept {
                let _ = shared
                    .file_transfers
                    .lock()
                    .await
                    .queue_inbound(&transfer_id);

                // Trigger the queue manager
                let bg_shared = shared.clone();
                tokio::spawn(async move {
                    pump_transfer_queue(&bg_shared).await;
                });

                // Record in feed.
                shared.activity.lock().await.record_file_transfer_started(
                    peer_id,
                    peer_name.clone(),
                    file_name.clone(),
                    file_bytes,
                    hex::encode(transfer_id),
                    false,
                );
            } else {
                // Prompt the user via event.
                let _ = shared
                    .event_tx
                    .send(EngineEvent::FileTransferIncoming {
                        transfer_id,
                        from_device: peer_id,
                        from_name: peer_name.clone(),
                        file_name,
                        file_bytes,
                        mime_type,
                    })
                    .await;
            }
        }
        AppMessage::FileTransferAccept {
            transfer_id,
            accepted,
            resume_from_chunk,
            reject_reason,
        } => {
            ctx.touch_last_seen();
            if !accepted {
                let mut mgr = shared.file_transfers.lock().await;
                // The receiver declined a folder: send none of the rest.
                if let Some(batch_id) = mgr.folder_of(&transfer_id) {
                    shared.folders.lock().unwrap().stop_sending(&batch_id);
                }
                mgr.cancel_outbound(&transfer_id);
                drop(mgr);
                let _ = shared
                    .event_tx
                    .send(EngineEvent::FileTransferFailed {
                        in_folder: false,
                        transfer_id,
                        from_device: peer_id,
                        reason: reject_reason.unwrap_or_else(|| "rejected".into()),
                    })
                    .await;
            } else {
                let Some(bg_send_run) = ({
                    let mut mgr = shared.file_transfers.lock().await;
                    mgr.get_outbound_mut(&transfer_id).map(|transfer| {
                        transfer.resume_from(resume_from_chunk);
                        transfer.start_send_run()
                    })
                }) else {
                    return Flow::Continue;
                };
                let bg_outbox = shared
                    .peer_manager
                    .file_sender(peer_id)
                    .unwrap_or(ctx.outbox_tx.clone());
                let bg_shared = shared.clone();
                let bg_event_tx = shared.event_tx.clone();
                let bg_transfer_id = transfer_id;
                let bg_peer_id = peer_id;
                let mut bg_last_prog_emit: std::collections::HashMap<[u8; 16], std::time::Instant> =
                    std::collections::HashMap::new();
                tokio::spawn(async move {
                    const BATCH_SIZE: usize = 4;
                    'outer: loop {
                        let (next_chunk, _last_acked, total_chunks): (u32, u32, u32) = {
                            let mut mgr = bg_shared.file_transfers.lock().await;
                            if let Some(t) = mgr.get_outbound_mut(&bg_transfer_id) {
                                (
                                    t.next_chunk,
                                    t.last_acked_chunk.unwrap_or(0),
                                    t.total_chunks,
                                )
                            } else {
                                break 'outer;
                            }
                        };
                        if next_chunk >= total_chunks {
                            break 'outer;
                        }

                        let (batch, progs) = match read_outbound_chunks(
                            bg_shared.clone(),
                            bg_transfer_id,
                            BATCH_SIZE,
                            bg_send_run,
                        )
                        .await
                        {
                            Some((batch, progs)) => (batch, progs),
                            None => break 'outer,
                        };

                        if let Some((prog, fname)) = progs.last() {
                            let now = std::time::Instant::now();
                            let last = bg_last_prog_emit
                                .get(&bg_transfer_id)
                                .copied()
                                .unwrap_or_else(|| {
                                    now.checked_sub(std::time::Duration::from_secs(1)).unwrap()
                                });
                            if now.duration_since(last).as_millis() >= 100 || prog.percent == 100 {
                                bg_last_prog_emit.insert(bg_transfer_id, now);
                                let _ = bg_event_tx.try_send(EngineEvent::FileTransferProgress {
                                    transfer_id: bg_transfer_id,
                                    from_device: bg_peer_id,
                                    file_name: fname.clone(),
                                    percent: prog.percent,
                                    bytes_received: prog.bytes_received,
                                    total_bytes: prog.total_bytes,
                                    speed_bps: prog.speed_bps,
                                    eta_secs: prog.eta_secs,
                                    outbound: true,
                                });
                            }
                        }

                        if batch.is_empty() {
                            break;
                        }
                        for wire_msg in batch {
                            if bg_outbox.send(wire_msg).await.is_err() {
                                break 'outer;
                            }
                        }
                    }

                    let final_checksum = {
                        let mut mgr = bg_shared.file_transfers.lock().await;
                        mgr.get_outbound_mut(&bg_transfer_id).and_then(|transfer| {
                            if transfer.is_all_sent() {
                                Some(transfer.finalize_checksum())
                            } else {
                                None
                            }
                        })
                    };
                    if let Some(sha256_checksum) = final_checksum {
                        let _ = bg_outbox
                            .send(AppMessage::FileTransferComplete {
                                transfer_id: bg_transfer_id,
                                sha256_checksum,
                            })
                            .await;
                    }
                });
            }
        }
        AppMessage::FileChunk {
            transfer_id,
            chunk_index,
            total_chunks: _,
            data: payload,
            compressed,
        } => {
            ctx.touch_last_seen();
            if !shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false)
            {
                return Flow::Continue;
            }

            let data = if compressed {
                match lz4_flex::decompress_size_prepended(&payload) {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::error!("Failed to decompress file chunk: {}", e);
                        let mut mgr = shared.file_transfers.lock().await;
                        mgr.cancel_inbound(&transfer_id, "decompression failed");
                        return Flow::Continue;
                    }
                }
            } else {
                payload
            };

            let validation = {
                let mut mgr = shared.file_transfers.lock().await;
                if let Some(transfer) = mgr.get_inbound_mut(&transfer_id) {
                    if transfer.from_device != peer_id {
                        Err(anyhow::anyhow!("peer mismatch"))
                    } else if !transfer.accepts_chunks_from(ctx.session_id) {
                        // Sent on a connection that has since been replaced; the
                        // sender resends it on the new one.
                        tracing::debug!(
                            "dropping chunk {} of {:?} from a stale session",
                            chunk_index,
                            transfer_id
                        );
                        return Flow::Continue;
                    } else {
                        transfer.validate_chunk(chunk_index, data.len())
                    }
                } else {
                    Err(anyhow::anyhow!("unknown transfer"))
                }
            };

            match validation {
                Ok((offset, padding, is_duplicate)) => {
                    if is_duplicate {
                        let mut mgr = shared.file_transfers.lock().await;
                        if let Some(t) = mgr.get_inbound_mut(&transfer_id) {
                            let prog = t.progress_snapshot();
                            let should_ack = t.should_ack();
                            let file_name = t.meta.file_name.clone();
                            let last_confirmed = t.last_confirmed_chunk;
                            drop(mgr);

                            let _ = shared.event_tx.try_send(EngineEvent::FileTransferProgress {
                                transfer_id,
                                from_device: peer_id,
                                file_name,
                                percent: prog.percent,
                                bytes_received: prog.bytes_received,
                                total_bytes: prog.total_bytes,
                                speed_bps: prog.speed_bps,
                                eta_secs: prog.eta_secs,
                                outbound: false,
                            });
                            if should_ack {
                                let _ = ctx
                                    .outbox_tx
                                    .send(AppMessage::FileChunkAck {
                                        transfer_id,
                                        last_confirmed_chunk: last_confirmed,
                                    })
                                    .await;
                            }
                        }
                    } else {
                        let _ = ctx
                            .disk_tx
                            .send(DiskTaskMsg::Chunk {
                                transfer_id,
                                chunk_index,
                                offset,
                                padding,
                                data,
                            })
                            .await;
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to validate chunk: {:?}", e);
                }
            };
        }
        AppMessage::FileChunkAck {
            transfer_id,
            last_confirmed_chunk,
        } => {
            ctx.touch_last_seen();
            if !shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false)
            {
                return Flow::Continue;
            }
            if let Some(transfer) = shared
                .file_transfers
                .lock()
                .await
                .get_outbound_mut(&transfer_id)
            {
                if transfer.target_device == Some(peer_id) || transfer.target_device.is_none() {
                    transfer.on_chunk_ack(last_confirmed_chunk);
                    let prog = transfer.progress();
                    let fname = transfer.meta.file_name.clone();
                    let event_tx = shared.event_tx.clone();
                    let tid = transfer_id;
                    tokio::spawn(async move {
                        let _ = event_tx.try_send(EngineEvent::FileTransferProgress {
                            transfer_id: tid,
                            from_device: peer_id,
                            file_name: fname,
                            percent: prog.percent,
                            bytes_received: prog.bytes_received,
                            total_bytes: prog.total_bytes,
                            speed_bps: prog.speed_bps,
                            eta_secs: prog.eta_secs,
                            outbound: true,
                        });
                    });
                }
            }
        }
        AppMessage::FileTransferComplete {
            transfer_id,
            sha256_checksum,
        } => {
            ctx.touch_last_seen();
            if !shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false)
            {
                return Flow::Continue;
            }
            // Finalize: verify SHA-256 and write to disk.
            let _ = ctx
                .disk_tx
                .send(DiskTaskMsg::Complete {
                    transfer_id,
                    sha256_checksum,
                })
                .await;
        }
        AppMessage::FileTransferCompleteAck {
            transfer_id,
            success,
            error,
        } => {
            ctx.touch_last_seen();
            if !shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false)
            {
                return Flow::Continue;
            }

            let (file_name, peer_name, file_bytes) = {
                let mut mgr = shared.file_transfers.lock().await;
                let (fname, fbytes) = mgr
                    .get_outbound_mut(&transfer_id)
                    .map(|t| (t.meta.file_name.clone(), t.meta.size_bytes))
                    .unwrap_or_default();
                mgr.remove_outbound(&transfer_id);
                let pname = shared
                    .peer_manager
                    .get(peer_id)
                    .map(|p| p.friendly_name.clone())
                    .unwrap_or_default();
                (fname, pname, fbytes)
            };

            if success {
                let hex_tid = hex::encode(transfer_id);
                shared.activity.lock().await.record_file_transfer_complete(
                    peer_id,
                    peer_name.clone(),
                    file_name.clone(),
                    file_bytes,
                    hex_tid,
                    None,
                );

                let _ = shared
                    .event_tx
                    .send(EngineEvent::FileTransferComplete {
                        transfer_id,
                        from_device: peer_id,
                        from_name: peer_name,
                        file_name,
                        dest_path: std::path::PathBuf::new(),
                    })
                    .await;
            } else {
                let _ = shared
                    .event_tx
                    .send(EngineEvent::FileTransferFailed {
                        in_folder: false,
                        transfer_id,
                        from_device: peer_id,
                        reason: error.unwrap_or_else(|| "Unknown error".to_string()),
                    })
                    .await;
            }
        }
        AppMessage::FileTransferCancel {
            transfer_id,
            reason,
        } => {
            ctx.touch_last_seen();
            if !shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false)
            {
                return Flow::Continue;
            }
            {
                let mut mgr = shared.file_transfers.lock().await;
                if let Some(t) = mgr.get_inbound_mut(&transfer_id) {
                    if t.from_device == peer_id {
                        mgr.cancel_inbound(&transfer_id, &reason);
                    }
                }
                if let Some(t) = mgr.get_outbound_mut(&transfer_id) {
                    if t.target_device == Some(peer_id) || t.target_device.is_none() {
                        mgr.cancel_outbound(&transfer_id);
                    }
                }
            }
            let _ = shared
                .event_tx
                .send(EngineEvent::FileTransferFailed {
                    in_folder: false,
                    transfer_id,
                    from_device: peer_id,
                    reason,
                })
                .await;
            pump_transfer_queue(shared).await;
        }
        AppMessage::FileTransferPause { transfer_id } => {
            ctx.touch_last_seen();
            if !shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false)
            {
                return Flow::Continue;
            }
            {
                let mut mgr = shared.file_transfers.lock().await;
                if let Some(t) = mgr.get_outbound_mut(&transfer_id) {
                    if t.target_device == Some(peer_id) || t.target_device.is_none() {
                        t.paused = true;
                    }
                } else if let Some(t) = mgr.get_inbound_mut(&transfer_id) {
                    if t.from_device == peer_id {
                        t.paused = true;
                    }
                }
            }
            let _ = shared
                .event_tx
                .send(EngineEvent::FileTransferPaused { transfer_id })
                .await;
        }
        AppMessage::FileTransferResume { transfer_id } => {
            ctx.touch_last_seen();
            let mut was_outbound = false;
            let mut bg_send_run = 0;
            {
                let mut mgr = shared.file_transfers.lock().await;
                if let Some(t) = mgr.get_outbound_mut(&transfer_id) {
                    if t.target_device == Some(peer_id) || t.target_device.is_none() {
                        t.paused = false;
                        // Same as a local resume: before the accept, the accept
                        // starts the sending; after it, restart after the last
                        // chunk the receiver confirmed.
                        if t.is_accepted() {
                            let resume_chunk = t.last_acked_chunk.map(|c| c + 1).unwrap_or(0);
                            t.resume_from(resume_chunk);
                            was_outbound = true;
                            bg_send_run = t.start_send_run();
                        }
                    }
                } else if let Some(t) = mgr.get_inbound_mut(&transfer_id) {
                    if t.from_device == peer_id {
                        t.paused = false;
                    }
                }
            }
            let _ = shared
                .event_tx
                .send(EngineEvent::FileTransferResumed { transfer_id })
                .await;

            if was_outbound {
                // Resume the background chunk loop if we are the sender
                let bg_outbox = shared
                    .peer_manager
                    .file_sender(peer_id)
                    .unwrap_or(ctx.outbox_tx.clone());
                let bg_shared = shared.clone();
                let bg_transfer_id = transfer_id;
                let bg_event_tx = shared.event_tx.clone();
                let bg_peer_id = peer_id;
                let mut bg_last_prog_emit: std::collections::HashMap<[u8; 16], std::time::Instant> =
                    std::collections::HashMap::new();
                tokio::spawn(async move {
                    const BATCH_SIZE: usize = 4;
                    'outer: loop {
                        let (next_chunk, _last_acked, total_chunks): (u32, u32, u32) = {
                            let mut mgr = bg_shared.file_transfers.lock().await;
                            if let Some(t) = mgr.get_outbound_mut(&bg_transfer_id) {
                                (
                                    t.next_chunk,
                                    t.last_acked_chunk.unwrap_or(0),
                                    t.total_chunks,
                                )
                            } else {
                                break 'outer;
                            }
                        };
                        if next_chunk >= total_chunks {
                            break 'outer;
                        }

                        let (batch, progs) = match read_outbound_chunks(
                            bg_shared.clone(),
                            bg_transfer_id,
                            BATCH_SIZE,
                            bg_send_run,
                        )
                        .await
                        {
                            Some((batch, progs)) => (batch, progs),
                            None => break 'outer,
                        };

                        if let Some((prog, fname)) = progs.last() {
                            let now = std::time::Instant::now();
                            let last = bg_last_prog_emit
                                .get(&bg_transfer_id)
                                .copied()
                                .unwrap_or_else(|| {
                                    now.checked_sub(std::time::Duration::from_secs(1)).unwrap()
                                });
                            if now.duration_since(last).as_millis() >= 100 || prog.percent == 100 {
                                bg_last_prog_emit.insert(bg_transfer_id, now);
                                let _ = bg_event_tx.try_send(EngineEvent::FileTransferProgress {
                                    transfer_id: bg_transfer_id,
                                    from_device: bg_peer_id,
                                    file_name: fname.clone(),
                                    percent: prog.percent,
                                    bytes_received: prog.bytes_received,
                                    total_bytes: prog.total_bytes,
                                    speed_bps: prog.speed_bps,
                                    eta_secs: prog.eta_secs,
                                    outbound: true,
                                });
                            }
                        }

                        if batch.is_empty() {
                            break 'outer;
                        }

                        // Send the batch
                        for wire_msg in batch {
                            if bg_outbox.send(wire_msg).await.is_err() {
                                break 'outer;
                            }
                        }
                    }

                    let final_checksum = {
                        let mut mgr = bg_shared.file_transfers.lock().await;
                        mgr.get_outbound_mut(&bg_transfer_id).and_then(|transfer| {
                            if transfer.is_all_sent() {
                                Some(transfer.finalize_checksum())
                            } else {
                                None
                            }
                        })
                    };
                    if let Some(sha256_checksum) = final_checksum {
                        let _ = bg_outbox
                            .send(AppMessage::FileTransferComplete {
                                transfer_id: bg_transfer_id,
                                sha256_checksum,
                            })
                            .await;
                    }
                });
            }
        }
        _ => unreachable!("dispatch_inbound routed a non-transfers message here"),
    }
    Flow::Continue
}
