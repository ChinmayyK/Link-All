//! Files: send files, browse what a paired device shares, follow what's
//! moving right now, and find what arrived recently.

use super::{
    act, ago, human_bytes, icon_button, page_column, placeholder_row, section, send, show_in_folder,
};
use crate::{hub::Hub, ipc};
use adw::prelude::*;
use gtk::glib;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

struct Live {
    row: adw::ActionRow,
    bar: gtk::ProgressBar,
    pause: gtk::Button,
    cancel: gtk::Button,
    accept: gtk::Button,
    decline: gtk::Button,
}

pub fn page(hub: &Hub) -> gtk::ScrolledWindow {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 24);

    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .css_classes(["drop-zone"])
        .build();
    let icon = crate::theme::well("folder-download-symbolic", "blue");
    card.append(&icon);
    card.append(
        &gtk::Label::builder()
            .label("Drop files or folders here")
            .css_classes(["title-3"])
            .build(),
    );
    card.append(
        &gtk::Label::builder()
            .label("They go to the device you pick. Folders keep their structure.")
            .css_classes(["muted"])
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build(),
    );
    let buttons = gtk::Box::builder()
        .spacing(10)
        .halign(gtk::Align::Center)
        .margin_top(4)
        .build();
    let files_btn = gtk::Button::builder()
        .label("Choose Files…")
        .css_classes(["pill", "suggested-action"])
        .build();
    let folder_btn = gtk::Button::builder()
        .label("Choose a Folder…")
        .css_classes(["pill"])
        .build();
    buttons.append(&files_btn);
    buttons.append(&folder_btn);
    card.append(&buttons);
    column.append(&card);
    {
        let hub = hub.clone();
        files_btn.connect_clicked(move |_| send::pick_files(&hub, None));
    }
    {
        let hub = hub.clone();
        folder_btn.connect_clicked(move |_| send::pick_folder(&hub, None));
    }
    send::accept_drops(&card, hub, || None);
    // Light up while files are dragged over it.
    let motion = gtk::DropControllerMotion::new();
    {
        let card = card.clone();
        motion.connect_enter(move |_, _, _| card.add_css_class("hovering"));
    }
    {
        let card = card.clone();
        motion.connect_leave(move |_| card.remove_css_class("hovering"));
    }
    card.add_controller(motion);

    let (browse_box, browse_list) = section("Browse");
    column.append(&browse_box);
    follow_paired(hub, &browse_list);

    let (active_box, active_list) = section("In Progress");
    let idle = placeholder_row("Nothing is transferring");
    active_list.append(&idle);
    column.append(&active_box);
    follow_active(hub, &active_list, move |n| idle.set_visible(n == 0));
    let (recent_box, recent_list) = section("Recent");
    column.append(&recent_box);

    let add_recent = {
        let recent_list = recent_list.clone();
        move |e: &Value, newest_first: bool| {
            let kind = e["kind"].as_str().unwrap_or_default();
            if !matches!(
                kind,
                "file_transfer_complete" | "folder_transfer_complete" | "file_transfer_failed"
            ) {
                return;
            }
            // The "nothing yet" row goes once something real arrives.
            if let Some(first) = recent_list.row_at_index(0) {
                if !first.is_sensitive() {
                    recent_list.remove(&first);
                }
            }
            let row = recent_row(e);
            if newest_first {
                recent_list.prepend(&row);
            } else {
                recent_list.append(&row);
            }
        }
    };
    {
        let (recent_list, add_recent) = (recent_list.clone(), add_recent.clone());
        glib::spawn_future_local(async move {
            let entries = ipc::call(json!({ "cmd": "activity_recent", "limit": 300 }))
                .await
                .ok()
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
            for e in entries
                .iter()
                .filter(|e| {
                    e["kind"].as_str().is_some_and(|k| {
                        k.contains("transfer_complete") || k == "file_transfer_failed"
                    })
                })
                .take(40)
            {
                add_recent(e, false);
            }
            if recent_list.first_child().is_none() {
                recent_list.append(&placeholder_row("Files you send and receive show up here"));
            }
        });
    }
    hub.on_activity(move |_, e| add_recent(e, true));

    page_column(&column)
}

