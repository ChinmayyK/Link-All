//! Deskdrop Linux — headless daemon + optional GTK4 tray.
//! Without --features gtk, runs as a pure headless daemon.
//!
//! Key guarantees vs the original:
//!  • No echo loop — suppress counter prevents re-pushing received clipboard.
//!  • IPC socket started so deskdrop-cli works on Linux.
//!  • TOFU prompts give actionable CLI instructions.
//!  • notify-send is rate-limited (max 1 per 2 s).
//!  • Clipboard poll backs off to 500 ms when idle.
//!  • Hash-based dedup — no full-text clone in memory for every tick.
//!  • Image clipboard applied via arboard (PNG).

#[cfg(target_os = "linux")]
use deskdrop_core::{
    engine::{Engine, EngineConfig, EngineEvent},
    protocol::ClipboardContent,
};
#[cfg(target_os = "linux")]
use std::{
    hash::{Hash, Hasher},
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
#[cfg(target_os = "linux")]
use tokio::sync::mpsc;

// ── Suppress counter ──────────────────────────────────────────────────────────
//
// Incremented before we write to the clipboard ourselves.
// The watcher checks and decrements it, skipping the push for that tick.
#[cfg(target_os = "linux")]
static SUPPRESS_COUNT: AtomicU32 = AtomicU32::new(0);

#[cfg(target_os = "linux")]
fn suppress_next() {
    SUPPRESS_COUNT.fetch_add(1, Ordering::SeqCst);
}

/// Returns true and decrements the counter if suppression is active.
#[cfg(target_os = "linux")]
fn should_suppress() -> bool {
    SUPPRESS_COUNT
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
            if v > 0 {
                Some(v - 1)
            } else {
                None
            }
        })
        .is_ok()
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("deskdrop-linux is only supported on Linux");
}

#[cfg(target_os = "linux")]
fn main() {
    unsafe {
        libc::umask(0o077);
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("DESKDROP_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let rt = Arc::new(tokio::runtime::Runtime::new().expect("Tokio runtime"));
    let (event_tx, event_rx) = mpsc::channel(256);
    let config = EngineConfig::default();

    let engine = Arc::new(
        rt.block_on(Engine::start(config, event_tx))
            .expect("Deskdrop engine failed to start"),
    );

    tracing::info!(
        "Deskdrop Linux started. IPC socket: {:?}",
        deskdrop_core::ipc::socket_path()
    );

    // ── IPC server ────────────────────────────────────────────────────────────
    // Starts the Unix socket so `deskdrop-cli` can communicate with us.
    // The IPC server deserializes commands, calls engine methods, and returns JSON.
    {
        let engine_ipc = engine.clone();
        rt.block_on(async move {
            // The IPC server takes a callback that maps IpcRequest → IpcResponse.
            // We reuse the same handler used by the standalone daemon binary.
            let result = deskdrop_core::ipc::server::spawn_with_engine(engine_ipc).await;
            match result {
                Ok(()) => tracing::info!(
                    "IPC server listening at {:?}",
                    deskdrop_core::ipc::socket_path()
                ),
                Err(err) => tracing::warn!("IPC server failed to start: {err:#}"),
            }
        });
    }

    // ── Clipboard watcher ─────────────────────────────────────────────────────
    {
        let engine_clip = engine.clone();
        let rt_clip = rt.clone();
        std::thread::Builder::new()
            .name("clipboard-watcher".into())
            .spawn(move || {
                let mut cb = match arboard::Clipboard::new() {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("clipboard init: {e}");
                        return;
                    }
                };

                let mut last_hash: u64 = 0;
                let mut idle_streak: u32 = 0;

                loop {
                    // Back off to 500 ms after 10 consecutive idle ticks.
                    let interval = if idle_streak < 10 {
                        Duration::from_millis(100)
                    } else {
                        Duration::from_millis(500)
                    };
                    std::thread::sleep(interval);

                    if should_suppress() {
                        idle_streak = idle_streak.saturating_add(1);
                        continue;
                    }

                    match cb.get_text() {
                        Ok(text) if !text.is_empty() => {
                            let hash = {
                                let mut h = std::collections::hash_map::DefaultHasher::new();
                                text.hash(&mut h);
                                h.finish()
                            };
                            if hash != last_hash {
                                last_hash = hash;
                                idle_streak = 0;
                                rt_clip.block_on(
                                    engine_clip.push_clipboard(ClipboardContent::Text(text)),
                                );
                            } else {
                                idle_streak = idle_streak.saturating_add(1);
                            }
                        }
                        _ => idle_streak = idle_streak.saturating_add(1),
                    }
                }
            })
            .expect("clipboard watcher thread");
    }

    // ── Event drain ───────────────────────────────────────────────────────────
    let engine_ev = engine.clone();
    rt.block_on(async move {
        let mut rx = event_rx;
        let mut last = Instant::now() - Duration::from_secs(10);
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to register SIGTERM handler");

        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    handle_event(event, &engine_ev, &mut last).await;
                }
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("Shutting down on SIGINT.");
                    break;
                }
                _ = sigterm.recv() => {
                    tracing::info!("Shutting down on SIGTERM.");
                    break;
                }
            }
        }

        tracing::info!("Cleaning up resources...");
        let _ = std::fs::remove_file(deskdrop_core::ipc::socket_path());
    });
}

