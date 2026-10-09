//! Listener and discovery supervision: server bind/rebind, mDNS
//! restarts, firewall-free discovery, and reacting to network changes.

use super::*;

pub(super) fn spawn_listener_supervisor(
    shared: EngineShared,
    mut rx: mpsc::Receiver<ListenerCommand>,
) {
    tokio::spawn(async move {
        let mut listener_task: Option<tokio::task::JoinHandle<()>> = None;

        while let Some(command) = rx.recv().await {
            match command {
                ListenerCommand::Rebind(addr) => {
                    if let Some(task) = listener_task.take() {
                        task.abort();
                        let _ = task.await;
                    }

                    match bind_server_with_retry(addr).await {
                        Ok(server) => {
                            {
                                let mut state = shared.network_state.lock().await;
                                if let Ok(local_addr) = server.local_addr() {
                                    state.bind_addr = local_addr;
                                }
                                state.listener_error = None;
                            }
                            let shared_clone = shared.clone();
                            listener_task = Some(tokio::spawn(async move {
                                run_server_loop(shared_clone, server).await;
                            }));
                        }
                        Err(err) => {
                            warn!(
                                addr = %addr,
                                error = %err,
                                "listener rebind failed after network change"
                            );
                            // The old listener is gone; until a rebind works,
                            // nothing can connect in (see engine::health).
                            shared.network_state.lock().await.listener_error =
                                Some(err.to_string());
                            // AddrNotAvailable here means the interface address
                            // this rebind targeted has already gone stale (the
                            // network changed again mid-transition) — the next
                            // NetworkChangeEvent will trigger another rebind
                            // with a current address, so this is self-healing.
                            // Surfacing it as a user-facing warning toast on
                            // every network flap (Wi-Fi/VPN reconnects, sleep
                            // wake) is just alarming noise with nothing for
                            // the user to act on.
                            if !is_addr_not_available(&err) {
                                let message = format!(
                                    "listener rebind to {addr} failed after network change: {err}"
                                );
                                let _ = shared.event_tx.send(EngineEvent::Warning(message)).await;
                            }
                        }
                    }
                }
            }
        }
    });
}

