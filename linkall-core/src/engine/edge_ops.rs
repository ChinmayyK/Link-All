//! One keyboard and mouse across computers. The host's pointer leaves its
//! screen over a configured edge (`edge_begin`); until the pointer comes back
//! (`EdgeLeave`) or either side ends it, the host's mouse and keyboard are sent
//! to the other computer, which plays them with `remote_input`.
//!
//! The receiving side only ever accepts this from a trusted, paired computer
//! while "Remote control" is on, the same gate the phone's trackpad passes.

use super::*;
use crate::remote_input::{self, Edge};

/// How long the host waits for the other computer to take control.
const REPLY_TIMEOUT: Duration = Duration::from_secs(3);

/// How far the pointer must be pushed outward against the edge it came in by
/// before it goes back, so brushing the edge doesn't hand control back.
const RETURN_PUSH: i32 = 24;

/// How long a key or button may wait for room on a backed-up link. Brief
/// stalls ride through; one this long ends the hand-off rather than drop an
/// event that may be a release.
const MUST_ARRIVE_WAIT: Duration = Duration::from_secs(2);

/// How often a hand-off checks that the other computer is still connected.
const WATCH_INTERVAL: Duration = Duration::from_secs(1);

fn understands_edge_control(app_version: Option<&str>) -> bool {
    crate::device_choice::at_least(app_version, crate::device_choice::EDGE_CONTROL_MIN)
}

impl Engine {
    /// The host's pointer left its screen by `edge`, `pos` of the way along
    /// it (0..=65535): ask `target` to take over the keyboard and mouse.
    pub async fn edge_begin(&self, target: Uuid, edge: u8, pos: u16) -> Result<(), String> {
        let result = self.begin_inner(target, edge, pos).await;
        // Remembered for a page that shows what to fix; being busy is not a refusal.
        if let Err(why) = &result {
            if !why.starts_with("The keyboard and mouse are already") {
                self.shared.edge_notice.lock().unwrap().refused = Some((target, why.clone()));
            }
        }
        result
    }

    async fn begin_inner(&self, target: Uuid, edge: u8, pos: u16) -> Result<(), String> {
        Edge::from_wire(edge).ok_or_else(|| "That isn't a side of the screen.".to_string())?;
        let peer = self
            .shared
            .peer_manager
            .get(target)
            .ok_or_else(|| "That computer isn't known.".to_string())?;
        if !peer.trusted {
            return Err("That computer isn't paired.".to_string());
        }
        if !understands_edge_control(peer.app_version.as_deref()) {
            return Err(
                "Update Link All on that computer to share the keyboard and mouse.".to_string(),
            );
        }
        let tx = self
            .shared
            .peer_manager
            .all_connected_senders()
            .into_iter()
            .find(|(id, _)| *id == target)
            .map(|(_, tx)| tx)
            .ok_or_else(|| "That computer isn't connected.".to_string())?;
        {
            let mut state = self.shared.edge.lock().unwrap();
            if *state != EdgeState::Idle {
                return Err("The keyboard and mouse are already being shared.".to_string());
            }
            // Claimed before asking, so two starts can't both go ahead.
            *state = EdgeState::Hosting { peer: target };
        }
        self.edge_clear_notice();
        let (answer_tx, answered) = tokio::sync::oneshot::channel();
        self.shared
            .remote_waiters
            .edge
            .lock()
            .await
            .insert(target, answer_tx);
        let result = match tx.send(AppMessage::EdgeEnter { edge, pos }).await {
            Err(_) => Err("That computer isn't connected.".to_string()),
            Ok(()) => match tokio::time::timeout(REPLY_TIMEOUT, answered).await {
                Ok(Ok(answer)) => answer,
                _ => {
                    // It may still take control after we gave up; tell it
                    // it's over so it isn't left held by this computer.
                    let _ = tokio::time::timeout(REPLY_TIMEOUT, tx.send(AppMessage::EdgeEnd)).await;
                    Err("That computer didn't answer.".to_string())
                }
            },
        };
        self.shared.remote_waiters.edge.lock().await.remove(&target);
        match &result {
            Ok(()) => watch(self.shared.clone(), target, true),
            Err(why) => {
                let mut state = self.shared.edge.lock().unwrap();
                if *state == (EdgeState::Hosting { peer: target }) {
                    *state = EdgeState::Idle;
                }
                self.shared.edge_notice.lock().unwrap().refused = Some((target, why.clone()));
            }
        }
        result
    }