/// One row per paired device: a connected one opens its files, an offline one its page.
fn follow_paired(hub: &Hub, list: &gtk::ListBox) {
    let list = list.clone();
    let signature = RefCell::new(String::new());
    hub.on_status(move |hub| {
        let paired: Vec<Value> = hub
            .peers()
            .into_iter()
            .filter(|p| p["trusted"] == true)
            .collect();
        let sig: String = paired
            .iter()
            .map(|p| format!("{}{}{}", p["id"], p["friendly_name"], p["status"]))
            .collect();
        if *signature.borrow() == sig {
            return;
        }
        *signature.borrow_mut() = sig;
        list.remove_all();
        if paired.is_empty() {
            list.append(&placeholder_row("Pair a device to browse its files"));
            return;
        }
        for p in &paired {
            let id = p["id"].as_str().unwrap_or_default().to_string();
            let connected = p["status"] == "connected";
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(
                    p["friendly_name"].as_str().unwrap_or("Device"),
                ))
                .subtitle(if connected {
                    "Browse its files"
                } else {
                    "Not connected"
                })
                .activatable(true)
                .build();
            row.add_prefix(&gtk::Image::from_icon_name(crate::theme::os_icon(
                p["platform"].as_str().unwrap_or_default(),
            )));
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            row.connect_activated(move |_| {
                if connected {
                    super::files::browse(&id);
                } else {
                    super::device::open(&id);
                }
            });
            list.append(&row);
        }
    });
}

