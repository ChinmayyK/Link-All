//! Trust and device management: pairing, QR auth, approve/reject/revoke,
//! rename, forget, and per-peer sync and auto-connect.

use super::*;

impl Engine {
    pub async fn approve_device(
        &self,
        device_id: Uuid,
        device_name: String,
        pubkey_bytes: Vec<u8>,
    ) -> Result<()> {
        let public_key: [u8; 32] = pubkey_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("approve_device expects a 32-byte public key"))?;
        let mut trust = self.shared.trust.lock().await;
        trust.observe_peer(device_id, device_name, &public_key)?;
        trust.trust_peer(device_id)?;
        drop(trust);
        self.shared.peer_manager.update_trust(device_id, true)?;
        retire_old_installs(&self.shared, device_id).await;
        Ok(())
    }

    pub async fn reject_device(&self, device_id: Uuid) -> Result<()> {
        self.reject_peer(device_id).await
    }

    pub async fn trusted_devices(&self) -> Vec<TrustRecord> {
        self.shared
            .trust
            .lock()
            .await
            .all_devices()
            .cloned()
            .collect()
    }

    pub async fn generate_qr_token(&self) -> String {
        use rand::RngCore;
        let mut bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token = hex::encode(bytes);
        *self.shared.qr_auth_token.lock().await = Some(QrAuthToken {
            token: token.clone(),
            // Pairing screens show one code for as long as they are open.
            expires_at: std::time::Instant::now() + std::time::Duration::from_secs(10 * 60),
        });
        token
    }

    pub async fn send_qr_auth(&self, target_device: Uuid, token: String) {
        let msg = AppMessage::QrAuth { token };
        let peers = self.shared.peer_manager.all_connected_senders();
        if let Some(tx) = peers
            .into_iter()
            .find(|(id, _)| *id == target_device)
            .map(|(_, tx)| tx)
        {
            let _ = tx.send(msg).await;
        } else {
            if let Some(peer) = self.shared.peer_manager.get(target_device) {
                let addrs = peer.socket_addrs();
                if !addrs.is_empty() {
                    let shared = self.shared.clone();
                    tokio::spawn(async move {
                        if let Ok(()) = connect_loop_ext(
                            shared.clone(),
                            addrs,
                            Some(target_device),
                            DiscoverySource::Manual,
                            true,
                        )
                        .await
                        {
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            let peers = shared.peer_manager.all_connected_senders();
                            if let Some(tx) = peers
                                .into_iter()
                                .find(|(id, _)| *id == target_device)
                                .map(|(_, tx)| tx)
                            {
                                let _ = tx.send(msg).await;
                            }
                        }
                    });
                }
            }
        }
    }

    pub async fn revoke_device(&self, device_id: Uuid) -> Result<bool> {
        self.revoke_peer(device_id).await
    }

    pub async fn rename_trusted_device(
        &self,
        device_id: Uuid,
        display_name: String,
    ) -> Result<bool> {
        let renamed = {
            let mut trust = self.shared.trust.lock().await;
            trust.rename_peer(device_id, display_name)?
        };
        Ok(renamed.is_some())
    }

    pub async fn is_trusted(&self, device_id: Uuid) -> bool {
        self.shared.trust.lock().await.is_trusted(device_id)
    }

    pub async fn trust_peer(&self, device_id: Uuid) -> Result<()> {
        let changed = {
            let mut trust = self.shared.trust.lock().await;
            trust.trust_peer(device_id)?
        };
        if changed.is_some() {
            self.shared.peer_manager.update_trust(device_id, true)?;
            let _ = self.shared.peer_manager.set_auto_connect(device_id, true);
            retire_old_installs(&self.shared, device_id).await;

            // Push local battery and network status to the newly trusted peer.
            let mut target_tx: Option<tokio::sync::mpsc::Sender<crate::protocol::AppMessage>> =
                None;
            for (id, tx) in self.shared.peer_manager.active_senders() {
                if id == device_id {
                    target_tx = Some(tx);
                    break;
                }
            }
            if let Some(tx) = target_tx {
                let sh = self.shared.clone();
                tokio::spawn(async move {
                    let battery_val = *sh.device_status.local_battery.lock().unwrap();
                    if let Some((level, charging)) = battery_val {
                        let _ = tx
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
                        let _ = tx
                            .send(AppMessage::NetworkStatus {
                                network_type: net,
                                origin_device: sh.config.device_id,
                                origin_device_name: sh.config.device_name.clone(),
                            })
                            .await;
                    }
                });
            }
        }
        Ok(())
    }

    pub async fn reject_peer(&self, device_id: Uuid) -> Result<()> {
        let changed = {
            let mut trust = self.shared.trust.lock().await;
            trust.reject_peer(device_id)?
        };
        if changed.is_some() {
            self.shared.peer_manager.update_trust(device_id, false)?;
            let _ = self.disconnect_peer(device_id).await;
        }
        Ok(())
    }

    pub async fn revoke_peer(&self, device_id: Uuid) -> Result<bool> {
        let removed = self.shared.trust.lock().await.revoke_peer(device_id)?;
        if removed {
            self.shared.peer_manager.update_trust(device_id, false)?;
            self.shared
                .peer_manager
                .mark_disconnected(device_id, Some("trust revoked".to_string()))?;
        }
        Ok(removed)
    }

    pub async fn unreject_peer(&self, device_id: Uuid) -> Result<bool> {
        let changed = {
            let mut trust = self.shared.trust.lock().await;
            trust.unreject_peer(device_id)?
        };
        Ok(changed)
    }

    pub async fn send_pairing_request(&self, target_device: Uuid) {
        // Already paired: clients use Pair as "connect" on paired rows too.
        // A pairing request there would show "waiting for approval" on a
        // device nobody needs to approve. Dialing is enough: if the other
        // side forgot us, reconcile_one_sided_trust asks it to re-pair.
        if self.is_trusted(target_device).await {
            let _ = self.reconnect_peer_by_id(target_device).await;
            return;
        }

        // Clear any previous Rejected or Revoked state so the outbound connection isn't blocked,
        // and a past Disconnect/Forget: asking to pair is asking to talk again.
        let _ = self.unreject_peer(target_device).await;
        let _ = self
            .shared
            .peer_manager
            .set_explicit_disconnect(target_device, false);

        // Mark that WE initiated a pairing request so the PairingResponse
        // handler accepts the response (CRIT-03 anti-spoof check). Lowering
        // the flag first restarts the clock when the user taps Pair again.
        let _ = self
            .shared
            .peer_manager
            .set_outgoing_pairing_waiting(target_device, false);
        let _ = self
            .shared
            .peer_manager
            .set_outgoing_pairing_waiting(target_device, true);
        arm_pairing_expiry(&self.shared, target_device);

        let live_tx = self
            .shared
            .peer_manager
            .all_connected_senders()
            .into_iter()
            .find(|(id, _)| *id == target_device)
            .map(|(_, tx)| tx);

        match live_tx {
            Some(tx) => {
                let _ = tx
                    .send(pairing_request_message(&self.shared, target_device))
                    .await;
            }
            None => {
                // Dial in the background instead of awaiting connect_loop's
                // full retry/backoff here: that blocked the IPC caller for
                // seconds (the Windows client gave up and reported failure)
                // and silently dropped the request whenever the session came
                // up some other way (e.g. the peer dialed us). The request is
                // delivered by register_session as soon as any session with
                // this peer goes live, since outgoing_pairing_waiting is set.
                if let Some(peer) = self.shared.peer_manager.get(target_device) {
                    let addrs = peer.socket_addrs();
                    if !addrs.is_empty() {
                        let shared = self.shared.clone();
                        tokio::spawn(async move {
                            if let Err(err) = connect_loop_ext(
                                shared,
                                addrs,
                                Some(target_device),
                                DiscoverySource::Manual,
                                true,
                            )
                            .await
                            {
                                warn!(peer_id = %target_device, error = %err, "pairing connect failed");
                            }
                        });
                    }
                }
            }
        }

        let peer = self.shared.peer_manager.get(target_device);
        let pin = peer
            .as_ref()
            .and_then(|p| p.pairing_pin.clone())
            .unwrap_or_else(|| "------".to_string());
        let device_name = peer
            .map(|p| p.friendly_name)
            .unwrap_or_else(|| "Unknown device".to_string());
        let _ = self
            .shared
            .event_tx
            .send(EngineEvent::OutgoingPairingWaiting {
                device_id: target_device,
                device_name,
                pin,
            })
            .await;
    }

    pub async fn initiate_pairing(&self, target_device: Uuid) -> Result<()> {
        self.send_pairing_request(target_device).await;
        Ok(())
    }

    pub async fn report_discovered_peer(
        &self,
        device_id: Uuid,
        device_name: String,
        ip: String,
        port: u16,
    ) -> Result<()> {
        let ip_addr = ip.parse::<std::net::IpAddr>().context("invalid IP")?;
        let endpoint = std::net::SocketAddr::new(ip_addr, port);
        let _ = self.shared.peer_manager.upsert_peer(
            device_id,
            device_name,
            endpoint,
            false,
            crate::peer_manager::DiscoverySource::Manual,
        );
        Ok(())
    }
    pub async fn respond_to_pairing(&self, requester_device: Uuid, accepted: bool) -> Result<()> {
        // A prompt left on screen can outlive its request (expired, withdrawn,
        // session gone). Accepting then would trust a device that is no
        // longer asking, leaving trust one-sided.
        let pending = self
            .shared
            .peer_manager
            .get(requester_device)
            .is_some_and(|p| p.pairing_requested);
        if accepted && !pending {
            anyhow::bail!("this pairing request is no longer pending");
        }
        if accepted {
            // Trust them persistently
            self.trust_peer(requester_device).await?;
        } else {
            // Declining is "not now", not a block: the device stays reachable
            // and may ask again once the cooldown passes. (Rejecting it in the
            // trust store refused every later connection from it, so its user
            // sat on "waiting" with no way to learn why.)
            let _ = self
                .shared
                .peer_manager
                .mark_pairing_declined(requester_device);
        }
        // Closes our side in both directions: when both users tapped Pair at
        // once, accepting their request also settles ours.
        let _ = self.shared.peer_manager.end_pairing(requester_device, None);
        if !pending {
            // Declining an already-ended request: nothing to tell them, and a
            // decline now would read as an answer to any request they send next.
            return Ok(());
        }
        send_to_live_session(
            &self.shared,
            requester_device,
            AppMessage::PairingResponse {
                origin_device: self.shared.config.device_id,
                accepted,
            },
        )
        .await;
        Ok(())
    }

    /// Withdraws our pending request: the other device's prompt closes too.
    pub async fn cancel_pairing_request(&self, target_device: Uuid) {
        let waiting = self
            .shared
            .peer_manager
            .get(target_device)
            .is_some_and(|p| p.outgoing_pairing_waiting);
        if !waiting {
            return;
        }
        let _ = self.shared.peer_manager.end_pairing(target_device, None);
        send_to_live_session(
            &self.shared,
            target_device,
            pairing_withdrawal(&self.shared),
        )
        .await;
        notify_pairing_changed(&self.shared, target_device).await;
    }

    /// Pause Sync: keep connection alive, suppress clipboard data flow.
    pub async fn pause_sync_peer(&self, device_id: Uuid) -> Result<bool> {
        self.shared.peer_manager.set_sync_enabled(device_id, false)
    }

    /// Resume Sync: re-enable clipboard data flow.
    pub async fn resume_sync_peer(&self, device_id: Uuid) -> Result<bool> {
        self.shared.peer_manager.set_sync_enabled(device_id, true)
    }

    /// Forget Device: remove persistent pairing and revoke trust.
    pub async fn forget_device(&self, device_id: Uuid) -> Result<bool> {
        let found = self.shared.peer_manager.forget_device(device_id)?;
        if found {
            let _ = self.shared.trust.lock().await.revoke_peer(device_id);
            // Drain pending RPC waiters for forgotten device
            drain_remote_waiters(&self.shared, device_id).await;
            // Disconnect the session — device will not auto-reconnect
            let session = self.shared.peer_manager.shutdown_peer_session(device_id)?;
            if let Some(session) = session {
                if let Some(shutdown_tx) = session.shutdown_tx {
                    let _ = shutdown_tx.send(crate::peer_manager::SessionShutdown {
                        reason: "device forgotten".to_string(),
                        send_bye: true,
                        explicit_disconnect: false,
                    });
                }
            }
        }
        Ok(found)
    }

    /// Set auto-connect for a device.
    pub async fn set_auto_connect(&self, device_id: Uuid, enabled: bool) -> Result<bool> {
        self.shared
            .peer_manager
            .set_auto_connect(device_id, enabled)
    }
}

