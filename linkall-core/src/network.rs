//! Link All network transport layer.
//!
//! Wire format (per frame):
//!   [u32 LE length][payload bytes]
//!
//! Handshake frames are postcard-encoded plaintext.
//! Post-handshake frames are postcard-encoded then AEAD-encrypted.
//!
//! # Sub-500 ms propagation budget
//! - mDNS resolution: ~10–50 ms (already running)
//! - TCP connect:      ~1 ms on LAN (timeout: 5 s)
//! - Handshake:        ~5–20 ms (2 RTT)
//! - Encrypt + send:   ~1 ms
//! - Total:            ~20–80 ms ✓

use crate::crypto::{EphemeralKeypair, SessionKey};
use crate::protocol::{AppMessage, EcdhFrame, PROTOCOL_VERSION};
use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, info};
use uuid::Uuid;

static BUFFER_POOL: std::sync::OnceLock<std::sync::Mutex<Vec<Vec<u8>>>> =
    std::sync::OnceLock::new();

pub fn get_buffer(capacity: usize) -> Vec<u8> {
    let mut pool = BUFFER_POOL
        .get_or_init(|| std::sync::Mutex::new(Vec::with_capacity(64)))
        .lock()
        .unwrap();
    if let Some(mut buf) = pool.pop() {
        if buf.capacity() < capacity {
            buf.reserve(capacity - buf.capacity());
        }
        buf.resize(capacity, 0);
        buf
    } else {
        vec![0u8; capacity]
    }
}

pub fn return_buffer(mut buf: Vec<u8>) {
    // Only pool up to 16 buffers, and don't pool huge ones (e.g., > MAX_FRAME_SIZE).
    // Chunk buffers are 4 MB, so a bigger pool keeps a lot of memory pinned
    // after a transfer.
    if buf.capacity() > MAX_FRAME_SIZE as usize {
        return;
    }
    let mut pool = BUFFER_POOL
        .get_or_init(|| std::sync::Mutex::new(Vec::with_capacity(64)))
        .lock()
        .unwrap();
    if pool.len() < 16 {
        buf.clear();
        pool.push(buf);
    }
}

const MAX_FRAME_SIZE: u32 = 40 * 1024 * 1024; // 40 MB limit for safety (to accommodate 32MB images)

/// v3 fix: outbound connections must succeed within this window.
/// A stale mDNS entry to a dead host would otherwise block forever.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// v3 fix: TCP keepalive — detect silently-dropped Wi-Fi connections.
/// Idle time before the first probe, then interval between probes.
/// While both sides are awake the 5 s app heartbeat keeps the socket busy
/// and catches dead links first, so keepalive only matters while a phone
/// sleeps - and a 10 s idle there woke it every 10 s for a probe.
const KEEPALIVE_IDLE: Duration = Duration::from_secs(120);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);
#[allow(dead_code)]
const KEEPALIVE_RETRIES: u32 = 3;
const SOCKET_BUFFER_MIN: usize = 8 * 1024 * 1024; // 8 MB
const SOCKET_BUFFER_PREFERRED: usize = 16 * 1024 * 1024; // 16 MB — room for multiple 4 MB chunks in flight

// ── TCP helpers ───────────────────────────────────────────────────────────────

/// Open an outbound TCP connection with timeout and keepalive.
///
/// v3 fixes applied here:
///   • Wrapped in `tokio::time::timeout` (CONNECT_TIMEOUT = 5 s) so a
///     dead-machine / firewall-drop stale mDNS entry can't hang forever.
///   • `set_nodelay(true)` — previously only set on the server accept path.
///   • `SO_KEEPALIVE` via `socket2` so silently-dropped Wi-Fi connections
///     are detected within KEEPALIVE_IDLE + KEEPALIVE_RETRIES × KEEPALIVE_INTERVAL.
pub async fn connect_with_timeout(addr: SocketAddr) -> Result<TcpStream> {
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .with_context(|| {
            format!(
                "TCP connect to {} timed out after {:?}",
                addr, CONNECT_TIMEOUT
            )
        })?
        .with_context(|| format!("TCP connect to {} failed", addr))?;

    optimize_stream(&stream, "outbound stream");

    Ok(stream)
}

