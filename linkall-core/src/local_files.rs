//! Serves this desktop's files to a trusted peer's Remote File Explorer.
//!
//! Android answers these requests itself (MediaStore); desktop hosts (the
//! macOS/Linux daemon and the Windows in-process engine) share this module.
//! Only files found under the user's standard folders are ever listed or
//! served: a pull or thumbnail request must name a `file_id` produced by a
//! scan, so a peer can't ask for arbitrary paths.

use crate::engine::{Engine, EngineEvent};
use crate::protocol::{RemoteFileCategory, RemoteFileEntry, RemoteFileSource, RemoteFilesSummary};
use anyhow::{anyhow, Result};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

const MAX_DEPTH: usize = 3;

struct IndexedFile {
    path: PathBuf,
    display_name: String,
    mime_type: String,
}

static INDEX: Mutex<Option<HashMap<u64, IndexedFile>>> = Mutex::new(None);

/// Answers the explorer requests a desktop host is responsible for. Returns
/// without doing anything for every other event.
pub fn handle_event(engine: &Engine, event: &EngineEvent) {
    match event {
        EngineEvent::RemoteFilesQueryReceived {
            request_id,
            from_device,
            summary_only,
            category,
            source,
            search_query,
            offset,
            limit,
        } => {
            let engine = engine.clone();
            let (request_id, from_device) = (*request_id, *from_device);
            let (summary_only, offset, limit) = (*summary_only, *offset, *limit);
            let (category, source, search_query) =
                (category.clone(), source.clone(), search_query.clone());
            tokio::spawn(async move {
                let res = tokio::task::spawn_blocking(move || {
                    scan(summary_only, category, source, search_query, offset, limit)
                })
                .await
                .map_err(|e| anyhow!("scan task failed: {e}"))
                .and_then(|r| r);
                match res {
                    Ok((summary, files, total)) => {
                        engine
                            .send_remote_files_response(
                                from_device,
                                request_id,
                                Some(summary),
                                files,
                                total,
                                None,
                            )
                            .await
                    }
                    Err(e) => {
                        engine
                            .send_remote_files_response(
                                from_device,
                                request_id,
                                None,
                                Vec::new(),
                                0,
                                Some(e.to_string()),
                            )
                            .await
                    }
                }
            });
        }
        EngineEvent::RemoteThumbnailRequestReceived {
            request_id,
            from_device,
            file_id,
            size_px,
        } => {
            let engine = engine.clone();
            let (request_id, from_device, file_id, size_px) =
                (*request_id, *from_device, *file_id, *size_px);
            tokio::spawn(async move {
                let res = tokio::task::spawn_blocking(move || thumbnail(file_id, size_px))
                    .await
                    .map_err(|e| anyhow!("thumbnail task failed: {e}"))
                    .and_then(|r| r);
                let (data, error) = match res {
                    Ok(data) => (data, None),
                    Err(e) => (Vec::new(), Some(e.to_string())),
                };
                engine
                    .send_remote_thumbnail_response(from_device, request_id, file_id, data, error)
                    .await;
            });
        }
        EngineEvent::RemoteFilePullRequestReceived {
            from_device,
            file_id,
            ..
        } => {
            let engine = engine.clone();
            let (from_device, file_id) = (*from_device, *file_id);
            tokio::spawn(async move {
                let resolved = tokio::task::spawn_blocking(move || resolve(file_id)).await;
                match resolved {
                    Ok(Some(file)) => {
                        if let Err(e) = engine
                            .send_file_path(
                                file.path,
                                file.display_name,
                                file.mime_type,
                                Some(from_device),
                                None,
                                false,
                                1,
                            )
                            .await
                        {
                            tracing::warn!(file_id, error = %e, "remote file pull failed");
                        }
                    }
                    _ => tracing::warn!(file_id, "remote file pull: unknown file id"),
                }
            });
        }
        _ => {}
    }
}

fn hash_path(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    hasher.finish()
}