/// Call after `kept` becomes trusted. See `TrustStore::retire_old_installs`.
pub(super) async fn retire_old_installs(shared: &EngineShared, kept: Uuid) {
    let retired = shared
        .trust
        .lock()
        .await
        .retire_old_installs(kept, |id| shared.peer_manager.sender(id).is_some());
    match retired {
        Ok(retired) => {
            for id in retired {
                tracing::info!(old = %id, new = %kept, "retired an old install of a paired device");
                let _ = shared.peer_manager.forget_device(id);
            }
        }
        Err(e) => tracing::warn!("could not retire old installs: {e:#}"),
    }
}

async fn send_to_live_session(shared: &EngineShared, peer_id: Uuid, msg: AppMessage) {
    if let Some(tx) = shared.peer_manager.sender(peer_id) {
        let _ = tx.send(msg).await;
    }
}

/// The wire has no "withdraw" message, and an unknown message variant would
/// drop the session with an older peer. A decline sent by the device that
/// *asked* means "withdrawn": newer peers close their prompt, older ones
/// ignore it as an unsolicited response.
pub(super) fn pairing_withdrawal(shared: &EngineShared) -> AppMessage {
    AppMessage::PairingResponse {
        origin_device: shared.config.device_id,
        accepted: false,
    }
}