/// Best-effort socket tuning.
///
/// Some Android builds reject `TCP_NODELAY` on accepted or freshly-connected
/// sockets even though the connection itself is otherwise usable. Treating
/// that as fatal tears down discovery-driven pairing before the first Hello
/// frame is exchanged, so we log and continue instead.
pub fn optimize_stream(stream: &TcpStream, label: &'static str) {
    if let Err(err) = stream.set_nodelay(true) {
        debug!(error = %err, %label, "TCP_NODELAY unavailable");
    }

    if let Err(err) = apply_socket_buffers(stream) {
        debug!(error = %err, %label, "socket buffer tuning unavailable");
    }

    if let Err(err) = apply_keepalive(stream) {
        debug!(error = %err, %label, "TCP keepalive unavailable");
    }
}

fn apply_socket_buffers(stream: &TcpStream) -> Result<()> {
    use socket2::SockRef;

    let sock_ref = SockRef::from(stream);
    let candidate_sizes = [
        SOCKET_BUFFER_PREFERRED,
        SOCKET_BUFFER_MIN,
        4 * 1024 * 1024,
        2 * 1024 * 1024,
        1024 * 1024,
        512 * 1024,
        256 * 1024,
    ];

    for &target in &candidate_sizes {
        let send_res = sock_ref.set_send_buffer_size(target);
        let recv_res = sock_ref.set_recv_buffer_size(target);
        if send_res.is_ok() && recv_res.is_ok() {
            // The OS may silently grant less than requested (common on
            // mobile/hotspot links); log what we actually got so a slow
            // transfer can be diagnosed from logs instead of guesswork.
            let actual_send = sock_ref.send_buffer_size().ok();
            let actual_recv = sock_ref.recv_buffer_size().ok();
            debug!(
                requested = target,
                actual_send, actual_recv, "socket buffers applied"
            );
            return Ok(());
        }
    }
    // If all custom sizes fail due to strict kernel limits, keep OS defaults without dropping connection.
    let actual_send = sock_ref.send_buffer_size().ok();
    let actual_recv = sock_ref.recv_buffer_size().ok();
    debug!(
        actual_send,
        actual_recv, "socket buffer tuning failed for all candidates, using OS defaults"
    );
    Ok(())
}

/// Apply TCP keepalive settings to any TcpStream (client or server).
fn apply_keepalive(stream: &TcpStream) -> Result<()> {
    use socket2::{SockRef, TcpKeepalive};

    let sock_ref = SockRef::from(stream);
    let keepalive = TcpKeepalive::new()
        .with_time(KEEPALIVE_IDLE)
        .with_interval(KEEPALIVE_INTERVAL);

    #[cfg(not(windows))]
    let keepalive = keepalive.with_retries(KEEPALIVE_RETRIES);

    sock_ref
        .set_tcp_keepalive(&keepalive)
        .context("setting TCP keepalive")?;

    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        let socket = stream.as_raw_socket();
        let val: u32 = KEEPALIVE_RETRIES;

        #[link(name = "ws2_32")]
        extern "system" {
            fn setsockopt(
                s: usize,
                level: i32,
                optname: i32,
                optval: *const u8,
                optlen: i32,
            ) -> i32;
        }

        let ret = unsafe {
            setsockopt(
                socket as usize,
                6,  // IPPROTO_TCP
                16, // TCP_KEEPCNT (supported since Windows 10 1703)
                &val as *const u32 as *const u8,
                4,
            )
        };
        if ret != 0 {
            tracing::warn!("Failed to set TCP_KEEPCNT on Windows, keepalive timeout may be slow");
        }
    }

    Ok(())
}

// ── Low-level framing ─────────────────────────────────────────────────────────

async fn send_frame<T: Serialize>(stream: &mut TcpStream, value: &T) -> Result<()> {
    let payload = postcard::to_stdvec(value).context("serializing frame")?;
    let len = payload.len() as u32;
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(&payload).await?;
    stream.flush().await?;
    Ok(())
}

