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

fn row(e: &Value) -> adw::ActionRow {
    let summary = e["summary"].as_str().unwrap_or_default();
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
            for e in &entries {
                list.append(&row(e));
            }
            empty.set_visible(entries.is_empty());
        });
    }
    {
        let list = list.clone();
        hub.on_activity(move |_, e| {
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