fn categorize_file_by_extension(ext: &str) -> (RemoteFileCategory, &'static str) {
    match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" => (RemoteFileCategory::Images, "image/jpeg"),
        "png" => (RemoteFileCategory::Images, "image/png"),
        "gif" => (RemoteFileCategory::Images, "image/gif"),
        "bmp" => (RemoteFileCategory::Images, "image/bmp"),
        "webp" => (RemoteFileCategory::Images, "image/webp"),
        "heic" => (RemoteFileCategory::Images, "image/heic"),
        "svg" => (RemoteFileCategory::Images, "image/svg+xml"),

        "mp4" | "m4v" => (RemoteFileCategory::Videos, "video/mp4"),
        "mkv" => (RemoteFileCategory::Videos, "video/x-matroska"),
        "mov" => (RemoteFileCategory::Videos, "video/quicktime"),
        "avi" => (RemoteFileCategory::Videos, "video/x-msvideo"),
        "wmv" => (RemoteFileCategory::Videos, "video/x-ms-wmv"),
        "flv" => (RemoteFileCategory::Videos, "video/x-flv"),
        "webm" => (RemoteFileCategory::Videos, "video/webm"),

        "mp3" => (RemoteFileCategory::Audio, "audio/mpeg"),
        "wav" => (RemoteFileCategory::Audio, "audio/wav"),
        "flac" => (RemoteFileCategory::Audio, "audio/flac"),
        "aac" => (RemoteFileCategory::Audio, "audio/aac"),
        "ogg" => (RemoteFileCategory::Audio, "audio/ogg"),
        "m4a" => (RemoteFileCategory::Audio, "audio/mp4"),
        "wma" => (RemoteFileCategory::Audio, "audio/x-ms-wma"),

        "pdf" => (RemoteFileCategory::Documents, "application/pdf"),
        "doc" => (RemoteFileCategory::Documents, "application/msword"),
        "docx" => (
            RemoteFileCategory::Documents,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        ),
        "txt" | "md" => (RemoteFileCategory::Documents, "text/plain"),
        "rtf" => (RemoteFileCategory::Documents, "application/rtf"),
        "xls" => (RemoteFileCategory::Documents, "application/vnd.ms-excel"),
        "xlsx" => (
            RemoteFileCategory::Documents,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ),
        "ppt" => (
            RemoteFileCategory::Documents,
            "application/vnd.ms-powerpoint",
        ),
        "pptx" => (
            RemoteFileCategory::Documents,
            "application/vnd.openxmlformats-officedocument.presentationml.slideshow",
        ),
        "csv" => (RemoteFileCategory::Documents, "text/csv"),

        "apk" => (
            RemoteFileCategory::Apks,
            "application/vnd.android.package-archive",
        ),

        "zip" => (RemoteFileCategory::Archives, "application/zip"),
        "tar" => (RemoteFileCategory::Archives, "application/x-tar"),
        "gz" => (RemoteFileCategory::Archives, "application/gzip"),
        "7z" => (RemoteFileCategory::Archives, "application/x-7z-compressed"),
        "rar" => (RemoteFileCategory::Archives, "application/vnd.rar"),
        "bz2" => (RemoteFileCategory::Archives, "application/x-bzip2"),
        "xz" => (RemoteFileCategory::Archives, "application/x-xz"),

        _ => (RemoteFileCategory::Other, "application/octet-stream"),
    }
}

fn determine_source(
    path: &Path,
    pictures_dir: Option<&Path>,
    downloads_dir: Option<&Path>,
) -> RemoteFileSource {
    let path_str = path.to_string_lossy().to_lowercase();
    if path_str.contains("whatsapp") {
        RemoteFileSource::WhatsApp
    } else if pictures_dir.is_some_and(|p| path.starts_with(p))
        || path_str.contains("camera")
        || path_str.contains("dcim")
    {
        RemoteFileSource::Camera
    } else if downloads_dir.is_some_and(|d| path.starts_with(d)) {
        RemoteFileSource::Downloads
    } else {
        RemoteFileSource::Other
    }
}

/// Walks the user's standard folders, refreshes the id→path index and
/// returns every file found (unfiltered) plus summary counts.
fn scan_all() -> (RemoteFilesSummary, Vec<RemoteFileEntry>) {
    let downloads_dir = dirs::download_dir();
    let pictures_dir = dirs::picture_dir();
    let roots: Vec<PathBuf> = [
        downloads_dir.clone(),
        dirs::document_dir(),
        pictures_dir.clone(),
        dirs::video_dir(),
        dirs::audio_dir(),
    ]
    .into_iter()
    .flatten()
    .collect();

    struct Walk<'a> {
        pictures_dir: Option<&'a Path>,
        downloads_dir: Option<&'a Path>,
        entries: Vec<RemoteFileEntry>,
        index: HashMap<u64, IndexedFile>,
        summary: RemoteFilesSummary,
    }

    fn walk_dir(w: &mut Walk, dir: &Path, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        let Ok(read_dir) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in read_dir.flatten() {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if file_name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                walk_dir(w, &path, depth + 1);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };

            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let (category, mime_type) = categorize_file_by_extension(ext);
            let source = determine_source(&path, w.pictures_dir, w.downloads_dir);

            let counts = &mut w.summary.type_counts;
            match category {
                RemoteFileCategory::Images => counts.images += 1,
                RemoteFileCategory::Videos => counts.videos += 1,
                RemoteFileCategory::Audio => counts.audio += 1,
                RemoteFileCategory::Documents => counts.documents += 1,
                RemoteFileCategory::Apks => counts.apks += 1,
                RemoteFileCategory::Archives => counts.archives += 1,
                _ => {}
            }
            let sources = &mut w.summary.source_counts;
            match source {
                RemoteFileSource::WhatsApp => sources.whatsapp += 1,
                RemoteFileSource::Downloads => sources.downloads += 1,
                RemoteFileSource::Camera => sources.camera += 1,
                _ => {}
            }

            let date_modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let file_id = hash_path(&path);

            w.index.insert(
                file_id,
                IndexedFile {
                    path: path.clone(),
                    display_name: file_name.clone(),
                    mime_type: mime_type.to_string(),
                },
            );
            w.entries.push(RemoteFileEntry {
                file_id,
                display_name: file_name,
                size_bytes: metadata.len(),
                mime_type: mime_type.to_string(),
                date_modified,
                category,
                source,
                content_uri: path.to_string_lossy().into_owned(),
            });
        }
    }

    let mut w = Walk {
        pictures_dir: pictures_dir.as_deref(),
        downloads_dir: downloads_dir.as_deref(),
        entries: Vec::new(),
        index: HashMap::new(),
        summary: RemoteFilesSummary::default(),
    };
    let mut visited = HashSet::new();
    for root in roots {
        let root = root.canonicalize().unwrap_or(root);
        if visited.insert(root.clone()) {
            walk_dir(&mut w, &root, 1);
        }
    }

    *INDEX.lock().unwrap_or_else(|e| e.into_inner()) = Some(w.index);
    (w.summary, w.entries)
}