async fn recv_frame<T: DeserializeOwned>(stream: &mut TcpStream, max_size: u32) -> Result<T> {
    let mut len_buf = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(10), stream.read_exact(&mut len_buf))
        .await
        .context("timeout waiting for frame length")?
        .context("reading frame length")?;
    let len = u32::from_le_bytes(len_buf);

    anyhow::ensure!(
        len <= max_size,
        "frame size {} exceeds limit {}",
        len,
        max_size
    );

    let mut buf = vec![0u8; len as usize];
    tokio::time::timeout(Duration::from_secs(10), stream.read_exact(&mut buf))
        .await
        .context("timeout waiting for frame body")?
        .context("reading frame body")?;

    postcard::from_bytes(&buf).context("deserializing frame")
}

async fn send_encrypted(
    stream: &mut (impl AsyncWriteExt + Unpin),
    session: &mut SessionKey,
    msg: &mut AppMessage,
) -> Result<()> {
    let payload = msg.take_raw_payload();

    let serialized = postcard::to_allocvec(msg).context("serializing AppMessage")?;
    let serialized_len = serialized.len();

    let mut buffer = get_buffer(16 + serialized_len + 16); // Header (16) + body + tag (16)
    buffer[16..16 + serialized_len].copy_from_slice(&serialized);

    let (nonce, tag) = session.encrypt_slice_in_place(&mut buffer[16..16 + serialized_len])?;
    buffer[16 + serialized_len..16 + serialized_len + 16].copy_from_slice(&tag);
    let total_ct_len = serialized_len + 16;
    let len = (12 + total_ct_len) as u32;

    buffer[0..4].copy_from_slice(&len.to_le_bytes());
    buffer[4..16].copy_from_slice(&nonce);

    use bytes::Buf;
    if let Some(mut p) = payload {
        let p_nonce = session
            .encrypt_in_place(&mut p)
            .context("encrypting payload")?;
        let p_len = (12 + p.len()) as u32;
        let p_len_bytes = p_len.to_le_bytes();
        let mut chained = Buf::chain(&buffer[..16 + total_ct_len], &p_len_bytes[..])
            .chain(&p_nonce[..])
            .chain(&p[..]);
        stream.write_all_buf(&mut chained).await?;
        return_buffer(p);
    } else {
        stream.write_all(&buffer[..16 + total_ct_len]).await?;
    }
    return_buffer(buffer);

    stream.flush().await?;
    Ok(())
}

/// Same as send_encrypted but without flush — for high-throughput file chunk
/// transfers where we want to saturate the socket buffer without per-message
/// syscall overhead.
async fn send_encrypted_no_flush(
    stream: &mut (impl AsyncWriteExt + Unpin),
    session: &mut SessionKey,
    msg: &mut AppMessage,
) -> Result<()> {
    let payload = msg.take_raw_payload();

    let serialized = postcard::to_allocvec(msg).context("serializing AppMessage")?;
    let serialized_len = serialized.len();

    let mut buffer = get_buffer(16 + serialized_len + 16); // Header (16) + body + tag (16)
    buffer[16..16 + serialized_len].copy_from_slice(&serialized);

    let (nonce, tag) = session.encrypt_slice_in_place(&mut buffer[16..16 + serialized_len])?;
    buffer[16 + serialized_len..16 + serialized_len + 16].copy_from_slice(&tag);
    let total_ct_len = serialized_len + 16;
    let len = (12 + total_ct_len) as u32;

    buffer[0..4].copy_from_slice(&len.to_le_bytes());
    buffer[4..16].copy_from_slice(&nonce);

    use bytes::Buf;
    if let Some(mut p) = payload {
        let p_nonce = session
            .encrypt_in_place(&mut p)
            .context("encrypting payload")?;
        let p_len = (12 + p.len()) as u32;
        let p_len_bytes = p_len.to_le_bytes();
        let mut chained = Buf::chain(&buffer[..16 + total_ct_len], &p_len_bytes[..])
            .chain(&p_nonce[..])
            .chain(&p[..]);
        stream.write_all_buf(&mut chained).await?;
        return_buffer(p);
    } else {
        stream.write_all(&buffer[..16 + total_ct_len]).await?;
    }
    return_buffer(buffer);

    Ok(())
}