    /// Sends one mouse or keyboard event to the computer being driven. Moves
    /// and scrolls are dropped when the link is backed up (a move that arrives
    /// late is worse than none); a button or key waits up to
    /// `MUST_ARRIVE_WAIT`, since losing its release would leave it stuck down.
    pub async fn edge_input(&self, message: AppMessage) -> Result<(), String> {
        self.edge_input_until(message, tokio::time::Instant::now() + MUST_ARRIVE_WAIT)
            .await
    }

    /// `edge_input` with a deadline shared by a whole batch, so a batch takes
    /// no longer than `MUST_ARRIVE_WAIT` however many keys it holds.
    async fn edge_input_until(
        &self,
        message: AppMessage,
        deadline: tokio::time::Instant,
    ) -> Result<(), String> {
        let must_arrive = match message {
            AppMessage::RemotePointerMove { .. } | AppMessage::RemotePointerScroll { .. } => false,
            AppMessage::RemotePointerButton { .. } | AppMessage::RemoteKeyEvent { .. } => true,
            _ => return Err("That isn't a keyboard or mouse event.".to_string()),
        };
        let peer = match *self.shared.edge.lock().unwrap() {
            EdgeState::Hosting { peer } => peer,
            _ => return Err("The keyboard and mouse aren't being shared.".to_string()),
        };
        let tx = self
            .shared
            .peer_manager
            .all_connected_senders()
            .into_iter()
            .find(|(id, _)| *id == peer)
            .map(|(_, tx)| tx)
            .ok_or_else(|| "That computer isn't connected.".to_string())?;
        if must_arrive {
            match tokio::time::timeout_at(deadline, tx.send(message)).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(_)) => Err("That computer isn't connected.".to_string()),
                Err(_) => {
                    // A lost key or button may be a release, which would
                    // leave it held down over there. End the hand-off: the
                    // other computer releases everything on EdgeEnd.
                    let why = "The link to that computer is too slow, so the keyboard and mouse came back.";
                    {
                        let mut state = self.shared.edge.lock().unwrap();
                        if *state == (EdgeState::Hosting { peer }) {
                            *state = EdgeState::Idle;
                        }
                    }
                    tokio::spawn(async move {
                        let _ =
                            tokio::time::timeout(REPLY_TIMEOUT, tx.send(AppMessage::EdgeEnd)).await;
                    });
                    announce_ended(&self.shared, why).await;
                    Err(why.to_string())
                }
            }
        } else {
            match tx.try_send(message) {
                Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => Ok(()),
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    Err("That computer isn't connected.".to_string())
                }
            }
        }
    }

    /// Takes the keyboard and mouse back and tells the other computer.
    pub async fn edge_end(&self) {
        let peer = {
            let mut state = self.shared.edge.lock().unwrap();
            match *state {
                EdgeState::Hosting { peer } => {
                    *state = EdgeState::Idle;
                    Some(peer)
                }
                _ => None,
            }
        };
        let Some(peer) = peer else { return };
        if let Some((_, tx)) = self
            .shared
            .peer_manager
            .all_connected_senders()
            .into_iter()
            .find(|(id, _)| *id == peer)
        {
            let _ = tx.send(AppMessage::EdgeEnd).await;
        }
    }
}

