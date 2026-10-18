//! Guest Drop: let someone who isn't paired send you a file. A short-lived web
//! page on this network takes the upload straight to a folder on this computer.
//! No account, no cloud, nothing remembered: the page stops working after five
//! minutes (or when stopped), and a guest never becomes a trusted device.
//!
//! The link carries a random token, and the guest also types a six-digit PIN
//! shown on this computer, so someone who only saw the address can't upload.
//! The page is plain HTTP on the local network, so the PIN and the file are
//! not encrypted on the way: use it on a network you trust.

use anyhow::{Context, Result};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use uuid::Uuid;

/// How long a session accepts new uploads.
pub const SESSION_SECS: u64 = 300;
/// The largest single file accepted.
pub const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const MAX_WRONG_PINS: u32 = 5;
const MAX_UPLOADS_AT_ONCE: usize = 4;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// What the host shows the guest.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GuestSession {
    pub url: String,
    pub pin: String,
    pub expires_in_secs: u64,
}

/// A running session; dropping it, or calling `stop`, ends it.
pub struct GuestHandle {
    pub session: GuestSession,
    stop: watch::Sender<bool>,
}

impl GuestHandle {
    pub fn stop(&self) {
        let _ = self.stop.send(true);
    }
}

impl Drop for GuestHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Called for every file a guest finished sending: its saved path, size and name.
pub type OnFile = Arc<dyn Fn(PathBuf, u64, String) + Send + Sync>;

struct Shared {
    token: String,
    pin: String,
    dir: PathBuf,
    wrong_pins: AtomicU32,
    uploads: AtomicUsize,
    on_file: OnFile,
    stop: watch::Receiver<bool>,
}

/// Starts a session that saves into `dir`, listening on `bind_ip` (any free port).
pub async fn start(bind_ip: IpAddr, dir: PathBuf, on_file: OnFile) -> Result<GuestHandle> {
    std::fs::create_dir_all(&dir).context("creating the guest folder")?;
    let prune_dir = dir.clone();
    tokio::spawn(async move {
        prune_stale_partial_uploads(&prune_dir).await;
    });
    let listener = TcpListener::bind(SocketAddr::new(bind_ip, 0))
        .await
        .context("opening the guest page")?;
    let port = listener.local_addr()?.port();
    let token = Uuid::new_v4().simple().to_string();
    let pin = format!(
        "{:06}",
        u32::from_le_bytes(Uuid::new_v4().as_bytes()[..4].try_into().unwrap()) % 1_000_000
    );
    let (stop_tx, stop_rx) = watch::channel(false);
    let shared = Arc::new(Shared {
        token: token.clone(),
        pin: pin.clone(),
        dir,
        wrong_pins: AtomicU32::new(0),
        uploads: AtomicUsize::new(0),
        on_file,
        stop: stop_rx.clone(),
    });
    let mut stop = stop_rx;
    let stopper = stop_tx.clone();
    tokio::spawn(async move {
        let deadline = tokio::time::sleep(Duration::from_secs(SESSION_SECS));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => break,
                _ = stop.changed() => break,
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { continue };
                    let shared = shared.clone();
                    let stopper = stopper.clone();
                    tokio::spawn(async move {
                        let _ = handle(stream, shared.clone()).await;
                        // Too many wrong PINs ends the session.
                        if shared.wrong_pins.load(Ordering::Relaxed) >= MAX_WRONG_PINS {
                            let _ = stopper.send(true);
                        }
                    });
                }
            }
        }
        // The listener is dropped here: no new connections. Uploads already
        // going carry on to the end.
    });
    let host = match bind_ip {
        IpAddr::V6(v6) => format!("[{v6}]"),
        other => other.to_string(),
    };
    Ok(GuestHandle {
        session: GuestSession {
            url: format!("http://{host}:{port}/g/{token}"),
            pin,
            expires_in_secs: SESSION_SECS,
        },
        stop: stop_tx,
    })
}

struct Request {
    method: String,
    path: String,
    query: String,
    content_length: Option<u64>,
    /// Body bytes that arrived with the headers.
    leftover: Vec<u8>,
}

