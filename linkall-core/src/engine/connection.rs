//! Establishing sessions: manual connect/reconnect/disconnect, the
//! inbound handshake, the outbound connect loop, and trust reconciliation.

use super::*;

impl Engine {
    pub async fn connect_to_peer(&self, ip: String, port: u16) -> Result<()> {
        let addr = SocketAddr::new(ip.parse().context("invalid peer IP")?, port);
        self.shared.peer_manager.note_manual_target(addr);
        match connect_once(
            self.shared.clone(),
            vec![addr],
            None,
            DiscoverySource::Manual,
            true, // Manual connection clears blocks
        )
        .await
        {
            Ok(()) => {
                self.shared.peer_manager.clear_manual_target(addr);
                Ok(())
            }
            Err(err) => {
                self.shared.peer_manager.record_manual_failure(addr);
                Err(err)
            }
        }
    }

    pub async fn reconnect_peer_by_id(&self, device_id: Uuid) -> Result<()> {
        let _ = self
            .shared
            .peer_manager
            .set_explicit_disconnect(device_id, false);

        let peer = self
            .shared
            .peer_manager
            .get(device_id)
            .context("peer not found")?;

        let endpoints = peer.socket_addrs();
        if endpoints.is_empty() {
            tracing::warn!("reconnect_peer_by_id: no endpoints known yet for {}, cleared explicit_disconnect for auto-discovery", device_id);
            return Ok(());
        }

        let shared = self.shared.clone();
        tokio::spawn(async move {
            tracing::debug!(
                peer_id = %device_id,
                endpoints = ?endpoints,
                "manual reconnect_peer_by_id triggered"
            );
            let _ = shared
                .peer_manager
                .mark_connecting(device_id, Some(endpoints[0]));
            if let Err(e) = connect_once(
                shared.clone(),
                endpoints,
                Some(device_id),
                DiscoverySource::Manual,
                true,
            )
            .await
            {
                tracing::warn!("Manual reconnect failed: {}", e);
                let _ = shared
                    .peer_manager
                    .mark_disconnected(device_id, Some(e.to_string()));
                let _ = shared
                    .event_tx
                    .send(EngineEvent::PeerDisconnected {
                        device_id,
                        device_name: None,
                        reason: Some(e.to_string()),
                    })
                    .await;
            }
        });

        Ok(())
    }

    pub async fn disconnect_peer(&self, device_id: Uuid) -> Result<bool> {
        let _ = self
            .shared
            .peer_manager
            .set_explicit_disconnect(device_id, true)?;

        // Drain pending RPC waiters immediately on explicit disconnect
        drain_remote_waiters(&self.shared, device_id).await;
        super::telemetry::clear_call_from(&self.shared, device_id).await;

        let session = self.shared.peer_manager.shutdown_peer_session(device_id)?;
        if let Some(session) = session {
            if let Some(shutdown_tx) = session.shutdown_tx {
                let _ = shutdown_tx.send(SessionShutdown {
                    reason: "manually disconnected".to_string(),
                    send_bye: true,
                    explicit_disconnect: true,
                });
            }
            let _ = self
                .shared
                .event_tx
                .send(EngineEvent::PeerDisconnected {
                    device_id,
                    device_name: self
                        .shared
                        .peer_manager
                        .get(device_id)
                        .map(|peer| Some(peer.friendly_name))
                        .unwrap_or(None),
                    reason: Some("manually disconnected".into()),
                })
                .await;
            return Ok(true);
        }
        Ok(false)
    }
}

