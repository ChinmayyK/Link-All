//! Folder transfers. A folder travels as a batch of ordinary file
//! transfers that share a batch id, each named by its path inside the
//! folder ("Photos/2024/a.jpg"); the receiver rebuilds the tree (see
//! `InboundTransfer::accept`). This module walks and paces the sending
//! side, applies one accept/decline answer to a whole folder, and turns
//! the per-file results into one result per folder on both sides.

use super::*;
use crate::file_transfer::{is_folder_item, sanitize_file_name};

/// Files of one folder in flight at once. A receiver refuses more than 50
/// open transfers from one peer, so a folder is never announced in one go.
const FOLDER_WINDOW: usize = 4;
pub(crate) const MAX_FOLDER_FILES: usize = 10_000;
/// How long a folder send waits with its target unreachable before stopping.
const TARGET_GONE_GRACE: Duration = Duration::from_secs(120);
/// Clutter the OS leaves in folders; never worth sending.
const SKIPPED_NAMES: &[&str] = &[".DS_Store", "Thumbs.db", "desktop.ini"];
const MAX_TRACKED_FOLDERS: usize = 64;

/// A folder send that has started.
#[derive(Debug, Clone, Serialize)]
pub struct FolderSend {
    pub batch_id: String,
    pub folder_name: String,
    pub file_count: u32,
    pub total_bytes: u64,
}

/// A folder transfer in flight, for status screens.
#[derive(Debug, Clone, Serialize)]
pub struct FolderProgress {
    pub batch_id: String,
    pub folder_name: String,
    pub peer_name: String,
    pub file_count: u32,
    pub done_count: u32,
    pub failed_count: u32,
    pub outbound: bool,
    /// Whole folder done, 0..1: finished files plus the done part of the
    /// files in flight, so big files move the bar while they travel.
    pub progress: f64,
    /// Combined speed of the files in flight, bytes per second.
    pub speed_bps: u64,
}

/// One file of a folder, for `Engine::send_folder_item`.
pub struct FolderItem {
    pub path: PathBuf,
    /// Already-open file (an Android content URI); `path` is then only a label.
    pub opened: Option<std::fs::File>,
    /// Path inside the folder, '/'-separated, without the folder's own name.
    pub rel_path: String,
}

struct FolderTally {
    peer_id: Uuid,
    peer_name: String,
    folder_name: String,
    total: u32,
    done: u32,
    failed: u32,
    outbound: bool,
    dest_dir: Option<PathBuf>,
    started: Instant,
}

/// Progress of every folder transfer in flight, keyed by batch id.
#[derive(Default)]
pub(crate) struct FolderTallies {
    folders: HashMap<String, FolderTally>,
    items: HashMap<[u8; 16], String>,
    /// The receiver's answer to a folder, applied to its remaining files.
    decisions: HashMap<String, bool>,
    cancelled: HashSet<String>,
}

impl FolderTallies {
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn track(
        &mut self,
        transfer_id: [u8; 16],
        batch_id: &str,
        folder_name: &str,
        total: u32,
        peer_id: Uuid,
        peer_name: &str,
        outbound: bool,
    ) {
        self.begin(batch_id, folder_name, total, peer_id, peer_name, outbound);
        self.items.insert(transfer_id, batch_id.to_string());
    }

    /// Start counting a folder, before any of its files is sent, so status
    /// lists it at once with its full file count.
    pub(crate) fn begin(
        &mut self,
        batch_id: &str,
        folder_name: &str,
        total: u32,
        peer_id: Uuid,
        peer_name: &str,
        outbound: bool,
    ) {
        if !self.folders.contains_key(batch_id) {
            if self.folders.len() >= MAX_TRACKED_FOLDERS {
                if let Some(oldest) = self
                    .folders
                    .iter()
                    .min_by_key(|(_, t)| t.started)
                    .map(|(id, _)| id.clone())
                {
                    self.forget(&oldest);
                }
            }
            self.folders.insert(
                batch_id.to_string(),
                FolderTally {
                    peer_id,
                    peer_name: peer_name.to_string(),
                    folder_name: folder_name.to_string(),
                    total: total.max(1),
                    done: 0,
                    failed: 0,
                    outbound,
                    dest_dir: None,
                    started: Instant::now(),
                },
            );
        }
    }