async fn read_request(stream: &mut TcpStream) -> Result<Request> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(pos) = find(&buf, b"\r\n\r\n") {
            break pos;
        }
        anyhow::ensure!(buf.len() < MAX_HEADER_BYTES, "headers too large");
        let n = tokio::time::timeout(READ_TIMEOUT, stream.read(&mut chunk)).await??;
        anyhow::ensure!(n > 0, "closed before the request was complete");
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..end]).to_string();
    let leftover = buf[end + 4..].to_vec();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or_default();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut content_length = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<u64>().ok();
            }
        }
    }
    Ok(Request {
        method,
        path: path.to_string(),
        query: query.to_string(),
        content_length,
        leftover,
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

async fn respond(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    Ok(())
}

async fn handle(mut stream: TcpStream, shared: Arc<Shared>) -> Result<()> {
    let request = read_request(&mut stream).await?;
    let page = format!("/g/{}", shared.token);
    let upload = format!("/g/{}/upload", shared.token);
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", p) if p == page => {
            respond(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                PAGE.as_bytes(),
            )
            .await
        }
        ("POST", p) if p == upload => receive(&mut stream, &shared, request).await,
        _ => respond(&mut stream, "404 Not Found", "text/plain", b"Not found").await,
    }
}

async fn receive(stream: &mut TcpStream, shared: &Shared, request: Request) -> Result<()> {
    if *shared.stop.borrow() {
        return respond(stream, "410 Gone", "text/plain", b"This link has ended").await;
    }
    let pin = query_value(&request.query, "pin").unwrap_or_default();
    if !constant_time_eq(pin.as_bytes(), shared.pin.as_bytes()) {
        shared.wrong_pins.fetch_add(1, Ordering::Relaxed);
        return respond(stream, "403 Forbidden", "text/plain", b"Wrong PIN").await;
    }
    let Some(length) = request.content_length else {
        return respond(
            stream,
            "411 Length Required",
            "text/plain",
            b"Missing length",
        )
        .await;
    };
    if length > MAX_FILE_BYTES {
        return respond(
            stream,
            "413 Payload Too Large",
            "text/plain",
            b"That file is too large",
        )
        .await;
    }
    if shared.uploads.fetch_add(1, Ordering::SeqCst) >= MAX_UPLOADS_AT_ONCE {
        shared.uploads.fetch_sub(1, Ordering::SeqCst);
        return respond(
            stream,
            "429 Too Many Requests",
            "text/plain",
            b"Too many files at once",
        )
        .await;
    }
    let result = save(stream, shared, &request, length).await;
    shared.uploads.fetch_sub(1, Ordering::SeqCst);
    match result {
        Ok(name) => respond(stream, "200 OK", "text/plain", name.as_bytes()).await,
        Err(_) => {
            respond(
                stream,
                "500 Internal Server Error",
                "text/plain",
                b"Couldn't save the file",
            )
            .await
        }
    }
}

async fn save(
    stream: &mut TcpStream,
    shared: &Shared,
    request: &Request,
    length: u64,
) -> Result<String> {
    let name = clean_name(&query_value(&request.query, "name").unwrap_or_default());
    // The final name is claimed with an empty file so two uploads can't pick the
    // same one; the data goes to a temporary name of its own and is renamed over
    // the claim when complete, so a cut-off upload never looks like a finished file.
    let path = reserve_path(&shared.dir, &name)?;
    let partial = shared
        .dir
        .join(format!(".{}.part-upload", Uuid::new_v4().simple()));
    let result = write_upload(stream, request, length, &partial).await;
    if let Err(err) = result {
        let _ = tokio::fs::remove_file(&partial).await;
        let _ = tokio::fs::remove_file(&path).await;
        return Err(err);
    }
    tokio::fs::rename(&partial, &path).await?;
    let saved = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or(name);
    (shared.on_file)(path, length, saved.clone());
    Ok(saved)
}

