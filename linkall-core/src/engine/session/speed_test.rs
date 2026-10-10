//! Inbound speed-test messages.

use super::*;

pub(super) async fn handle(ctx: &InboundCtx, msg: AppMessage) -> Flow {
    let shared = &ctx.shared;
    let peer_id = ctx.peer_id;
    // Only a paired device may start or take part in a speed test.
    if !shared
        .peer_manager
        .get(peer_id)
        .map(|p| p.trusted)
        .unwrap_or(false)
    {
        ctx.touch_last_seen();
        return Flow::Continue;
    }
    match msg {
        AppMessage::SpeedTestRequest {
            test_id,
            duration_secs,
        } => {
            ctx.touch_last_seen();

            let mut can_accept = false;
            {
                let mut tests = shared.speed_tests.lock().await;
                // Accept if we aren't already running a test with this peer
                let entry = tests.entry(peer_id).or_insert_with(|| {
                    crate::speed_test::SpeedTestState::new(ctx.outbox_tx.clone())
                });
                if entry.phase == crate::speed_test::SpeedTestPhase::Idle {
                    entry.start_receiving(test_id, duration_secs);
                    can_accept = true;
                }
            }

            let _ = ctx
                .outbox_tx
                .send(AppMessage::SpeedTestResponse {
                    test_id,
                    accepted: can_accept,
                    reason: if can_accept {
                        None
                    } else {
                        Some("Busy".into())
                    },
                })
                .await;
        }
        AppMessage::SpeedTestResponse {
            test_id,
            accepted,
            reason: _,
        } => {
            ctx.touch_last_seen();
            if accepted {
                let mut tests = shared.speed_tests.lock().await;
                if let Some(state) = tests.get_mut(&peer_id) {
                    if state.test_id == Some(test_id) {
                        state.start_sending(test_id, state.duration_secs);
                    }
                }
            } else {
                let mut tests = shared.speed_tests.lock().await;
                if let Some(state) = tests.get_mut(&peer_id) {
                    if state.test_id == Some(test_id) {
                        state.reset();
                    }
                }
                let _ = shared
                    .event_tx
                    .send(EngineEvent::SpeedTestComplete { test_id, peer_id })
                    .await;
            }
        }
        AppMessage::SpeedTestData {
            test_id,
            seq: _,
            data,
        } => {
            ctx.touch_last_seen();
            let send_stats = {
                let mut tests = shared.speed_tests.lock().await;
                if let Some(state) = tests.get_mut(&peer_id) {
                    if state.test_id == Some(test_id)
                        && state.phase == crate::speed_test::SpeedTestPhase::Receiving
                    {
                        state.handle_chunk(data.len());

                        // Should we emit stats back to sender?
                        if let Some(last_tick) = state.last_tick_time {
                            if last_tick.elapsed().as_millis() >= 500 {
                                state.last_tick_time = Some(std::time::Instant::now());
                                Some(
                                    state
                                        .bytes_transferred
                                        .load(std::sync::atomic::Ordering::Relaxed),
                                )
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            if let Some(bytes) = send_stats {
                let _ = ctx
                    .outbox_tx
                    .send(AppMessage::SpeedTestStats {
                        test_id,
                        received_bytes: bytes,
                    })
                    .await;

                let duration_secs = {
                    let tests = shared.speed_tests.lock().await;
                    tests.get(&peer_id).map(|s| s.duration_secs)
                };
                if let Some(dur) = duration_secs {
                    let _ = shared.event_tx.try_send(EngineEvent::SpeedTestProgress {
                        test_id,
                        peer_id,
                        direction: "download".to_string(),
                        bytes_transferred: bytes,
                        duration_secs: dur,
                    });
                }
            }
        }
        AppMessage::SpeedTestStats {
            test_id,
            received_bytes,
        } => {
            ctx.touch_last_seen();
            let duration_secs = {
                let tests = shared.speed_tests.lock().await;
                if let Some(state) = tests.get(&peer_id) {
                    if state.test_id == Some(test_id) {
                        Some(state.duration_secs)
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            if let Some(dur) = duration_secs {
                let _ = shared.event_tx.try_send(EngineEvent::SpeedTestProgress {
                    test_id,
                    peer_id,
                    direction: "upload".to_string(),
                    bytes_transferred: received_bytes,
                    duration_secs: dur,
                });
            }
        }
        AppMessage::SpeedTestComplete { test_id } => {
            ctx.touch_last_seen();
            let mut tests = shared.speed_tests.lock().await;
            if let Some(state) = tests.get_mut(&peer_id) {
                if state.test_id == Some(test_id) {
                    state.reset();
                }
            }
            let _ = shared
                .event_tx
                .send(EngineEvent::SpeedTestComplete { test_id, peer_id })
                .await;
        }
        _ => unreachable!("dispatch_inbound routed a non-speed_test message here"),
    }
    Flow::Continue
}
