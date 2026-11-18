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
            let allowed = shared
                .settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .allow_remote_control;
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
        AppMessage::FindDevice { ring } => {
            ctx.touch_last_seen();
            let trusted = shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false);
            if !trusted {
                return Flow::Continue;
            }
            let (success, error) = if cfg!(target_os = "android") {
                if !shared
                    .settings
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .allow_find_device
                {
                    (
                        false,
                        Some(
                            "Finding is off on this phone. Turn it on in Link All's settings."
                                .to_string(),
                        ),
                    )
                } else {
                    let _ = shared
                        .event_tx
                        .send(EngineEvent::FindDeviceRequested {
                            from_device: peer_id,
                            ring,
                        })
                        .await;
                    (true, None)
                }
            } else if !shared
                .settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .allow_remote_control
            {
                (
                    false,
                    Some("Remote control is off on this computer. Turn it on in Link All's settings.".to_string()),
                )
            } else {
                match tokio::task::spawn_blocking(move || crate::remote_control::ring(ring)).await {
                    Ok(Ok(())) => (true, None),
                    Ok(Err(e)) => (false, Some(e)),
                    Err(e) => (false, Some(e.to_string())),
                }
            };
            let _ = ctx
                .outbox_tx
                .send(AppMessage::FindDeviceAck { success, error })
                .await;
        }
        AppMessage::FindDeviceAck { success, error } => {
            ctx.touch_last_seen();
            if let Some(waiter) = shared.remote_waiters.find.lock().await.remove(&peer_id) {
                let _ = waiter.send(if success {
                    Ok(())
                } else {
                    Err(error.unwrap_or_else(|| "It didn't ring.".into()))
                });
            }
        }
        AppMessage::RemotePointerMove { dx, dy } => {
            // During a hand-off the edge check needs the pointer before and
            // after the move. Reading it can block (X11), so it happens in the
            // same blocking call as the move.
            let edge = super::super::edge_ops::controlled_by(&ctx.shared, ctx.peer_id);
            let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
            input(ctx, move || {
                use crate::remote_input::{cursor_position, pointer_move, screen_size};
                if edge {
                    crate::remote_input::heard_from_driver();
                }
                let before = if edge { cursor_position() } else { None };
                let moved = pointer_move(dx.into(), dy.into());
                if edge {
                    let _ = seen_tx.send((before, cursor_position(), screen_size()));
                }
                moved
            })
            .await;
            if let Ok((before, after, size)) = seen_rx.await {
                super::super::edge_ops::after_move(ctx, dx, dy, before, after, size).await;
            }
        }
        AppMessage::RemotePointerButton { button, down } => {
            match crate::remote_input::Button::from_wire(button) {
                Some(button) => {
                    let first = edge_first(ctx);
                    input(ctx, move || {
                        super::super::edge_ops::play(&first)?;
                        crate::remote_input::pointer_button(button, down)
                    })
                    .await
                }
                None => tracing::debug!(button, "ignoring an unknown mouse button"),
            }
        }
        AppMessage::RemotePointerScroll { dx, dy } => {
            let first = edge_first(ctx);
            input(ctx, move || {
                super::super::edge_ops::play(&first)?;
                crate::remote_input::scroll(dx.into(), dy.into())
            })
            .await;
        }
        AppMessage::EdgeEnter { edge, pos } => {
            super::super::edge_ops::on_enter(ctx, edge, pos).await
        }
        AppMessage::EdgeReply { accepted, error } => {
            super::super::edge_ops::on_reply(ctx, accepted, error).await
        }
        AppMessage::EdgeLeave { pos } => super::super::edge_ops::on_leave(ctx, pos).await,
        AppMessage::EdgeEnd => super::super::edge_ops::on_end(ctx).await,
        AppMessage::RemoteKeyEvent { scancode, down } => {
            // Only the computer whose hand-off is running may press keys.
            if super::super::edge_ops::controlled_by(&ctx.shared, ctx.peer_id) {
                let acts = super::super::edge_ops::key_acts(scancode, down);
                input(ctx, move || super::super::edge_ops::play(&acts)).await;
            }
        }
        AppMessage::RemoteTypeText { text } => {
            input(ctx, move || crate::remote_input::type_text(&text)).await;
        }
        AppMessage::RemoteKeyPress { key } => match crate::remote_input::Key::parse(&key) {
            Some(key) => input(ctx, move || crate::remote_input::press_key(key)).await,
            None => tracing::debug!("ignoring a key that isn't on the list"),
        },
        AppMessage::NowPlaying { info } => {
            ctx.touch_last_seen();
            let trusted = shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false);
            if !trusted {
                return Flow::Continue;
            }
            let mut info = info.sanitized();
            if info.is_empty() {
                shared.now_playing_remote.remove(&peer_id);
            } else {
                // The cover is sent once per track: keep it for the heartbeats that follow.
                if info.art.is_none() {
                    if let Some(previous) = shared.now_playing_remote.get(&peer_id) {
                        if previous.0.same_track(&info) {
                            info.art = previous.0.art.clone();
                        }
                    }
                }
                shared
                    .now_playing_remote
                    .insert(peer_id, (info, std::time::Instant::now()));
            }
            let _ = shared.event_tx.send(EngineEvent::NowPlayingChanged).await;
        }
        AppMessage::MediaCommand { action, value } => {
            ctx.touch_last_seen();
            let trusted = shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false);
            if !trusted {
                return Flow::Continue;
            }
            if cfg!(target_os = "android") {
                // The phone's app owns its media sessions.
                let _ = shared
                    .event_tx
                    .send(EngineEvent::MediaCommandReceived {
                        from_device: peer_id,
                        action,
                        value,
                    })
                    .await;
            } else if shared
                .settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .allow_remote_control
            {
                if let Some(action) = crate::now_playing::MediaAction::parse(&action, value) {
                    let _ =
                        tokio::task::spawn_blocking(move || crate::now_playing::control(action))
                            .await;
                }
            }
        }
        AppMessage::RemoteShortcut { name } => match crate::remote_input::Shortcut::parse(&name) {
            Some(shortcut) => input(ctx, move || crate::remote_input::shortcut(shortcut)).await,
            None => tracing::debug!("ignoring a shortcut that isn't on the list"),
        },
        AppMessage::RemoteModifier { modifier, down } => {
            match crate::remote_input::Modifier::parse(&modifier) {
                Some(m) => input(ctx, move || crate::remote_input::set_modifier(m, down)).await,
                None => tracing::debug!("ignoring a modifier that isn't on the list"),
            }
        }
        AppMessage::DialNumber { number } => {
            ctx.touch_last_seen();
            let trusted = shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false);
            // Only a phone can be asked to call, and only with a number that
            // is a number.
            if trusted && cfg!(target_os = "android") {
                if let Some(number) = crate::remote_input::clean_phone_number(&number) {
                    let _ = shared
                        .event_tx
                        .send(EngineEvent::DialRequested {
                            from_device: peer_id,
                            from_name: peer_name.clone(),
                            number,
                        })
                        .await;
                }
            }
        }
        _ => unreachable!("dispatch_inbound routed a non-remote message here"),
    }
    Flow::Continue
}