#[tracing::instrument(skip_all, fields(device_id = tracing::field::Empty))]
pub(super) async fn handle_incoming(shared: EngineShared, mut stream: TcpStream) -> Result<()> {
    network::optimize_stream(&stream, "incoming engine stream");
    let shared_clone = shared.clone();
    let hs = network::handshake_responder(
        &mut stream,
        shared.config.device_id,
        shared.config.device_name.clone(),
        shared.identity_key.clone(),
        |peer_id, peer_identity| async move {
            let store = shared_clone.trust.lock().await;
            if store.is_trusted(peer_id) {
                return true;
            }
            if let Some(record) = store.get(peer_id) {
                if record.key_fingerprint == peer_identity {
                    return true;
                }
            }
            false
        },
    )
    .await?;

    tracing::Span::current().record("device_id", tracing::field::display(hs.peer_device_id));

    let _is_trusted_peer = shared.trust.lock().await.is_trusted(hs.peer_device_id);
    if hs.is_manual_reconnect {
        tracing::debug!("Manual reconnect initiated; clearing explicit disconnect");
        let _ = shared
            .peer_manager
            .set_explicit_disconnect(hs.peer_device_id, false);
    } else if shared
        .peer_manager
        .is_explicitly_disconnected(hs.peer_device_id)
    {
        anyhow::bail!(
            "ignoring inbound session from {} because it was explicitly disconnected",
            hs.peer_device_id
        );
    }

    if hs.peer_device_id == shared.config.device_id {
        anyhow::bail!("aborting inbound connect — cannot connect to self");
    }

    // A duplicate connection (e.g. both sides dialed at once) is resolved by
    // the deterministic tie-break in replace_live_session, never by "keep
    // whichever finished first": that choice differs per side, so each end
    // would keep a different TCP stream and close the other's live one.
    let endpoint = stream.peer_addr().context("reading remote address")?;
    let trusted = observe_trust(
        &shared,
        hs.peer_device_id,
        hs.peer_device_name.clone(),
        hs.peer_identity_pubkey_bytes,
        &hs.pin,
    )
    .await?;

    shared.peer_manager.upsert_peer(
        hs.peer_device_id,
        hs.peer_device_name.clone(),
        endpoint,
        trusted,
        DiscoverySource::Mdns,
    )?;
    shared.peer_manager.set_handshake_info(
        hs.peer_device_id,
        hs.peer_app_version,
        hs.peer_platform,
    );

    register_session(
        shared,
        stream,
        endpoint,
        hs.peer_device_id,
        hs.peer_device_name,
        hs.session,
        trusted,
        None, // the responder never learns the initiator's verdict
        DiscoverySource::Mdns,
        Some(hs.pin.display()),
        false, // is_outbound
    )
}

pub(super) async fn connect_loop(
    shared: EngineShared,
    endpoints: Vec<SocketAddr>,
    expected_device_id: Option<Uuid>,
    discovery: DiscoverySource,
) -> Result<()> {
    connect_loop_ext(shared, endpoints, expected_device_id, discovery, false).await
}

/// `connect_loop` for a dial the user asked for (pairing): `manual` marks the
/// handshake so a device that disconnected or forgot us still lets it in.
pub(super) async fn connect_loop_ext(
    shared: EngineShared,
    endpoints: Vec<SocketAddr>,
    expected_device_id: Option<Uuid>,
    discovery: DiscoverySource,
    manual: bool,
) -> Result<()> {
    if endpoints.is_empty() {
        return Ok(());
    }

    if let Some(device_id) = expected_device_id {
        if !shared
            .peer_manager
            .mark_connecting(device_id, Some(endpoints[0]))?
        {
            return Ok(());
        }
    }

    let mut backoff = Backoff::new(endpoints[0].to_string());
    loop {
        match connect_once(
            shared.clone(),
            endpoints.clone(),
            expected_device_id,
            discovery,
            manual,
        )
        .await
        {
            Ok(()) => {
                for ep in &endpoints {
                    shared.peer_manager.clear_manual_target(*ep);
                }
                return Ok(());
            }
            Err(err) => {
                if let Some(device_id) = expected_device_id {
                    let _ =
                        shared
                            .peer_manager
                            .mark_failed(device_id, endpoints[0], err.to_string());
                } else {
                    for ep in &endpoints {
                        shared.peer_manager.record_manual_failure(*ep);
                    }
                }

                match backoff.next() {
                    Some(delay) => {
                        warn!(error = %err, retry_in_ms = delay.as_millis(), "peer connect multi failed");
                        tokio::time::sleep(delay).await;
                    }
                    None => {
                        let message =
                            format!("connection to multiple endpoints failed after retries: {err}");
                        let _ = shared.event_tx.send(EngineEvent::Warning(message)).await;
                        if let Some(device_id) = expected_device_id {
                            let _ = shared
                                .peer_manager
                                .mark_failed_all(device_id, err.to_string());
                        }
                        return Err(err);
                    }
                }
            }
        }
    }
}

