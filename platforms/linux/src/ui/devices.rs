//! Devices: this computer, what you can send, your paired devices, nearby
//! devices to pair, and what just happened. A device opens into a panel with
//! its battery, storage and everything you can do with it.

use super::{
    act, ago, human_bytes, icon_button, open_page, pairing, placeholder_row, send, toast, window,
};
use crate::{hub::Hub, ipc, theme};
use adw::prelude::*;
use gtk::{gio, glib};
use serde_json::{json, Value};
use std::cell::RefCell;

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .css_classes(classes.to_vec())
        .build()
}

/// A big tappable tile: tinted icon, title, one line of help, chevron.
fn tile(icon: &str, tint: &str, title: &str, sub: &str) -> gtk::Button {
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    inner.append(&theme::well(icon, tint));
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_valign(gtk::Align::Center);
    text.set_hexpand(true);
    text.append(&label(title, &["tile-title"]));
    let s = label(sub, &["tile-sub"]);
    s.set_wrap(true);
    s.set_lines(2);
    text.append(&s);
    inner.append(&text);
    inner.append(&gtk::Image::from_icon_name("go-next-symbolic"));
    gtk::Button::builder()
        .child(&inner)
        .css_classes(["tile"])
        .hexpand(true)
        .build()
}

/// A section title, with an optional link-style action at the end.
type HeaderAction<'a> = Option<(&'a str, Box<dyn Fn()>)>;

fn section_header(title: &str, action: HeaderAction) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let l = label(title, &["section-title"]);
    l.set_hexpand(true);
    row.append(&l);
    if let Some((text, f)) = action {
        let b = gtk::Button::builder()
            .label(text)
            .css_classes(["flat", "accent"])
            .build();
        b.connect_clicked(move |_| f());
        row.append(&b);
    }
    row
}