/// During a hand-off, what has to happen before a click or scroll from the
/// computer driving this one: its modifiers, put back after a rewritten
/// shortcut lifted them. Nothing for a phone.
fn edge_first(ctx: &InboundCtx) -> Vec<crate::edge_keys::Act> {
    if super::super::edge_ops::controlled_by(&ctx.shared, ctx.peer_id) {
        super::super::edge_ops::pointer_acts()
    } else {
        Vec::new()
    }
}

/// Carries out one mouse or keyboard message from a paired phone, if this
/// computer allows remote control. A refusal or failure is reported back at
/// most once every two seconds, because these messages stream.
async fn input(ctx: &InboundCtx, run: impl FnOnce() -> Result<(), String> + Send + 'static) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST_REPORT_MS: AtomicU64 = AtomicU64::new(0);

    ctx.touch_last_seen();
    let shared = &ctx.shared;
    let trusted = shared
        .peer_manager
        .get(ctx.peer_id)
        .map(|p| p.trusted)
        .unwrap_or(false);
    if !trusted {
        return;
    }
    let problem = if !shared
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .allow_remote_control
    {
        Some(
            "Remote control is off on this computer. Turn it on in Link All's settings."
                .to_string(),
        )
    } else {
        match tokio::task::spawn_blocking(run).await {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e),
            Err(e) => Some(e.to_string()),
        }
    };
    let Some(error) = problem else { return };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let last = LAST_REPORT_MS.load(Ordering::Relaxed);
    if now.saturating_sub(last) < 2000 {
        return;
    }
    LAST_REPORT_MS.store(now, Ordering::Relaxed);
    let _ = ctx
        .outbox_tx
        .send(AppMessage::RemoteControlAck {
            success: false,
            error: Some(error),
        })
        .await;
}

/// A paired computer asked this phone to take a picture. Only a phone can; a
/// computer ignores it. `mode` is "photo" or "document", anything else is a photo.
pub(super) async fn capture_request(ctx: &InboundCtx, mode: String) -> Flow {
    ctx.touch_last_seen();
    let shared = &ctx.shared;
    let trusted = shared
        .peer_manager
        .get(ctx.peer_id)
        .map(|p| p.trusted)
        .unwrap_or(false);
    if trusted && cfg!(target_os = "android") {
        let mode = if mode == "document" {
            mode
        } else {
            "photo".to_string()
        };
        let _ = shared
            .event_tx
            .send(EngineEvent::CaptureRequested {
                from_device: ctx.peer_id,
                from_name: ctx.peer_name.clone(),
                mode,
            })
            .await;
    }
    Flow::Continue
}