    /// Count a file result, marking a failure as a folder item. Returns
    /// the folder's result once every file has one.
    pub(crate) fn record(&mut self, event: &mut EngineEvent) -> Option<EngineEvent> {
        let (transfer_id, ok) = match event {
            EngineEvent::FileTransferComplete { transfer_id, .. } => (*transfer_id, true),
            EngineEvent::FileTransferFailed {
                transfer_id,
                in_folder,
                ..
            } => {
                *in_folder |= self.items.contains_key(transfer_id);
                (*transfer_id, false)
            }
            _ => return None,
        };
        let batch_id = self.items.remove(&transfer_id)?;
        let tally = self.folders.get_mut(&batch_id)?;
        if ok {
            tally.done += 1;
            if let EngineEvent::FileTransferComplete {
                file_name,
                dest_path,
                ..
            } = event
            {
                if !tally.outbound && tally.dest_dir.is_none() {
                    tally.dest_dir = folder_dir_of(dest_path, file_name);
                }
            }
        } else {
            tally.failed += 1;
        }
        if tally.done + tally.failed >= tally.total {
            return self.finish(&batch_id);
        }
        None
    }

    /// End a folder now; files without a result count as failed.
    pub(crate) fn finish(&mut self, batch_id: &str) -> Option<EngineEvent> {
        let tally = self.folders.remove(batch_id)?;
        self.items.retain(|_, b| b != batch_id);
        Some(EngineEvent::FolderTransferComplete {
            batch_id: batch_id.to_string(),
            peer_id: tally.peer_id,
            peer_name: tally.peer_name,
            folder_name: tally.folder_name,
            file_count: tally.total,
            failed_count: tally.total.saturating_sub(tally.done),
            outbound: tally.outbound,
            dest_dir: if tally.outbound { None } else { tally.dest_dir },
        })
    }

    /// Drop a folder without reporting it.
    pub(crate) fn forget(&mut self, batch_id: &str) {
        self.folders.remove(batch_id);
        self.items.retain(|_, b| b != batch_id);
    }

    pub(crate) fn decision(&self, batch_id: &str) -> Option<bool> {
        self.decisions.get(batch_id).copied()
    }

    pub(crate) fn decide(&mut self, batch_id: &str, accept: bool) {
        if self.decisions.len() >= 256 {
            self.decisions.clear();
        }
        self.decisions.insert(batch_id.to_string(), accept);
    }

    pub(crate) fn progress(&self) -> Vec<FolderProgress> {
        let mut out: Vec<_> = self
            .folders
            .iter()
            .map(|(batch_id, t)| FolderProgress {
                batch_id: batch_id.clone(),
                folder_name: t.folder_name.clone(),
                peer_name: t.peer_name.clone(),
                file_count: t.total,
                done_count: t.done,
                failed_count: t.failed,
                outbound: t.outbound,
                progress: (t.done + t.failed) as f64 / t.total.max(1) as f64,
                speed_bps: 0,
            })
            .collect();
        out.sort_by(|a, b| a.batch_id.cmp(&b.batch_id));
        out
    }

    /// Stop sending a folder: its unsent files are not sent.
    pub(crate) fn stop_sending(&mut self, batch_id: &str) {
        self.cancelled.insert(batch_id.to_string());
    }

    fn is_cancelled(&self, batch_id: &str) -> bool {
        self.cancelled.contains(batch_id)
    }
}

/// The folder a received item landed in: its path minus the item's path
/// inside the folder.
fn folder_dir_of(dest_path: &Path, file_name: &str) -> Option<PathBuf> {
    let depth = file_name.split('/').filter(|p| !p.is_empty()).count();
    dest_path
        .ancestors()
        .nth(depth.checked_sub(1)?)
        .map(Path::to_path_buf)
}

