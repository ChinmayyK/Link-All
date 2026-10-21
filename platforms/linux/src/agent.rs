//! What Link All does on the desktop while it runs in the background: keeps
//! the system clipboard in sync, and shows phone notifications, calls and
//! received files as desktop notifications.

use crate::{hub::Hub, ipc};
use adw::prelude::*;
use base64::Engine as _;
use gtk::{gio, glib};
use serde_json::{json, Value};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::mpsc;
use std::time::Duration;

/// Something to put on this computer's clipboard.
pub enum Incoming {
    Text(String),
    Png(Vec<u8>),
}

pub type ClipboardTx = mpsc::Sender<Incoming>;

fn hash_of(data: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    data.hash(&mut h);
    h.finish()
}

/// Watches the clipboard and writes incoming clips to it, on its own thread.
///
/// Wayland only shows the clipboard to the focused window, so this goes
/// through `arboard`: the wlr/ext data-control protocol where the compositor
/// offers it (KDE, Sway, Hyprland…) and XWayland otherwise (GNOME).
pub fn start_clipboard() -> ClipboardTx {
    let (tx, rx) = mpsc::channel::<Incoming>();
    std::thread::Builder::new()
        .name("linkall-clipboard".into())
        .spawn(move || {
            let mut clipboard = loop {
                match arboard::Clipboard::new() {
                    Ok(c) => break c,
                    Err(error) => {
                        tracing::warn!("clipboard unavailable, retrying: {error}");
                        std::thread::sleep(Duration::from_secs(5));
                    }
                }
            };
            let mut last_text: Option<u64> = clipboard.get_text().ok().map(hash_of);
            let mut last_image: Option<u64> = clipboard.get_image().ok().map(|i| hash_of(&i.bytes));
            let mut tick: u32 = 0;
            loop {
                while let Ok(item) = rx.try_recv() {
                    match item {
                        Incoming::Text(text) => {
                            last_text = Some(hash_of(&text));
                            if let Err(e) = clipboard.set_text(text) {
                                tracing::warn!("couldn't set the clipboard: {e}");
                            }
                        }
                        Incoming::Png(png) => match image::load_from_memory(&png) {
                            Ok(img) => {
                                let rgba = img.to_rgba8();
                                let (w, h) = rgba.dimensions();
                                let bytes = rgba.into_raw();
                                last_image = Some(hash_of(&bytes));
                                let _ = clipboard.set_image(arboard::ImageData { width: w as usize, height: h as usize, bytes: bytes.into() });
                            }
                            Err(e) => tracing::warn!("couldn't read an incoming image: {e}"),
                        },
                    }
                }

                if let Ok(text) = clipboard.get_text() {
                    let h = hash_of(&text);
                    if last_text != Some(h) {
                        last_text = Some(h);
                        if !text.trim().is_empty() {
                            if let Err(e) = ipc::call_blocking(json!({ "cmd": "push_text", "text": text })) {
                                tracing::debug!("clipboard not sent: {e}");
                            }
                        }
                    }
                }
                // Images are big to compare; look less often.
                if tick.is_multiple_of(4) {
                    if let Ok(img) = clipboard.get_image() {
                        let h = hash_of(&img.bytes);
                        if last_image != Some(h) {
                            last_image = Some(h);
                            if let Some(png) = to_png(&img) {
                                let data = base64::engine::general_purpose::STANDARD.encode(png);
                                let _ = ipc::call_blocking(json!({ "cmd": "push_image", "mime": "image/png", "data_base64": data }));
                            }
                        }
                    }
                }
                tick = tick.wrapping_add(1);
                std::thread::sleep(Duration::from_millis(500));
            }
        })
        .expect("clipboard thread");
    tx
}

fn to_png(img: &arboard::ImageData) -> Option<Vec<u8>> {
    let buf = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.to_vec())?;
    let mut out = std::io::Cursor::new(Vec::new());
    buf.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// Reacts to activity and calls with clipboard writes and notifications.
