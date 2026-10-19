//! Activity: what happened across your devices, newest first.

use super::{ago, page_column, placeholder_row};
use crate::{hub::Hub, ipc};
use adw::prelude::*;
use gtk::glib;
use serde_json::{json, Value};

fn icon(kind: &str) -> &'static str {
    match kind {
        "clipboard_text" | "clipboard_applied" | "remote_clipboard_available" => {
            "edit-paste-symbolic"
        }
        "clipboard_image" => "image-x-generic-symbolic",
        "file_transfer_started" | "file_transfer_complete" | "folder_transfer_complete" => {
            "folder-download-symbolic"
        }
        "file_transfer_failed" => "dialog-warning-symbolic",
        "peer_connected" => "network-transmit-receive-symbolic",
        "peer_disconnected" => "network-offline-symbolic",
        "sync_paused" => "media-playback-pause-symbolic",
        "sync_resumed" => "media-playback-start-symbolic",
        "remote_notification" => "preferences-system-notifications-symbolic",
        _ => "emblem-default-symbolic",
    }
}

/// A finished file's line, with its size in this app's units (the daemon's
/// summary counts in 1024s, the Transfers page in 1000s).
fn file_summary(e: &Value) -> Option<String> {
    if e["kind"] != "file_transfer_complete" {
        return None;
    }
    let name = e["file_name"].as_str()?;
    let device = e["device_name"].as_str().unwrap_or_default();
    let size = super::human_bytes(e["file_bytes"].as_u64()?);
    // A received file has the path it was saved to.
    Some(if e["dest_path"].as_str().is_some_and(|p| !p.is_empty()) {
        format!("Received {name} from {device} · {size}")
    } else {
        format!("Sent {name} to {device} · {size}")
    })
}

/// The start of a transfer gets an entry of its own, which the daemon later
/// rewrites to the finished line: next to the completion entry that is the
/// same file twice. In-progress transfers live on the Transfers page.
fn shown(e: &Value) -> bool {
    e["kind"] != "file_transfer_started"
}

fn row(e: &Value) -> adw::ActionRow {
    let summary =
        file_summary(e).unwrap_or_else(|| e["summary"].as_str().unwrap_or_default().to_string());
    let summary: String = summary
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(160)
        .collect();
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(summary)
        .title_lines(2)
        .subtitle(format!(
            "{} · {}",
            e["device_name"].as_str().unwrap_or_default(),
            ago(e["timestamp_ms"].as_u64().unwrap_or(0) / 1000)
        ))
        .build();
    row.add_prefix(&gtk::Image::from_icon_name(icon(
        e["kind"].as_str().unwrap_or_default(),
    )));
    row
}

pub fn page(hub: &Hub) -> gtk::ScrolledWindow {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let empty = placeholder_row("Nothing yet. Copy, send or connect something.");
    list.append(&empty);
    {
        let (list, empty) = (list.clone(), empty.clone());
        glib::spawn_future_local(async move {
            let entries = ipc::call(json!({ "cmd": "activity_recent", "limit": 150 }))
                .await
                .ok()
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
            let entries: Vec<_> = entries.iter().filter(|e| shown(e)).collect();
            for e in &entries {
                list.append(&row(e));
            }
            empty.set_visible(entries.is_empty());
        });
    }
    {
        let list = list.clone();
        hub.on_activity(move |_, e| {
            if !shown(e) {
                return;
            }
            empty.set_visible(false);
            list.prepend(&row(e));
            // Keep the page light.
            while let Some(last) = list.row_at_index(300) {
                list.remove(&last);
            }
        });
    }
    page_column(&list)
}
