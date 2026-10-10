//! Session-level messages: ping/pong, sleep and sync state, key rotation,
//! permission errors, and goodbye.

use super::*;

pub(super) async fn handle(ctx: &InboundCtx, msg: AppMessage) -> Flow {
    let shared = &ctx.shared;
    let peer_id = ctx.peer_id;
    let peer_name = &ctx.peer_name;
    match msg {
        AppMessage::DeviceSleepState { is_asleep } => {
            ctx.touch_last_seen();
            tracing::info!(peer = %peer_name, is_asleep, "received device sleep state");
            ctx.peer_sleeping
                .store(is_asleep, std::sync::atomic::Ordering::Relaxed);
            // Send an instant ping if they just woke up to update their last_seen_millis
            // and prevent their local 15s grace period from expiring before our next tick.
            if !is_asleep {
                let ping = probe::make_ping();
                *ctx.ping_sent_at.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some(std::time::Instant::now());
                let _ = ctx.outbox_tx.send(ping).await;
            }
        }
        AppMessage::DeviceSyncState { enabled } => {
            ctx.touch_last_seen();
            if !shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false)
            {
                return Flow::Continue;
            }
            tracing::info!(peer = %peer_name, enabled, "received device sync state");
            let _ = shared
                .peer_manager
                .set_remote_sync_enabled(peer_id, enabled);
            let _ = shared
                .event_tx
                .send(EngineEvent::PeerSyncStateChanged {
                    device_id: peer_id,
                    enabled,
                })
                .await;
        }
        AppMessage::KeyRotated { new_pubkey_bytes } => {
            ctx.touch_last_seen();
            // Only accept key rotation from currently trusted peers over the established AEAD tunnel.
            if shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false)
            {
                let mut trust = shared.trust.lock().await;
                if trust.rotate_peer_key(peer_id, &new_pubkey_bytes).is_ok() {
                    tracing::info!(peer_id = %peer_id, "Successfully processed KeyRotated from peer");
                }
            }
        }
        AppMessage::Ping { timestamp_ms } => {
            ctx.touch_last_seen();
            let _ = ctx.outbox_tx.send(AppMessage::Pong { timestamp_ms }).await;
        }
        AppMessage::Pong { timestamp_ms: _ } => {
            ctx.touch_last_seen();
            // Feed the RTT sample into the peer's quality probe
            // using the Instant captured at send time, which is
            // far more accurate than round-tripping wall-clock ms
            // over the network (HIGH-03).
            let maybe_sent_at = ctx
                .ping_sent_at
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            if let Some(sent_at) = maybe_sent_at {
                let rtt_us = probe::measure_rtt_us(sent_at);
                let result = ProbeResult::from_samples(vec![rtt_us]);

                shared
                    .quality_probes
                    .entry(peer_id)
                    .or_insert_with(|| QualityProbe::new(peer_name.as_str()))
                    .record(result);
            }
        }
        AppMessage::Bye => {
            // Do NOT set explicit_disconnect here — receiving Bye from
            // the remote peer (e.g., Android OS killed the socket) is
            // not a user-initiated disconnect. Auto-reconnect must stay
            // enabled so the watchdog can re-establish the link.
            return Flow::Disconnect("peer closed session".to_string());
        }
        AppMessage::PermissionError {
            feature,
            message,
            origin_device: _,
            origin_device_name: _,
        } => {
            // The text is shown to the user: never take it from a stranger.
            if !shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false)
            {
                tracing::warn!("Ignoring PermissionError from untrusted peer {}", peer_id);
                return Flow::Continue;
            }
            let _ = shared
                .event_tx
                .send(EngineEvent::Warning(format!("{}: {}", feature, message)))
                .await;
        }
        _ => unreachable!("dispatch_inbound routed a non-link message here"),
    }
    Flow::Continue
}