pub fn page(hub: &Hub) -> gtk::ScrolledWindow {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 14);

    // ── Hero ──
    let hero = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let hero_text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    hero_text.set_hexpand(true);
    let name_line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let prefix = label("Visible as", &["hero-prefix"]);
    prefix.set_valign(gtk::Align::End);
    let name = label("This computer", &["hero-name"]);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name_line.append(&prefix);
    name_line.append(&name);
    let state_line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let state_dot = theme::dot(false);
    let state = label("Starting…", &["live-text"]);
    let detail = label("", &["muted"]);
    state_line.append(&state_dot);
    state_line.append(&state);
    state_line.append(&detail);
    hero_text.append(&name_line);
    hero_text.append(&state_line);
    hero.append(&hero_text);
    let add = gtk::Button::builder()
        .valign(gtk::Align::Center)
        .css_classes(["pill", "suggested-action"])
        .build();
    add.set_child(Some(
        &adw::ButtonContent::builder()
            .icon_name("list-add-symbolic")
            .label("Add Device")
            .build(),
    ));
    hero.append(&add);
    let more = gio::Menu::new();
    more.append(Some("Pair a Phone with a QR Code…"), Some("app.pair-phone"));
    more.append(Some("Add by IP Address…"), Some("app.add-by-address"));
    more.append(Some("Scan for Devices"), Some("app.rescan"));
    hero.append(
        &gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .menu_model(&more)
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .tooltip_text("More")
            .build(),
    );
    column.append(&hero);
    add.set_action_name(Some("app.pair-phone"));

    // ── First run: pair a phone right here ──
    let welcome = gtk::Box::builder()
        .spacing(24)
        .css_classes(["hero-card"])
        .visible(false)
        .build();
    let steps = gtk::Box::new(gtk::Orientation::Vertical, 10);
    steps.set_hexpand(true);
    steps.set_valign(gtk::Align::Center);
    steps.append(&label("GET STARTED", &["eyebrow"]));
    steps.append(&label("Pair your phone", &["title-1"]));
    for (n, text) in [
        ("1", "Install Link All on your Android phone from Google Play or linkall.chinmaykudalkar.com"),
        ("2", "Open it on the same Wi-Fi as this computer"),
        ("3", "Tap + and scan this code, or tap this computer in its list"),
    ] {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let badge = gtk::Label::builder().label(n).css_classes(["well", "blue", "small", "heading"]).width_request(28).height_request(28).build();
        badge.set_valign(gtk::Align::Start);
        line.append(&badge);
        let t = label(text, &[]);
        t.set_wrap(true);
        t.set_hexpand(true);
        line.append(&t);
        steps.append(&line);
    }
    let get = gtk::LinkButton::builder()
        .label("Get Link All for Android")
        .uri("https://linkall.chinmaykudalkar.com/download")
        .halign(gtk::Align::Start)
        .build();
    steps.append(&get);
    welcome.append(&steps);
    welcome.append(&pairing::inline_qr(hub));
    column.append(&welcome);

    // ── Send ──
    column.append(&section_header("Send", None));
    let tiles = gtk::Grid::builder()
        .column_spacing(12)
        .row_spacing(12)
        .column_homogeneous(true)
        .valign(gtk::Align::Start)
        .build();
    let files_tile = tile(
        "document-send-symbolic",
        "blue",
        "Files & folders",
        "Send files or entire folders",
    );
    let clip_tile = tile(
        "edit-paste-symbolic",
        "red",
        "Clipboard",
        "Send what you last copied",
    );
    let browse_tile = tile(
        "folder-remote-symbolic",
        "violet",
        "Browse a device",
        "Open files on a phone or computer",
    );
    tiles.attach(&files_tile, 0, 0, 1, 1);
    tiles.attach(&clip_tile, 1, 0, 1, 1);
    tiles.attach(&browse_tile, 2, 0, 1, 1);
    column.append(&tiles);
    {
        let hub = hub.clone();
        files_tile.connect_clicked(move |_| send::pick_files(&hub, None));
    }
    {
        let hub = hub.clone();
        clip_tile.connect_clicked(move |_| send_clipboard(&hub, None));
    }
    browse_tile.connect_clicked(|_| open_page("files"));
    send::accept_drops(&files_tile, hub, || None);

    // ── Your devices ──
    let paired_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    column.append(&section_header("Your devices", None));
    column.append(&paired_list);

    // ── Nearby ──
    let nearby_head = section_header("Nearby", None);
    let nearby_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let nearby_hint = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    nearby_hint.append(&gtk::Spinner::builder().spinning(true).build());
    let hint_text = label(
        "Looking for devices on your network. Open Link All on the other device to see it here.",
        &["muted"],
    );
    hint_text.set_wrap(true);
    nearby_hint.append(&hint_text);
    column.append(&nearby_head);
    column.append(&nearby_list);
    column.append(&nearby_hint);

    // ── Recent ──
    column.append(&section_header(
        "Recent",
        Some(("See all", Box::new(|| open_page("activity")))),
    ));
    let recent = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    column.append(&recent);
    load_recent(&recent);
    {
        let recent = recent.clone();
        hub.on_activity(move |_, e| {
            if is_recent_worthy(e) {
                if let Some(first) = recent.row_at_index(0) {
                    if first.has_css_class("dim") {
                        recent.remove(&first);
                    }
                }
                recent.prepend(&recent_row(e));
                while let Some(r) = recent.row_at_index(6) {
                    recent.remove(&r);
                }
            }
        });
    }

    let signature = RefCell::new(String::new());
    let hub_rows = hub.clone();
    hub.on_status(move |hub| {
        let status = hub.status.borrow().clone();
        name.set_label(
            status["device_name"]
                .as_str()
                .or(status["local_device_name"].as_str())
                .unwrap_or("This computer"),
        );
        let connected = hub.connected().len();
        let (text, live) = if !hub.online() {
            ("Not running".to_string(), false)
        } else if status["sync_enabled"] == false {
            ("Sync paused".to_string(), false)
        } else if connected == 0 {
            ("Waiting for devices".to_string(), false)
        } else {
            ("Connected".to_string(), true)
        };
        state.set_label(&text);
        state.set_css_classes(if live { &["live-text"] } else { &["muted"] });
        state_dot.set_css_classes(if live {
            &["status-dot", "live"]
        } else {
            &["status-dot"]
        });
        detail.set_label(&match (connected, status["last_sync_at"].as_u64()) {
            (0, _) => String::new(),
            (_, Some(t)) if t > 0 => format!("synced {}", ago(t)),
            (n, _) => format!("{n} device{}", if n == 1 { "" } else { "s" }),
        });

        let sig = format!(
            "{}{}{}",
            status["peers"], status["peer_batteries"], status["peer_storages"]
        );
        if *signature.borrow() == sig {
            return;
        }
        *signature.borrow_mut() = sig;
        paired_list.remove_all();
        nearby_list.remove_all();
        let peers = hub.peers();
        let mut paired: Vec<&Value> = peers.iter().filter(|p| p["trusted"] == true).collect();
        paired.sort_by_key(|p| {
            (
                p["status"] != "connected",
                p["friendly_name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_lowercase(),
            )
        });
        for p in &paired {
            paired_list.append(&paired_row(&hub_rows, p, &status));
        }
        if paired.is_empty() {
            paired_list.append(&placeholder_row(
                "No devices yet. Scan the code above with your phone.",
            ));
        }
        welcome.set_visible(paired.is_empty());
        let nearby: Vec<&Value> = peers
            .iter()
            .filter(|p| p["trusted"] != true && p["status"] == "connected")
            .collect();
        for p in &nearby {
            nearby_list.append(&nearby_row(&hub_rows, p));
        }
        nearby_list.set_visible(!nearby.is_empty());
        nearby_hint.set_visible(nearby.is_empty());
    });

    let clamp = adw::Clamp::builder()
        .maximum_size(980)
        .tightening_threshold(700)
        .child(&column)
        .build();
    clamp.set_margin_top(22);
    clamp.set_margin_bottom(28);
    clamp.set_margin_start(18);
    clamp.set_margin_end(18);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();

    // Tiles go one per row when the page is narrow.
    let tiles_for_width = tiles.clone();
    let files_tile2 = files_tile.clone();
    let (clip_tile2, browse_tile2) = (clip_tile.clone(), browse_tile.clone());
    let last_narrow = std::cell::Cell::new(None::<bool>);
    scroll.add_tick_callback(move |w, _| {
        let narrow = w.width() < 640;
        if last_narrow.get() != Some(narrow) {
            last_narrow.set(Some(narrow));
            for t in [&files_tile2, &clip_tile2, &browse_tile2] {
                tiles_for_width.remove(t);
            }
            for (i, t) in [&files_tile2, &clip_tile2, &browse_tile2]
                .into_iter()
                .enumerate()
            {
                let (col, row) = if narrow { (0, i as i32) } else { (i as i32, 0) };
                tiles_for_width.attach(t, col, row, 1, 1);
            }
        }
        glib::ControlFlow::Continue
    });
    scroll
}