#[tracing::instrument(skip_all, fields(device_id = ?expected_device_id))]
pub(super) async fn connect_once(
    shared: EngineShared,
    endpoints: Vec<SocketAddr>,
    expected_device_id: Option<Uuid>,
    discovery: DiscoverySource,
    is_manual_reconnect: bool,
) -> Result<()> {
    if !is_manual_reconnect {
        if let Some(device_id) = expected_device_id {
            if shared.peer_manager.is_connected(device_id) {
                return Ok(());
            }
            if shared.peer_manager.is_explicitly_disconnected(device_id) {
                tracing::debug!(
                    peer_id = %device_id,
                    "aborting outbound connect — peer is explicitly disconnected"
                );
                return Ok(());
            }
        }
    }
    let started = Instant::now();
    let mut tasks = tokio::task::JoinSet::new();
    let timeout_dur = shared.config.connect_timeout;
    let delay_dur = std::time::Duration::from_millis(250);

    let mut connected_stream = None;
    let mut connected_endpoint = None;
    let mut last_err = None;

    let mut ep_iter = endpoints.into_iter();

    if let Some(first_ep) = ep_iter.next() {
        tasks.spawn(async move {
            tracing::warn!("connect_once: attempting to connect to {}", first_ep);
            let res = timeout(timeout_dur, TcpStream::connect(first_ep)).await;
            (first_ep, res)
        });
    }

    loop {
        if tasks.is_empty() && ep_iter.len() == 0 {
            break;
        }

        let mut spawn_next = false;

        if ep_iter.len() > 0 {
            tokio::select! {
                res = tasks.join_next(), if !tasks.is_empty() => {
                    match res {
                        Some(Ok((ep, Ok(Ok(stream))))) => {
                            connected_stream = Some(stream);
                            connected_endpoint = Some(ep);
                            break;
                        }
                        Some(Ok((_, Err(err)))) => {
                            last_err = Some(anyhow::anyhow!("timeout: {}", err));
                            spawn_next = true;
                        }
                        Some(Ok((_, Ok(Err(err))))) => {
                            last_err = Some(anyhow::anyhow!("io error: {}", err));
                            spawn_next = true;
                        }
                        _ => { spawn_next = true; }
                    }
                }
                _ = tokio::time::sleep(delay_dur) => {
                    spawn_next = true;
                }
            }
        } else {
            match tasks.join_next().await {
                Some(Ok((ep, Ok(Ok(stream))))) => {
                    connected_stream = Some(stream);
                    connected_endpoint = Some(ep);
                    break;
                }
                Some(Ok((_, Err(err)))) => {
                    last_err = Some(anyhow::anyhow!("timeout: {}", err));
                }
                Some(Ok((_, Ok(Err(err))))) => {
                    last_err = Some(anyhow::anyhow!("io error: {}", err));
                }
                _ => {}
            }
        }

        if spawn_next {
            if let Some(next_ep) = ep_iter.next() {
                tasks.spawn(async move {
                    tracing::warn!("connect_once: attempting to connect to {}", next_ep);
                    let res = timeout(timeout_dur, TcpStream::connect(next_ep)).await;
                    (next_ep, res)
                });
            }
        }
    }

    let mut stream = match connected_stream {
        Some(s) => s,
        None => {
            let err_msg =
                last_err.unwrap_or_else(|| anyhow::anyhow!("all connection attempts failed"));
            tracing::warn!("connect_once: all connection attempts failed: {}", err_msg);
            return Err(err_msg);
        }
    };
    let endpoint = connected_endpoint.unwrap();
    network::optimize_stream(&stream, "outgoing engine stream");

    // Always send the real device name — name is not a security concern.
    let name_to_send = &shared.config.device_name;

    let hs = network::handshake_initiator(
        &mut stream,
        shared.config.device_id,
        name_to_send,
        shared.identity_key.clone(),
        is_manual_reconnect,
    )
    .await?;

    if let Some(expected) = expected_device_id {
        anyhow::ensure!(
            expected == hs.peer_device_id,
            "peer identity changed during connect: expected {}, got {}",
            expected,
            hs.peer_device_id
        );
    }

    if hs.peer_device_id == shared.config.device_id {
        anyhow::bail!("aborting outbound connect — cannot connect to self");
    }

    // An incoming session accepted while we were handshaking is resolved by
    // replace_live_session's tie-break (see handle_incoming).
    let trusted = observe_trust(
        &shared,
        hs.peer_device_id,
        hs.peer_device_name.clone(),
        hs.peer_identity_pubkey_bytes,
        &hs.pin,
    )
    .await?;

    info!(
        peer_id = %hs.peer_device_id,
        peer_name = %hs.peer_device_name,
        addr = %endpoint,
        trusted,
        connect_ms = started.elapsed().as_millis(),
        "peer connected"
    );

    shared.peer_manager.upsert_peer(
        hs.peer_device_id,
        hs.peer_device_name.clone(),
        endpoint,
        trusted,
        discovery,
    )?;
    shared.peer_manager.set_handshake_info(
        hs.peer_device_id,
        hs.peer_app_version,
        hs.peer_platform,
    );

    register_session(
        shared,
        stream,
        endpoint,
        hs.peer_device_id,
        hs.peer_device_name,
        hs.session,
        trusted,
        Some(hs.peer_already_trusted),
        discovery,
        Some(hs.pin.display()),
        true, // is_outbound
    )
}