pub(super) fn spawn_discovery_supervisor(
    shared: EngineShared,
    mut rx: mpsc::Receiver<DiscoveryCommand>,
) {
    let (peer_tx, mut peer_rx) = mpsc::channel::<PeerEvent>(64);
    let peer_shared = shared.clone();

    tokio::spawn(async move {
        while let Some(event) = peer_rx.recv().await {
            match event {
                PeerEvent::Found(peer) => {
                    if let Err(err) = on_peer_found(peer_shared.clone(), peer).await {
                        warn!(error = %err, "peer discovery connect failed");
                    }
                }
                PeerEvent::Lost(device_id) => {
                    if peer_shared.peer_manager.is_connected(device_id) {
                        continue;
                    }
                    let name = peer_shared
                        .peer_manager
                        .get(device_id)
                        .map(|peer| peer.friendly_name.clone());

                    if let Some(peer) = peer_shared.peer_manager.get(device_id) {
                        if !peer.trusted && !peer.remembered {
                            let _ = peer_shared.peer_manager.forget_device(device_id);
                        } else {
                            let _ = peer_shared.peer_manager.mark_disconnected(
                                device_id,
                                Some("mDNS announcement lost".to_string()),
                            );
                        }
                    }
                    let _ = peer_shared
                        .event_tx
                        .send(EngineEvent::PeerDisconnected {
                            device_id,
                            device_name: name,
                            reason: Some("mDNS announcement lost".into()),
                        })
                        .await;
                }
            }
        }
    });

    tokio::spawn(async move {
        let mut current: Option<Discovery> = None;

        while let Some(command) = rx.recv().await {
            match command {
                DiscoveryCommand::Restart { bind_ip, port } => {
                    if let Some(discovery) = current.take() {
                        let _ = discovery.shutdown();
                    }

                    if bind_ip.is_unspecified() {
                        continue;
                    }

                    match Discovery::new(shared.config.device_id) {
                        Ok(discovery) => {
                            let advertised = discovery.advertise(
                                &shared.config.device_name,
                                port,
                                Some(bind_ip),
                            );
                            let browsed =
                                advertised.and_then(|_| discovery.browse(peer_tx.clone()));
                            match browsed {
                                Ok(()) => {
                                    current = Some(discovery);
                                }
                                Err(err) => {
                                    warn!(
                                        bind_ip = %bind_ip,
                                        port,
                                        error = %err,
                                        "discovery restart failed after network change"
                                    );
                                    // Same self-healing AddrNotAvailable case as
                                    // the listener rebind above: the next
                                    // network-change event retries with a
                                    // current address.
                                    if !is_addr_not_available(&err) {
                                        let message = format!(
                                            "discovery restart on {bind_ip}:{port} failed after network change: {err}"
                                        );
                                        let _ = shared
                                            .event_tx
                                            .send(EngineEvent::Warning(message))
                                            .await;
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            warn!(error = %err, "creating discovery daemon after network change failed");
                            if !is_addr_not_available(&err) {
                                let message = format!(
                                    "creating discovery daemon after network change failed: {err}"
                                );
                                let _ = shared.event_tx.send(EngineEvent::Warning(message)).await;
                            }
                        }
                    }
                }
            }
        }
    });
}

/// Spawn the firewall-free discovery pipeline: active outbound-only probing
/// that keeps working even when the OS firewall silently blocks unsolicited
/// inbound traffic (mDNS replies, UDP beacon replies) on locked-down
/// networks with no admin rights available to add an exception.
///
/// This is purely additive alongside `spawn_discovery_supervisor`'s mDNS
/// path — on networks where mDNS already works fine, this just finds the
/// same peers a second way and does nothing extra once they're connected.
pub(super) fn spawn_firewall_free_discovery(shared: EngineShared) {
    let (manager, discovery_handle, mut output_rx) = DiscoveryManager::new(shared.config.device_id);
    tokio::spawn(manager.run());

    spawn_lan_probe(
        shared.config.port,
        discovery_handle,
        shared.peer_manager.clone(),
        shared.local_sleeping.clone(),
    );

    tokio::spawn(async move {
        while let Some(event) = output_rx.recv().await {
            match event {
                DiscoveryEvent::PeerAppeared(peer) | DiscoveryEvent::PeerUpdated(peer) => {
                    if let Err(err) = on_peer_found_via(
                        shared.clone(),
                        peer.device_id,
                        peer.device_name,
                        peer.addrs,
                        peer.port,
                        peer.source,
                    )
                    .await
                    {
                        warn!(error = %err, "firewall-free discovery connect failed");
                    }
                }
                DiscoveryEvent::PeerDisappeared { .. } => {
                    // Active probes are re-swept periodically; a stale entry
                    // simply won't be re-reported. Nothing to do here — the
                    // mDNS path already owns disconnect/lost-peer handling.
                }
            }
        }
    });
}

pub(super) async fn run_server_loop(shared: EngineShared, server: Server) {
    loop {
        match server.accept().await {
            Ok(stream) => {
                let shared = shared.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle_incoming(shared, stream).await {
                        warn!(error = %err, "incoming connection failed");
                    }
                });
            }
            Err(err) => {
                error!(error = %err, "server accept error");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
}

/// Whether `err`'s source chain contains an `AddrNotAvailable` io error —
/// the OS declining to bind/join a local address that no longer exists on
/// any interface. Network-change handlers hit this transiently mid-transition
/// (the address computed for a rebind goes stale before the rebind runs);
/// the next `NetworkChangeEvent` retries with a current address, so it's
/// self-healing and not worth surfacing as a user-facing warning.
pub(super) fn is_addr_not_available(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| matches!(cause.downcast_ref::<std::io::Error>(), Some(io_err) if io_err.kind() == std::io::ErrorKind::AddrNotAvailable))
}

pub(super) async fn bind_server_with_retry(addr: SocketAddr) -> Result<Server> {
    let mut attempt = 0u32;

    loop {
        match Server::bind(addr).await {
            Ok(server) => return Ok(server),
            Err(err) if attempt < 11 => {
                attempt += 1;
                warn!(
                    addr = %addr,
                    error = %err,
                    attempt,
                    "listener bind failed during rebind, retrying"
                );
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(err) => return Err(err),
        }
    }
}

pub(super) async fn send_listener_rebind(
    shared: &EngineShared,
    bind_addr: SocketAddr,
) -> Result<()> {
    shared
        .listener_tx
        .send(ListenerCommand::Rebind(bind_addr))
        .await
        .map_err(|_| anyhow::anyhow!("listener supervisor stopped"))
}

pub(super) async fn handle_network_change(
    shared: EngineShared,
    change: NetworkChangeEvent,
) -> Result<()> {
    let _guard = shared.network_reconcile.lock().await;
    let previous_addr = change.previous.bind_addr;
    let current_addr = change.current.bind_addr;

    {
        let mut state = shared.network_state.lock().await;
        state.bind_addr = current_addr;
        state.active_interface = change.current.active_interface.clone();
    }

    let reason = format!(
        "network changed from {} to {} ({})",
        previous_addr,
        current_addr,
        describe_change_kinds(&change)
    );

    let should_rebind = previous_addr != current_addr;
    let should_shutdown_sessions = should_rebind
        || change
            .kinds
            .contains(&crate::network_manager::NetworkChangeKind::NetworkLost);

    if should_shutdown_sessions {
        let sessions = shared.peer_manager.shutdown_all_sessions(&reason)?;
        for session in sessions {
            if let Some(shutdown_tx) = session.shutdown_tx {
                let _ = shutdown_tx.send(SessionShutdown {
                    reason: reason.clone(),
                    send_bye: false,
                    explicit_disconnect: false,
                });
            }
        }
    }

    if should_rebind {
        send_listener_rebind(&shared, current_addr).await?;
    }

    let discovery_ip = {
        let state = shared.network_state.lock().await;
        state
            .active_interface
            .as_ref()
            .map(|i| i.ip)
            .unwrap_or(current_addr.ip())
    };
    if let Some(discovery_tx) = &shared.discovery_tx {
        let _ = discovery_tx
            .send(DiscoveryCommand::Restart {
                bind_ip: discovery_ip,
                port: shared.config.port,
            })
            .await;
    }

    let _ = shared
        .event_tx
        .send(EngineEvent::Warning(reason.clone()))
        .await;

    reconnect_known_peers(shared.clone()).await;
    Ok(())
}

pub(super) fn describe_change_kinds(change: &NetworkChangeEvent) -> String {
    change
        .kinds
        .iter()
        .map(|kind| match kind {
            network_manager::NetworkChangeKind::IpChanged => "ip_changed",
            network_manager::NetworkChangeKind::InterfaceChanged => "interface_changed",
            network_manager::NetworkChangeKind::NetworkLost => "network_lost",
            network_manager::NetworkChangeKind::NetworkRestored => "network_restored",
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub(super) async fn reconnect_known_peers(shared: EngineShared) {
    let local_ip = shared.network_state.lock().await.bind_addr.ip();
    let peers = shared.peer_manager.list();
    let mut scheduled = HashSet::new();

    for peer in peers {
        if is_obviously_local_peer(
            peer.id,
            &peer.friendly_name,
            peer.ips.first().cloned(),
            shared.config.device_id,
            &shared.config.device_name,
            Some(local_ip),
        ) {
            continue;
        }

        if !peer.should_auto_reconnect() {
            continue;
        }

        let endpoints = peer.socket_addrs();
        if !endpoints.is_empty() {
            if !should_initiate_session(&shared.peer_manager, peer.id, peer.discovery) {
                continue;
            }

            // CRITICAL FIX: Do not spawn a new connection loop if the peer is already
            // connected or currently connecting. Spawning unconditionally causes massive
            // connection storms (handshakes + diffie-hellman) which overheats mobile devices.
            let is_offline = peer.status == crate::peer_manager::PeerConnectionState::Disconnected
                || peer.status == crate::peer_manager::PeerConnectionState::Failed;
            if !is_offline {
                continue;
            }

            for &endpoint in &endpoints {
                scheduled.insert(endpoint);
            }
            let shared_clone = shared.clone();
            tokio::spawn(async move {
                if let Err(err) =
                    connect_loop(shared_clone, endpoints, Some(peer.id), peer.discovery).await
                {
                    warn!(peer_id = %peer.id, error = %err, "network-change reconnect failed");
                }
            });
        }
    }

    if scheduled.is_empty() {
        if let Some(endpoint) = guessed_hotspot_gateway_endpoint(&shared).await {
            shared.peer_manager.note_manual_target(endpoint);
            let shared_clone = shared.clone();
            tokio::spawn(async move {
                if let Err(err) =
                    connect_loop(shared_clone, vec![endpoint], None, DiscoverySource::Manual).await
                {
                    warn!(
                        addr = %endpoint,
                        error = %err,
                        "android-hotspot fallback connection failed"
                    );
                }
            });
            scheduled.insert(endpoint);
        }
    }

    for endpoint in shared.peer_manager.manual_targets() {
        if scheduled.contains(&endpoint) {
            continue;
        }

        let shared_clone = shared.clone();
        tokio::spawn(async move {
            if let Err(err) =
                connect_loop(shared_clone, vec![endpoint], None, DiscoverySource::Manual).await
            {
                warn!(addr = %endpoint, error = %err, "manual reconnect after network change failed");
            }
        });
    }
}

pub(super) async fn guessed_hotspot_gateway_endpoint(shared: &EngineShared) -> Option<SocketAddr> {
    let state = shared.network_state.lock().await.clone();
    let iface = state.active_interface.as_ref()?;
    let gateway = network_manager::detect_android_hotspot_gateway(iface)?;
    Some(SocketAddr::new(gateway, shared.config.port))
}

pub(super) fn resolve_bind_address(
    config: &EngineConfig,
) -> Result<(Option<NetworkInterfaceInfo>, SocketAddr)> {
    let snapshot = network_manager::resolve_snapshot(config.bind_ip, config.port)?;
    Ok((snapshot.active_interface, snapshot.bind_addr))
}

#[cfg(test)]
mod is_addr_not_available_tests {
    use super::*;

    #[test]
    fn detects_addr_not_available_through_context_chain() {
        let io_err = std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            "address not available",
        );
        let wrapped: anyhow::Error =
            anyhow::Error::new(io_err).context("binding to 10.0.0.5:47823");
        assert!(is_addr_not_available(&wrapped));
    }

    #[test]
    fn does_not_flag_other_io_errors() {
        let io_err = std::io::Error::new(std::io::ErrorKind::AddrInUse, "address in use");
        let wrapped: anyhow::Error =
            anyhow::Error::new(io_err).context("binding to 10.0.0.5:47823");
        assert!(!is_addr_not_available(&wrapped));
    }

    #[test]
    fn does_not_flag_non_io_errors() {
        let err = anyhow::anyhow!("mdns daemon failed for an unrelated reason");
        assert!(!is_addr_not_available(&err));
    }
}