pub fn send_clipboard(hub: &Hub, target: Option<String>) {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let hub = hub.clone();
    glib::spawn_future_local(async move {
        match display.clipboard().read_text_future().await {
            Ok(Some(text)) if !text.trim().is_empty() => {
                let req = match &target {
                    Some(t) => json!({ "cmd": "push_text_to", "text": text.as_str(), "target": t }),
                    None => json!({ "cmd": "push_text", "text": text.as_str() }),
                };
                let done = if target.is_some() {
                    "Clipboard sent"
                } else {
                    "Clipboard sent to all devices"
                };
                act(req, Some(done), &hub)
            }
            _ => toast("Copy some text first, then send it"),
        }
    });
}

/// Offline because the phone's battery settings pause Link All in the
/// background (it said so when it last connected), not because of the network.
fn paused_by_phone(p: &Value) -> bool {
    p["status"] != "connected"
        && p["explicit_disconnect"] != true
        && p["background_restricted"] == true
}

fn describe(p: &Value, status: &Value) -> String {
    let id = p["id"].as_str().unwrap_or_default();
    if paused_by_phone(p) {
        return "Paused by the phone · open Link All on it".into();
    }
    if p["status"] != "connected" {
        return match p["last_seen"].as_u64() {
            Some(t) if t > 0 => format!("Not connected · seen {}", ago(t)),
            _ => "Not connected".into(),
        };
    }
    let mut parts = vec![if p["sync_enabled"] == false {
        "Sync paused".to_string()
    } else {
        "Connected".to_string()
    }];
    if let Some(b) = battery(status, id) {
        parts.push(format!(
            "{}%{}",
            b["level"],
            if b["charging"] == true {
                " charging"
            } else {
                ""
            }
        ));
    }
    if let Some(s) = storage(status, id) {
        if let Some(free) = s["free_bytes"].as_u64() {
            parts.push(format!("{} free", human_bytes(free)));
        }
    }
    parts.join(" · ")
}