/// Keeps `list` showing the transfers moving now, one row each, with their
/// controls; `on_count` hears how many there are after each change.
pub fn follow_active(hub: &Hub, list: &gtk::ListBox, on_count: impl Fn(usize) + 'static) {
    let live: Rc<RefCell<HashMap<String, Live>>> = Rc::default();
    let (hub2, list) = (hub.clone(), list.clone());
    hub.on_status(move |hub| {
        let transfers = hub.status.borrow()["active_transfers"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut live = live.borrow_mut();
        let ids: Vec<String> = transfers
            .iter()
            .filter_map(|t| t["transfer_id"].as_str().map(str::to_string))
            .collect();
        live.retain(|id, l| {
            let keep = ids.contains(id);
            if !keep {
                list.remove(&l.row);
            }
            keep
        });
        for t in &transfers {
            let Some(id) = t["transfer_id"].as_str() else {
                continue;
            };
            let entry = live.entry(id.to_string()).or_insert_with(|| {
                let l = live_row(&hub2, t);
                list.append(&l.row);
                l
            });
            update(entry, t);
        }
        on_count(live.len());
    });
}

fn live_row(hub: &Hub, t: &Value) -> Live {
    let id = t["transfer_id"].as_str().unwrap_or_default().to_string();
    let batch = t["batch_id"].as_str().map(str::to_string);
    let title = if t["is_directory"] == true {
        format!(
            "{} ({} items)",
            t["file_name"].as_str().unwrap_or("Folder"),
            t["item_count"].as_u64().unwrap_or(0)
        )
    } else {
        t["file_name"].as_str().unwrap_or("File").to_string()
    };
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(title)
        .build();
    let (icon, tint) = if t["is_outbound"] == true {
        ("go-up-symbolic", "blue")
    } else {
        ("go-down-symbolic", "green")
    };
    let w = crate::theme::well(icon, tint);
    w.add_css_class("small");
    row.add_prefix(&w);
    let bar = gtk::ProgressBar::builder()
        .valign(gtk::Align::Center)
        .width_request(140)
        .build();
    row.add_suffix(&bar);
    let pause = icon_button("media-playback-pause-symbolic", "Pause");
    let cancel = icon_button("process-stop-symbolic", "Cancel");
    let accept = gtk::Button::builder()
        .label("Accept")
        .valign(gtk::Align::Center)
        .css_classes(["pill", "suggested-action"])
        .visible(false)
        .build();
    let decline = gtk::Button::builder()
        .label("Decline")
        .valign(gtk::Align::Center)
        .css_classes(["pill"])
        .visible(false)
        .build();
    {
        let (hub, id, pause_btn) = (hub.clone(), id.clone(), pause.clone());
        pause.connect_clicked(move |_| {
            let paused = pause_btn.icon_name().is_some_and(|i| i.contains("start"));
            let cmd = if paused {
                "resume_file_transfer"
            } else {
                "pause_file_transfer"
            };
            act(json!({ "cmd": cmd, "transfer_id": id }), None, &hub);
        });
    }
    {
        // A folder is cancelled as a whole.
        let (hub, id) = (hub.clone(), id.clone());
        cancel.connect_clicked(move |_| match &batch {
            Some(b) => act(json!({ "cmd": "cancel_folder", "batch_id": b }), None, &hub),
            None => act(
                json!({ "cmd": "cancel_file_transfer", "transfer_id": id }),
                None,
                &hub,
            ),
        });
    }
    {
        let (hub, id) = (hub.clone(), id.clone());
        accept.connect_clicked(move |_| {
            act(
                json!({ "cmd": "accept_file_transfer", "transfer_id": id }),
                None,
                &hub,
            )
        });
    }
    {
        let (hub, id) = (hub.clone(), id.clone());
        decline.connect_clicked(move |_| {
            act(
                json!({ "cmd": "reject_file_transfer", "transfer_id": id, "reason": "Declined" }),
                None,
                &hub,
            )
        });
    }
    row.add_suffix(&decline);
    row.add_suffix(&accept);
    row.add_suffix(&pause);
    row.add_suffix(&cancel);
    Live {
        row,
        bar,
        pause,
        cancel,
        accept,
        decline,
    }
}

fn update(l: &Live, t: &Value) {
    // A file waiting for an answer shows Accept and Decline instead of progress.
    let waiting = t["status"] == "incoming";
    for w in [
        l.bar.upcast_ref::<gtk::Widget>(),
        l.pause.upcast_ref(),
        l.cancel.upcast_ref(),
    ] {
        w.set_visible(!waiting);
    }
    l.accept.set_visible(waiting);
    l.decline.set_visible(waiting);
    if waiting {
        let size = t["bytes_total"]
            .as_u64()
            .map(human_bytes)
            .unwrap_or_default();
        l.row.set_subtitle(&format!(
            "{} wants to send this · {size}",
            t["from_device"].as_str().unwrap_or("A device")
        ));
        return;
    }
    let pct = t["percent"].as_f64().unwrap_or_else(|| {
        let total = t["bytes_total"].as_f64().unwrap_or(0.0);
        if total > 0.0 {
            t["bytes_received"].as_f64().unwrap_or(0.0) * 100.0 / total
        } else {
            0.0
        }
    });
    l.bar.set_fraction((pct / 100.0).clamp(0.0, 1.0));
    let paused = t["status"]
        .as_str()
        .is_some_and(|s| s.to_ascii_lowercase().contains("pause"));
    l.pause.set_icon_name(if paused {
        "media-playback-start-symbolic"
    } else {
        "media-playback-pause-symbolic"
    });
    l.pause
        .set_tooltip_text(Some(if paused { "Resume" } else { "Pause" }));
    let who = t["from_device"].as_str().unwrap_or("device");
    let mut parts = vec![
        format!(
            "{} {who}",
            if t["is_outbound"] == true {
                "To"
            } else {
                "From"
            }
        ),
        format!("{pct:.0}%"),
    ];
    if paused {
        parts.push("Paused".into());
    } else {
        if let Some(bps) = t["speed_bps"].as_f64().filter(|b| *b > 0.0) {
            parts.push(format!("{}/s", human_bytes(bps as u64)));
        }
        if let Some(eta) = t["eta_secs"].as_u64().filter(|e| *e > 0) {
            parts.push(if eta < 60 {
                format!("{eta} s left")
            } else {
                format!("{} min left", eta / 60)
            });
        }
    }
    l.row.set_subtitle(&parts.join(" · "));
}

fn recent_row(e: &Value) -> adw::ActionRow {
    let failed = e["kind"] == "file_transfer_failed";
    let name = e["file_name"].as_str().unwrap_or("File");
    let device = e["device_name"].as_str().unwrap_or("a device");
    let dest = e["dest_path"].as_str().map(str::to_string);
    let what = match (failed, dest.is_some()) {
        (true, _) => format!("Failed · {device}"),
        (false, true) => format!("Received from {device}"),
        (false, false) => format!("Sent · {device}"),
    };
    let size = e["file_bytes"]
        .as_u64()
        .map(|b| format!(" · {}", human_bytes(b)))
        .unwrap_or_default();
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(name)
        .subtitle(format!(
            "{what}{size} · {}",
            ago(e["timestamp_ms"].as_u64().unwrap_or(0) / 1000)
        ))
        .build();
    row.add_prefix(&gtk::Image::from_icon_name(if failed {
        "dialog-warning-symbolic"
    } else {
        "emblem-ok-symbolic"
    }));
    if let Some(path) = dest {
        let open = gtk::Button::builder()
            .label("Open")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        let reveal = icon_button("folder-open-symbolic", "Show in Folder");
        {
            let path = path.clone();
            open.connect_clicked(move |_| super::open_file(std::path::Path::new(&path)));
        }
        reveal.connect_clicked(move |_| show_in_folder(std::path::Path::new(&path)));
        row.add_suffix(&open);
        row.add_suffix(&reveal);
    }
    row
}
