//! "Link All Drop": a folder on this computer whose files go to the paired
//! devices on their own. Drop a file in, and it is sent: no picker, and the
//! receiving device accepts it like any file from a paired device.
//!
//! The folder is read every couple of seconds instead of watched, so it works
//! the same on every platform and over network shares. A file is sent once it
//! has stopped growing, so one still being copied in is never sent half done.

use super::*;
use std::collections::HashMap;
use std::time::SystemTime;

const SCAN_EVERY: Duration = Duration::from_secs(2);
/// How long to wait before trying a file that failed to send again.
const RETRY_AFTER: Duration = Duration::from_secs(30);
const SENT_FILE: &str = "drop_sent.json";

/// Where dropped files go from. None on a phone, which has no such folder.
pub fn drop_folder_path() -> Option<PathBuf> {
    if cfg!(target_os = "android") {
        return None;
    }
    dirs::home_dir().map(|home| home.join("Link All Drop"))
}

/// What a file looked like when it was sent; a change means it should go again.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: u64,
}

fn stamp_of(meta: &std::fs::Metadata) -> Stamp {
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    Stamp {
        len: meta.len(),
        modified,
    }
}

/// Files that are not meant to be sent: hidden files and the temporary names
/// browsers and editors use while a file is being written.
fn is_ignored(name: &str) -> bool {
    name.starts_with('.')
        || name.starts_with('~')
        || name.ends_with('~')
        || [".part", ".crdownload", ".download", ".tmp", ".partial"]
            .iter()
            .any(|ext| name.to_lowercase().ends_with(ext))
}

fn load_sent(path: &Path) -> HashMap<String, Stamp> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let rows: Vec<(String, u64, u64)> = serde_json::from_str(&text).unwrap_or_default();
    rows.into_iter()
        .map(|(name, len, modified)| (name, Stamp { len, modified }))
        .collect()
}

fn save_sent(path: &Path, sent: &HashMap<String, Stamp>) {
    let rows: Vec<(&String, u64, u64)> = sent.iter().map(|(n, s)| (n, s.len, s.modified)).collect();
    if let Ok(text) = serde_json::to_string(&rows) {
        let _ = std::fs::write(path, text);
    }
}

impl Engine {
    pub(super) fn spawn_drop_folder(&self) {
        let Some(folder) = drop_folder_path() else {
            return;
        };
        let engine = self.clone();
        tokio::spawn(async move {
            let state_path = engine.shared.config.data_dir.join(SENT_FILE);
            let mut sent = load_sent(&state_path);
            // Size and time seen at the last scan: a file is stable when they match.
            let mut seen: HashMap<String, Stamp> = HashMap::new();
            let mut failed: HashMap<String, tokio::time::Instant> = HashMap::new();
            let mut interval = tokio::time::interval(SCAN_EVERY);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if !engine.shared.settings.lock().unwrap().drop_folder_enabled {
                    seen.clear();
                    continue;
                }
                if std::fs::create_dir_all(&folder).is_err() {
                    continue;
                }
                let has_target = engine
                    .shared
                    .peer_manager
                    .list()
                    .iter()
                    .any(|p| p.trusted && p.status == PeerConnectionState::Connected);
                let Ok(entries) = std::fs::read_dir(&folder) else {
                    continue;
                };
                let mut present = Vec::new();
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let Ok(meta) = entry.metadata() else { continue };
                    if !meta.is_file() || is_ignored(&name) {
                        continue;
                    }
                    present.push(name.clone());
                    let stamp = stamp_of(&meta);
                    let stable = seen.get(&name) == Some(&stamp) && stamp.len > 0;
                    seen.insert(name.clone(), stamp);
                    if !stable || !has_target || sent.get(&name) == Some(&stamp) {
                        continue;
                    }
                    if failed
                        .get(&name)
                        .is_some_and(|at| at.elapsed() < RETRY_AFTER)
                    {
                        continue;
                    }
                    match engine
                        .send_file_path(
                            entry.path(),
                            name.clone(),
                            "application/octet-stream".into(),
                            None,
                            None,
                            false,
                            1,
                        )
                        .await
                    {
                        Ok(_) => {
                            failed.remove(&name);
                            sent.insert(name, stamp);
                            save_sent(&state_path, &sent);
                        }
                        Err(err) => {
                            tracing::warn!(file = %name, error = %err, "couldn't send a dropped file");
                            failed.insert(name, tokio::time::Instant::now());
                        }
                    }
                }
                // Forget files that left the folder, so a new file with the
                // same name is sent again.
                seen.retain(|name, _| present.contains(name));
                let before = sent.len();
                sent.retain(|name, _| present.contains(name));
                if sent.len() != before {
                    save_sent(&state_path, &sent);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_and_hidden_names_are_ignored() {
        for name in [
            ".DS_Store",
            "~$report.docx",
            "movie.mp4.crdownload",
            "a.PART",
            "x.tmp",
            "notes~",
        ] {
            assert!(is_ignored(name), "{name}");
        }
        for name in ["report.pdf", "photo.jpg", "my.part.of.it.txt"] {
            assert!(!is_ignored(name), "{name}");
        }
    }

    #[test]
    fn sent_files_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SENT_FILE);
        let mut sent = HashMap::new();
        sent.insert(
            "a.txt".to_string(),
            Stamp {
                len: 3,
                modified: 9,
            },
        );
        save_sent(&path, &sent);
        let back = load_sent(&path);
        assert!(
            back.get("a.txt")
                == Some(&Stamp {
                    len: 3,
                    modified: 9
                })
        );
    }
}