pub fn start(app: &adw::Application, hub: &Hub, clipboard: ClipboardTx) {
    install_actions(app, hub, &clipboard);
    watch_incoming_files(app, hub);
    watch_speed_tests(hub);
    report_power_and_storage();
    watch_screenshots(hub);

    let app_ref = app.clone();
    hub.on_activity(move |hub, entry| {
        let from_other = entry["device_id"]
            .as_str()
            .is_some_and(|id| id != hub.local_id());
        let device = entry["device_name"]
            .as_str()
            .unwrap_or("A device")
            .to_string();
        let id = entry["id"].as_u64().unwrap_or(0);
        match entry["kind"].as_str().unwrap_or_default() {
            "clipboard_text" | "clipboard_image" | "remote_clipboard_available" if from_other => {
                let apply = hub.setting("auto_apply_remote_clipboard") != false;
                let announce = hub.setting("show_receive_notification") == true;
                let (app, clipboard) = (app_ref.clone(), clipboard.clone());
                glib::spawn_future_local(async move {
                    let Ok(clip) =
                        ipc::call(json!({ "cmd": "incoming_clipboard", "id": id })).await
                    else {
                        return;
                    };
                    let item = match clip["type"].as_str() {
                        Some("text") => {
                            clip["text"].as_str().map(|t| Incoming::Text(t.to_string()))
                        }
                        Some("image") => clip["data_base64"]
                            .as_str()
                            .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
                            .map(Incoming::Png),
                        _ => None,
                    };
                    let Some(item) = item else { return };
                    if apply {
                        let _ = clipboard.send(item);
                    }
                    if announce {
                        let n = gio::Notification::new(&format!("Clipboard from {device}"));
                        if let Some(text) = clip["text"].as_str() {
                            n.set_body(Some(&preview(text)));
                        }
                        n.set_default_action("app.show");
                        app.send_notification(Some("clipboard"), &n);
                    }
                });
            }
            "remote_notification" => {
                let title = entry["file_name"].as_str().unwrap_or(&device).to_string();
                let text = entry["text_preview"].as_str().unwrap_or_default();
                // A one-time code from the phone's texts gets a popup of its own.
                if let Some(code) = linkall_core::remote_input::verification_code(&title, text) {
                    let n = gio::Notification::new(&format!("Verification code: {code}"));
                    n.set_body(Some(&format!(
                        "{}\n{device}",
                        title
                            .trim_start_matches("Verification code")
                            .trim_start_matches(" from ")
                    )));
                    n.set_priority(gio::NotificationPriority::Urgent);
                    n.add_button_with_target_value(
                        "Copy Code",
                        "app.copy-code",
                        Some(&code.to_variant()),
                    );
                    n.add_button_with_target_value(
                        "Paste",
                        "app.paste-code",
                        Some(&code.to_variant()),
                    );
                    n.set_default_action_and_target_value(
                        "app.copy-code",
                        Some(&code.to_variant()),
                    );
                    app_ref.send_notification(Some(&format!("code-{id}")), &n);
                    return;
                }
                let n = gio::Notification::new(&title);
                n.set_body(Some(&format!("{}\n{device}", preview(text))));
                n.set_default_action("app.show");
                app_ref.send_notification(Some(&format!("phone-{id}")), &n);
            }
            "file_transfer_complete" | "folder_transfer_complete" if from_other => {
                let Some(path) = entry["dest_path"].as_str() else {
                    return;
                };
                let name = entry["file_name"].as_str().unwrap_or("A file");
                let n = gio::Notification::new(&format!("Received {name}"));
                n.set_body(Some(&format!("From {device}")));
                n.add_button_with_target_value("Open", "app.open-path", Some(&path.to_variant()));
                n.add_button_with_target_value(
                    "Show in Folder",
                    "app.show-in-folder",
                    Some(&path.to_variant()),
                );
                n.set_default_action_and_target_value("app.open-path", Some(&path.to_variant()));
                app_ref.send_notification(Some(&format!("file-{id}")), &n);
            }
            "file_transfer_failed" if from_other => {
                let name = entry["file_name"].as_str().unwrap_or("A file");
                let n = gio::Notification::new(&format!("Couldn't receive {name}"));
                n.set_body(Some(&format!("From {device}")));
                n.set_default_action("app.show");
                app_ref.send_notification(Some(&format!("file-{id}")), &n);
            }
            _ => {}
        }
    });

    // A ringing phone is shown until it stops ringing.
    let app_ref = app.clone();
    let ringing = std::cell::RefCell::new(None::<String>);
    hub.on_status(move |hub| {
        let call = hub.status.borrow()["active_call"].clone();
        let now = (call["state"] == "ringing")
            .then(|| format!("{}{}", call["device_id"], call["number"]));
        if now == *ringing.borrow() {
            return;
        }
        if let Some(key) = &now {
            let who = [call["contact_name"].as_str(), call["number"].as_str()]
                .into_iter()
                .flatten()
                .find(|s| !s.is_empty())
                .unwrap_or("Unknown caller");
            let n = gio::Notification::new(&format!("Incoming call: {who}"));
            n.set_body(Some(&format!(
                "On {}",
                call["device_name"].as_str().unwrap_or("your phone")
            )));
            n.set_priority(gio::NotificationPriority::Urgent);
            n.set_default_action("app.show");
            let device = call["device_id"].as_str().unwrap_or_default();
            n.add_button_with_target_value(
                "Decline",
                "app.call-action",
                Some(&format!("decline|{device}").to_variant()),
            );
            n.add_button_with_target_value(
                "Answer",
                "app.call-action",
                Some(&format!("accept|{device}").to_variant()),
            );
            app_ref.send_notification(Some("call"), &n);
            tracing::info!("call {key}");
        } else {
            app_ref.withdraw_notification("call");
        }
        *ringing.borrow_mut() = now;
    });
}