async fn write_upload(
    stream: &mut TcpStream,
    request: &Request,
    length: u64,
    partial: &Path,
) -> Result<()> {
    let mut file = tokio::fs::File::create(partial).await?;
    let mut remaining = length;
    let take = request.leftover.len().min(remaining as usize);
    file.write_all(&request.leftover[..take]).await?;
    remaining -= take as u64;
    let mut buf = vec![0u8; 256 * 1024];
    while remaining > 0 {
        let want = buf.len().min(remaining as usize);
        let n = match tokio::time::timeout(READ_TIMEOUT, stream.read(&mut buf[..want])).await {
            Ok(Ok(n)) if n > 0 => n,
            _ => anyhow::bail!("upload cut off"),
        };
        file.write_all(&buf[..n]).await?;
        remaining -= n as u64;
    }
    file.flush().await?;
    Ok(())
}

/// A file name that can't leave the folder or hide: no path, no control
/// characters, not empty, not too long.
pub fn clean_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned
        .trim()
        .trim_start_matches('.')
        .trim_end_matches(['.', ' '])
        .trim()
        .to_string();
    let cleaned: String = cleaned.chars().take(120).collect();
    if cleaned.is_empty() {
        return "file".to_string();
    }

    // Windows reserved device names check (stem before the first dot).
    let stem = match cleaned.split_once('.') {
        Some((s, _)) => s,
        None => &cleaned,
    };
    let upper = stem.to_ascii_uppercase();
    let is_reserved = match upper.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        s if (s.len() == 4
            && (s.starts_with("COM") || s.starts_with("LPT"))
            && s.chars()
                .nth(3)
                .is_some_and(|c| c.is_ascii_digit() && c != '0')) =>
        {
            true
        }
        _ => false,
    };

    if is_reserved {
        format!("file_{}", cleaned)
    } else {
        cleaned
    }
}