/// Trust is stored per side, so it can drift: one device reinstalls or
/// forgets the other while the other still remembers it. The session then
/// comes up, every trusted-only message is silently dropped, and neither
/// user is asked anything - the peer just looks broken. The initiator is
/// the only side that learns both verdicts (HelloAck carries the
/// responder's), so it turns the mismatch into a real pairing prompt on
/// whichever side lost trust.
pub(super) fn reconcile_one_sided_trust(
    shared: &EngineShared,
    peer_id: Uuid,
    peer_name: &str,
    we_trust_them: bool,
    they_trust_us: bool,
    pin: String,
) {
    match (we_trust_them, they_trust_us) {
        (true, false) => {
            // They forgot us. Ask them to pair again; register_session
            // delivers the request because outgoing_pairing_waiting is set.
            info!(peer_id = %peer_id, "peer no longer trusts us - requesting re-pair");
            let _ = shared
                .peer_manager
                .set_outgoing_pairing_waiting(peer_id, true);
            arm_pairing_expiry(shared, peer_id);
            let _ = shared
                .event_tx
                .try_send(EngineEvent::OutgoingPairingWaiting {
                    device_id: peer_id,
                    device_name: peer_name.to_string(),
                    pin,
                });
        }
        (false, true) => {
            // Our user is already pairing with them: the request
            // register_session delivers is auto-accepted by a peer that
            // trusts us, so there is nothing to ask.
            if shared
                .peer_manager
                .get(peer_id)
                .is_some_and(|p| p.outgoing_pairing_waiting)
            {
                return;
            }
            // We forgot them but they still remember us: ask our user,
            // exactly as if they had sent a PairingRequest.
            info!(peer_id = %peer_id, "peer still trusts us - prompting to re-pair");
            let _ = shared.peer_manager.set_pairing_requested(peer_id, true);
            arm_pairing_expiry(shared, peer_id);
            let _ = shared.event_tx.try_send(EngineEvent::PairingRequested {
                device_id: peer_id,
                device_name: peer_name.to_string(),
                pin,
            });
        }
        _ => {}
    }
}

pub(super) async fn observe_trust(
    shared: &EngineShared,
    device_id: Uuid,
    device_name: String,
    identity_pubkey: [u8; 32],
    _pin: &crate::pairing::PairingPin,
) -> Result<bool> {
    let record = {
        let mut trust = shared.trust.lock().await;
        trust.observe_peer(device_id, device_name.clone(), &identity_pubkey)?
    };

    match record.state {
        TrustState::Trusted => {
            shared.peer_manager.update_trust(device_id, true)?;
            Ok(true)
        }
        TrustState::Rejected => {
            shared.peer_manager.update_trust(device_id, false)?;
            anyhow::bail!("peer {} is not trusted ({:?})", device_id, record.state);
        }
        // Forgotten or revoked: no longer paired, but not blocked. It may
        // reconnect and ask to pair again like any nearby device (refusing
        // the connection left its user on "no answer" forever). Automatic
        // reconnects stay off through explicit_disconnect.
        TrustState::Revoked | TrustState::Untrusted => {
            shared.peer_manager.update_trust(device_id, false)?;

            // We NO LONGER emit PairingRequested or OutgoingPairingWaiting here.
            // Eager TCP connections should be silent.
            // Pairing prompts are now exclusively triggered by explicit
            // AppMessage::PairingRequest packets sent when the user taps 'Pair'.

            Ok(false)
        }
    }
}

pub(super) fn pairing_request_message(shared: &EngineShared, target_device: Uuid) -> AppMessage {
    AppMessage::PairingRequest {
        origin_device: shared.config.device_id,
        origin_device_name: shared.config.device_name.clone(),
        pin: shared
            .peer_manager
            .get(target_device)
            .and_then(|p| p.pairing_pin.clone()),
    }
}