async fn recv_encrypted(
    stream: &mut (impl AsyncReadExt + Unpin),
    session: &mut SessionKey,
) -> Result<AppMessage> {
    // No timeout while waiting for the next frame: a link may be quiet for
    // minutes while a phone sleeps (pings drop to one per 5 minutes). The
    // session heartbeat and TCP keepalive decide when a quiet link is dead;
    // handshake reads wrap this call in their own timeout.
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .context("reading encrypted frame length")?;

    let len = u32::from_le_bytes(len_buf);
    anyhow::ensure!(
        (16..=MAX_FRAME_SIZE).contains(&len),
        "encrypted frame length invalid: {len}"
    );

    let mut cipher_buffer = get_buffer(len as usize);
    tokio::time::timeout(
        Duration::from_secs(30),
        stream.read_exact(&mut cipher_buffer),
    )
    .await
    .context("timeout waiting for encrypted frame body")?
    .context("reading encrypted frame body")?;

    session
        .decrypt_in_place(&mut cipher_buffer)
        .context("decrypting")?;
    let mut msg: AppMessage =
        postcard::from_bytes(&cipher_buffer).context("deserializing AppMessage")?;

    return_buffer(cipher_buffer); // Return metadata buffer immediately

    if msg.expects_raw_payload() {
        let mut p_len_buf = [0u8; 4];
        tokio::time::timeout(Duration::from_secs(30), stream.read_exact(&mut p_len_buf))
            .await
            .context("timeout waiting for payload frame length")?
            .context("reading payload frame length")?;

        let p_len = u32::from_le_bytes(p_len_buf);
        anyhow::ensure!(
            (16..=MAX_FRAME_SIZE).contains(&p_len),
            "payload frame length invalid: {p_len}"
        );

        let mut p_cipher_buffer = get_buffer(p_len as usize);
        tokio::time::timeout(
            Duration::from_secs(30),
            stream.read_exact(&mut p_cipher_buffer),
        )
        .await
        .context("timeout waiting for payload frame body")?
        .context("reading payload frame body")?;

        session
            .decrypt_in_place(&mut p_cipher_buffer)
            .context("decrypting payload")?;
        msg.set_raw_payload(p_cipher_buffer);
    }

    Ok(msg)
}

// ── Handshake ─────────────────────────────────────────────────────────────────

pub struct HandshakeResult {
    pub session: SessionKey,
    pub pin: crate::pairing::PairingPin,
    pub peer_device_id: Uuid,
    pub peer_device_name: String,
    pub peer_identity_pubkey_bytes: [u8; 32],
    pub peer_already_trusted: bool,
    pub is_manual_reconnect: bool,
    /// Empty when the peer predates the version exchange.
    pub peer_app_version: String,
    /// "Android", "macOS", "Windows" or "Linux"; empty from older peers.
    pub peer_platform: String,
}

/// The OS this build runs on, as peers show it.
pub const MY_PLATFORM: &str = if cfg!(target_os = "android") {
    "Android"
} else if cfg!(target_os = "macos") {
    "macOS"
} else if cfg!(target_os = "windows") {
    "Windows"
} else {
    "Linux"
};

fn my_metadata_json(device_name: &str, is_manual_reconnect: bool) -> Option<String> {
    serde_json::to_string(&crate::protocol::DeviceMetadata {
        device_name: device_name.to_string(),
        app_version: crate::protocol::APP_VERSION.to_string(),
        platform: MY_PLATFORM.to_string(),
        is_manual_reconnect: Some(is_manual_reconnect),
        ..Default::default()
    })
    .ok()
}

fn parse_metadata(json: Option<String>) -> crate::protocol::DeviceMetadata {
    json.and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default()
}