fn battery<'a>(status: &'a Value, id: &str) -> Option<&'a Value> {
    status["peer_batteries"]
        .as_array()?
        .iter()
        .find(|b| b["device_id"] == id)
}

fn storage<'a>(status: &'a Value, id: &str) -> Option<&'a Value> {
    status["peer_storages"]
        .as_array()?
        .iter()
        .find(|s| s["device_id"] == id)
}

fn paired_row(hub: &Hub, p: &Value, status: &Value) -> adw::ActionRow {
    let id = p["id"].as_str().unwrap_or_default().to_string();
    let connected = p["status"] == "connected";
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(p["friendly_name"].as_str().unwrap_or("Device"))
        .subtitle(describe(p, status))
        .activatable(true)
        .build();
    let well = theme::well(
        theme::os_icon(p["platform"].as_str().unwrap_or_default()),
        if connected { "blue" } else { "" },
    );
    row.add_prefix(&well);
    if connected {
        let send_btn = icon_button("document-send-symbolic", "Send Files");
        let browse = icon_button("folder-remote-symbolic", "Browse Files");
        {
            let (hub, id) = (hub.clone(), id.clone());
            send_btn.connect_clicked(move |_| send::pick_files(&hub, Some(id.clone())));
        }
        {
            let id = id.clone();
            browse.connect_clicked(move |_| super::files::browse(&id));
        }
        row.add_suffix(&send_btn);
        row.add_suffix(&browse);
        let target = id.clone();
        send::accept_drops(&row, hub, move || Some(target.clone()));
    }
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let hub = hub.clone();
    row.connect_activated(move |_| open_device(&hub, &id));
    row
}

fn nearby_row(hub: &Hub, p: &Value) -> adw::ActionRow {
    let id = p["id"].as_str().unwrap_or_default().to_string();
    let name = p["friendly_name"].as_str().unwrap_or("Device").to_string();
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(&name)
        .subtitle(format!(
            "{} · Not paired",
            p["platform"].as_str().unwrap_or("Device")
        ))
        .build();
    row.add_prefix(&theme::well(
        theme::os_icon(p["platform"].as_str().unwrap_or_default()),
        "",
    ));
    let pair = gtk::Button::builder()
        .label("Pair")
        .valign(gtk::Align::Center)
        .css_classes(["pill"])
        .build();
    let hub = hub.clone();
    pair.connect_clicked(move |_| pairing::request(&hub, &id, &name));
    row.add_suffix(&pair);
    row
}

// ── Recent ─────────────────────────────────────────────────────────────────

fn is_recent_worthy(e: &Value) -> bool {
    matches!(
        e["kind"].as_str().unwrap_or_default(),
        "clipboard_text"
            | "clipboard_image"
            | "remote_clipboard_available"
            | "file_transfer_complete"
            | "folder_transfer_complete"
            | "remote_notification"
    )
}

fn recent_row(e: &Value) -> adw::ActionRow {
    let kind = e["kind"].as_str().unwrap_or_default();
    let (icon, tint) = match kind {
        "clipboard_text" | "remote_clipboard_available" => ("edit-paste-symbolic", "red"),
        "clipboard_image" => ("image-x-generic-symbolic", "red"),
        "remote_notification" => ("preferences-system-notifications-symbolic", "amber"),
        _ => ("folder-download-symbolic", "blue"),
    };
    let title = match kind {
        "clipboard_text" | "clipboard_image" | "remote_clipboard_available" => {
            e["text_preview"].as_str().unwrap_or("Image").to_string()
        }
        "remote_notification" => format!(
            "{}: {}",
            e["file_name"].as_str().unwrap_or_default(),
            e["text_preview"].as_str().unwrap_or_default()
        ),
        _ => e["file_name"].as_str().unwrap_or("File").to_string(),
    };
    let title: String = title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(140)
        .collect();
    let verb = match kind {
        "clipboard_text" | "clipboard_image" | "remote_clipboard_available" => "Copied on",
        "remote_notification" => "Notification from",
        _ if e["dest_path"].is_string() => "Received from",
        _ => "Sent with",
    };
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(title)
        .subtitle(format!(
            "{verb} {}",
            e["device_name"].as_str().unwrap_or("a device")
        ))
        .build();
    let w = theme::well(icon, tint);
    w.add_css_class("small");
    row.add_prefix(&w);
    row.add_suffix(&label(
        &ago(e["timestamp_ms"].as_u64().unwrap_or(0) / 1000),
        &["caption", "muted"],
    ));
    row
}

