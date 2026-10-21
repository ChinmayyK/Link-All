//! The text a user can attach to a problem report: versions, what the engine
//! sees, and the last log lines. It holds no clipboard text, file names, paths,
//! device names or full addresses.

use super::*;
use crate::diag_log;
use std::fmt::Write as _;

/// How many log lines go into a report.
const LOG_LINES: usize = 200;

impl Engine {
    /// `header` carries what only the host app knows (OS version, device
    /// model, store build); it is printed first, as given.
    pub async fn diagnostic_report(&self, header: &str) -> String {
        let status = self.status_snapshot().await;
        let health = self.health().await;
        let settings = self.current_settings().await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut out = String::new();
        let _ = writeln!(out, "Link All problem report");
        let _ = writeln!(out, "Created (unix time): {now}");
        let _ = writeln!(
            out,
            "Version: {} (protocol {})",
            crate::protocol::APP_VERSION,
            crate::protocol::PROTOCOL_VERSION
        );
        let _ = writeln!(
            out,
            "Core on: {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        for line in header.lines().filter(|l| !l.trim().is_empty()) {
            let _ = writeln!(out, "{}", diag_log::redact(line.trim()));
        }
        let _ = writeln!(
            out,
            "This device: {}",
            diag_log::short_id(&self.local_device_id())
        );
        let _ = writeln!(
            out,
            "Listening on: {}",
            diag_log::redact(&status.bind_address.to_string())
        );
        let _ = writeln!(
            out,
            "Network: {}",
            match &status.active_interface {
                Some(i) => format!("{} ({})", i.name, diag_log::redact(&i.ip.to_string())),
                None => "no active interface".to_string(),
            }
        );

        let _ = writeln!(out, "\nProblems found");
        if health.is_empty() {
            let _ = writeln!(out, "  none");
        }
        for issue in &health {
            let _ = writeln!(out, "  {}: {}", issue.kind, issue.title);
        }

        let _ = writeln!(out, "\nSettings");
        let _ = writeln!(
            out,
            "  sync {} (text {}, images {}, files {})",
            on_off(settings.sync_enabled),
            on_off(settings.sync_text),
            on_off(settings.sync_images),
            on_off(settings.sync_files)
        );
        let _ = writeln!(
            out,
            "  block sensitive text {}, auto-apply remote clipboard {}, file browsing {}, remote control {}",
            on_off(settings.block_sensitive_text),
            on_off(settings.auto_apply_remote_clipboard),
            on_off(settings.allow_file_browsing),
            on_off(settings.allow_remote_control)
        );

        let connected = status
            .peers
            .iter()
            .filter(|p| p.status == PeerConnectionState::Connected)
            .count();
        let _ = writeln!(
            out,
            "\nDevices: {} known, {} connected",
            status.peers.len(),
            connected
        );
        for p in &status.peers {
            let seen = match p.last_seen {
                Some(t) if t <= now => format!("{}s ago", now - t),
                _ => "never".to_string(),
            };
            let _ = writeln!(
                out,
                "  {} {} | {:?} | paired {} | remote sync {} | seen {} | {} address(es){}",
                diag_log::short_id(&p.id),
                p.platform.as_deref().unwrap_or("unknown"),
                p.status,
                yes_no(p.trusted),
                on_off(p.remote_sync_enabled),
                seen,
                p.ips.len(),
                match &p.last_error {
                    Some(e) => format!(" | last error: {}", diag_log::redact(e)),
                    None => String::new(),
                }
            );
        }

        let _ = writeln!(out, "\nRecent log (newest last)");
        for line in diag_log::tail(LOG_LINES) {
            let _ = writeln!(out, "  {line}");
        }
        out
    }
}

fn on_off(v: bool) -> &'static str {
    if v {
        "on"
    } else {
        "off"
    }
}

fn yes_no(v: bool) -> &'static str {
    if v {
        "yes"
    } else {
        "no"
    }
}
