//! The Scratchpad: short notes, links and snippets shared between your own
//! devices, kept on each device and never sent anywhere else. Every note is
//! edited anywhere and the newest edit wins everywhere; a deleted note stays
//! as an empty marker for a month so a device that was off still learns of it.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// The most text one note may hold.
pub const MAX_TEXT_BYTES: usize = 20_000;
/// The most notes kept at once.
pub const MAX_NOTES: usize = 500;
/// How long a deleted note's marker is kept, in milliseconds.
const TOMBSTONE_KEEP_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// The time now, in milliseconds since 1970.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: Uuid,
    pub text: String,
    pub pinned: bool,
    /// When this version was written, in milliseconds since 1970.
    pub updated_at_ms: u64,
    pub deleted: bool,
}

impl Note {
    /// Newer wins; for the same time, a fixed order so every device lands on
    /// the same version.
    fn wins_over(&self, other: &Note) -> bool {
        (self.updated_at_ms, self.deleted, &self.text, self.pinned)
            > (
                other.updated_at_ms,
                other.deleted,
                &other.text,
                other.pinned,
            )
    }
}

pub struct Scratchpad {
    notes: HashMap<Uuid, Note>,
    path: Option<PathBuf>,
}

impl Scratchpad {
    pub fn new(path: Option<PathBuf>) -> Self {
        Scratchpad {
            notes: HashMap::new(),
            path,
        }
    }

    pub fn load(path: &Path) -> Self {
        let text = std::fs::read_to_string(path).or_else(|_| {
            let tmp = path.with_extension("json.tmp");
            std::fs::read_to_string(tmp)
        });
        let notes: Vec<Note> = text
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Scratchpad {
            notes: notes.into_iter().map(|n| (n.id, n)).collect(),
            path: Some(path.to_path_buf()),
        }
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let all: Vec<&Note> = self.notes.values().collect();
        if let Ok(text) = serde_json::to_string(&all) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, path).is_err() {
                let _ = std::fs::copy(&tmp, path);
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }

    /// Notes to show: pinned first, then the most recently edited.
    pub fn visible(&self) -> Vec<Note> {
        let mut notes: Vec<Note> = self
            .notes
            .values()
            .filter(|n| !n.deleted)
            .cloned()
            .collect();
        notes.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then(b.updated_at_ms.cmp(&a.updated_at_ms))
                .then(a.id.cmp(&b.id))
        });
        notes
    }

    /// Everything, deleted markers included, for sending to a device that just connected.
    pub fn all(&self) -> Vec<Note> {
        self.notes.values().cloned().collect()
    }

    /// Search visible notes for matching keywords. Pinned and title (first line) matches are ranked first.
    pub fn search(&self, query: &str) -> Vec<Note> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.visible();
        }
        let terms: Vec<&str> = q.split_whitespace().collect();
        let mut scored: Vec<(u32, Note)> = self
            .notes
            .values()
            .filter(|n| !n.deleted)
            .filter_map(|n| {
                let text_lower = n.text.to_lowercase();
                if !terms.iter().all(|term| text_lower.contains(term)) {
                    return None;
                }
                let first_line = text_lower.lines().next().unwrap_or("");
                let title_matches = terms
                    .iter()
                    .filter(|term| first_line.contains(**term))
                    .count() as u32;
                let mut score = title_matches * 10;
                if n.pinned {
                    score += 20;
                }
                Some((score, n.clone()))
            })
            .collect();

        scored.sort_by(|(score_a, note_a), (score_b, note_b)| {
            score_b
                .cmp(score_a)
                .then(note_b.updated_at_ms.cmp(&note_a.updated_at_ms))
                .then(note_a.id.cmp(&note_b.id))
        });

        scored.into_iter().map(|(_, n)| n).collect()
    }

    fn visible_count(&self) -> usize {
        self.notes.values().filter(|n| !n.deleted).count()
    }

    /// Creates a note (`id` None) or edits one. Returns the note to send on.
    pub fn upsert_local(
        &mut self,
        id: Option<Uuid>,
        text: &str,
        pinned: bool,
        now_ms: u64,
    ) -> Result<Note, String> {
        if text.len() > MAX_TEXT_BYTES {
            return Err("That note is too long.".into());
        }
        let id = id.unwrap_or_else(Uuid::new_v4);
        if !self.notes.contains_key(&id) && self.visible_count() >= MAX_NOTES {
            return Err("There are too many notes. Delete some first.".into());
        }
        // Never go back in time: an edit right after a newer one still wins.
        let at = self
            .notes
            .get(&id)
            .map_or(now_ms, |old| now_ms.max(old.updated_at_ms + 1));
        let note = Note {
            id,
            text: text.to_string(),
            pinned,
            updated_at_ms: at,
            deleted: false,
        };
        self.notes.insert(id, note.clone());
        self.save();
        Ok(note)
    }

    /// Deletes a note; returns its marker to send on, or None if there was no such note.
    pub fn delete_local(&mut self, id: Uuid, now_ms: u64) -> Option<Note> {
        let old = self.notes.get(&id)?;
        if old.deleted {
            return None;
        }
        let marker = Note {
            id,
            text: String::new(),
            pinned: false,
            updated_at_ms: now_ms.max(old.updated_at_ms + 1),
            deleted: true,
        };
        self.notes.insert(id, marker.clone());
        self.save();
        Some(marker)
    }

    /// Deletes every note that is not pinned; returns the markers to send on.
    pub fn clear_unpinned(&mut self, now_ms: u64) -> Vec<Note> {
        let ids: Vec<Uuid> = self
            .notes
            .values()
            .filter(|n| !n.deleted && !n.pinned)
            .map(|n| n.id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.delete_local(id, now_ms))
            .collect()
    }

    /// Takes a note from another device. True when it changed what is shown or kept.
    pub fn merge(&mut self, incoming: Note, now_ms: u64) -> bool {
        self.merge_batch(vec![incoming], now_ms)
    }

    /// Takes several notes from another device. True when it changed what is shown or kept.
    /// Saves to disk once for the batch instead of per-note.
    pub fn merge_batch(&mut self, incoming: Vec<Note>, now_ms: u64) -> bool {
        let mut any_changed = false;
        for note in incoming {
            if note.text.len() > MAX_TEXT_BYTES {
                continue;
            }
            if note.updated_at_ms > now_ms + 24 * 60 * 60 * 1000 {
                continue;
            }
            let insert = match self.notes.get(&note.id) {
                Some(existing) if !note.wins_over(existing) => false,
                None if !note.deleted && self.visible_count() >= MAX_NOTES => false,
                _ => true,
            };
            if insert {
                self.notes.insert(note.id, note);
                any_changed = true;
            }
        }
        if any_changed {
            self.save();
        }
        any_changed
    }

    /// Drops deleted markers older than a month.
    pub fn prune(&mut self, now_ms: u64) {
        let before = self.notes.len();
        self.notes
            .retain(|_, n| !n.deleted || n.updated_at_ms + TOMBSTONE_KEEP_MS > now_ms);
        if self.notes.len() != before {
            self.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_note_shows_pinned_first_then_newest() {
        let mut pad = Scratchpad::new(None);
        let a = pad.upsert_local(None, "a", false, 100).unwrap();
        let b = pad.upsert_local(None, "b", false, 200).unwrap();
        let c = pad.upsert_local(None, "c", true, 50).unwrap();
        let order: Vec<_> = pad.visible().into_iter().map(|n| n.id).collect();
        assert_eq!(order, vec![c.id, b.id, a.id]);
    }

    #[test]
    fn the_newest_edit_wins_on_both_sides() {
        let mut left = Scratchpad::new(None);
        let mut right = Scratchpad::new(None);
        let first = left.upsert_local(None, "one", false, 100).unwrap();
        right.merge(first.clone(), 1_000);
        let l = left
            .upsert_local(Some(first.id), "left", false, 200)
            .unwrap();
        let r = right
            .upsert_local(Some(first.id), "right", false, 300)
            .unwrap();
        left.merge(r.clone(), 1_000);
        right.merge(l, 1_000);
        assert_eq!(left.visible()[0].text, "right");
        assert_eq!(right.visible()[0].text, "right");
    }

    #[test]
    fn edits_made_at_the_same_instant_still_agree() {
        let id = Uuid::new_v4();
        let mut left = Scratchpad::new(None);
        let mut right = Scratchpad::new(None);
        let a = Note {
            id,
            text: "aaa".into(),
            pinned: false,
            updated_at_ms: 500,
            deleted: false,
        };
        let b = Note {
            id,
            text: "bbb".into(),
            pinned: false,
            updated_at_ms: 500,
            deleted: false,
        };
        left.merge(a.clone(), 1_000);
        left.merge(b.clone(), 1_000);
        right.merge(b, 1_000);
        right.merge(a, 1_000);
        assert_eq!(left.visible()[0].text, right.visible()[0].text);
    }

    #[test]
    fn a_deleted_note_stays_deleted_when_an_old_copy_arrives() {
        let mut pad = Scratchpad::new(None);
        let note = pad.upsert_local(None, "x", false, 100).unwrap();
        pad.delete_local(note.id, 200).unwrap();
        assert!(!pad.merge(note, 1_000));
        assert!(pad.visible().is_empty());
    }

    #[test]
    fn clear_keeps_pinned_notes() {
        let mut pad = Scratchpad::new(None);
        pad.upsert_local(None, "keep", true, 100).unwrap();
        pad.upsert_local(None, "go", false, 100).unwrap();
        let gone = pad.clear_unpinned(200);
        assert_eq!(gone.len(), 1);
        assert_eq!(pad.visible().len(), 1);
        assert_eq!(pad.visible()[0].text, "keep");
    }

    #[test]
    fn too_long_notes_and_far_future_edits_are_refused() {
        let mut pad = Scratchpad::new(None);
        assert!(pad
            .upsert_local(None, &"x".repeat(MAX_TEXT_BYTES + 1), false, 1)
            .is_err());
        let future = Note {
            id: Uuid::new_v4(),
            text: "x".into(),
            pinned: false,
            updated_at_ms: 10_000_000_000_000,
            deleted: false,
        };
        assert!(!pad.merge(future, 1_000));
    }

    #[test]
    fn old_markers_are_pruned_and_new_ones_kept() {
        let mut pad = Scratchpad::new(None);
        let a = pad.upsert_local(None, "a", false, 100).unwrap();
        let b = pad.upsert_local(None, "b", false, 100).unwrap();
        pad.delete_local(a.id, 200).unwrap();
        pad.delete_local(b.id, TOMBSTONE_KEEP_MS + 1_000).unwrap();
        pad.prune(TOMBSTONE_KEEP_MS + 5_000);
        assert_eq!(pad.all().len(), 1);
    }

    #[test]
    fn notes_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scratchpad.json");
        {
            let mut pad = Scratchpad::load(&path);
            pad.upsert_local(None, "remember me", true, 100).unwrap();
        }
        let pad = Scratchpad::load(&path);
        assert_eq!(pad.visible()[0].text, "remember me");
    }

    #[test]
    fn notes_recover_from_tmp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scratchpad.json");
        let tmp = dir.path().join("scratchpad.json.tmp");
        let note = Note {
            id: Uuid::new_v4(),
            text: "recovered".into(),
            pinned: false,
            updated_at_ms: 50,
            deleted: false,
        };
        std::fs::write(&tmp, serde_json::to_string(&vec![&note]).unwrap()).unwrap();
        let pad = Scratchpad::load(&path);
        assert_eq!(pad.visible().len(), 1);
        assert_eq!(pad.visible()[0].text, "recovered");
    }

    #[test]
    fn merge_batch_updates_all_notes() {
        let mut pad = Scratchpad::new(None);
        let n1 = Note {
            id: Uuid::new_v4(),
            text: "n1".into(),
            pinned: false,
            updated_at_ms: 100,
            deleted: false,
        };
        let n2 = Note {
            id: Uuid::new_v4(),
            text: "n2".into(),
            pinned: true,
            updated_at_ms: 200,
            deleted: false,
        };
        assert!(pad.merge_batch(vec![n1, n2], 300));
        assert_eq!(pad.visible().len(), 2);
    }

    #[test]
    fn search_filters_and_ranks_notes() {
        let mut pad = Scratchpad::new(None);
        pad.upsert_local(None, "Meeting Notes\nDiscuss project deadlines", false, 100).unwrap();
        pad.upsert_local(None, "Grocery List\nMilk, eggs, coffee", false, 200).unwrap();
        pad.upsert_local(None, "Important Tasks\nReview pull requests and meeting summary", true, 300).unwrap();

        // Empty search returns all visible
        assert_eq!(pad.search("").len(), 3);
        assert_eq!(pad.search("   ").len(), 3);

        // Search "meeting" matches two notes; the pinned one ("Important Tasks") ranks first
        let meeting_results = pad.search("meeting");
        assert_eq!(meeting_results.len(), 2);
        assert!(meeting_results[0].pinned);
        assert_eq!(meeting_results[0].text.lines().next().unwrap(), "Important Tasks");

        // Multi-term search
        let multi = pad.search("coffee milk");
        assert_eq!(multi.len(), 1);
        assert_eq!(multi[0].text.lines().next().unwrap(), "Grocery List");

        // Case-insensitive search
        let lower = pad.search("grocery");
        assert_eq!(lower.len(), 1);

        // Non-matching query
        assert!(pad.search("nonexistent").is_empty());
    }
}