/// The folder's name and its files as (path, path inside the folder,
/// size), in a stable order. Symlinks are skipped so a link cannot pull in
/// files from outside the folder.
fn list_folder(root: &Path) -> Result<(String, Vec<(PathBuf, String, u64)>)> {
    let meta = std::fs::metadata(root).with_context(|| format!("reading {}", root.display()))?;
    anyhow::ensure!(meta.is_dir(), "{} is not a folder", root.display());
    let folder_name = root
        .file_name()
        .map(|n| sanitize_file_name(&n.to_string_lossy()))
        .context("the folder has no name")?;

    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let mut entries: Vec<_> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
            // An unreadable subfolder is skipped, not the whole folder.
            Err(e) if dir != root => {
                warn!(dir = %dir.display(), error = %e, "skipping unreadable folder");
                continue;
            }
            Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
        };
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if kind.is_symlink() || SKIPPED_NAMES.contains(&name.as_str()) || name.starts_with("._")
            {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                dirs.push(path);
            } else if kind.is_file() {
                let rel = path
                    .strip_prefix(root)?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                files.push((path, rel, size));
                anyhow::ensure!(
                    files.len() <= MAX_FOLDER_FILES,
                    "the folder has more than {MAX_FOLDER_FILES} files"
                );
            }
        }
    }
    anyhow::ensure!(!files.is_empty(), "the folder has no files");
    Ok((folder_name, files))
}

/// Forward engine events to the host, adding a folder's result after the
/// event for its last file, and recording that result in the feed.
pub(super) fn spawn_folder_event_tap(
    folders: Arc<std::sync::Mutex<FolderTallies>>,
    activity: Arc<Mutex<ActivityFeed>>,
    mut events: mpsc::Receiver<EngineEvent>,
    host: mpsc::Sender<EngineEvent>,
) {
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            let mut event = event;
            let folder_done = folders.lock().unwrap().record(&mut event);
            for event in std::iter::once(event).chain(folder_done) {
                if let EngineEvent::FolderTransferComplete {
                    peer_id,
                    peer_name,
                    folder_name,
                    file_count,
                    failed_count,
                    dest_dir,
                    ..
                } = &event
                {
                    activity.lock().await.record_folder_transfer_complete(
                        *peer_id,
                        peer_name.clone(),
                        folder_name.clone(),
                        *file_count,
                        *failed_count,
                        dest_dir.as_ref().map(|d| d.to_string_lossy().into_owned()),
                    );
                }
                if host.send(event).await.is_err() {
                    return;
                }
            }
        }
    });
}

/// A new incoming file that belongs to a folder: (batch id, folder name,
/// file count).
pub(crate) fn incoming_folder(meta: &FileTransferMetadata) -> Option<(String, String, u32)> {
    let batch_id = meta.batch_id.clone()?;
    if !is_folder_item(&meta.file_name) {
        return None;
    }
    let top = meta.file_name.split('/').next().unwrap_or_default();
    let total = meta.item_count.clamp(1, MAX_FOLDER_FILES as u32);
    Some((batch_id, sanitize_file_name(top), total))
}

impl Engine {
    /// Folder transfers in flight, in both directions.
    pub async fn folders(&self) -> Vec<FolderProgress> {
        let mut folders = self.shared.folders.lock().unwrap().progress();
        let mgr = self.shared.file_transfers.lock().await;
        for f in &mut folders {
            let (in_flight, speed) = mgr.folder_in_flight(&f.batch_id);
            let finished = (f.done_count + f.failed_count) as f64;
            f.progress = ((finished + in_flight) / f.file_count.max(1) as f64).min(1.0);
            f.speed_bps = speed;
        }
        folders
    }

    /// Send a folder and everything in it, a few files at a time, in the
    /// background. Returns once the folder has been read.
    pub async fn send_folder(&self, root: PathBuf, target: Option<Uuid>) -> Result<FolderSend> {
        anyhow::ensure!(self.target_reachable(target), "the device is not connected");
        let (folder_name, files) = tokio::task::spawn_blocking(move || list_folder(&root))
            .await
            .context("reading the folder")??;
        let summary = FolderSend {
            batch_id: Uuid::new_v4().to_string(),
            folder_name,
            file_count: files.len() as u32,
            total_bytes: files.iter().map(|(_, _, size)| size).sum(),
        };
        let (peer_id, peer_name) = self.target_label(target);
        self.shared.folders.lock().unwrap().begin(
            &summary.batch_id,
            &summary.folder_name,
            summary.file_count,
            peer_id,
            &peer_name,
            true,
        );
        let engine = self.clone();
        let send = summary.clone();
        tokio::spawn(async move {
            for (path, rel_path, _) in files {
                let item = FolderItem {
                    path,
                    opened: None,
                    rel_path,
                };
                if let Err(e) = engine
                    .send_folder_item(
                        item,
                        target,
                        &send.batch_id,
                        &send.folder_name,
                        send.file_count,
                    )
                    .await
                {
                    warn!(folder = %send.folder_name, error = %e, "folder send stopped");
                    let _ = engine
                        .shared
                        .event_tx
                        .send(EngineEvent::Warning(format!(
                            "Stopped sending {}: {e}",
                            send.folder_name
                        )))
                        .await;
                    break;
                }
            }
            engine.finish_folder_send(send.batch_id).await;
        });
        Ok(summary)
    }

