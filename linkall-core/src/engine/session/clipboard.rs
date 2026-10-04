//! Inbound clipboard pushes, acks, and history metadata.

use super::*;

pub(super) async fn handle(ctx: &InboundCtx, msg: AppMessage) -> Flow {
    let shared = &ctx.shared;
    let peer_id = ctx.peer_id;
    let peer_name = &ctx.peer_name;
    match msg {
        AppMessage::ClipboardPush {
            seq,
            mut content,
            origin_device,
            origin_device_name,
            relay_path,
        } => {
            ctx.touch_last_seen();
            if shared
                .peer_manager
                .get(peer_id)
                .map(|peer| peer.is_sync_eligible())
                .unwrap_or(false)
            {
                let _ = shared.peer_manager.update_last_sync(peer_id);
                let display_name = if origin_device_name.is_empty() {
                    peer_name.clone()
                } else {
                    origin_device_name.clone()
                };

                // Run smart clipboard transformers (URL UTM parameter stripping, whitespace cleaning)
                crate::transformer::TransformerPipeline::default_pipeline()
                    .transform(std::sync::Arc::make_mut(&mut content));

                // --- Clipboard Security & Throttling ---
                let payload_size = match &*content {
                    ClipboardContent::Text(t) => t.len(),
                    ClipboardContent::Image { data, .. } => data.len(),
                    ClipboardContent::File { data, .. } => data.len(),
                };

                // Limit sizes to prevent OOM
                const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
                const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;

                let allowed = match &*content {
                    ClipboardContent::Text(t) => t.len() <= MAX_TEXT_BYTES,
                    ClipboardContent::Image { data, .. } => data.len() <= MAX_IMAGE_BYTES,
                    ClipboardContent::File { data, .. } => data.len() <= MAX_IMAGE_BYTES,
                };

                if !allowed {
                    tracing::warn!(peer_id = %peer_id, size = payload_size, "dropped oversized clipboard payload");
                    return Flow::Continue;
                }

                // Run inbound payload through the FilterChain (e.g. executable blocking, etc.)
                let filter_chain =
                    crate::filter::FilterChain::from_settings(&shared.settings.lock().unwrap());
                if let crate::filter::Verdict::Deny { reason } = filter_chain.run(&content) {
                    tracing::warn!(peer_id = %peer_id, reason, "inbound clipboard payload denied by filter");
                    return Flow::Continue;
                }

                // ── Timeline-first clipboard UX ───────────────
                let hash = hash_content(&content);
                let hash_hex = hex::encode(hash);

                // ── Deduplicator Check ───────────────
                let should_apply = {
                    let mut dedup = shared.dedup.lock().await;
                    dedup.should_apply(origin_device, hash)
                };

                if !should_apply {
                    tracing::debug!("suppressing inbound clipboard push (dedup)");
                    // It is either an echo of our own send, or a duplicate from a second peer.
                    // Acknowledge it, but skip all local UI/clipboard updates.
                    let _ = ctx.outbox_tx.send(AppMessage::ClipboardAck { seq }).await;
                    return Flow::Continue;
                }

                // The global "Share clipboard" switch. It used to be
                // saved and never read, so turning it off changed
                // nothing. Off: remote clips still land in history
                // (so they can be applied by hand) but never
                // overwrite the local clipboard or get relayed on.
                let sharing_on = shared.settings.lock().unwrap().sync_enabled;
                let auto_apply = sharing_on
                    && shared
                        .apply_policy
                        .lock()
                        .await
                        .should_auto_apply(origin_device);

                // Record in activity feed.
                let activity_id = {
                    if let ClipboardContent::Text(ref text) = *content {
                        shared
                            .clipboard_store
                            .lock()
                            .await
                            .insert(hash_hex.clone(), text.clone());
                    }
                    let mut feed = shared.activity.lock().await;
                    match &*content {
                        ClipboardContent::Text(ref text) => feed.record_remote_clipboard_text(
                            origin_device,
                            display_name.clone(),
                            text,
                            hash_hex.clone(),
                            relay_path.clone(),
                        ),
                        ClipboardContent::Image { mime, data } => feed
                            .record_remote_clipboard_image(
                                origin_device,
                                display_name.clone(),
                                mime,
                                data.len() as u64,
                                hash_hex.clone(),
                                relay_path.clone(),
                            ),
                        ClipboardContent::File { name, data } => feed.record_file_transfer_started(
                            origin_device,
                            display_name.clone(),
                            name.clone(),
                            data.len() as u64,
                            hash_hex.clone(),
                            false,
                        ),
                    }
                };

                // If auto-applying, mark immediately applied.
                if auto_apply {
                    let mut feed = shared.activity.lock().await;
                    feed.record_clipboard_applied(
                        origin_device,
                        display_name.clone(),
                        hash_hex.clone(),
                    );
                }

                // Wrap content in Arc here so all downstream users — the
                // EngineEvent and every relay-fanout hop — share one heap
                // allocation instead of N independent clones (MED-01).
                // (content is already Arc<ClipboardContent>)
                let _ = shared
                    .event_tx
                    .send(EngineEvent::ClipboardReceived {
                        from_device: origin_device,
                        from_name: display_name.clone(),
                        content: content.clone(),
                        auto_applied: auto_apply,
                        relay_path: relay_path.clone(),
                        activity_id,
                    })
                    .await;
                let _ = ctx.outbox_tx.send(AppMessage::ClipboardAck { seq }).await;

                // Persist the incoming item to history.
                {
                    let max_bytes = shared.settings.lock().unwrap().max_history_text_bytes;
                    let source = display_name.clone();
                    let _ = shared
                        .history
                        .lock()
                        .await
                        .push_with_options(&content, source, max_bytes);
                }

                // ── Mesh fanout relay ──────────────────────────
                // If we received from a direct peer but there are other
                // peers in the mesh, relay onwards (excluding origin + seen).
                // Wrap content in Arc so each relay hop shares the same
                // heap allocation instead of cloning the full payload
                // (MED-01 — AppMessage::clone on relay hops).
                let fanout_peers = if sharing_on {
                    shared.peer_manager.active_senders()
                } else {
                    Vec::new()
                };
                let mut router = shared.mesh_router.lock().await;
                // shared_content is already Arc-wrapped above; no further
                // full clone needed here — each fan-out is a pointer clone
                // plus one cheap metadata-struct clone (MED-01).
                for (fp_id, fp_tx) in fanout_peers {
                    if fp_id == peer_id {
                        continue;
                    }
                    let Some(fp) = shared.peer_manager.get(fp_id) else {
                        continue;
                    };
                    if !fp.is_sync_eligible() {
                        continue;
                    }
                    if !router.should_relay_to(hash, origin_device, fp_id, &relay_path) {
                        continue;
                    }
                    let mut extended_path = relay_path.clone();
                    extended_path.push(shared.config.device_name.clone());
                    let _ = fp_tx.try_send(AppMessage::ClipboardPush {
                        seq,
                        content: content.clone(),
                        origin_device,
                        origin_device_name: display_name.clone(),
                        relay_path: extended_path,
                    });
                }
            } else {
                let _ = shared
                    .event_tx
                    .send(EngineEvent::Warning(format!(
                        "ignoring clipboard payload from untrusted/paused peer {}",
                        peer_name
                    )))
                    .await;
            }
        }
        AppMessage::HistoryMetadata { entry } => {
            ctx.touch_last_seen();
            // MED-03 FIX: Only accept history metadata from trusted peers.
            if !shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false)
            {
                return Flow::Continue;
            }
            let _ = shared.peer_manager.update_last_sync(peer_id);
            let _ = shared
                .event_tx
                .send(EngineEvent::HistoryMetadataReceived {
                    from_device: peer_id,
                    from_name: peer_name.clone(),
                    entry,
                })
                .await;
        }
        AppMessage::ClipboardAck { seq } => {
            ctx.touch_last_seen();
            let _ = shared.peer_manager.update_last_sync(peer_id);
            let _ = shared
                .event_tx
                .send(EngineEvent::ClipboardSynced {
                    peer_device: peer_id,
                    peer_name: peer_name.clone(),
                    seq,
                })
                .await;
        }
        _ => unreachable!("dispatch_inbound routed a non-clipboard message here"),
    }
    Flow::Continue
}