/// Initiator side (we connected to the peer).
///
/// v3 fix (Fix 2): nonce echo verification is now fully implemented.
/// The responder must include `xor_nonces(hello.nonce, responder_nonce)` as
/// `nonce_response`, AND echo back our original `my_nonce` unchanged.
/// This proves the responder saw the exact nonce we sent and prevents replay.
pub async fn handshake_initiator(
    stream: &mut TcpStream,
    my_device_id: Uuid,
    my_device_name: &str,
    my_identity_key: std::sync::Arc<std::sync::RwLock<crate::identity::IdentityKey>>,
    is_manual_reconnect: bool,
) -> Result<HandshakeResult> {
    let ephemeral = EphemeralKeypair::generate();
    let my_nonce = crate::crypto::random_nonce16();

    let ecdh = EcdhFrame {
        version: PROTOCOL_VERSION,
        ecdh_pubkey: ephemeral.public_bytes,
        nonce: my_nonce,
    };

    send_frame(stream, &ecdh)
        .await
        .context("sending EcdhFrame")?;

    let ack_ecdh: EcdhFrame =
        tokio::time::timeout(Duration::from_secs(5), recv_frame(stream, 8192))
            .await
            .context("timeout waiting for EcdhFrame")?
            .context("receiving EcdhFrame")?;

    if ack_ecdh.version != PROTOCOL_VERSION {
        anyhow::bail!(
            "protocol version mismatch: peer={} us={}",
            ack_ecdh.version,
            PROTOCOL_VERSION
        );
    }

    let (mut session, pin, session_salt) = ephemeral
        .derive_session_key(ack_ecdh.ecdh_pubkey, true)
        .context("ECDH key derivation")?;

    let identity_proof = my_identity_key
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .compute_proof(&ack_ecdh.ecdh_pubkey, &session_salt);

    let metadata_json = my_metadata_json(my_device_name, is_manual_reconnect);

    let mut hello = AppMessage::Hello {
        device_id: my_device_id,
        device_name: my_device_name.to_string(),
        identity_pubkey: my_identity_key
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .public_bytes,
        identity_proof,
        metadata_json,
    };

    send_encrypted(stream, &mut session, &mut hello)
        .await
        .context("sending encrypted Hello")?;

    let ack_msg: AppMessage =
        tokio::time::timeout(Duration::from_secs(5), recv_encrypted(stream, &mut session))
            .await
            .context("timeout waiting for HelloAck")?
            .context("receiving HelloAck")?;

    let AppMessage::HelloAck {
        device_id,
        device_name,
        identity_pubkey,
        nonce_response,
        identity_proof,
        trusted,
        metadata_json: ack_metadata_json,
    } = ack_msg
    else {
        anyhow::bail!("expected HelloAck");
    };

    // CRIT-02 FIX: Verify the nonce response is meaningful.
    //
    // The responder computes: nonce_response = XOR(initiator_nonce, responder_nonce)
    // We recover: responder_nonce = XOR(initiator_nonce, nonce_response)
    //
    // Previous code: `expected = XOR(my_nonce, recovered)` then `ensure!(expected == nonce_response)`
    // That was a tautology (always true). Instead, we verify:
    //   1. The recovered responder nonce is non-trivial (not all zeros).
    //   2. The recovered responder nonce is not equal to our nonce
    //      (which would mean attacker just reflected our nonce back as nonce_response = [0;16]).
    //   3. The nonce_response itself is non-trivial.
    let recovered_responder_nonce = xor_nonces(&my_nonce, &nonce_response);
    anyhow::ensure!(
        recovered_responder_nonce != [0u8; 16],
        "handshake nonce verification failed: responder nonce is trivial (zero)"
    );
    anyhow::ensure!(
        recovered_responder_nonce != my_nonce,
        "handshake nonce verification failed: responder reflected our nonce"
    );
    anyhow::ensure!(
        nonce_response != [0u8; 16],
        "handshake nonce verification failed: nonce_response is trivial (zero)"
    );

    if !ephemeral.verify_proof(&identity_pubkey, &session_salt, &identity_proof) {
        anyhow::bail!("handshake failed: invalid identity proof (MITM or spoofed key)");
    }

    info!("Handshake complete with '{}' ({})", device_name, device_id);

    let ack_metadata = parse_metadata(ack_metadata_json);

    Ok(HandshakeResult {
        session,
        pin,
        peer_device_id: device_id,
        peer_device_name: device_name,
        peer_identity_pubkey_bytes: identity_pubkey,
        peer_already_trusted: trusted,
        is_manual_reconnect: false,
        peer_app_version: ack_metadata.app_version,
        peer_platform: ack_metadata.platform,
    })
}

