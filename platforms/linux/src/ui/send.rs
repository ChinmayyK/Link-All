//! Sending files and folders to a device: from the app, the tray, drag and
//! drop, or "Open With Link All" in the file manager.

use super::{act, toast, window};
use crate::hub::Hub;
use adw::prelude::*;
use gtk::gio;
use serde_json::json;
use std::path::PathBuf;

/// Lets the user pick files, then a device.
pub fn pick_files(hub: &Hub, target: Option<String>) {
    let dialog = gtk::FileDialog::builder()
        .title("Send Files")
        .modal(true)
        .build();
    let hub = hub.clone();
    dialog.open_multiple(
        window().as_ref(),
        None::<&gio::Cancellable>,
        move |result| {
            let Ok(files) = result else { return };
            let paths: Vec<PathBuf> = (0..files.n_items())
                .filter_map(|i| files.item(i).and_downcast::<gio::File>())
                .filter_map(|f| f.path())
                .collect();
            send_paths(&hub, paths, target.clone());
        },
    );
}

/// Lets the user pick a folder, then a device.
pub fn pick_folder(hub: &Hub, target: Option<String>) {
    let dialog = gtk::FileDialog::builder()
        .title("Send a Folder")
        .modal(true)
        .build();
    let hub = hub.clone();
    dialog.select_folder(
        window().as_ref(),
        None::<&gio::Cancellable>,
        move |result| {
            if let Some(path) = result.ok().and_then(|f| f.path()) {
                send_paths(&hub, vec![path], target.clone());
            }
        },
    );
}

/// Sends to `target`, or asks which device when more than one is connected.
pub fn send_paths(hub: &Hub, paths: Vec<PathBuf>, target: Option<String>) {
    if paths.is_empty() {
        return;
    }
    if let Some(target) = target {
        return send_to(hub, &paths, &target);
    }
    let devices = hub.connected();
    match devices.len() {
        0 => toast("No device is connected"),
        1 => send_to(hub, &paths, devices[0]["id"].as_str().unwrap_or_default()),
        _ => {
            let what = if paths.len() == 1 {
                paths[0]
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            } else {
                format!("{} items", paths.len())
            };
            let dialog = adw::AlertDialog::builder()
                .heading("Send to Which Device?")
                .body(what)
                .close_response("cancel")
                .build();
            dialog.add_response("cancel", "Cancel");
            for d in &devices {
                dialog.add_response(
                    d["id"].as_str().unwrap_or_default(),
                    d["friendly_name"].as_str().unwrap_or("Device"),
                );
            }
            let hub = hub.clone();
            dialog.connect_response(None, move |_, response| {
                if response != "cancel" {
                    send_to(&hub, &paths, response);
                }
            });
            if let Some(win) = window() {
                win.present();
                dialog.present(Some(&win));
            }
        }
    }
}

fn send_to(hub: &Hub, paths: &[PathBuf], device: &str) {
    for path in paths {
        let path_str = path.to_string_lossy().to_string();
        let req = if path.is_dir() {
            json!({ "cmd": "send_folder", "path": path_str, "target_device": device })
        } else {
            json!({
                "cmd": "send_file_path",
                "path": path_str,
                "name": path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                "mime": mime_guess::from_path(path).first_or_octet_stream().to_string(),
                "target_device": device,
                "batch_id": null,
                "is_directory": false,
                "item_count": 1,
            })
        };
        act(req, None, hub);
    }
    let name = hub
        .peer(device)
        .and_then(|p| p["friendly_name"].as_str().map(str::to_string))
        .unwrap_or_else(|| "the device".into());
    toast(&match paths.len() {
        1 => format!("Sending to {name}"),
        n => format!("Sending {n} items to {name}"),
    });
}

/// Accepts files dropped onto a widget and sends them to `target` (or asks).
pub fn accept_drops(
    widget: &impl IsA<gtk::Widget>,
    hub: &Hub,
    target: impl Fn() -> Option<String> + 'static,
) {
    let drop = gtk::DropTarget::new(gio::File::static_type(), gtk::gdk::DragAction::COPY);
    drop.set_types(&[gtk::gdk::FileList::static_type()]);
    let hub = hub.clone();
    drop.connect_drop(move |_, value, _, _| {
        let Ok(list) = value.get::<gtk::gdk::FileList>() else {
            return false;
        };
        let paths: Vec<PathBuf> = list.files().iter().filter_map(|f| f.path()).collect();
        send_paths(&hub, paths, target());
        true
    });
    widget.add_controller(drop);
}