/// Ends a hand-off when the other computer goes away, so neither side is left
/// holding a keyboard that no longer reaches anything.
fn watch(shared: EngineShared, peer: Uuid, hosting: bool) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(WATCH_INTERVAL).await;
            let mine = {
                let state = shared.edge.lock().unwrap();
                match (&*state, hosting) {
                    (EdgeState::Hosting { peer: p }, true) => *p == peer,
                    (EdgeState::Controlled { peer: p, .. }, false) => *p == peer,
                    _ => false,
                }
            };
            if !mine {
                return;
            }
            let connected = shared
                .peer_manager
                .all_connected_senders()
                .iter()
                .any(|(id, _)| *id == peer);
            if connected && !shared.shutdown.is_cancelled() {
                continue;
            }
            *shared.edge.lock().unwrap() = EdgeState::Idle;
            if hosting {
                announce_ended(&shared, "The other computer disconnected.").await;
            } else {
                let _ = tokio::task::spawn_blocking(remote_input::release_all).await;
            }
            return;
        }
    });
}

// ── The computer being driven ────────────────────────────────────────────────

/// Why this computer won't take the keyboard and mouse, if it won't.
fn refusal(shared: &EngineShared, peer: Uuid) -> Option<String> {
    let trusted = shared
        .peer_manager
        .get(peer)
        .map(|p| p.trusted)
        .unwrap_or(false);
    if !trusted {
        return Some("That computer isn't paired.".to_string());
    }
    if !shared.settings.lock().unwrap().allow_remote_control {
        return Some(
            "Remote control is off on this computer. Turn it on in Link All's settings."
                .to_string(),
        );
    }
    match *shared.edge.lock().unwrap() {
        EdgeState::Idle => None,
        EdgeState::Controlled { peer: p, .. } if p == peer => None,
        _ => Some("This computer is already sharing its keyboard and mouse.".to_string()),
    }
}