fn load_recent(list: &gtk::ListBox) {
    let list = list.clone();
    glib::spawn_future_local(async move {
        let entries = ipc::call(json!({ "cmd": "activity_recent", "limit": 120 }))
            .await
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        list.remove_all();
        let mut shown = 0;
        for e in entries.iter().filter(|e| is_recent_worthy(e)).take(6) {
            list.append(&recent_row(e));
            shown += 1;
        }
        if shown == 0 {
            let row =
                placeholder_row("Copies, files and notifications from your devices show up here");
            row.add_css_class("dim");
            list.append(&row);
        }
    });
}

// ── Device panel ───────────────────────────────────────────────────────────

fn meter(fraction: f64, class: &str) -> gtk::Box {
    let bar = gtk::Box::builder()
        .css_classes(["meter"])
        .hexpand(true)
        .build();
    let fill = gtk::Box::builder().css_classes(["fill", class]).build();
    bar.append(&fill);
    let f = fraction.clamp(0.0, 1.0);
    bar.connect_map(move |b| {
        let fill = fill.clone();
        let b = b.clone();
        glib::idle_add_local_once(move || fill.set_size_request((b.width() as f64 * f) as i32, 8));
    });
    bar
}

/// A bar split into coloured parts, e.g. photos / videos / apps / other on a phone.
fn split_meter(parts: &[(f64, &str)]) -> gtk::Box {
    let bar = gtk::Box::builder()
        .css_classes(["meter"])
        .hexpand(true)
        .spacing(2)
        .build();
    let parts: Vec<(f64, gtk::Box)> = parts
        .iter()
        .filter(|(f, _)| *f > 0.003)
        .map(|(f, c)| {
            let fill = gtk::Box::builder().css_classes(["fill", c]).build();
            bar.append(&fill);
            (*f, fill)
        })
        .collect();
    bar.connect_map(move |b| {
        let (b, parts) = (b.clone(), parts.clone());
        glib::idle_add_local_once(move || {
            for (f, fill) in &parts {
                fill.set_size_request((b.width() as f64 * f.min(1.0)) as i32, 8);
            }
        });
    });
    bar
}

fn legend(items: &[(&str, &str)]) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    for (class, text) in items {
        let item = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        item.append(
            &gtk::Box::builder()
                .valign(gtk::Align::Center)
                .css_classes(["legend-dot", class])
                .build(),
        );
        item.append(&label(text, &["caption", "muted"]));
        row.append(&item);
    }
    row
}