    /// Send one file of a folder, first waiting until fewer than
    /// `FOLDER_WINDOW` of its files are in flight. For hosts that walk the
    /// folder themselves (Android document trees); they call
    /// `finish_folder_send` after the last one.
    pub async fn send_folder_item(
        &self,
        item: FolderItem,
        target: Option<Uuid>,
        batch_id: &str,
        folder_name: &str,
        file_count: u32,
    ) -> Result<[u8; 16]> {
        self.wait_for_folder_room(batch_id, target).await?;
        let name = format!("{}/{}", folder_name, item.rel_path.trim_start_matches('/'));
        let transfer_id = self
            .send_outbound_source(
                item.path,
                item.opened,
                name,
                "application/octet-stream".into(),
                target,
                Some(batch_id.to_string()),
                true,
                file_count,
            )
            .await?;
        let (peer_id, peer_name) = self.target_label(target);
        self.shared.folders.lock().unwrap().track(
            transfer_id,
            batch_id,
            folder_name,
            file_count,
            peer_id,
            &peer_name,
            true,
        );
        Ok(transfer_id)
    }

    /// Report a folder send once its last files finish. Files never sent
    /// (the send stopped early) count as failed. Returns at once.
    pub async fn finish_folder_send(&self, batch_id: String) {
        let shared = self.shared.clone();
        tokio::spawn(async move {
            while !shared
                .file_transfers
                .lock()
                .await
                .outbound_in_folder(&batch_id)
                .is_empty()
            {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            // Let the last file's result reach the tally first.
            tokio::time::sleep(Duration::from_secs(2)).await;
            let done = shared.folders.lock().unwrap().finish(&batch_id);
            shared.folders.lock().unwrap().cancelled.remove(&batch_id);
            if let Some(event) = done {
                let _ = shared.event_tx.send(event).await;
            }
        });
    }

    /// Stop a folder transfer in either direction: unsent files are not
    /// sent, files in flight are cancelled, and later files are declined.
    pub async fn cancel_folder(&self, batch_id: &str) -> Result<()> {
        {
            let mut folders = self.shared.folders.lock().unwrap();
            folders.stop_sending(batch_id);
            folders.decide(batch_id, false);
        }
        let items = {
            let mgr = self.shared.file_transfers.lock().await;
            let mut items = mgr.outbound_in_folder(batch_id);
            items.extend(mgr.inbound_in_folder(batch_id));
            items
        };
        for transfer_id in items {
            self.cancel_file_transfer(transfer_id).await?;
        }
        Ok(())
    }

    async fn wait_for_folder_room(&self, batch_id: &str, target: Option<Uuid>) -> Result<()> {
        let mut gone_since: Option<Instant> = None;
        loop {
            anyhow::ensure!(
                !self.shared.folders.lock().unwrap().is_cancelled(batch_id),
                "cancelled"
            );
            let in_flight = self
                .shared
                .file_transfers
                .lock()
                .await
                .outbound_in_folder(batch_id)
                .len();
            if in_flight < FOLDER_WINDOW {
                return Ok(());
            }
            if self.target_reachable(target) {
                gone_since = None;
            } else if gone_since.get_or_insert_with(Instant::now).elapsed() > TARGET_GONE_GRACE {
                anyhow::bail!("the device disconnected");
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    fn target_label(&self, target: Option<Uuid>) -> (Uuid, String) {
        match target {
            Some(id) => (
                id,
                self.shared
                    .peer_manager
                    .get(id)
                    .map(|p| p.friendly_name)
                    .unwrap_or_else(|| "device".into()),
            ),
            None => (Uuid::nil(), "your devices".into()),
        }
    }

    fn target_reachable(&self, target: Option<Uuid>) -> bool {
        match target {
            Some(id) => self.shared.peer_manager.sender(id).is_some(),
            None => !self.shared.peer_manager.all_trusted_senders().is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete(tid: [u8; 16], name: &str, dest: &str) -> EngineEvent {
        EngineEvent::FileTransferComplete {
            transfer_id: tid,
            from_device: Uuid::nil(),
            from_name: "Mac".into(),
            file_name: name.into(),
            dest_path: PathBuf::from(dest),
        }
    }

    fn failed(tid: [u8; 16]) -> EngineEvent {
        EngineEvent::FileTransferFailed {
            in_folder: false,
            transfer_id: tid,
            from_device: Uuid::nil(),
            reason: "x".into(),
        }
    }

    #[test]
    fn a_folder_reports_once_after_its_last_file() {
        let mut t = FolderTallies::default();
        t.track([1; 16], "b", "Photos", 3, Uuid::nil(), "Mac", false);
        t.track([2; 16], "b", "Photos", 3, Uuid::nil(), "Mac", false);
        t.track([3; 16], "b", "Photos", 3, Uuid::nil(), "Mac", false);
        assert!(t
            .record(&mut complete(
                [1; 16],
                "Photos/2024/a.jpg",
                "/dl/Photos (2)/2024/a.jpg"
            ))
            .is_none());
        assert!(t.record(&mut failed([2; 16])).is_none());
        // A second result for the same file is not counted again.
        assert!(t.record(&mut failed([2; 16])).is_none());
        let Some(EngineEvent::FolderTransferComplete {
            file_count,
            failed_count,
            dest_dir,
            folder_name,
            ..
        }) = t.record(&mut complete(
            [3; 16],
            "Photos/b.jpg",
            "/dl/Photos (2)/b.jpg",
        ))
        else {
            panic!("expected the folder result");
        };
        assert_eq!((file_count, failed_count), (3, 1));
        assert_eq!(folder_name, "Photos");
        assert_eq!(dest_dir, Some(PathBuf::from("/dl/Photos (2)")));
        assert!(t.folders.is_empty() && t.items.is_empty());
    }

    #[test]
    fn finishing_early_counts_unsent_files_as_failed() {
        let mut t = FolderTallies::default();
        t.track([1; 16], "b", "Docs", 5, Uuid::nil(), "Phone", true);
        assert!(t.record(&mut complete([1; 16], "Docs/a.txt", "")).is_none());
        let Some(EngineEvent::FolderTransferComplete {
            failed_count,
            outbound,
            dest_dir,
            ..
        }) = t.finish("b")
        else {
            panic!("expected the folder result");
        };
        assert_eq!(failed_count, 4);
        assert!(outbound);
        assert_eq!(dest_dir, None);
        assert!(t.finish("b").is_none());
    }

    #[test]
    fn files_outside_a_folder_are_ignored() {
        let mut t = FolderTallies::default();
        assert!(t
            .record(&mut complete([9; 16], "a.txt", "/dl/a.txt"))
            .is_none());
    }

    #[test]
    fn listing_skips_links_and_clutter_and_keeps_the_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Trip");
        std::fs::create_dir_all(root.join("day 1/raw")).unwrap();
        std::fs::write(root.join("notes.txt"), b"hi").unwrap();
        std::fs::write(root.join("day 1/raw/a.jpg"), b"jpg").unwrap();
        std::fs::write(root.join(".DS_Store"), b"").unwrap();
        std::fs::create_dir_all(root.join("empty")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(tmp.path(), root.join("escape")).unwrap();

        let (name, files) = list_folder(&root).unwrap();
        assert_eq!(name, "Trip");
        let mut rel: Vec<_> = files.iter().map(|(_, r, _)| r.as_str()).collect();
        rel.sort();
        assert_eq!(rel, ["day 1/raw/a.jpg", "notes.txt"]);
        assert_eq!(files.iter().map(|(_, _, s)| s).sum::<u64>(), 5);
    }

    #[test]
    fn an_empty_folder_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(list_folder(tmp.path()).is_err());
    }
}