pub async fn handshake_responder<F, Fut>(
    stream: &mut TcpStream,
    my_device_id: Uuid,
    my_device_name: String,
    my_identity_key: std::sync::Arc<std::sync::RwLock<crate::identity::IdentityKey>>,
    check_trust: F,
) -> Result<HandshakeResult>
where
    F: FnOnce(Uuid, [u8; 32]) -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let ecdh: EcdhFrame = tokio::time::timeout(Duration::from_secs(5), recv_frame(stream, 8192))
        .await
        .context("timeout waiting for EcdhFrame")?
        .context("receiving EcdhFrame")?;

    anyhow::ensure!(
        ecdh.version == PROTOCOL_VERSION,
        "protocol version mismatch: peer={} us={}",
        ecdh.version,
        PROTOCOL_VERSION
    );

    let ephemeral = EphemeralKeypair::generate();
    let my_nonce = crate::crypto::random_nonce16();
    let nonce_response = xor_nonces(&ecdh.nonce, &my_nonce);

    let ack_ecdh = EcdhFrame {
        version: PROTOCOL_VERSION,
        ecdh_pubkey: ephemeral.public_bytes,
        nonce: my_nonce,
    };

    send_frame(stream, &ack_ecdh)
        .await
        .context("sending EcdhFrame ack")?;

    let (mut session, pin, session_salt) = ephemeral
        .derive_session_key(ecdh.ecdh_pubkey, false)
        .context("ECDH key derivation")?;

    let hello_msg: AppMessage =
        tokio::time::timeout(Duration::from_secs(5), recv_encrypted(stream, &mut session))
            .await
            .context("timeout waiting for Hello")?
            .context("receiving Hello")?;

    let AppMessage::Hello {
        device_id,
        device_name,
        identity_pubkey,
        identity_proof,
        metadata_json,
    } = hello_msg
    else {
        anyhow::bail!("expected Hello");
    };

    let peer_metadata = parse_metadata(metadata_json);
    let is_manual_reconnect = peer_metadata.is_manual_reconnect.unwrap_or(false);

    if !ephemeral.verify_proof(&identity_pubkey, &session_salt, &identity_proof) {
        anyhow::bail!("handshake failed: invalid identity proof (MITM or spoofed key)");
    }

    let peer_is_trusted = check_trust(device_id, identity_pubkey).await;
    // Always send the real device name — name is not a security concern.
    // Trust controls data access (clipboard, files), not name visibility.
    // Masking as "Link All Device" confused users who couldn't identify
    // which device was trying to pair with them.
    let name_to_send = my_device_name.to_string();

    let identity_proof = my_identity_key
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .compute_proof(&ecdh.ecdh_pubkey, &session_salt);

    let mut ack = AppMessage::HelloAck {
        device_id: my_device_id,
        device_name: name_to_send,
        identity_pubkey: my_identity_key
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .public_bytes,
        nonce_response,
        identity_proof,
        trusted: peer_is_trusted,
        metadata_json: my_metadata_json(&my_device_name, false),
    };

    send_encrypted(stream, &mut session, &mut ack)
        .await
        .context("sending HelloAck")?;

    Ok(HandshakeResult {
        session,
        pin,
        peer_device_id: device_id,
        peer_device_name: device_name,
        peer_identity_pubkey_bytes: identity_pubkey,
        peer_already_trusted: peer_is_trusted,
        is_manual_reconnect,
        peer_app_version: peer_metadata.app_version,
        peer_platform: peer_metadata.platform,
    })
}

fn xor_nonces(a: &[u8; 16], b: &[u8; 16]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for i in 0..16 {
        out[i] = a[i] ^ b[i];
    }
    out
}

// ── Session ───────────────────────────────────────────────────────────────────

/// An established, encrypted connection to a peer.
pub struct PeerSession {
    pub stream: TcpStream,
    pub session: SessionKey,
    pub peer_device_id: Uuid,
    pub peer_device_name: String,
}

