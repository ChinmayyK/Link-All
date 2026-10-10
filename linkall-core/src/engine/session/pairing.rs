//! Inbound pairing requests and responses, and QR auth.

use super::*;

pub(super) async fn handle(ctx: &InboundCtx, msg: AppMessage) -> Flow {
    let shared = &ctx.shared;
    let peer_id = ctx.peer_id;
    let peer_name = &ctx.peer_name;
    match msg {
        AppMessage::PairingRequest {
            origin_device,
            origin_device_name,
            pin: _req_pin,
        } => {
            ctx.touch_last_seen();
            if origin_device != peer_id {
                tracing::warn!(
                    peer_id = %peer_id,
                    claimed_device = %origin_device,
                    "ignoring PairingRequest: origin_device does not match session peer"
                );
                return Flow::Continue;
            }

            // Self-healing trust: if we already trust this peer cryptographically,
            // and they are asking to pair again (perhaps they lost their app data),
            // automatically accept the request so they can trust us back.
            let is_trusted = shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false);
            if is_trusted {
                tracing::info!(peer_id = %peer_id, "Auto-accepting pairing request from already trusted device");
                // Any prompt we still show for them is settled too.
                let had_prompt = shared
                    .peer_manager
                    .get(peer_id)
                    .is_some_and(|p| p.pairing_requested || p.outgoing_pairing_waiting);
                if had_prompt {
                    let _ = shared
                        .peer_manager
                        .end_pairing(peer_id, Some(PairingOutcome::Accepted));
                    notify_pairing_changed(shared, peer_id).await;
                }
                let _ = ctx
                    .outbox_tx
                    .send(AppMessage::PairingResponse {
                        origin_device: shared.config.device_id,
                        accepted: true,
                    })
                    .await;
                return Flow::Continue;
            }

            // Our user just declined this device: answer for them rather than
            // re-prompting, so a declined device can't nag.
            if shared
                .peer_manager
                .get(peer_id)
                .is_some_and(|p| p.in_decline_cooldown())
            {
                tracing::info!(peer_id = %peer_id, "declining pairing request inside decline cooldown");
                let _ = ctx
                    .outbox_tx
                    .send(AppMessage::PairingResponse {
                        origin_device: shared.config.device_id,
                        accepted: false,
                    })
                    .await;
                return Flow::Continue;
            }

            // A repeat of a request we are already showing (the requester
            // re-asks while waiting, or its session was replaced) keeps its
            // clock; only a new one arms it.
            let shown = shared.peer_manager.get(peer_id);
            let already_shown = shown.as_ref().is_some_and(|p| p.pairing_requested);
            let _ = shared.peer_manager.set_pairing_requested(peer_id, true);
            if !already_shown {
                arm_pairing_expiry(shared, peer_id);
            }

            let pin = ctx
                .session_pin
                .clone()
                .or_else(|| shared.peer_manager.get(peer_id).and_then(|p| p.pairing_pin))
                .unwrap_or_else(|| "------".to_string());
            // Same request, same code: the prompt on screen is already right,
            // and re-raising it would make it flash on every repeat.
            if already_shown && shown.and_then(|p| p.pairing_pin).as_deref() == Some(pin.as_str()) {
                return Flow::Continue;
            }

            // Re-emit PairingRequested with the REAL name and PIN so the UI updates
            let _ = shared
                .peer_manager
                .set_pairing_pin(peer_id, Some(pin.clone()));
            let _ = shared
                .event_tx
                .send(EngineEvent::PairingRequested {
                    device_id: origin_device,
                    device_name: origin_device_name.clone(),
                    pin,
                })
                .await;

            let _ = shared
                .event_tx
                .send(EngineEvent::PairingRequest {
                    device_id: origin_device,
                    device_name: origin_device_name,
                })
                .await;
        }
        AppMessage::PairingResponse {
            origin_device,
            accepted,
        } => {
            ctx.touch_last_seen();

            // CRIT-03 FIX: Only process PairingResponse if:
            //   1. The origin_device matches the actual session peer_id
            //      (prevents a connected peer from spoofing trust for a different device).
            //   2. We previously sent a PairingRequest to this peer
            //      (tracked via the pairing_requested flag set in respond_to_pairing).
            //   3. The peer is not already trusted (prevents re-trust of revoked peers).
            if origin_device != peer_id {
                tracing::warn!(
                    peer_id = %peer_id,
                    claimed_device = %origin_device,
                    "ignoring PairingResponse: origin_device does not match session peer"
                );
                return Flow::Continue;
            }

            // Check that we actually initiated pairing with this peer.
            // `outgoing_pairing_waiting` is set when our user taps Pair and
            // cleared when the request is answered, withdrawn or expires.
            // A remote peer sending an unsolicited PairingResponse is rejected.
            let peer = shared.peer_manager.get(peer_id);
            let we_requested_pairing = peer.as_ref().is_some_and(|p| p.outgoing_pairing_waiting);
            let we_already_trust_them = peer.as_ref().is_some_and(|p| p.trusted);
            let they_asked_us = peer.as_ref().is_some_and(|p| p.pairing_requested);

            if !we_requested_pairing && !we_already_trust_them {
                // A decline from the device that asked us is its withdrawal
                // (see pairing_withdrawal): close the prompt we are showing.
                if they_asked_us && !accepted {
                    tracing::info!(peer_id = %peer_id, "peer withdrew its pairing request");
                    let _ = shared
                        .peer_manager
                        .end_pairing(peer_id, Some(PairingOutcome::Cancelled));
                    notify_pairing_changed(shared, peer_id).await;
                    return Flow::Continue;
                }
                tracing::warn!(
                    peer_id = %peer_id,
                    "ignoring unsolicited PairingResponse — no pending pairing request and not already trusted"
                );
                return Flow::Continue;
            }

            if !accepted {
                tracing::info!(peer_id = %peer_id, "peer rejected pairing request");
                let _ = shared
                    .peer_manager
                    .end_pairing(peer_id, Some(PairingOutcome::Declined));
                let _ = shared
                    .event_tx
                    .send(EngineEvent::PairingResponse {
                        device_id: origin_device,
                        accepted,
                    })
                    .await;
                return Flow::Disconnect("peer rejected pairing request".to_string());
            } else {
                // ── CRITICAL: Establish mutual trust ──────────────
                // The remote peer accepted our pairing request and
                // already trusts us (set in respond_to_pairing).
                // We must trust them back so the connection is fully
                // bidirectional — otherwise the dashboard shows
                // "not connected" and file transfers fail.
                tracing::info!(peer_id = %peer_id, "peer accepted pairing — establishing mutual trust");
                if !we_already_trust_them {
                    let _ = shared.trust.lock().await.trust_peer(peer_id);
                    let _ = shared.peer_manager.update_trust(peer_id, true);
                    retire_old_installs(shared, peer_id).await;
                }
                let _ = shared.peer_manager.set_auto_connect(peer_id, true);
                // Also closes the prompt for their request when both users
                // tapped Pair at once.
                let _ = shared
                    .peer_manager
                    .end_pairing(peer_id, Some(PairingOutcome::Accepted));

                // Emit PeerConnected so the UI updates immediately.
                let _ = shared
                    .event_tx
                    .send(EngineEvent::PeerConnected {
                        device_id: peer_id,
                        device_name: peer_name.clone(),
                        addr: ctx.endpoint,
                        trusted: true,
                    })
                    .await;
            }
            let _ = shared
                .event_tx
                .send(EngineEvent::PairingResponse {
                    device_id: origin_device,
                    accepted,
                })
                .await;
        }
        AppMessage::QrAuth { token } => {
            // Not consumed on use: the code stays on screen so a second
            // device can scan it, and a wrong or stale attempt must not
            // invalidate it for the real one. It only ever travels inside the
            // encrypted session, so knowing it means having seen the screen.
            let valid = shared
                .qr_auth_token
                .lock()
                .await
                .as_ref()
                .is_some_and(|t| t.token == token && t.expires_at > std::time::Instant::now());

            if valid {
                tracing::info!(peer_id = %peer_id, "peer provided valid QR auth token — establishing mutual trust");
                let _ = shared
                    .peer_manager
                    .end_pairing(peer_id, Some(PairingOutcome::Accepted));

                let _ = shared.trust.lock().await.trust_peer(peer_id);
                let _ = shared.peer_manager.update_trust(peer_id, true);
                let _ = shared.peer_manager.set_auto_connect(peer_id, true);
                retire_old_installs(shared, peer_id).await;

                // Emit PeerConnected so the UI updates immediately.
                let _ = shared
                    .event_tx
                    .send(EngineEvent::PeerConnected {
                        device_id: peer_id,
                        device_name: peer_name.clone(),
                        addr: ctx.endpoint,
                        trusted: true,
                    })
                    .await;
            } else {
                tracing::warn!(peer_id = %peer_id, "peer provided invalid QR auth token");
            }
        }
        _ => unreachable!("dispatch_inbound routed a non-pairing message here"),
    }
    Flow::Continue
}