pub(super) async fn notify_pairing_changed(shared: &EngineShared, peer_id: Uuid) {
    let _ = shared
        .event_tx
        .send(EngineEvent::PairingChanged { device_id: peer_id })
        .await;
}

/// Runs the pairing request with `peer_id` until it is answered, restarted
/// or `PAIRING_TIMEOUT` passes, then expires it. While we are the one asking,
/// it re-asks every `PAIRING_RESEND`: an answer sent on a session that was
/// being replaced (both devices dialing at once) is lost, leaving the other
/// device trusting us while we wait forever. A device that already trusts us
/// answers a repeat at once, so that heals within one interval. Lives only
/// as long as the request, so an idle phone never wakes for this.
pub(super) fn arm_pairing_expiry(shared: &EngineShared, peer_id: Uuid) {
    let Some(started) = shared
        .peer_manager
        .get(peer_id)
        .and_then(|p| p.pairing_started_at)
    else {
        return;
    };
    let shared = shared.clone();
    tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + crate::pairing::PAIRING_TIMEOUT;
        let peer = loop {
            let next = (tokio::time::Instant::now() + crate::pairing::PAIRING_RESEND).min(deadline);
            tokio::time::sleep_until(next).await;
            let Some(peer) = shared.peer_manager.get(peer_id) else {
                return;
            };
            if peer.pairing_started_at != Some(started) {
                return; // answered, or a newer request owns the clock
            }
            if tokio::time::Instant::now() >= deadline {
                break peer;
            }
            if peer.outgoing_pairing_waiting {
                send_to_live_session(&shared, peer_id, pairing_request_message(&shared, peer_id))
                    .await;
            }
        };
        if peer.outgoing_pairing_waiting {
            send_to_live_session(&shared, peer_id, pairing_withdrawal(&shared)).await;
        }
        info!(peer_id = %peer_id, "pairing request expired");
        let _ = shared
            .peer_manager
            .end_pairing(peer_id, Some(PairingOutcome::Expired));
        notify_pairing_changed(&shared, peer_id).await;
    });
}