// ── Event handler ─────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
async fn handle_event(event: EngineEvent, _engine: &Arc<Engine>, last_notify: &mut Instant) {
    match event {
        EngineEvent::ClipboardReceived {
            from_name,
            content,
            auto_applied,
            ..
        } => {
            tracing::info!("{} from '{}'", content.kind_str(), from_name);
            if auto_applied {
                suppress_next();
                match apply_clipboard_content(&content) {
                    Ok(()) => rate_limited_notify(
                        last_notify,
                        "Link All",
                        &format!("Clipboard from {from_name}"),
                    ),
                    Err(e) => {
                        tracing::warn!("Failed to apply clipboard: {e}");
                        SUPPRESS_COUNT
                            .try_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
                                Some(v.saturating_sub(1))
                            })
                            .ok();
                    }
                }
            } else {
                tracing::info!("  (not auto-applied — timeline-first mode)");
                rate_limited_notify(
                    last_notify,
                    &format!("Clipboard from {from_name}"),
                    &content.preview_string(),
                );
            }
        }

        EngineEvent::ClipboardSynced { peer_name, .. } => {
            tracing::debug!("synced to {}", peer_name);
        }

        EngineEvent::ClipboardSyncFailed {
            peer_name, reason, ..
        } => {
            tracing::warn!("sync to '{}' failed: {}", peer_name, reason);
        }

        EngineEvent::PeerConnected { device_name, .. } => {
            tracing::info!("connected to {}", device_name);
            rate_limited_notify(
                last_notify,
                "Link All",
                &format!("Connected to {device_name}"),
            );
        }

        EngineEvent::PeerDisconnected { device_name, .. } => {
            tracing::info!("{} disconnected", device_name.as_deref().unwrap_or("peer"));
        }

        // New device wants to pair.
        // Headless: log prominently + notify; user responds via deskdrop-cli.
        EngineEvent::PairingRequested {
            device_id,
            device_name,
            pin,
        } => {
            let fp = pin
                .lines()
                .map(|l| format!("   {l}"))
                .collect::<Vec<_>>()
                .join("\n");

            // `pair accept` answers the request, so the other device stops
            // waiting; the old hint named a command that doesn't exist.
            tracing::warn!(
                "🔐 Pairing request from '{}' ({})\n   Code:\n{fp}\n\n\
                 Accept only if the other device shows the same code:\n\
                 To accept:  deskdrop-cli pair accept {}\n\
                 To decline: deskdrop-cli pair decline {}",
                device_name,
                device_id,
                device_id,
                device_id,
            );

            rate_limited_notify(
                last_notify,
                &format!("{device_name} wants to pair · code {pin}"),
                &format!("Run: deskdrop-cli pair accept {device_id}"),
            );
        }

        EngineEvent::Warning(w) => tracing::warn!("{}", w),

        _ => {}
    }
}

// ── Clipboard application ─────────────────────────────────────────────────────
//
// Called only from handle_event when auto_applied=true — meaning the engine
// already recorded the item as applied but delegated the actual OS write to us.
// (On desktop Linux the engine can't touch the clipboard directly since it runs
// as a library; the binary is responsible for calling arboard.)

#[cfg(target_os = "linux")]
pub fn apply_clipboard_content(content: &ClipboardContent) -> anyhow::Result<()> {
    match content {
        ClipboardContent::Text(text) => {
            let mut cb = arboard::Clipboard::new()?;
            cb.set_text(text)?;
            Ok(())
        }

        ClipboardContent::Image { data, mime } => {
            if !mime.starts_with("image/") {
                anyhow::bail!("unsupported image mime type: {mime}");
            }
            let img = image::load_from_memory(data)?;
            let rgba = img.into_rgba8();
            let (w, h) = rgba.dimensions();
            let mut cb = arboard::Clipboard::new()?;
            cb.set_image(arboard::ImageData {
                width: w as usize,
                height: h as usize,
                bytes: std::borrow::Cow::Owned(rgba.into_raw()),
            })?;
            Ok(())
        }

        ClipboardContent::File { .. } => {
            // Files are saved to disk by the engine — nothing to do here.
            Ok(())
        }
    }
}

// ── Notification helpers ──────────────────────────────────────────────────────

/// At most one notification every 2 seconds.
#[cfg(target_os = "linux")]
fn rate_limited_notify(last: &mut Instant, summary: &str, body: &str) {
    if last.elapsed() < Duration::from_secs(2) {
        return;
    }
    *last = Instant::now();
    notify(summary, body);
}

#[cfg(target_os = "linux")]
fn notify(summary: &str, body: &str) {
    let _ = std::process::Command::new("notify-send")
        .args([
            "--app-name=Deskdrop",
            "--icon=deskdrop",
            "--urgency=normal",
            "--expire-time=3000",
            summary,
            body,
        ])
        .spawn();
}