/// `EdgeEnter` arrived: put the pointer where it came in and take control.
pub(super) async fn on_enter(ctx: &InboundCtx, edge: u8, pos: u16) {
    ctx.touch_last_seen();
    let shared = &ctx.shared;
    let peer = ctx.peer_id;
    let outcome = if let Some(why) = refusal(shared, peer) {
        Err(why)
    } else if let Some(left_by) = Edge::from_wire(edge) {
        let arrived = left_by.opposite();
        // Off the session loop: the first X11 call opens the display.
        let placed = tokio::task::spawn_blocking(move || {
            let (w, h) = remote_input::screen_size()
                .ok_or_else(|| "This computer can't take a keyboard and mouse yet.".to_string())?;
            let (x, y) = remote_input::entry_point(arrived, pos, w, h);
            remote_input::set_cursor_position(x, y)
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
        // Checked again: another computer may have taken over while the
        // pointer was being placed.
        let taken = placed.and_then(|()| {
            let mut state = shared.edge.lock().unwrap();
            let again = match *state {
                EdgeState::Idle => false,
                EdgeState::Controlled { peer: p, .. } if p == peer => true,
                _ => {
                    return Err(
                        "This computer is already sharing its keyboard and mouse.".to_string()
                    )
                }
            };
            *state = EdgeState::Controlled {
                peer,
                entry: arrived,
                push: 0,
            };
            Ok(again)
        });
        match taken {
            // A second start from the same computer means it lost track of
            // the first, whose end may never have arrived: let go of
            // anything still held from it.
            Ok(true) => {
                let _ = tokio::task::spawn_blocking(remote_input::release_all).await;
                Ok(())
            }
            Ok(false) => {
                watch(shared.clone(), peer, false);
                Ok(())
            }
            Err(e) => Err(e),
        }
    } else {
        Err("That isn't a side of the screen.".to_string())
    };
    let (accepted, error) = match outcome {
        Ok(()) => (true, None),
        Err(e) => (false, Some(e)),
    };
    let _ = ctx
        .outbox_tx
        .send(AppMessage::EdgeReply { accepted, error })
        .await;
}

/// The pointer was asked to move by (`dx`, `dy`) from `before`: if it is being
/// pushed out over the side it came in by, hand control back. Only the part of
/// a move that the screen edge stopped counts as a push, so arriving at the
/// edge quickly is not one.
pub(super) async fn after_move(
    ctx: &InboundCtx,
    dx: i16,
    dy: i16,
    before: Option<(i32, i32)>,
    after: Option<(i32, i32)>,
    size: Option<(i32, i32)>,
) {
    let shared = &ctx.shared;
    let entry = match *shared.edge.lock().unwrap() {
        EdgeState::Controlled { peer, entry, .. } if peer == ctx.peer_id => entry,
        _ => return,
    };
    let at_edge = match (after, size) {
        (Some((x, y)), Some((w, h))) => remote_input::on_edge(entry, x, y, w, h),
        _ => None,
    };
    let asked = remote_input::outward_push(entry, i32::from(dx), i32::from(dy));
    let overshoot = match (before, after) {
        (Some(b), Some(a)) => {
            (asked - remote_input::outward_push(entry, a.0 - b.0, a.1 - b.1)).max(0)
        }
        _ => asked.max(0),
    };
    let leave = {
        let mut state = shared.edge.lock().unwrap();
        let EdgeState::Controlled { peer, push, .. } = &mut *state else {
            return;
        };
        if *peer != ctx.peer_id {
            return;
        }
        match at_edge {
            Some(pos) => {
                *push += overshoot;
                if *push >= RETURN_PUSH {
                    *state = EdgeState::Idle;
                    Some(pos)
                } else {
                    None
                }
            }
            None => {
                *push = 0;
                None
            }
        }
    };
    if let Some(pos) = leave {
        let _ = tokio::task::spawn_blocking(remote_input::release_all).await;
        let _ = ctx.outbox_tx.send(AppMessage::EdgeLeave { pos }).await;
    }
}

/// True while `peer`'s keyboard and mouse drive this computer.
pub(super) fn controlled_by(shared: &EngineShared, peer: Uuid) -> bool {
    matches!(*shared.edge.lock().unwrap(), EdgeState::Controlled { peer: p, .. } if p == peer)
}

/// `EdgeReply` arrived: wake the `edge_begin` waiting for it.
pub(super) async fn on_reply(ctx: &InboundCtx, accepted: bool, error: Option<String>) {
    ctx.touch_last_seen();
    if let Some(waiter) = ctx
        .shared
        .remote_waiters
        .edge
        .lock()
        .await
        .remove(&ctx.peer_id)
    {
        let _ = waiter.send(if accepted {
            Ok(())
        } else {
            Err(error.unwrap_or_else(|| "That computer wouldn't take it.".to_string()))
        });
    }
}

/// `EdgeLeave` arrived: the pointer is back, so the host takes over again.
pub(super) async fn on_leave(ctx: &InboundCtx, pos: u16) {
    ctx.touch_last_seen();
    let hosting = {
        let mut state = ctx.shared.edge.lock().unwrap();
        if *state == (EdgeState::Hosting { peer: ctx.peer_id }) {
            *state = EdgeState::Idle;
            true
        } else {
            false
        }
    };
    if hosting {
        ctx.shared.edge_notice.lock().unwrap().returned_pos = Some(pos);
        let _ = ctx
            .shared
            .event_tx
            .send(EngineEvent::EdgeReturned { pos })
            .await;
    }
}

/// `EdgeEnd` arrived: whichever side this computer was on, it is over.
pub(super) async fn on_end(ctx: &InboundCtx) {
    ctx.touch_last_seen();
    end_with_peer(&ctx.shared, ctx.peer_id, "The other computer ended it.").await;
}

/// Ends a hand-off with `peer`, whichever side this computer is on. Keys and
/// buttons it held down here are let go.
pub(super) async fn end_with_peer(shared: &EngineShared, peer: Uuid, reason: &str) {
    let was = {
        let mut state = shared.edge.lock().unwrap();
        match &*state {
            EdgeState::Controlled { peer: p, .. } | EdgeState::Hosting { peer: p }
                if *p == peer =>
            {
                std::mem::take(&mut *state)
            }
            _ => EdgeState::Idle,
        }
    };
    match was {
        EdgeState::Controlled { .. } => {
            let _ = tokio::task::spawn_blocking(remote_input::release_all).await;
        }
        EdgeState::Hosting { .. } => {
            announce_ended(shared, reason).await;
        }
        EdgeState::Idle => {}
    }
}

/// Tells the app a hand-off ended without the pointer coming back.
async fn announce_ended(shared: &EngineShared, reason: &str) {
    shared.edge_notice.lock().unwrap().ended_reason = Some(reason.to_string());
    let _ = shared
        .event_tx
        .send(EngineEvent::EdgeEnded {
            reason: reason.to_string(),
        })
        .await;
}

/// What an app that does not receive engine events can ask about a hand-off.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EdgeStatus {
    /// This computer's keyboard and mouse are driving another computer now.
    pub hosting: bool,
    /// Another computer's keyboard and mouse are driving this one now.
    pub controlled: bool,
    /// The pointer came back this far along the edge it left by (0..=65535).
    pub returned_pos: Option<u16>,
    /// The hand-off ended without the pointer coming back, and why.
    pub ended_reason: Option<String>,
    /// The computer driving or being driven now.
    pub peer: Option<String>,
    /// The last computer that refused, and the reason, until the next attempt.
    pub refused_target: Option<String>,
    pub refused_reason: Option<String>,
}

impl Engine {
    /// Where a hand-off stands. What happened since the last call (the pointer
    /// coming back, or an end) is reported once, then cleared; `peek` leaves it
    /// for whoever acts on it, so a page can look without taking it.
    pub fn edge_status(&self, peek: bool) -> EdgeStatus {
        let (hosting, controlled, peer) = match *self.shared.edge.lock().unwrap() {
            EdgeState::Hosting { peer } => (true, false, Some(peer.to_string())),
            EdgeState::Controlled { peer, .. } => (false, true, Some(peer.to_string())),
            EdgeState::Idle => (false, false, None),
        };
        let mut notice = self.shared.edge_notice.lock().unwrap();
        let refused = notice.refused.clone();
        EdgeStatus {
            hosting,
            controlled,
            returned_pos: if peek {
                notice.returned_pos
            } else {
                notice.returned_pos.take()
            },
            ended_reason: if peek {
                notice.ended_reason.clone()
            } else {
                notice.ended_reason.take()
            },
            peer,
            refused_target: refused.as_ref().map(|(id, _)| id.to_string()),
            refused_reason: refused.map(|(_, why)| why),
        }
    }

    /// Clears what an earlier hand-off left to report; a new one starts clean.
    pub(super) fn edge_clear_notice(&self) {
        *self.shared.edge_notice.lock().unwrap() = EdgeNotice::default();
    }
}

impl Engine {
    /// Sends one batch of the host's input to the computer being driven: the
    /// summed pointer move and scroll, then each button and key in order.
    pub async fn edge_input_batch(
        &self,
        dx: i32,
        dy: i32,
        scroll_x: i32,
        scroll_y: i32,
        buttons: &[(u8, bool)],
        keys: &[(u16, bool)],
    ) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + MUST_ARRIVE_WAIT;
        let clamp = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        if dx != 0 || dy != 0 {
            self.edge_input_until(
                AppMessage::RemotePointerMove {
                    dx: clamp(dx),
                    dy: clamp(dy),
                },
                deadline,
            )
            .await?;
        }
        if scroll_x != 0 || scroll_y != 0 {
            self.edge_input_until(
                AppMessage::RemotePointerScroll {
                    dx: clamp(scroll_x),
                    dy: clamp(scroll_y),
                },
                deadline,
            )
            .await?;
        }
        for &(button, down) in buttons {
            self.edge_input_until(AppMessage::RemotePointerButton { button, down }, deadline)
                .await?;
        }
        for &(scancode, down) in keys {
            self.edge_input_until(AppMessage::RemoteKeyEvent { scancode, down }, deadline)
                .await?;
        }
        Ok(())
    }
}