/// The device panel: status, battery, storage and every action for one device.
pub fn open_device(hub: &Hub, id: &str) {
    let Some(p) = hub.peer(id) else { return };
    let status = hub.status.borrow().clone();
    let name = p["friendly_name"].as_str().unwrap_or("Device").to_string();
    let connected = p["status"] == "connected";
    let id = id.to_string();

    let column = gtk::Box::new(gtk::Orientation::Vertical, 18);
    column.set_margin_top(6);
    column.set_margin_bottom(24);
    column.set_margin_start(24);
    column.set_margin_end(24);

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    let w = theme::well(
        theme::os_icon(p["platform"].as_str().unwrap_or_default()),
        if connected { "blue" } else { "" },
    );
    head.append(&w);
    let head_text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let title = label(&name, &["title-2"]);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    head_text.append(&title);
    let state = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    state.append(&theme::dot(connected));
    state.append(&label(
        &if connected {
            if p["sync_enabled"] == false {
                "Connected · sync paused".into()
            } else {
                "Connected".into()
            }
        } else {
            describe(&p, &status)
        },
        &[if connected { "live-text" } else { "muted" }],
    ));
    head_text.append(&state);
    head.append(&head_text);
    column.append(&head);

    if paused_by_phone(&p) {
        let note = gtk::Box::new(gtk::Orientation::Vertical, 4);
        note.append(&label(
            "Your phone is pausing Link All",
            &["heading", "warning"],
        ));
        let detail = label(
            "It stops Link All in the background to save battery, so it drops off when the screen is off. On the phone, open Link All and tap Allow on the battery notice.",
            &["dim-label"],
        );
        detail.set_wrap(true);
        note.append(&detail);
        column.append(&note);
    }

    if let Some(b) = battery(&status, &id) {
        let level = b["level"].as_u64().unwrap_or(0);
        let block = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let l = label("Battery", &["heading"]);
        l.set_hexpand(true);
        top.append(&l);
        top.append(&label(
            &format!(
                "{level}%{}",
                if b["charging"] == true {
                    " · charging"
                } else {
                    ""
                }
            ),
            &["heading"],
        ));
        block.append(&top);
        let m = meter(level as f64 / 100.0, "battery");
        if level <= 20 {
            if let Some(fill) = m.first_child() {
                fill.add_css_class("low");
            }
        }
        block.append(&m);
        column.append(&block);
    }

    if let Some(s) = storage(&status, &id) {
        let total = s["total_bytes"].as_f64().unwrap_or(0.0);
        if total > 0.0 {
            let free = s["free_bytes"].as_f64().unwrap_or(0.0);
            let images = s["images_bytes"].as_f64().unwrap_or(0.0);
            let videos = s["videos_bytes"].as_f64().unwrap_or(0.0);
            let apps = s["apps_bytes"].as_f64().unwrap_or(0.0);
            let other = (total - free - images - videos - apps).max(0.0);
            let block = gtk::Box::new(gtk::Orientation::Vertical, 8);
            let top = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            let l = label("Storage", &["heading"]);
            l.set_hexpand(true);
            top.append(&l);
            top.append(&label(
                &format!(
                    "{} free of {}",
                    human_bytes(free as u64),
                    human_bytes(total as u64)
                ),
                &["muted"],
            ));
            block.append(&top);
            block.append(&split_meter(&[
                (images / total, "images"),
                (videos / total, "videos"),
                (apps / total, "apps"),
                (other / total, "other"),
            ]));
            block.append(&legend(&[
                ("images", "Photos"),
                ("videos", "Videos"),
                ("apps", "Apps"),
                ("other", "Other"),
            ]));
            column.append(&block);
        }
    }

    let actions = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let dialog = adw::Dialog::builder()
        .title(&name)
        .content_width(460)
        .build();
    let add = |icon: &str, tint: &str, title: &str, sub: &str, f: Box<dyn Fn()>| {
        let row = adw::ActionRow::builder()
            .title(title)
            .activatable(true)
            .build();
        if !sub.is_empty() {
            row.set_subtitle(sub);
        }
        let w = theme::well(icon, tint);
        w.add_css_class("small");
        row.add_prefix(&w);
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        let d = dialog.clone();
        row.connect_activated(move |_| {
            d.close();
            f();
        });
        actions.append(&row);
    };
    if connected {
        let (h, i) = (hub.clone(), id.clone());
        add(
            "document-send-symbolic",
            "blue",
            "Send Files",
            "Or drop files onto the device",
            Box::new(move || send::pick_files(&h, Some(i.clone()))),
        );
        let (h, i) = (hub.clone(), id.clone());
        add(
            "folder-symbolic",
            "blue",
            "Send a Folder",
            "",
            Box::new(move || send::pick_folder(&h, Some(i.clone()))),
        );
        let (h, i) = (hub.clone(), id.clone());
        add(
            "edit-paste-symbolic",
            "red",
            "Send Clipboard",
            "What you last copied here",
            Box::new(move || send_clipboard(&h, Some(i.clone()))),
        );
        let i = id.clone();
        add(
            "folder-remote-symbolic",
            "violet",
            "Browse Files",
            "Open and download its files",
            Box::new(move || super::files::browse(&i)),
        );
        let (h, i, n) = (hub.clone(), id.clone(), name.clone());
        add(
            "web-browser-symbolic",
            "amber",
            "Open a Link",
            &format!("Opens it on {n}"),
            Box::new(move || open_link(&h, &i, &n)),
        );
        let (h, i) = (hub.clone(), id.clone());
        add(
            "network-transmit-receive-symbolic",
            "teal",
            "Test Connection Speed",
            "Takes 5 seconds",
            Box::new(move || {
                act(
                    json!({ "cmd": "start_speed_test", "device_id": i, "duration_secs": 5 }),
                    Some("Testing the connection…"),
                    &h,
                )
            }),
        );
    }
    column.append(&actions);

    let manage = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let paused = p["sync_enabled"] == false;
    let sync_row = adw::SwitchRow::builder()
        .title("Sync with this device")
        .active(!paused)
        .build();
    {
        let (h, i) = (hub.clone(), id.clone());
        sync_row.connect_active_notify(move |r| {
            let cmd = if r.is_active() {
                "resume_sync_peer"
            } else {
                "pause_sync_peer"
            };
            act(json!({ "cmd": cmd, "device_id": i }), None, &h);
        });
    }
    manage.append(&sync_row);
    let auto_row = adw::SwitchRow::builder()
        .title("Connect automatically")
        .subtitle("Reconnect whenever both devices are on the same network")
        .active(p["auto_connect"] != false)
        .build();
    {
        let (h, i) = (hub.clone(), id.clone());
        auto_row.connect_active_notify(move |r| {
            act(
                json!({ "cmd": "set_auto_connect", "device_id": i, "enabled": r.is_active() }),
                None,
                &h,
            )
        });
    }
    manage.append(&auto_row);
    let conn = action_row(if connected { "Disconnect" } else { "Connect" }, false);
    {
        let (h, i, d) = (hub.clone(), id.clone(), dialog.clone());
        conn.connect_activated(move |_| {
            d.close();
            let cmd = if connected {
                "disconnect_peer"
            } else {
                "reconnect_peer"
            };
            act(json!({ "cmd": cmd, "device_id": i }), None, &h);
        });
    }
    manage.append(&conn);
    let rename_row = action_row("Rename…", false);
    {
        let (h, i, n, d) = (hub.clone(), id.clone(), name.clone(), dialog.clone());
        rename_row.connect_activated(move |_| {
            d.close();
            rename(&h, &i, &n);
        });
    }
    manage.append(&rename_row);
    let forget_row = action_row("Forget This Device…", true);
    {
        let (h, i, n, d) = (hub.clone(), id.clone(), name.clone(), dialog.clone());
        forget_row.connect_activated(move |_| {
            d.close();
            forget(&h, &i, &n);
        });
    }
    manage.append(&forget_row);
    column.append(&manage);

    let fp = p["fingerprint_display"].as_str().unwrap_or_default();
    if !fp.is_empty() {
        let f = label(
            &format!("Key fingerprint {fp}"),
            &["caption", "muted", "mono"],
        );
        f.set_selectable(true);
        f.set_wrap(true);
        column.append(&f);
    }

    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::builder().show_title(false).build());
    view.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .child(&column)
            .build(),
    ));
    dialog.set_child(Some(&view));
    dialog.present(window().as_ref());
}