pub struct PeerSessionTx {
    pub stream: tokio::net::tcp::OwnedWriteHalf,
    pub session: SessionKey,
}

pub struct PeerSessionRx {
    pub stream: tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>,
    pub session: SessionKey,
}

impl PeerSession {
    pub fn split(self) -> (PeerSessionTx, PeerSessionRx) {
        let (rx, tx) = self.stream.into_split();
        (
            PeerSessionTx {
                stream: tx,
                session: self.session.clone(),
            },
            PeerSessionRx {
                stream: tokio::io::BufReader::new(rx),
                session: self.session,
            },
        )
    }

    pub async fn send(&mut self, msg: &mut AppMessage) -> Result<()> {
        send_encrypted(&mut self.stream, &mut self.session, msg).await
    }

    pub async fn send_no_flush(&mut self, msg: &mut AppMessage) -> Result<()> {
        send_encrypted_no_flush(&mut self.stream, &mut self.session, msg).await
    }

    pub async fn flush(&mut self) -> Result<()> {
        self.stream.flush().await.context("flushing stream")
    }

    pub async fn recv(&mut self) -> Result<AppMessage> {
        recv_encrypted(&mut self.stream, &mut self.session).await
    }
}

impl PeerSessionTx {
    pub async fn send(&mut self, msg: &mut AppMessage) -> Result<()> {
        send_encrypted(&mut self.stream, &mut self.session, msg).await
    }

    pub async fn send_no_flush(&mut self, msg: &mut AppMessage) -> Result<()> {
        send_encrypted_no_flush(&mut self.stream, &mut self.session, msg).await
    }

    pub async fn flush(&mut self) -> Result<()> {
        self.stream.flush().await.context("flushing stream")
    }
}

impl PeerSessionRx {
    pub async fn recv(&mut self) -> Result<AppMessage> {
        recv_encrypted(&mut self.stream, &mut self.session).await
    }
}

// ── Server ────────────────────────────────────────────────────────────────────

pub struct Server {
    listener: TcpListener,
}

impl Server {
    pub async fn bind(addr: SocketAddr) -> Result<Self> {
        let listener = TcpListener::bind(addr)
            .await
            .context(format!("binding to {}", addr))?;
        info!("Link All server listening on {}", addr);
        Ok(Self { listener })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.listener.local_addr().context("getting local addr")
    }

