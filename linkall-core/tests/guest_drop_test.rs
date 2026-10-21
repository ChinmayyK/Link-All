//! Guest Drop over a real socket: the page, an upload with the right PIN, a wrong
//! PIN, a hostile file name, and stopping the session.

use linkall_core::guest_drop::{self, GuestHandle};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn session(dir: &TempDir) -> (GuestHandle, Arc<Mutex<Vec<String>>>) {
    let got = Arc::new(Mutex::new(Vec::new()));
    let sink = got.clone();
    let handle = guest_drop::start(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        dir.path().join("Guest"),
        Arc::new(move |_path, _bytes, name| sink.lock().unwrap().push(name)),
    )
    .await
    .unwrap();
    (handle, got)
}

/// (host:port, path) of the session's link.
fn parts(handle: &GuestHandle) -> (String, String) {
    let rest = handle.session.url.trim_start_matches("http://");
    let (addr, path) = rest.split_once('/').unwrap();
    (addr.to_string(), format!("/{path}"))
}

async fn raw(addr: &str, request: Vec<u8>) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(&request).await.unwrap();
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).await.unwrap();
    String::from_utf8_lossy(&reply).to_string()
}

fn upload(path: &str, pin: &str, name: &str, body: &[u8]) -> Vec<u8> {
    let mut request = format!(
        "POST {path}/upload?pin={pin}&name={name} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(body);
    request
}

#[tokio::test]
async fn the_page_and_an_upload_with_the_right_pin() {
    let dir = TempDir::new().unwrap();
    let (handle, got) = session(&dir).await;
    let (addr, path) = parts(&handle);

    let page = raw(
        &addr,
        format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").into_bytes(),
    )
    .await;
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert!(page.contains("Send a file"));

    let body = vec![7u8; 1_000_000];
    let reply = raw(
        &addr,
        upload(&path, &handle.session.pin, "movie.mp4", &body),
    )
    .await;
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    let saved = std::fs::read(dir.path().join("Guest/movie.mp4")).unwrap();
    assert_eq!(saved.len(), body.len());
    assert_eq!(got.lock().unwrap().as_slice(), ["movie.mp4"]);

    // The same name again is kept, not overwritten.
    raw(
        &addr,
        upload(&path, &handle.session.pin, "movie.mp4", b"second"),
    )
    .await;
    assert!(dir.path().join("Guest/movie (2).mp4").exists());
}

#[tokio::test]
async fn a_wrong_pin_saves_nothing_and_five_wrong_pins_end_the_session() {
    let dir = TempDir::new().unwrap();
    let (handle, got) = session(&dir).await;
    let (addr, path) = parts(&handle);
    let wrong = if handle.session.pin == "000000" {
        "111111"
    } else {
        "000000"
    };
    for _ in 0..5 {
        let reply = raw(&addr, upload(&path, wrong, "a.txt", b"x")).await;
        assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");
    }
    assert!(got.lock().unwrap().is_empty());
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    // The right PIN no longer works either: the session is over.
    let late = TcpStream::connect(&addr).await;
    if let Ok(mut stream) = late {
        let _ = stream
            .write_all(&upload(&path, &handle.session.pin, "late.txt", b"x"))
            .await;
        let mut reply = Vec::new();
        let _ = stream.read_to_end(&mut reply).await;
        assert!(!String::from_utf8_lossy(&reply).starts_with("HTTP/1.1 200"));
    }
    assert!(!dir.path().join("Guest/late.txt").exists());
}

#[tokio::test]
async fn a_hostile_name_stays_inside_the_folder_and_a_wrong_token_finds_nothing() {
    let dir = TempDir::new().unwrap();
    let (handle, _got) = session(&dir).await;
    let (addr, path) = parts(&handle);
    let reply = raw(
        &addr,
        upload(&path, &handle.session.pin, "..%2F..%2Fescaped.txt", b"x"),
    )
    .await;
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(dir.path().join("Guest/escaped.txt").exists());
    assert!(!dir.path().join("escaped.txt").exists());

    let wrong = raw(&addr, "GET /g/deadbeef HTTP/1.1\r\nHost: x\r\n\r\n".into()).await;
    assert!(wrong.starts_with("HTTP/1.1 404"), "{wrong}");
}

#[tokio::test]
async fn stopping_ends_the_session() {
    let dir = TempDir::new().unwrap();
    let (handle, _got) = session(&dir).await;
    let (addr, _path) = parts(&handle);
    handle.stop();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(TcpStream::connect(&addr).await.is_err());
}
