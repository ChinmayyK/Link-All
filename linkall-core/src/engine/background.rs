//! Long-running engine tasks: UDP beacon and listener, peer and
//! history pruners, auto-reconnector, and the network monitor.

use super::*;

impl Engine {
    pub(super) fn spawn_udp_beacon(&self) {
        let shared = self.shared.clone();
        tokio::spawn(async move {
            let socket = match tokio::net::UdpSocket::bind("0.0.0.0:0").await {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!(error = %err, "failed to bind UDP beacon socket");
                    return;
                }
            };
            if let Err(err) = socket.set_broadcast(true) {
                tracing::warn!(error = %err, "failed to set broadcast flag on UDP beacon socket");
            }

            // TRU-06: Do NOT include device_name in the beacon — only opaque UUIDs
            // are broadcast. The friendly device name is exchanged only after a
            // successful encrypted handshake via HelloFrame/HelloAck.
            // Format: DESKDROP_BEACON:<uuid>:<tcp_port>:<protocol_version>
            // The port we actually listen on: a configured 0 means "any", and
            // advertising 0 sent peers dialing nowhere.
            let port = loop {
                let port = shared.network_state.lock().await.bind_addr.port();
                if port != 0 {
                    break port;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            };
            let payload = format!(
                "DESKDROP_BEACON:{}:{}:{}",
                shared.config.device_id,
                port,
                crate::protocol::PROTOCOL_VERSION
            )
            .into_bytes();

            let broadcast_addr: SocketAddr =
                "255.255.255.255:47824".parse().expect("static IP is valid");

            // ── AirDrop-style startup burst ──────────────────────────────────
            // Send 3 rapid beacons in the first 300ms so peers discover us
            // almost instantly, then fall back to the regular interval.
            if network_manager::has_local_network() {
                for _ in 0..3 {
                    let _ = socket.send_to(&payload, broadcast_addr).await;
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }

            // Beacon fast for a short window after start, or after the last
            // peer drops, so first pairing and reconnects are quick. Outside
            // that window there's no rush: a newly arriving device beacons
            // fast itself and we answer it, so a slow cadence here costs
            // little latency but saves waking every phone on the LAN (each
            // beacon arrives as two broadcasts). mDNS also announces us.
            const BEACON_FAST_INTERVAL: tokio::time::Duration =
                tokio::time::Duration::from_millis(1500);
            const BEACON_STEADY_INTERVAL: tokio::time::Duration =
                tokio::time::Duration::from_secs(60);
            const BEACON_FAST_WINDOW: tokio::time::Duration = tokio::time::Duration::from_secs(60);

            let mut fast_until = tokio::time::Instant::now() + BEACON_FAST_WINDOW;
            let mut had_peers = false;
            loop {
                // A fresh `tokio::time::interval` completes its first tick at
                // once, so rebuilding one per pass (as this loop used to)
                // broadcast back to back with no wait at all. Sleep instead.
                let has_peers = shared.peer_manager.connected_count() > 0;
                if had_peers && !has_peers {
                    fast_until = tokio::time::Instant::now() + BEACON_FAST_WINDOW;
                }
                had_peers = has_peers;
                let wait = if !has_peers && tokio::time::Instant::now() < fast_until {
                    BEACON_FAST_INTERVAL
                } else {
                    BEACON_STEADY_INTERVAL
                };
                tokio::time::sleep(wait).await;
                // On mobile data alone no device can hear us; broadcasting
                // would only go out over cellular and wake its radio.
                if !network_manager::has_local_network() {
                    continue;
                }
                // Send to limited broadcast address.
                if let Err(err) = socket.send_to(&payload, broadcast_addr).await {
                    tracing::trace!(error = %err, "failed to send UDP beacon");
                }
                // Also send to subnet-directed broadcast addresses for better
                // delivery on networks that filter limited broadcast.
                if let Ok(ifaces) = if_addrs::get_if_addrs() {
                    for iface in ifaces {
                        if iface.is_loopback() || network_manager::looks_like_cellular(&iface.name)
                        {
                            continue;
                        }
                        if let if_addrs::IfAddr::V4(v4) = &iface.addr {
                            if let Some(bcast) = v4.broadcast {
                                let dest = SocketAddr::new(std::net::IpAddr::V4(bcast), 47824);
                                if dest != broadcast_addr {
                                    let _ = socket.send_to(&payload, dest).await;
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    pub(super) fn spawn_udp_listener(&self) {
        let shared = self.shared.clone();
        tokio::spawn(async move {
            let socket = match tokio::net::UdpSocket::bind("0.0.0.0:47824").await {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!(error = %err, "failed to bind UDP listener socket on port 47824");
                    return;
                }
            };
            let socket = Arc::new(socket);
            let mut buf = vec![0u8; 1024];
            // Each beacon arrives twice (limited + subnet broadcast), and
            // older builds sent them back to back with no delay. Handle at
            // most one per peer per second; the rest carry nothing new.
            const BEACON_MIN_GAP: Duration = Duration::from_secs(1);
            let mut last_beacon: std::collections::HashMap<Uuid, Instant> =
                std::collections::HashMap::new();
            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((len, addr)) => {
                        let text = String::from_utf8_lossy(&buf[..len]);

                        // ── Handle CONNECTBACK requests ──────────────────────
                        // A peer whose outbound TCP failed is asking US to
                        // initiate the connection to THEM instead.
                        if text.starts_with("DESKDROP_CONNECTBACK:") {
                            let parts: Vec<&str> = text.splitn(4, ':').collect();
                            if parts.len() < 3 {
                                continue;
                            }
                            let peer_id = match uuid::Uuid::parse_str(parts[1]) {
                                Ok(id) => id,
                                Err(_) => continue,
                            };
                            if peer_id == shared.config.device_id {
                                continue;
                            }
                            let peer_port = match parts[2].parse::<u16>() {
                                Ok(p) if p != 0 => p, // nothing to dial
                                _ => continue,
                            };
                            let peer_addr = SocketAddr::new(addr.ip(), peer_port);

                            // Skip if already connected to this peer.
                            if shared.peer_manager.is_connected(peer_id) {
                                continue;
                            }
                            // Strangers get the same dial cooldown as beacons.
                            let has_relationship = shared
                                .peer_manager
                                .get(peer_id)
                                .map(|r| {
                                    r.trusted
                                        || r.remembered
                                        || r.pairing_requested
                                        || r.outgoing_pairing_waiting
                                })
                                .unwrap_or(false);
                            if !has_relationship
                                && !allow_discovery_connect_attempt(peer_id, peer_addr)
                            {
                                continue;
                            }

                            tracing::info!(
                                "UDP CONNECTBACK: peer {} at {} is asking us to connect to them",
                                peer_id,
                                peer_addr
                            );
                            let shared_clone = shared.clone();
                            tokio::spawn(async move {
                                if let Err(err) = connect_once(
                                    shared_clone,
                                    vec![peer_addr],
                                    Some(peer_id),
                                    DiscoverySource::UdpBeacon,
                                    false,
                                )
                                .await
                                {
                                    tracing::warn!(
                                        peer_id = %peer_id,
                                        error = %err,
                                        "CONNECTBACK connection attempt failed"
                                    );
                                }
                            });
                            continue;
                        }

                        if !text.starts_with("DESKDROP_BEACON:") {
                            continue;
                        }
                        let parts: Vec<&str> = text.splitn(5, ':').collect();
                        // Accept both new format (4 fields: magic:uuid:port:version)
                        // and legacy format (4 fields: magic:uuid:port:name).
                        if parts.len() < 3 {
                            continue;
                        }
                        let peer_id = match uuid::Uuid::parse_str(parts[1]) {
                            Ok(id) => id,
                            Err(_) => continue,
                        };
                        if peer_id == shared.config.device_id {
                            continue;
                        }
                        let now = Instant::now();
                        if last_beacon
                            .get(&peer_id)
                            .is_some_and(|t| now.duration_since(*t) < BEACON_MIN_GAP)
                        {
                            continue;
                        }
                        if last_beacon.len() > 1000 {
                            last_beacon.retain(|_, t| now.duration_since(*t) < BEACON_MIN_GAP);
                        }
                        last_beacon.insert(peer_id, now);
                        let peer_port = match parts[2].parse::<u16>() {
                            Ok(p) if p != 0 => p, // nothing to dial
                            _ => continue,
                        };
                        // Protocol version check: if the 4th field parses as a
                        // small integer, treat it as a version. Otherwise, treat
                        // it as a legacy device name (backward compatibility).
                        let peer_name;
                        if parts.len() >= 4 {
                            if let Ok(version) = parts[3].parse::<u16>() {
                                // New format — check protocol compatibility.
                                if version != crate::protocol::PROTOCOL_VERSION {
                                    tracing::debug!(
                                        "UDP beacon: skipping peer {} with protocol v{} (we speak v{})",
                                        peer_id, version, crate::protocol::PROTOCOL_VERSION
                                    );
                                    continue;
                                }
                                peer_name = format!("device-{}", &peer_id.to_string()[..8]);
                            } else {
                                // Legacy format — 4th field is device name.
                                peer_name = parts[3].to_string();
                            }
                        } else {
                            peer_name = format!("device-{}", &peer_id.to_string()[..8]);
                        }

                        let peer_addr = SocketAddr::new(addr.ip(), peer_port);

                        let trusted = {
                            let trust_guard = shared.trust.lock().await;
                            trust_guard.is_trusted(peer_id)
                        };

                        if let Err(err) = shared.peer_manager.upsert_peer(
                            peer_id,
                            peer_name.clone(),
                            peer_addr,
                            trusted,
                            DiscoverySource::UdpBeacon,
                        ) {
                            tracing::warn!(error = %err, "failed to upsert UDP beacon peer");
                        } else {
                            if !should_initiate_session(
                                &shared.peer_manager,
                                peer_id,
                                DiscoverySource::UdpBeacon,
                            ) {
                                continue;
                            }
                            if shared.peer_manager.live_endpoint(peer_id) == Some(peer_addr) {
                                continue;
                            }
                            let record = shared.peer_manager.get(peer_id);
                            if matches!(
                                &record,
                                Some(record) if record.status == PeerConnectionState::Connecting && record.socket_addrs().contains(&peer_addr)
                            ) {
                                continue;
                            }
                            // A stranger we can't reach beacons on regardless;
                            // without this, every beacon after a failed
                            // connect_loop started another one - on a busy
                            // office LAN, hundreds of doomed dials a minute.
                            // Same cooldown the mDNS path applies.
                            let has_relationship = trusted
                                || record
                                    .as_ref()
                                    .map(|r| {
                                        r.remembered
                                            || r.pairing_requested
                                            || r.outgoing_pairing_waiting
                                    })
                                    .unwrap_or(false);
                            if !has_relationship
                                && !allow_discovery_connect_attempt(peer_id, peer_addr)
                            {
                                continue;
                            }

                            tracing::info!(
                                "UDP Beacon discovered peer {} at {}",
                                peer_id,
                                peer_addr
                            );

                            // ── Fast Connect-Back ────────────────────────────
                            // Send UDP CONNECTBACK immediately so the remote peer
                            // can connect to us concurrently with our outbound
                            // TCP connect attempt. This eliminates delay when
                            // asymmetric routing / AP isolation blocks outbound TCP.
                            if !shared.peer_manager.is_connected(peer_id) {
                                let our_port = shared.network_state.lock().await.bind_addr.port();
                                let connectback = format!(
                                    "DESKDROP_CONNECTBACK:{}:{}",
                                    shared.config.device_id, our_port,
                                );
                                let target = SocketAddr::new(
                                    addr.ip(),
                                    47824, // UDP beacon port
                                );
                                let _ = socket.send_to(connectback.as_bytes(), target).await;
                            }

                            let shared_clone = shared.clone();
                            let socket_clone = socket.clone();
                            let beacon_source_addr = addr;
                            tokio::spawn(async move {
                                if let Err(err) = connect_loop(
                                    shared_clone.clone(),
                                    vec![peer_addr],
                                    Some(peer_id),
                                    DiscoverySource::UdpBeacon,
                                )
                                .await
                                {
                                    tracing::warn!(peer_id = %peer_id, error = %err, "UDP beacon peer connection failed");

                                    if !shared_clone.peer_manager.is_connected(peer_id) {
                                        let our_port = shared_clone
                                            .network_state
                                            .lock()
                                            .await
                                            .bind_addr
                                            .port();
                                        let connectback = format!(
                                            "DESKDROP_CONNECTBACK:{}:{}",
                                            shared_clone.config.device_id, our_port,
                                        );
                                        let target =
                                            SocketAddr::new(beacon_source_addr.ip(), 47824);
                                        let _ = socket_clone
                                            .send_to(connectback.as_bytes(), target)
                                            .await;
                                    }
                                }
                            });
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "UDP listener recv_from failed");
                        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                    }
                }
            }
        });
    }

    // ── Call Continuity ───────────────────────────────────────────────────────

    // ── F20: Battery synchronization ──────────────────────────────────────────

    // ── Activity Feed ─────────────────────────────────────────────────────────

    // ── Settings ──────────────────────────────────────────────────────────────

    /// Spawn a background task that periodically prunes transient, untrusted
    /// peer records to prevent unbounded memory/disk growth (MED-05).
    pub(super) fn spawn_peer_pruner(&self) {
        let peer_manager = self.shared.peer_manager.clone();
        let trust = self.shared.trust.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 minutes
            interval.tick().await; // skip the first immediate tick
            loop {
                interval.tick().await;
                peer_manager.prune_stale_peers();

                // Also prune stale trust records (Untrusted/Rejected not seen in 7 days)
                const TRUST_MAX_AGE: u64 = 7 * 24 * 3600;
                match trust.lock().await.prune_stale(TRUST_MAX_AGE) {
                    Ok(n) if n > 0 => {
                        tracing::info!(
                            pruned = n,
                            "pruned stale trust records (untrusted, >7 days old)"
                        );
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "trust store pruning failed");
                    }
                    _ => {}
                }
            }
        });
    }

    /// Ends a call its phone stopped reporting (see `CALL_LEASE`), so every
    /// desktop drops it even if the phone's "idle" was lost.
    pub(super) fn spawn_call_lease_watch(&self) {
        let shared = self.shared.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let stale = shared
                    .device_status
                    .active_call
                    .lock()
                    .await
                    .as_ref()
                    .filter(|c| c.expired())
                    .map(|c| c.device_id);
                if let Some(device) = stale {
                    tracing::info!("call from {device} not heard of for a while; ending it");
                    super::telemetry::clear_call_from(&shared, device).await;
                }
            }
        });
    }

    pub(super) fn spawn_sensitive_history_pruner(&self) {
        let history = self.shared.history.clone();
        let settings = self.shared.settings.clone();
        // Only prune the cache directory, leaving user's downloaded files safely intact.
        let cache_dir = self.shared.config.data_dir.join("cache");
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut disk_prune_counter = 0;
            loop {
                interval.tick().await;
                let mut hist = history.lock().await;
                if let Err(err) = hist.purge_expired_sensitive_entries() {
                    tracing::warn!(error = %err, "sensitive history pruning failed");
                }

                disk_prune_counter += 1;
                // Run the expensive filesystem scan only once every 10 minutes (60 ticks of 10s)
                if disk_prune_counter >= 60 {
                    disk_prune_counter = 0;
                    let retention_days = settings.lock().unwrap().history_retention_days;
                    if retention_days > 0 {
                        if let Err(err) = hist.purge_expired_retention(retention_days) {
                            tracing::warn!(error = %err, "retention history pruning failed");
                        }

                        let cache_dir_clone = cache_dir.clone();
                        tokio::task::spawn_blocking(move || {
                            let cutoff = std::time::SystemTime::now()
                                .checked_sub(Duration::from_secs(retention_days * 86400));

                            if let Some(cutoff_time) = cutoff {
                                if let Ok(entries) = std::fs::read_dir(&cache_dir_clone) {
                                    for entry in entries.flatten() {
                                        if let Ok(meta) = entry.metadata() {
                                            if meta.is_file() {
                                                if let Ok(modified) = meta.modified() {
                                                    if modified < cutoff_time {
                                                        if let Err(e) =
                                                            std::fs::remove_file(entry.path())
                                                        {
                                                            tracing::warn!(
                                                                "Failed to prune old file {:?}: {}",
                                                                entry.path(),
                                                                e
                                                            );
                                                        } else {
                                                            tracing::info!(
                                                                "Pruned old hoarded file {:?}",
                                                                entry.path()
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        });
                    }
                }
            }
        });
    }

    /// Background watchdog to aggressively reconnect to known endpoints for trusted
    /// peers that drop offline. Bypasses the need for mDNS discovery to trigger a reconnect.
    pub(super) fn spawn_auto_reconnector(&self) {
        let shared = self.shared.clone();
        tokio::spawn(async move {
            // Retrying an unreachable peer every few seconds forever kept a
            // phone's radio (Wi-Fi, or cellular when away from home) from
            // ever idling. Back off per peer, exponentially, to a cap;
            // network changes still reconnect at once via
            // reconnect_known_peers, which doesn't go through here.
            const RETRY_BASE: Duration = Duration::from_secs(5);
            const RETRY_CAP: Duration = Duration::from_secs(5 * 60);
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            // Per peer: earliest next attempt and attempts made so far.
            let mut backoff: std::collections::HashMap<uuid::Uuid, (tokio::time::Instant, u32)> =
                std::collections::HashMap::new();
            loop {
                interval.tick().await;
                // Known peers live on a local network. With mobile data alone
                // a dial goes out over cellular to an address it cannot reach;
                // the network monitor reconnects at once when Wi-Fi returns.
                if !network_manager::has_local_network() {
                    continue;
                }
                let peers = shared.peer_manager.list();
                let sleeping = shared
                    .local_sleeping
                    .load(std::sync::atomic::Ordering::Relaxed);
                for peer in peers {
                    // Only consider peers that are not currently connected.
                    let is_offline = peer.status
                        == crate::peer_manager::PeerConnectionState::Disconnected
                        || peer.status == crate::peer_manager::PeerConnectionState::Failed;
                    if !is_offline {
                        if peer.status == crate::peer_manager::PeerConnectionState::Connected {
                            backoff.remove(&peer.id);
                        }
                        continue;
                    }
                    // Must be trusted + remembered + auto_connect.
                    if !peer.trusted || !peer.remembered || !peer.auto_connect {
                        continue;
                    }
                    // If explicit_disconnect was set by user, respect it.
                    if peer.explicit_disconnect {
                        continue;
                    }
                    let now = tokio::time::Instant::now();
                    let attempts = match backoff.get(&peer.id) {
                        Some(&(next, _)) if now < next => continue,
                        Some(&(_, attempts)) => attempts,
                        None => 0,
                    };
                    let endpoints = peer.socket_addrs();
                    if !endpoints.is_empty() {
                        let mut delay = RETRY_BASE
                            .saturating_mul(1u32 << attempts.min(6))
                            .min(RETRY_CAP);
                        if sleeping {
                            delay = RETRY_CAP;
                        }
                        backoff.insert(peer.id, (now + delay, attempts.saturating_add(1)));
                        let shared_clone = shared.clone();
                        let peer_id = peer.id;
                        let discovery = peer.discovery;
                        tokio::spawn(async move {
                            tracing::debug!(
                                peer_id = %peer_id,
                                endpoints = ?endpoints,
                                "auto-reconnector: attempting reconnection"
                            );
                            let _ = connect_once(
                                shared_clone,
                                endpoints,
                                Some(peer_id),
                                discovery,
                                false,
                            )
                            .await;
                        });
                    }
                }
            }
        });
    }

    pub(super) async fn spawn_network_monitor(&self) -> Result<()> {
        let (mut changes, hint) = network_manager::spawn_network_monitor(
            self.shared.config.bind_ip,
            self.shared.config.port,
            self.shared.config.network_poll_interval,
        )?;
        let _ = self.shared.network_hint.set(hint);
        let shared = self.shared.clone();

        // MED-02: task panics inside tokio::spawn are silently swallowed.
        // We attach a `JoinHandle` watcher that logs the panic payload before
        // the engine continues running without its network monitor.
        let handle = tokio::spawn(async move {
            while let Some(change) = changes.recv().await {
                if let Err(err) = handle_network_change(shared.clone(), change).await {
                    warn!(error = %err, "network change handling failed");
                }
            }
        });
        tokio::spawn(async move {
            if let Err(panic) = handle.await {
                error!(error = ?panic, "network monitor task panicked — daemon may miss interface changes");
            }
        });

        Ok(())
    }
}