    pub async fn accept(&self) -> Result<TcpStream> {
        let (stream, addr) = self.listener.accept().await?;
        debug!("Accepted connection from {}", addr);
        optimize_stream(&stream, "accepted stream");
        Ok(stream)
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Framing round-trip ────────────────────────────────────────────────────

    #[tokio::test]
    async fn frame_round_trip_small() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let send_handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            send_frame::<String>(&mut stream, &"hello Link All v3".to_string())
                .await
                .unwrap();
        });

        let (mut server_stream, _) = listener.accept().await.unwrap();
        let received: String = recv_frame(&mut server_stream, MAX_FRAME_SIZE)
            .await
            .unwrap();
        assert_eq!(received, "hello Link All v3");
        send_handle.await.unwrap();
    }

    #[tokio::test]
    async fn frame_rejects_oversized() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            // Write a fake 80 MB length prefix (exceeds MAX_FRAME_SIZE).
            let len: u32 = 80 * 1024 * 1024;
            stream.write_all(&len.to_le_bytes()).await.unwrap();
            // Don't send body — receiver should reject before reading it.
        });

        let (mut server_stream, _) = listener.accept().await.unwrap();
        let result = recv_frame::<String>(&mut server_stream, MAX_FRAME_SIZE).await;
        assert!(result.is_err(), "oversized frame must be rejected");
    }

    // ── Nonce helpers ─────────────────────────────────────────────────────────

    #[test]
    fn xor_nonces_zero_identity() {
        let a = [0xAB_u8; 16];
        let zero = [0u8; 16];
        assert_eq!(xor_nonces(&a, &zero), a);
        assert_eq!(xor_nonces(&zero, &a), a);
    }

    #[test]
    fn xor_nonces_self_is_zero() {
        let a = [0x42_u8; 16];
        assert_eq!(xor_nonces(&a, &a), [0u8; 16]);
    }

    #[test]
    fn xor_nonces_commutative() {
        let a = [0x11_u8; 16];
        let b = [0xEE_u8; 16];
        assert_eq!(xor_nonces(&a, &b), xor_nonces(&b, &a));
    }

    // ── Nonce echo verification ───────────────────────────────────────────────

    #[test]
    fn nonce_echo_verification_logic() {
        // Simulate: initiator sends my_nonce; responder generates its own nonce
        // and computes nonce_response = XOR(my_nonce, responder_nonce).
        let my_nonce = [0x01_u8; 16];
        let responder_nonce = [0xFE_u8; 16];
        let nonce_response = xor_nonces(&my_nonce, &responder_nonce);

        // Initiator verification: recover responder_nonce from nonce_response.
        let recovered = xor_nonces(&my_nonce, &nonce_response);
        let recomputed_response = xor_nonces(&my_nonce, &recovered);
        assert_eq!(
            recomputed_response, nonce_response,
            "nonce verification failed"
        );
        assert_ne!(recovered, [0u8; 16], "responder nonce must not be all-zero");
    }

    #[test]
    fn nonce_echo_detects_trivial_replay() {
        // A replayer who doesn't know responder_nonce might just echo my_nonce back.
        let my_nonce = [0x01_u8; 16];
        // Attacker sends nonce_response = my_nonce (i.e., XOR with 0).
        let fake_nonce_response = my_nonce;
        let recovered_responder_nonce = xor_nonces(&my_nonce, &fake_nonce_response);
        // recovered_responder_nonce would be all-zero → rejected.
        assert_eq!(recovered_responder_nonce, [0u8; 16]);
    }

    // ── Handshake integration ─────────────────────────────────────────────────

    #[tokio::test]
    async fn handshake_succeeds_loopback() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        let key_a = std::sync::Arc::new(std::sync::RwLock::new(
            crate::identity::IdentityKey::generate(),
        ));
        let key_b = std::sync::Arc::new(std::sync::RwLock::new(
            crate::identity::IdentityKey::generate(),
        ));

        let key_b_clone = key_b.clone();
        let server_handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            handshake_responder(
                &mut stream,
                id_b,
                "PeerB".to_string(),
                key_b_clone,
                |_, _| async { true },
            )
            .await
            .unwrap()
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let initiator_result = handshake_initiator(&mut client, id_a, "PeerA", key_a, false)
            .await
            .unwrap();

        let responder_result = server_handle.await.unwrap();
        assert_eq!(responder_result.peer_device_id, id_a);
        assert_eq!(initiator_result.peer_device_name, "PeerB");
        assert_eq!(responder_result.peer_device_name, "PeerA");
    }

    // ── Connect timeout (structural test — doesn't actually hit network) ──────

    #[test]
    fn connect_timeout_constant_is_reasonable() {
        // Ensure the timeout is in a sensible range (1–30 s).
        assert!(CONNECT_TIMEOUT.as_secs() >= 1);
        assert!(CONNECT_TIMEOUT.as_secs() <= 30);
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn keepalive_constants_are_reasonable() {
        assert!(KEEPALIVE_IDLE.as_secs() >= 10);
        assert!(KEEPALIVE_INTERVAL.as_secs() >= 1);
        assert!(KEEPALIVE_RETRIES >= 1);
    }
    #[tokio::test(start_paused = true)]
    async fn a_quiet_link_is_not_dropped_by_the_reader() {
        let alice = EphemeralKeypair::generate();
        let (mut session, _, _) = alice
            .derive_session_key(EphemeralKeypair::generate().public_bytes, true)
            .unwrap();
        let (_writer, mut reader) = tokio::io::duplex(64);

        // Ten minutes of silence, as while a phone sleeps.
        let waited = tokio::time::timeout(
            Duration::from_secs(600),
            recv_encrypted(&mut reader, &mut session),
        )
        .await;
        assert!(waited.is_err(), "the reader gave up on a quiet link");
    }
}
