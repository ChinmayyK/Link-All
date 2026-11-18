//! Inbound Scratchpad notes from a paired device.

use super::*;

pub(super) async fn handle(ctx: &InboundCtx, msg: AppMessage) -> Flow {
    let AppMessage::ScratchpadNote {
        id,
        text,
        pinned,
        updated_at_ms,
        deleted,
    } = msg
    else {
        unreachable!("dispatch_inbound routed a non-scratchpad message here");
    };
    ctx.touch_last_seen();
    let shared = &ctx.shared;
    let trusted = shared
        .peer_manager
        .get(ctx.peer_id)
        .map(|p| p.trusted)
        .unwrap_or(false);
    if !trusted {
        return Flow::Continue;
    }
    let changed = shared
        .scratchpad
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .merge(
            crate::scratchpad::Note {
                id,
                text,
                pinned,
                updated_at_ms,
                deleted,
            },
            crate::scratchpad::now_ms(),
        );
    if changed {
        let _ = shared.event_tx.send(EngineEvent::ScratchpadChanged).await;
    }
    Flow::Continue
}
