//! Inbound remote file browsing and link-handoff messages.

use super::*;

pub(super) async fn handle(ctx: &InboundCtx, msg: AppMessage) -> Flow {
    let shared = &ctx.shared;
    let peer_id = ctx.peer_id;
    let peer_name = &ctx.peer_name;
    match msg {
        AppMessage::RemoteFilesQuery {
            request_id,
            origin_device,
            summary_only,
            category,
            source,
            search_query,
            offset,
            limit,
        } => {
            ctx.touch_last_seen();
            if origin_device != peer_id {
                return Flow::Continue;
            }
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
                .send(EngineEvent::RemoteFilesQueryReceived {
                    request_id,
                    from_device: peer_id,
                    summary_only,
                    category,
                    source,
                    search_query,
                    offset,
                    limit,
                })
                .await;
        }
        AppMessage::RemoteFilesResponse {
            request_id,
            summary,
            files,
            total_matching,
            error,
        } => {
            ctx.touch_last_seen();
            if let Some((_target, tx)) =
                shared.remote_waiters.files.lock().await.remove(&request_id)
            {
                let _ = tx.send(RemoteFilesResult {
                    summary: summary.clone(),
                    files: files.clone(),
                    total_matching,
                    error: error.clone(),
                });
            }
            let _ = shared
                .event_tx
                .send(EngineEvent::RemoteFilesResponseReceived {
                    request_id,
                    from_device: peer_id,
                    summary,
                    files,
                    total_matching,
                    error,
                })
                .await;
        }
        AppMessage::RemoteThumbnailRequest {
            request_id,
            origin_device,
            file_id,
            size_px,
        } => {
            ctx.touch_last_seen();
            if origin_device != peer_id {
                return Flow::Continue;
            }
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
                .send(EngineEvent::RemoteThumbnailRequestReceived {
                    request_id,
                    from_device: peer_id,
                    file_id,
                    size_px,
                })
                .await;
        }
        AppMessage::RemoteThumbnailResponse {
            request_id,
            file_id,
            data,
            error,
        } => {
            ctx.touch_last_seen();
            if let Some((_target, tx)) = shared
                .remote_waiters
                .thumbnails
                .lock()
                .await
                .remove(&request_id)
            {
                let _ = tx.send(RemoteThumbnailResult {
                    file_id,
                    data: data.clone(),
                    error: error.clone(),
                });
            }
            let _ = shared
                .event_tx
                .send(EngineEvent::RemoteThumbnailResponseReceived {
                    request_id,
                    from_device: peer_id,
                    file_id,
                    data,
                    error,
                })
                .await;
        }
        AppMessage::RemoteFilePullRequest {
            request_id,
            origin_device,
            file_id,
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
            if origin_device != peer_id {
                tracing::warn!(
                    peer_id = %peer_id,
                    claimed_device = %origin_device,
                    "ignoring RemoteFilePullRequest: origin_device does not match session peer"
                );
                return Flow::Continue;
            }
            let _ = shared
                .event_tx
                .send(EngineEvent::RemoteFilePullRequestReceived {
                    request_id,
                    from_device: peer_id,
                    file_id,
                })
                .await;
        }
        AppMessage::RemoteFileActionRequest {
            action,
            file_id,
            new_name,
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
                .send(EngineEvent::RemoteFileActionRequestReceived {
                    from_device: peer_id,
                    action,
                    file_id,
                    new_name,
                })
                .await;
        }
        AppMessage::OpenUrlOnDevice {
            url,
            origin_device: _,
            origin_device_name: _,
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
            // Restrict to http/https — this is the only case
            // actually asked for ("open a link"), and it closes
            // off URI-scheme-hijacking as a risk class entirely.
            if url.starts_with("http://") || url.starts_with("https://") {
                let _ = shared
                    .event_tx
                    .send(EngineEvent::OpenUrlOnDeviceRequested {
                        from_device: peer_id,
                        from_name: peer_name.clone(),
                        url,
                    })
                    .await;
            } else {
                let _ = ctx
                    .outbox_tx
                    .send(AppMessage::OpenUrlOnDeviceAck {
                        success: false,
                        error: Some("rejected: only http/https URLs are allowed".into()),
                    })
                    .await;
            }
        }
        AppMessage::OpenUrlOnDeviceAck { success, error } => {
            ctx.touch_last_seen();
            let _ = shared
                .event_tx
                .send(EngineEvent::OpenUrlOnDeviceAckReceived {
                    from_device: peer_id,
                    success,
                    error,
                })
                .await;
        }
        AppMessage::RemoteControl { action } => {
            ctx.touch_last_seen();
            let trusted = shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false);
            if !trusted {
                return Flow::Continue;
            }
            let allowed = shared.settings.lock().unwrap().allow_remote_control;
            let (success, error) = if !allowed {
                (
                    false,
                    Some("Remote control is off on this computer. Turn it on in Link All's settings.".to_string()),
                )
            } else if let Some(action) = crate::remote_control::Action::parse(&action) {
                match tokio::task::spawn_blocking(move || crate::remote_control::perform(action))
                    .await
                {
                    Ok(Ok(())) => (true, None),
                    Ok(Err(e)) => (false, Some(e)),
                    Err(e) => (false, Some(e.to_string())),
                }
            } else {
                (false, Some("That action isn't allowed.".to_string()))
            };
            let _ = ctx
                .outbox_tx
                .send(AppMessage::RemoteControlAck { success, error })
                .await;
        }
        AppMessage::RemoteControlAck { success, error } => {
            ctx.touch_last_seen();
            let _ = shared
                .event_tx
                .send(EngineEvent::RemoteControlAckReceived {
                    from_device: peer_id,
                    success,
                    error,
                })
                .await;
        }
        _ => unreachable!("dispatch_inbound routed a non-remote message here"),
    }
    Flow::Continue
}
