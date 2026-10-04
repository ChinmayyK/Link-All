//! Phone features relayed between devices: calls, battery, network and
//! storage status, notifications, and the camera stream.

use super::*;

pub(super) async fn handle(ctx: &InboundCtx, msg: AppMessage) -> Flow {
    let shared = &ctx.shared;
    let peer_id = ctx.peer_id;
    match msg {
        AppMessage::CallStateUpdate {
            state,
            number,
            contact_name,
            origin_device,
            origin_device_name,
        } => {
            ctx.touch_last_seen();
            // MED-03 FIX: Only process call state from trusted peers.
            if !shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false)
            {
                return Flow::Continue;
            }
            // Persist in shared state for IPC status polling. A repeat of
            // the same state only renews the call's lease (see CALL_LEASE).
            let changed = {
                let mut call = shared.device_status.active_call.lock().await;
                let repeat = call.as_ref().is_some_and(|c| {
                    c.device_id == origin_device
                        && c.state == state
                        && c.number == number
                        && c.contact_name == contact_name
                });
                if state == "idle" {
                    call.take().is_some()
                } else if repeat {
                    if let Some(c) = call.as_mut() {
                        c.heard_at = std::time::Instant::now();
                    }
                    false
                } else {
                    *call = Some(ActiveCallState {
                        device_id: origin_device,
                        device_name: origin_device_name.clone(),
                        state: state.clone(),
                        number: number.clone(),
                        contact_name: contact_name.clone(),
                        heard_at: std::time::Instant::now(),
                    });
                    true
                }
            };
            if !changed {
                return Flow::Continue;
            }
            let _ = shared
                .event_tx
                .send(EngineEvent::CallStateChanged {
                    from_device: origin_device,
                    from_name: origin_device_name,
                    state,
                    number,
                    contact_name,
                })
                .await;
        }
        AppMessage::BatteryStatus {
            level,
            charging,
            origin_device,
            origin_device_name,
        } => {
            ctx.touch_last_seen();
            tracing::info!(
                "Received BatteryStatus: level={}, charging={} from {}",
                level,
                charging,
                origin_device
            );
            // MED-03 FIX: Only process battery status from trusted peers.
            if !shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false)
            {
                tracing::warn!("Ignoring BatteryStatus from untrusted peer {}", peer_id);
                return Flow::Continue;
            }
            // Persist in shared state for IPC status polling.
            {
                shared.device_status.peer_batteries.insert(
                    origin_device,
                    PeerBatteryState {
                        device_id: origin_device,
                        device_name: origin_device_name.clone(),
                        level,
                        charging,
                    },
                );
            }
            let _ = shared
                .event_tx
                .send(EngineEvent::BatteryStateChanged {
                    from_device: origin_device,
                    from_name: origin_device_name,
                    level,
                    charging,
                })
                .await;
        }
        AppMessage::NetworkStatus {
            network_type,
            origin_device,
            origin_device_name,
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
                shared.device_status.peer_networks.insert(
                    origin_device,
                    PeerNetworkState {
                        device_id: origin_device,
                        device_name: origin_device_name.clone(),
                        network_type: network_type.clone(),
                    },
                );
            }
            let _ = shared
                .event_tx
                .send(EngineEvent::NetworkStateChanged {
                    from_device: origin_device,
                    from_name: origin_device_name,
                    network_type,
                })
                .await;
        }
        AppMessage::StorageStatus {
            images_bytes,
            videos_bytes,
            apps_bytes,
            free_bytes,
            total_bytes,
            origin_device,
            origin_device_name,
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
                shared.device_status.peer_storage.insert(
                    origin_device,
                    PeerStorageState {
                        device_id: origin_device,
                        device_name: origin_device_name.clone(),
                        images_bytes,
                        videos_bytes,
                        apps_bytes,
                        free_bytes,
                        total_bytes,
                    },
                );
            }
        }
        AppMessage::CallAction {
            action,
            origin_device,
        } => {
            ctx.touch_last_seen();
            if action == "system:explicit_disconnect" {
                tracing::info!("Peer explicitly disconnected. Pausing auto-reconnect.");
                let _ = shared.peer_manager.set_explicit_disconnect(peer_id, true);
                return Flow::Disconnect("explicitly disconnected by peer".to_string());
            }
            tracing::info!("Received CallAction: {} from {:?}", action, origin_device);
            let _ = shared
                .event_tx
                .send(EngineEvent::CallActionRequest {
                    action,
                    from_device: origin_device,
                })
                .await;
        }
        AppMessage::NotificationRelay {
            id,
            package,
            title,
            text,
            origin_device,
            origin_device_name,
        } => {
            ctx.touch_last_seen();
            // MED-03 FIX: Only process notifications from trusted peers.
            if !shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false)
            {
                return Flow::Continue;
            }
            let _activity_id = {
                let mut feed = shared.activity.lock().await;
                feed.record_remote_notification(
                    origin_device,
                    origin_device_name.clone(),
                    package.clone(),
                    title.clone(),
                    text.clone(),
                )
            };
            let _ = shared
                .event_tx
                .send(EngineEvent::NotificationReceived {
                    id,
                    package,
                    title,
                    text,
                    from_device: origin_device,
                    from_name: origin_device_name,
                })
                .await;
        }
        AppMessage::CameraStreamRequest { origin_device } => {
            ctx.touch_last_seen();
            if !shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false)
            {
                return Flow::Continue;
            }
            let _ = shared
                .event_tx
                .send(EngineEvent::CameraStreamRequest {
                    from_device: origin_device,
                })
                .await;
        }
        AppMessage::CameraStreamAccept {
            origin_device,
            accepted,
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
            let _ = shared
                .event_tx
                .send(EngineEvent::CameraStreamAccept {
                    from_device: origin_device,
                    accepted,
                })
                .await;
        }
        AppMessage::CameraStreamStop { origin_device } => {
            ctx.touch_last_seen();
            if !shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false)
            {
                return Flow::Continue;
            }
            shared.camera_frames.remove(&origin_device);
            let _ = shared
                .event_tx
                .send(EngineEvent::CameraStreamStop {
                    from_device: origin_device,
                })
                .await;
        }
        AppMessage::CameraFrame {
            origin_device,
            data,
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
            shared.camera_frames.insert(origin_device, data);
            let _ = shared
                .event_tx
                .send(EngineEvent::CameraFrameReceived {
                    from_device: origin_device,
                })
                .await;
        }
        _ => unreachable!("dispatch_inbound routed a non-device message here"),
    }
    Flow::Continue
}