fn scan(
    summary_only: bool,
    category_filter: Option<RemoteFileCategory>,
    source_filter: Option<RemoteFileSource>,
    search_query: Option<String>,
    offset: u32,
    limit: u32,
) -> Result<(RemoteFilesSummary, Vec<RemoteFileEntry>, u32)> {
    let (summary, entries) = scan_all();
    let query = search_query.map(|q| q.to_lowercase());

    let mut matching: Vec<RemoteFileEntry> = entries
        .into_iter()
        .filter(|e| match &category_filter {
            Some(cat) if *cat != RemoteFileCategory::All => e.category == *cat,
            _ => true,
        })
        .filter(|e| match &source_filter {
            Some(src) if *src != RemoteFileSource::All => e.source == *src,
            _ => true,
        })
        .filter(|e| match &query {
            Some(q) if !q.is_empty() => e.display_name.to_lowercase().contains(q),
            _ => true,
        })
        .collect();
    let total = matching.len() as u32;

    if summary_only {
        return Ok((summary, Vec::new(), total));
    }
    matching.sort_by_key(|e| std::cmp::Reverse(e.date_modified));
    let page = matching
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect();
    Ok((summary, page, total))
}

/// Looks up a file a previous scan handed out. Rescans once if the id is
/// unknown (e.g. the index was built before a restart or the file is new).
fn resolve(file_id: u64) -> Option<IndexedFile> {
    let lookup = || {
        INDEX
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|index| index.get(&file_id))
            .filter(|f| f.path.is_file())
            .map(|f| IndexedFile {
                path: f.path.clone(),
                display_name: f.display_name.clone(),
                mime_type: f.mime_type.clone(),
            })
    };
    lookup().or_else(|| {
        scan_all();
        lookup()
    })
}

// Without the image codecs a small, browser-decodable image is sent as-is;
// the requester scales it for display.
const RAW_THUMBNAIL_MAX_BYTES: u64 = 2 * 1024 * 1024;

fn thumbnail(file_id: u64, size_px: u32) -> Result<Vec<u8>> {
    let file = resolve(file_id).ok_or_else(|| anyhow!("File not found"))?;
    if !file.mime_type.starts_with("image/") || file.mime_type == "image/svg+xml" {
        return Err(anyhow!("No preview available"));
    }

    #[cfg(feature = "image")]
    {
        let size = size_px.clamp(32, 1024);
        if let Ok(img) = image::open(&file.path) {
            let mut out = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(img.thumbnail(size, size).to_rgb8())
                .write_to(&mut out, image::ImageOutputFormat::Jpeg(80))?;
            return Ok(out.into_inner());
        }
    }
    #[cfg(not(feature = "image"))]
    let _ = size_px;

    let len = std::fs::metadata(&file.path)?.len();
    if len > RAW_THUMBNAIL_MAX_BYTES {
        return Err(anyhow!("No preview available"));
    }
    Ok(std::fs::read(&file.path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categorizes_known_and_unknown_extensions() {
        assert_eq!(
            categorize_file_by_extension("JPG"),
            (RemoteFileCategory::Images, "image/jpeg")
        );
        assert_eq!(
            categorize_file_by_extension("xyz").0,
            RemoteFileCategory::Other
        );
    }

    #[test]
    fn unknown_file_id_does_not_resolve() {
        assert!(resolve(0xdead_beef_dead_beef).is_none());
    }
}
