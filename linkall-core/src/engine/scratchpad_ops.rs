//! Scratchpad calls from the apps: list, save, delete and clear notes, with
//! every change sent on to the paired devices that are connected.

use super::*;
use crate::scratchpad::{now_ms, Note};

fn message_of(note: Note) -> AppMessage {
    AppMessage::ScratchpadNote {
        id: note.id,
        text: note.text,
        pinned: note.pinned,
        updated_at_ms: note.updated_at_ms,
        deleted: note.deleted,
    }
}

impl Engine {
    /// The notes to show, pinned first.
    pub fn scratchpad_list(&self) -> Vec<Note> {
        self.shared.scratchpad.lock().unwrap().visible()
    }

    /// Creates a note (`id` None) or edits one, and sends it on.
    pub async fn scratchpad_save(
        &self,
        id: Option<Uuid>,
        text: &str,
        pinned: bool,
    ) -> Result<Note, String> {
        let note =
            self.shared
                .scratchpad
                .lock()
                .unwrap()
                .upsert_local(id, text, pinned, now_ms())?;
        self.send_notes(vec![note.clone()]);
        let _ = self
            .shared
            .event_tx
            .send(EngineEvent::ScratchpadChanged)
            .await;
        Ok(note)
    }

    pub async fn scratchpad_delete(&self, id: Uuid) {
        let marker = self
            .shared
            .scratchpad
            .lock()
            .unwrap()
            .delete_local(id, now_ms());
        if let Some(marker) = marker {
            self.send_notes(vec![marker]);
            let _ = self
                .shared
                .event_tx
                .send(EngineEvent::ScratchpadChanged)
                .await;
        }
    }

    /// Deletes every note that isn't pinned.
    pub async fn scratchpad_clear(&self) {
        let markers = self
            .shared
            .scratchpad
            .lock()
            .unwrap()
            .clear_unpinned(now_ms());
        if !markers.is_empty() {
            self.send_notes(markers);
            let _ = self
                .shared
                .event_tx
                .send(EngineEvent::ScratchpadChanged)
                .await;
        }
    }

    fn send_notes(&self, notes: Vec<Note>) {
        for (peer_id, tx) in self.shared.peer_manager.all_connected_senders() {
            let trusted = self
                .shared
                .peer_manager
                .get(peer_id)
                .map(|p| p.trusted)
                .unwrap_or(false);
            if !trusted {
                continue;
            }
            for note in &notes {
                let _ = tx.try_send(message_of(note.clone()));
            }
        }
    }
}

/// Sends every note to a device that has just connected, so both sides
/// converge on the newest version of each.
pub(super) fn send_all_to(shared: &EngineShared, outbox: mpsc::Sender<AppMessage>) {
    let notes = {
        let mut pad = shared.scratchpad.lock().unwrap();
        pad.prune(now_ms());
        pad.all()
    };
    tokio::spawn(async move {
        for note in notes {
            if outbox.send(message_of(note)).await.is_err() {
                break;
            }
        }
    });
}