/// Asks a device to open a web link, prefilled with a link on the clipboard.
fn open_link(hub: &Hub, id: &str, name: &str) {
    let entry = adw::EntryRow::builder().title("https://").build();
    let list = gtk::ListBox::builder()
        .css_classes(["boxed-list"])
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.append(&entry);
    if let Some(display) = gtk::gdk::Display::default() {
        let entry = entry.clone();
        glib::spawn_future_local(async move {
            if let Ok(Some(text)) = display.clipboard().read_text_future().await {
                let t = text.trim();
                if t.starts_with("http://") || t.starts_with("https://") {
                    entry.set_text(t);
                }
            }
        });
    }
    let dialog = adw::AlertDialog::builder()
        .heading(format!("Open a Link on {name}"))
        .extra_child(&list)
        .close_response("cancel")
        .default_response("open")
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("open", "Open")]);
    dialog.set_response_appearance("open", adw::ResponseAppearance::Suggested);
    let (hub, id, name) = (hub.clone(), id.to_string(), name.to_string());
    dialog.connect_response(Some("open"), move |_, _| {
        let mut url = entry.text().trim().to_string();
        if !url.is_empty() && !url.contains("://") {
            url = format!("https://{url}");
        }
        if url.is_empty() {
            return;
        }
        let (hub, id, name) = (hub.clone(), id.clone(), name.clone());
        glib::spawn_future_local(async move {
            match ipc::call(json!({ "cmd": "open_url_on_device", "target_device": id, "url": url }))
                .await
            {
                Ok(_) => toast(&format!("Opened on {name}")),
                Err(e) => toast(&e.to_string()),
            }
            hub.poke();
        });
    });
    dialog.present(window().as_ref());
}