/// Claims `dir/name`, or `dir/name (2).ext` and so on when that exists, by
/// creating it empty; `create_new` makes the claim atomic between uploads.
fn reserve_path(dir: &Path, name: &str) -> std::io::Result<PathBuf> {
    let claim = |p: &PathBuf| {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(p)
            .map(|_| ())
    };
    let first = dir.join(name);
    match claim(&first) {
        Ok(()) => return Ok(first),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for n in 2..10_000 {
        let candidate = dir.join(format!("{stem} ({n}){ext}"));
        match claim(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    let fallback = dir.join(format!("{stem}-{}{ext}", Uuid::new_v4().simple()));
    claim(&fallback)?;
    Ok(fallback)
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                if let Some(b) = hex {
                    out.push(b);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Cleans up partial upload temporary files older than 1 hour left behind by crashed or aborted sessions.
async fn prune_stale_partial_uploads(dir: &Path) {
    if let Some(threshold) = std::time::SystemTime::now().checked_sub(Duration::from_secs(3600)) {
        prune_stale_partial_uploads_with_threshold(dir, threshold).await;
    }
}

async fn prune_stale_partial_uploads_with_threshold(dir: &Path, threshold: std::time::SystemTime) {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if name.starts_with('.') && name.ends_with(".part-upload") {
            if let Ok(meta) = entry.metadata().await {
                if let Ok(modified) = meta.modified() {
                    if modified <= threshold {
                        let _ = tokio::fs::remove_file(entry.path()).await;
                    }
                }
            }
        }
    }
}

const PAGE: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Send a file</title>
<style>
body{font-family:system-ui,sans-serif;margin:0;padding:24px;background:#faf8f5;color:#1b1a17;max-width:520px;margin:auto}
h1{font-size:22px}input,button{font-size:16px;padding:12px;border-radius:12px;border:1px solid #d8d2c8;width:100%;box-sizing:border-box;margin:6px 0}
button{background:#2f5bff;color:#fff;border:0;font-weight:600}
.row{padding:10px 0;border-top:1px solid #e7e2da}.bar{height:6px;background:#e7e2da;border-radius:3px;overflow:hidden}
.bar i{display:block;height:100%;width:0;background:#2f5bff}.ok{color:#16a34a}.bad{color:#dc2626}
</style></head><body>
<h1>Send a file</h1>
<p>Type the PIN shown on the other screen, choose your files, and they go straight to that computer. Nothing is stored anywhere else.</p>
<input id="pin" inputmode="numeric" maxlength="6" placeholder="PIN" autocomplete="off">
<input id="files" type="file" multiple>
<button id="go">Send</button>
<div id="list"></div>
<script>
const base = location.pathname;
document.getElementById('go').onclick = () => {
  const pin = document.getElementById('pin').value.trim();
  const files = document.getElementById('files').files;
  if (!pin || !files.length) return;
  const list = document.getElementById('list');
  for (const file of files) {
    const row = document.createElement('div'); row.className = 'row';
    row.textContent = file.name; const bar = document.createElement('div'); bar.className = 'bar';
    const fill = document.createElement('i'); bar.appendChild(fill); row.appendChild(bar);
    const status = document.createElement('div'); row.appendChild(status); list.appendChild(row);
    const xhr = new XMLHttpRequest();
    xhr.open('POST', base + '/upload?pin=' + encodeURIComponent(pin) + '&name=' + encodeURIComponent(file.name));
    xhr.upload.onprogress = e => { if (e.lengthComputable) fill.style.width = (100 * e.loaded / e.total) + '%'; };
    xhr.onload = () => { if (xhr.status === 200) { fill.style.width = '100%'; status.className = 'ok'; status.textContent = 'Sent'; }
      else { status.className = 'bad'; status.textContent = xhr.responseText || 'Failed'; } };
    xhr.onerror = () => { status.className = 'bad'; status.textContent = 'Connection lost'; };
    xhr.send(file);
  }
};
</script></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_cannot_escape_or_hide() {
        assert_eq!(clean_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_name("C:\\Users\\me\\a.txt"), "a.txt");
        assert_eq!(clean_name(".bashrc"), "bashrc");
        assert_eq!(clean_name("a:b*c.txt"), "a_b_c.txt");
        assert_eq!(clean_name(""), "file");
        assert_eq!(clean_name("  "), "file");
        assert_eq!(clean_name("..."), "file");
        assert_eq!(clean_name("foo.txt. "), "foo.txt");
        assert_eq!(clean_name("CON"), "file_CON");
        assert_eq!(clean_name("con.txt"), "file_con.txt");
        assert_eq!(clean_name("aux.tar.gz"), "file_aux.tar.gz");
        assert_eq!(clean_name("NUL"), "file_NUL");
        assert_eq!(clean_name("com1.png"), "file_com1.png");
        assert_eq!(clean_name("lpt9.pdf"), "file_lpt9.pdf");
        assert_eq!(clean_name("safe_con.txt"), "safe_con.txt");
        assert!(clean_name(&"x".repeat(500)).chars().count() <= 120);
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b+c%2Fd"), "a b c/d");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("caf%C3%A9"), "café");
    }

    #[test]
    fn names_get_a_number_when_taken() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        assert_eq!(
            reserve_path(dir.path(), "a.txt").unwrap(),
            dir.path().join("a (2).txt")
        );
        assert_eq!(
            reserve_path(dir.path(), "b.txt").unwrap(),
            dir.path().join("b.txt")
        );
        // The claim is real: the same name is not handed out twice.
        assert_eq!(
            reserve_path(dir.path(), "b.txt").unwrap(),
            dir.path().join("b (2).txt")
        );
    }

    #[test]
    fn pins_compare_in_constant_time_and_exactly() {
        assert!(constant_time_eq(b"123456", b"123456"));
        assert!(!constant_time_eq(b"123456", b"123457"));
        assert!(!constant_time_eq(b"12345", b"123456"));
    }

    #[tokio::test]
    async fn prune_stale_partial_uploads_removes_old_tmp_files() {
        let dir = tempfile::tempdir().unwrap();
        let old_part = dir.path().join(".old.part-upload");
        let regular_file = dir.path().join("regular.txt");

        std::fs::write(&old_part, "old partial data").unwrap();
        std::fs::write(&regular_file, "regular file").unwrap();

        let threshold = std::time::SystemTime::now() + Duration::from_secs(60);
        prune_stale_partial_uploads_with_threshold(dir.path(), threshold).await;

        assert!(!old_part.exists(), "stale partial upload must be pruned");
        assert!(regular_file.exists(), "regular files must not be touched");
    }
}