fn preview(text: &str) -> String {
    let one_line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > 140 {
        format!("{}…", one_line.chars().take(140).collect::<String>())
    } else {
        one_line
    }
}

/// Actions notification buttons point at.
fn install_actions(app: &adw::Application, hub: &Hub, clipboard: &ClipboardTx) {
    // Verification codes: "Copy Code" puts just the code on the clipboard, and
    // "Paste" also presses Control-V in the app that has focus.
    for (name, paste) in [("copy-code", false), ("paste-code", true)] {
        let action = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
        let clipboard = clipboard.clone();
        action.connect_activate(move |_, code| {
            let Some(code) = code.and_then(|c| c.str()) else {
                return;
            };
            let _ = clipboard.send(Incoming::Text(code.to_string()));
            if paste {
                // Give the clipboard a moment to take the code first.
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    let _ = linkall_core::remote_input::paste();
                });
            }
        });
        app.add_action(&action);
    }

    let open = gio::SimpleAction::new("open-path", Some(glib::VariantTy::STRING));
    open.connect_activate(|_, path| {
        if let Some(path) = path.and_then(|p| p.str()) {
            let _ = gio::AppInfo::launch_default_for_uri(
                &gio::File::for_path(path).uri(),
                None::<&gio::AppLaunchContext>,
            );
        }
    });
    app.add_action(&open);

    let reveal = gio::SimpleAction::new("show-in-folder", Some(glib::VariantTy::STRING));
    reveal.connect_activate(|_, path| {
        if let Some(path) = path.and_then(|p| p.str()) {
            crate::ui::show_in_folder(std::path::Path::new(path));
        }
    });
    app.add_action(&reveal);

    let accept = gio::SimpleAction::new("accept-transfer", Some(glib::VariantTy::STRING));
    {
        let hub = hub.clone();
        accept.connect_activate(move |_, id| {
            if let Some(id) = id.and_then(|v| v.str()) {
                crate::ui::act(
                    json!({ "cmd": "accept_file_transfer", "transfer_id": id }),
                    None,
                    &hub,
                );
            }
        });
    }
    app.add_action(&accept);

    let decline = gio::SimpleAction::new("decline-transfer", Some(glib::VariantTy::STRING));
    {
        let hub = hub.clone();
        decline.connect_activate(move |_, id| {
            if let Some(id) = id.and_then(|v| v.str()) {
                crate::ui::act(json!({ "cmd": "reject_file_transfer", "transfer_id": id, "reason": "Declined" }), None, &hub);
            }
        });
    }
    app.add_action(&decline);

    // "accept|<device>" or "decline|<device>".
    let call = gio::SimpleAction::new("call-action", Some(glib::VariantTy::STRING));
    {
        let hub = hub.clone();
        call.connect_activate(move |_, v| {
            if let Some((action, device)) = v.and_then(|v| v.str()).and_then(|s| s.split_once('|'))
            {
                crate::ui::act(
                    json!({ "cmd": "call_action", "action": action, "target_device": device }),
                    None,
                    &hub,
                );
            }
        });
    }
    app.add_action(&call);
}