/// A row that does one thing when clicked, its title centred like a button.
fn action_row(title: &str, destructive: bool) -> adw::ActionRow {
    let row = adw::ActionRow::builder().activatable(true).build();
    let l = gtk::Label::builder()
        .label(title)
        .hexpand(true)
        .css_classes(["heading"])
        .build();
    if destructive {
        l.add_css_class("error");
    }
    row.set_child(Some(&l));
    row
}

fn rename(hub: &Hub, id: &str, name: &str) {
    let entry = adw::EntryRow::builder().title("Name").text(name).build();
    let list = gtk::ListBox::builder()
        .css_classes(["boxed-list"])
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.append(&entry);
    let dialog = adw::AlertDialog::builder()
        .heading("Rename Device")
        .extra_child(&list)
        .close_response("cancel")
        .default_response("rename")
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("rename", "Rename")]);
    dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
    let (hub, id) = (hub.clone(), id.to_string());
    dialog.connect_response(Some("rename"), move |_, _| {
        let text = entry.text().trim().to_string();
        if !text.is_empty() {
            act(
                json!({ "cmd": "rename_trusted_device", "device_id": id, "display_name": text }),
                None,
                &hub,
            );
        }
    });
    dialog.present(window().as_ref());
}

fn forget(hub: &Hub, id: &str, name: &str) {
    let dialog = adw::AlertDialog::builder()
        .heading(format!("Forget {name}?"))
        .body("It will have to be paired again before it can connect.")
        .close_response("cancel")
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("forget", "Forget")]);
    dialog.set_response_appearance("forget", adw::ResponseAppearance::Destructive);
    let (hub, id) = (hub.clone(), id.to_string());
    dialog.connect_response(Some("forget"), move |_, _| {
        act(
            json!({ "cmd": "forget_device", "device_id": id }),
            None,
            &hub,
        )
    });
    dialog.present(window().as_ref());
}

/// "Add by IP Address" for devices discovery can't see.
pub fn add_by_address(hub: &Hub) {
    let entry = adw::EntryRow::builder()
        .title("IP address or host name")
        .build();
    let list = gtk::ListBox::builder()
        .css_classes(["boxed-list"])
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.append(&entry);
    let dialog = adw::AlertDialog::builder()
        .heading("Add by IP Address")
        .body("For a device on another part of your network. Link All must be open on it.")
        .extra_child(&list)
        .close_response("cancel")
        .default_response("connect")
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("connect", "Connect")]);
    dialog.set_response_appearance("connect", adw::ResponseAppearance::Suggested);
    let hub = hub.clone();
    dialog.connect_response(Some("connect"), move |_, _| {
        let text = entry.text().trim().to_string();
        let (host, port) = match text.rsplit_once(':') {
            Some((h, p)) if p.parse::<u16>().is_ok() && !h.contains(':') => {
                (h.to_string(), p.parse::<u16>().ok())
            }
            _ => (text.clone(), None),
        };
        if host.is_empty() {
            return toast("Enter an address");
        }
        act(
            json!({ "cmd": "connect_manual", "host": host, "port": port }),
            Some("Connecting…"),
            &hub,
        );
    });
    dialog.present(window().as_ref());
}