/// Files waiting for an answer (auto-accept off) get a notification with Accept and Decline.
fn watch_incoming_files(app: &adw::Application, hub: &Hub) {
    let app = app.clone();
    let seen = std::cell::RefCell::new(std::collections::HashSet::<String>::new());
    hub.on_status(move |hub| {
        let transfers = hub.status.borrow()["active_transfers"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let waiting: Vec<&Value> = transfers
            .iter()
            .filter(|t| t["status"] == "incoming")
            .collect();
        let ids: std::collections::HashSet<String> = waiting
            .iter()
            .filter_map(|t| t["transfer_id"].as_str().map(str::to_string))
            .collect();
        for t in &waiting {
            let id = t["transfer_id"].as_str().unwrap_or_default();
            if seen.borrow().contains(id) {
                continue;
            }
            let from = t["from_device"].as_str().unwrap_or("A device");
            let name = t["file_name"].as_str().unwrap_or("a file");
            let n = gio::Notification::new(&format!("{from} wants to send {name}"));
            if let Some(size) = t["bytes_total"].as_u64() {
                n.set_body(Some(&crate::ui::human_bytes(size)));
            }
            n.set_priority(gio::NotificationPriority::High);
            n.add_button_with_target_value(
                "Decline",
                "app.decline-transfer",
                Some(&id.to_variant()),
            );
            n.add_button_with_target_value("Accept", "app.accept-transfer", Some(&id.to_variant()));
            n.set_default_action("app.show");
            app.send_notification(Some(&format!("incoming-{id}")), &n);
        }
        for gone in seen.borrow().difference(&ids) {
            app.withdraw_notification(&format!("incoming-{gone}"));
        }
        *seen.borrow_mut() = ids;
    });
}

/// Shows the result when a speed test ends: the last reading of bytes over the test's length.
fn watch_speed_tests(hub: &Hub) {
    let running = std::cell::RefCell::new(std::collections::HashMap::<String, (u64, u64)>::new());
    hub.on_status(move |hub| {
        let tests = hub.status.borrow()["active_speed_tests"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut now = std::collections::HashMap::new();
        for t in &tests {
            if let Some(peer) = t["peer_id"].as_str() {
                now.insert(
                    peer.to_string(),
                    (
                        t["bytes_transferred"].as_u64().unwrap_or(0),
                        t["duration_secs"].as_u64().unwrap_or(0),
                    ),
                );
            }
        }
        for (peer, (bytes, secs)) in running.borrow().iter() {
            if !now.contains_key(peer) && *secs > 0 && *bytes > 0 {
                let mbps = *bytes as f64 * 8.0 / *secs as f64 / 1_000_000.0;
                let name = hub
                    .peer(peer)
                    .and_then(|p| p["friendly_name"].as_str().map(str::to_string))
                    .unwrap_or_else(|| "the device".into());
                crate::ui::toast(&format!("Connection to {name}: {mbps:.1} Mbps"));
            }
        }
        *running.borrow_mut() = now;
    });
}

/// Laptops tell phones their battery, and every computer its free space, as the Windows app does.
fn report_power_and_storage() {
    std::thread::Builder::new()
        .name("linkall-status".into())
        .spawn(|| loop {
            if let Some((level, charging)) = battery() {
                let _ = ipc::call_blocking(
                    json!({ "cmd": "push_battery_status", "level": level, "charging": charging }),
                );
            }
            if let Some((free, total)) = disk_space() {
                let _ = ipc::call_blocking(json!({
                    "cmd": "push_storage_status",
                    "images_bytes": 0, "videos_bytes": 0, "apps_bytes": 0,
                    "free_bytes": free, "total_bytes": total,
                }));
            }
            std::thread::sleep(Duration::from_secs(60));
        })
        .expect("status thread");
}

fn battery() -> Option<(u8, bool)> {
    for entry in std::fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
        let dir = entry.path();
        if std::fs::read_to_string(dir.join("type")).ok()?.trim() != "Battery" {
            continue;
        }
        let level: u8 = std::fs::read_to_string(dir.join("capacity"))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        let status = std::fs::read_to_string(dir.join("status")).unwrap_or_default();
        return Some((level.min(100), matches!(status.trim(), "Charging" | "Full")));
    }
    None
}

fn disk_space() -> Option<(u64, u64)> {
    let home =
        std::ffi::CString::new(dirs::home_dir()?.into_os_string().into_encoded_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(home.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let block = st.f_frsize as u64;
    Some((st.f_bavail as u64 * block, st.f_blocks as u64 * block))
}

/// Sends each new screenshot to your devices, when turned on in Settings.
fn watch_screenshots(hub: &Hub) {
    let Some(pictures) = glib::user_special_dir(glib::UserDirectory::Pictures) else {
        return;
    };
    let dirs = [pictures.join("Screenshots"), pictures.clone()];
    let hub = hub.clone();
    let monitors: Vec<gio::FileMonitor> = dirs
        .iter()
        .filter(|d| d.is_dir())
        .filter_map(|d| {
            gio::File::for_path(d)
                .monitor_directory(
                    gio::FileMonitorFlags::WATCH_MOVES,
                    None::<&gio::Cancellable>,
                )
                .ok()
        })
        .collect();
    for monitor in &monitors {
        let hub = hub.clone();
        monitor.connect_changed(move |_, file, other, event| {
            let file = match event {
                gio::FileMonitorEvent::ChangesDoneHint => file.clone(),
                gio::FileMonitorEvent::MovedIn | gio::FileMonitorEvent::Renamed => other.cloned().unwrap_or_else(|| file.clone()),
                _ => return,
            };
            let Some(path) = file.path() else { return };
            let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !name.ends_with(".png") || !(name.contains("screenshot") || name.starts_with("screen")) {
                return;
            }
            if !crate::prefs::get_bool(crate::prefs::SEND_SCREENSHOTS, false) {
                return;
            }
            for device in hub.connected() {
                if let Some(id) = device["id"].as_str() {
                    let req = json!({
                        "cmd": "send_file_path",
                        "path": path.to_string_lossy(),
                        "name": path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                        "mime": "image/png",
                        "target_device": id,
                        "batch_id": null,
                        "is_directory": false,
                        "item_count": 1,
                    });
                    crate::ui::act(req, None, &hub);
                }
            }
        });
    }
    // Monitors stop when dropped; these live as long as the app.
    std::mem::forget(monitors);
}

/// The text a clipboard history entry holds, for "Copy" in the app.
pub fn history_text(entry: &Value) -> Option<String> {
    entry["payload"]["full_text"]
        .as_str()
        .or(entry["payload"]["preview"].as_str())
        .map(str::to_string)
}
