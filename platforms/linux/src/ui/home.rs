//! Home: this computer, a card for each linked device with what it plays and
//! the things done most with it, the other devices to reconnect or pair, and
//! what was copied, is moving or just happened. A device opens into its page.

use super::{
    act, ago, device, files, icon_button, label, name_button, now_playing, open_notes, open_page,
    pairing, placeholder_row, section_header, send, toast, transfers,
};
use crate::agent::{history_text, ClipboardTx, Incoming};
use crate::{hub::Hub, ipc, theme};
use adw::prelude::*;
use gtk::{gio, glib};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// How many entries the Recent peek shows; Activity has the rest.
const RECENT: i32 = 3;

pub fn page(hub: &Hub, clipboard: &ClipboardTx) -> gtk::ScrolledWindow {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 14);
    column.append(&this_computer(hub));
    column.append(&devices(hub));
    column.append(&latest_clip(hub, clipboard));
    column.append(&moving(hub));
    column.append(&recent(hub));

    let clamp = adw::Clamp::builder()
        .maximum_size(980)
        .tightening_threshold(700)
        .child(&column)
        .build();
    clamp.set_margin_top(22);
    clamp.set_margin_bottom(28);
    clamp.set_margin_start(18);
    clamp.set_margin_end(18);
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build()
}

fn small_well(icon: &str, tint: &str) -> gtk::Box {
    let w = theme::well(icon, tint);
    w.add_css_class("small");
    w
}

/// This computer: its name, whether sync is on, and the address other
/// devices reach it at. Notes and adding a device sit next to it.
fn this_computer(hub: &Hub) -> gtk::Box {
    let block = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let name_line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let prefix = label("Visible as", &["hero-prefix"]);
    prefix.set_valign(gtk::Align::End);
    let name = label("This computer", &["hero-name"]);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name_line.append(&prefix);
    name_line.append(&name);
    let state_line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let dot = theme::dot(false);
    let state = label("Starting…", &["muted"]);
    let address = label("", &["muted"]);
    address.set_ellipsize(gtk::pango::EllipsizeMode::End);
    state_line.append(&dot);
    state_line.append(&state);
    state_line.append(&address);
    text.append(&name_line);
    text.append(&state_line);
    block.append(&text);

    let notes = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .valign(gtk::Align::Center)
        .css_classes(["flat", "circular"])
        .build();
    name_button(&notes, "Notes");
    notes.connect_clicked(|_| open_notes());
    block.append(&notes);
    let add = gtk::Button::builder()
        .valign(gtk::Align::Center)
        .css_classes(["pill", "suggested-action"])
        .action_name("app.pair-phone")
        .build();
    add.set_child(Some(
        &adw::ButtonContent::builder()
            .icon_name("list-add-symbolic")
            .label("Add device")
            .build(),
    ));
    block.append(&add);
    let more = gio::Menu::new();
    more.append(Some("Pair a phone with a QR code…"), Some("app.pair-phone"));
    more.append(Some("Add by IP address…"), Some("app.add-by-address"));
    more.append(Some("Receive from a guest…"), Some("app.receive-guest"));
    more.append(Some("Call from phone…"), Some("app.call-phone"));
    more.append(Some("Scan for devices"), Some("app.rescan"));
    let more = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .menu_model(&more)
        .valign(gtk::Align::Center)
        .css_classes(["flat", "circular"])
        .build();
    name_button(&more, "More ways to add a device");
    block.append(&more);

    hub.on_status(move |hub| {
        name.set_label(&super::device_name(hub));
        let (text, class) = super::sync_state(hub);
        state.set_label(text);
        state.set_css_classes(&[class]);
        dot.set_css_classes(match class {
            "live-text" => &["status-dot", "live"],
            "warning" => &["status-dot", "warn"],
            _ => &["status-dot"],
        });
        address.set_label(&if hub.online() {
            format!("· {}", pairing::local_address())
        } else {
            String::new()
        });
    });
    block
}

/// First run: how to pair a phone, with the code to scan right here.
fn welcome(hub: &Hub) -> gtk::Box {
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
        let badge = gtk::Label::builder()
            .label(n)
            .css_classes(["well", "blue", "small", "heading"])
            .width_request(28)
            .height_request(28)
            .build();
        badge.set_valign(gtk::Align::Start);
        line.append(&badge);
        let t = label(text, &[]);
        t.set_wrap(true);
        t.set_hexpand(true);
        line.append(&t);
        steps.append(&line);
    }
    steps.append(
        &gtk::LinkButton::builder()
            .label("Get Link All for Android")
            .uri("https://linkall.chinmaykudalkar.com/download")
            .halign(gtk::Align::Start)
            .build(),
    );
    welcome.append(&steps);
    welcome.append(&pairing::inline_qr(hub));
    welcome
}

/// A linked device on Home: who it is, what it plays, and three things to do with it.
struct Card {
    child: gtk::FlowBoxChild,
    icon: gtk::Image,
    name: gtk::Label,
    status: gtk::Label,
    head: gtk::Button,
    send: gtk::Button,
    clip: gtk::Button,
    browse: gtk::Button,
    mini: Rc<now_playing::Mini>,
    named: RefCell<String>,
}

/// A quick action on a card: an icon over its label, so three fit across.
fn quick(icon: &str, text: &str) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let image = gtk::Image::from_icon_name(icon);
    image.add_css_class("accent");
    content.append(&image);
    content.append(
        &gtk::Label::builder()
            .label(text)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["caption-heading"])
            .build(),
    );
    gtk::Button::builder()
        .child(&content)
        .css_classes(["quick-action"])
        .build()
}

fn card(hub: &Hub, id: &str) -> Card {
    let frame = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .width_request(260)
        .css_classes(["card", "la", "device-card"])
        .build();

    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let well = theme::well("computer-symbolic", "blue");
    let icon = well
        .first_child()
        .and_downcast::<gtk::Image>()
        .expect("a well holds an image");
    line.append(&well);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let name = label("", &["heading"]);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let state = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    state.append(&theme::dot(true));
    let status = label("", &["caption", "muted"]);
    status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    state.append(&status);
    text.append(&name);
    text.append(&state);
    line.append(&text);
    line.append(&gtk::Image::from_icon_name("go-next-symbolic"));
    let head = gtk::Button::builder()
        .child(&line)
        .css_classes(["flat", "device-card-head"])
        .build();
    frame.append(&head);

    let mini = now_playing::Mini::new();
    frame.append(&mini.widget);

    let actions = gtk::Box::builder().homogeneous(true).spacing(8).build();
    let send_button = quick("document-send-symbolic", "Send");
    let clip = quick("edit-paste-symbolic", "Clipboard");
    let browse = quick("folder-remote-symbolic", "Browse");
    actions.append(&send_button);
    actions.append(&clip);
    actions.append(&browse);
    frame.append(&actions);

    {
        let id = id.to_string();
        head.connect_clicked(move |_| device::open(&id));
    }
    {
        let (hub, id) = (hub.clone(), id.to_string());
        send_button.connect_clicked(move |_| send::pick_files(&hub, Some(id.clone())));
    }
    {
        let (hub, id) = (hub.clone(), id.to_string());
        clip.connect_clicked(move |_| send::send_clipboard(&hub, Some(id.clone())));
    }
    {
        let id = id.to_string();
        browse.connect_clicked(move |_| files::browse(&id));
    }
    // A click on the card's own space opens it too; its buttons keep their clicks.
    let click = gtk::GestureClick::builder().button(1).build();
    let dragged = Rc::new(Cell::new(false));
    {
        let dragged = dragged.clone();
        click.connect_pressed(move |_, _, _, _| dragged.set(false));
    }
    {
        let dragged = dragged.clone();
        click.connect_stopped(move |_| dragged.set(true));
    }
    {
        let id = id.to_string();
        click.connect_released(move |_, n, _, _| {
            if n == 1 && !dragged.get() {
                device::open(&id);
            }
        });
    }
    frame.add_controller(click);
    {
        let id = id.to_string();
        send::accept_drops(&frame, hub, move || Some(id.clone()));
    }

    Card {
        child: gtk::FlowBoxChild::builder()
            .child(&frame)
            .focusable(false)
            .build(),
        icon,
        name,
        status,
        head,
        send: send_button,
        clip,
        browse,
        mini,
        named: RefCell::default(),
    }
}

fn update_card(card: &Card, p: &Value, status: &Value) {
    let name = p["friendly_name"].as_str().unwrap_or("Device");
    card.icon.set_icon_name(Some(theme::os_icon(
        p["platform"].as_str().unwrap_or_default(),
    )));
    card.status.set_label(&device::describe(p, status));
    if *card.named.borrow() != name {
        *card.named.borrow_mut() = name.to_string();
        card.name.set_label(name);
        card.head.set_tooltip_text(Some(&format!("Open {name}")));
        card.send
            .set_tooltip_text(Some(&format!("Send files to {name}")));
        card.clip
            .set_tooltip_text(Some(&format!("Send clipboard to {name}")));
        card.browse
            .set_tooltip_text(Some(&format!("Browse files on {name}")));
    }
}

/// A device without a card: paired but away, asking to pair, or nearby to pair.
struct Other {
    id: String,
    name: String,
    platform: String,
    subtitle: String,
    /// Nearby and not paired: offered a Pair button.
    pairable: bool,
}

/// Asking to pair first, then paired devices seen most recently, then ours
/// waiting on an answer, then nearby devices to pair.
fn others(peers: &[Value], status: &Value) -> Vec<Other> {
    let mut found: Vec<(u8, i64, String, Other)> = peers
        .iter()
        .filter_map(|p| {
            let trusted = p["trusted"] == true;
            let connected = p["status"] == "connected";
            let asking = p["pairing_requested"] == true && !trusted;
            let waiting = p["outgoing_pairing_waiting"] == true && !trusted;
            let name = p["friendly_name"].as_str().unwrap_or("Device").to_string();
            let platform = p["platform"].as_str().unwrap_or_default().to_string();
            let (rank, subtitle) = if trusted && connected {
                return None;
            } else if asking {
                (0, "Wants to pair".to_string())
            } else if trusted {
                (1, device::describe(p, status))
            } else if waiting {
                (2, format!("Waiting for {name} to accept"))
            } else if connected {
                let what = if platform.is_empty() {
                    "Device"
                } else {
                    &platform
                };
                let state = device::pairing_outcome(p).unwrap_or("Not paired");
                (3, format!("{what} · {state}"))
            } else {
                return None;
            };
            let seen = -(p["last_seen"].as_u64().unwrap_or(0) as i64);
            Some((
                rank,
                if rank == 1 { seen } else { 0 },
                name.to_lowercase(),
                Other {
                    id: p["id"].as_str().unwrap_or_default().to_string(),
                    name,
                    platform,
                    subtitle,
                    pairable: rank == 3,
                },
            ))
        })
        .collect();
    found.sort_by(|a, b| (a.0, a.1, &a.2).cmp(&(b.0, b.1, &b.2)));
    found.into_iter().map(|(.., o)| o).collect()
}

fn other_row(hub: &Hub, o: &Other) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(&o.name)
        .subtitle(&o.subtitle)
        .activatable(true)
        .build();
    row.add_prefix(&theme::well(theme::os_icon(&o.platform), ""));
    if o.pairable {
        let pair = gtk::Button::builder()
            .label("Pair")
            .valign(gtk::Align::Center)
            .css_classes(["pill"])
            .tooltip_text(format!("Pair with {}", o.name))
            .build();
        let (hub, id, name) = (hub.clone(), o.id.clone(), o.name.clone());
        pair.connect_clicked(move |_| pairing::request(&hub, &id, &name));
        row.add_suffix(&pair);
    } else {
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    }
    let id = o.id.clone();
    row.connect_activated(move |_| device::open(&id));
    row
}

/// "Your devices": a card for each paired device connected now, and the
/// rest in a short list under it. Before anything is paired, how to start.
fn devices(hub: &Hub) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 14);
    let welcome = welcome(hub);
    column.append(&welcome);

    let (cards_head, _) = section_header("Your devices", None);
    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(3)
        .column_spacing(12)
        .row_spacing(12)
        .valign(gtk::Align::Start)
        .build();
    column.append(&cards_head);
    column.append(&flow);

    let (others_head, others_title) = section_header("Other devices", None);
    let others_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let hint = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    hint.append(&gtk::Spinner::builder().spinning(true).build());
    let hint_text = label(
        "Looking for devices on your network. Open Link All on the other device to see it here.",
        &["muted"],
    );
    hint_text.set_wrap(true);
    hint.append(&hint_text);
    column.append(&others_head);
    column.append(&others_list);
    column.append(&hint);

    let cards: Rc<RefCell<HashMap<String, Card>>> = Rc::default();
    {
        let cards = cards.clone();
        now_playing::watch(move || {
            for (id, card) in cards.borrow().iter() {
                card.mini.update(now_playing::playing_on(id).as_ref());
            }
        });
    }
    let order = RefCell::new(Vec::<String>::new());
    let shown_others = RefCell::new(String::new());
    let hub2 = hub.clone();
    hub.on_status(move |hub| {
        let status = hub.status.borrow().clone();
        let peers = hub.peers();
        // A fixed order (name, then id), so a card never jumps when something syncs.
        let mut linked: Vec<&Value> = peers
            .iter()
            .filter(|p| p["trusted"] == true && p["status"] == "connected")
            .collect();
        linked.sort_by_key(|p| {
            (
                p["friendly_name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_lowercase(),
                p["id"].as_str().unwrap_or_default().to_string(),
            )
        });
        let ids: Vec<String> = linked
            .iter()
            .map(|p| p["id"].as_str().unwrap_or_default().to_string())
            .collect();
        {
            let mut cards = cards.borrow_mut();
            cards.retain(|id, _| ids.contains(id));
            for (p, id) in linked.iter().zip(&ids) {
                let card = cards.entry(id.clone()).or_insert_with(|| {
                    let c = card(&hub2, id);
                    c.mini.update(now_playing::playing_on(id).as_ref());
                    c
                });
                update_card(card, p, &status);
            }
            if *order.borrow() != ids {
                flow.remove_all();
                for id in &ids {
                    flow.append(&cards[id].child);
                }
                *order.borrow_mut() = ids.clone();
            }
        }
        cards_head.set_visible(!ids.is_empty());
        flow.set_visible(!ids.is_empty());

        let paired = peers.iter().any(|p| p["trusted"] == true);
        welcome.set_visible(!paired);
        let rows = others(&peers, &status);
        let sig: String = rows
            .iter()
            .map(|o| {
                format!(
                    "{}|{}|{}|{}|{};",
                    o.id, o.name, o.platform, o.subtitle, o.pairable
                )
            })
            .collect();
        if *shown_others.borrow() != sig {
            *shown_others.borrow_mut() = sig;
            others_list.remove_all();
            for o in &rows {
                others_list.append(&other_row(&hub2, o));
            }
        }
        others_title.set_label(if !ids.is_empty() {
            "Other devices"
        } else if paired {
            "Your devices"
        } else {
            "Nearby"
        });
        others_list.set_visible(!rows.is_empty());
        // Until something is paired, say what Home is waiting for.
        let searching = !paired && !rows.iter().any(|o| o.pairable);
        hint.set_visible(searching);
        others_head.set_visible(!rows.is_empty() || searching);
    });
    column
}

/// What a history entry is, in a line. Passwords, codes and keys are named, not shown.
fn clip_title(e: &Value) -> String {
    if let Some(kind) = e["sensitive_kind"].as_str() {
        return match kind {
            "otp" => "A one-time code",
            "payment_card" => "A card number",
            "api_token" => "A key or token",
            _ => "Something private",
        }
        .into();
    }
    match e["payload"]["type"].as_str() {
        Some("Image") => "An image".into(),
        Some("File") => e["payload"]["name"].as_str().unwrap_or("A file").into(),
        _ => match history_text(e) {
            Some(text) => text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(140)
                .collect(),
            None => e["payload"]["summary"]
                .as_str()
                .unwrap_or("Clipboard item")
                .into(),
        },
    }
}

/// The newest clip from any device, to copy here or send again, and any
/// clips waiting to be pasted while automatic pasting is off.
fn latest_clip(hub: &Hub, clipboard: &ClipboardTx) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
    column.set_visible(false);
    let (head, _) = section_header(
        "Clipboard",
        Some(("See all", Box::new(|| open_page("clipboard")))),
    );
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    column.append(&head);
    column.append(&list);

    let waiting = adw::ActionRow::builder()
        .activatable(true)
        .subtitle("Automatic pasting is off. Paste them from Clipboard.")
        .visible(false)
        .build();
    waiting.add_prefix(&small_well("edit-paste-symbolic", "amber"));
    waiting.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    waiting.connect_activated(|_| open_page("clipboard"));
    list.append(&waiting);

    let latest = adw::ActionRow::builder()
        .use_markup(false)
        .visible(false)
        .build();
    latest.add_prefix(&small_well("edit-paste-symbolic", "red"));
    let copy = icon_button("edit-copy-symbolic", "Copy");
    let again = icon_button("document-send-symbolic", "Send to your devices again");
    latest.add_suffix(&copy);
    latest.add_suffix(&again);
    list.append(&latest);

    let entry: Rc<RefCell<Option<Value>>> = Rc::default();
    {
        let (clipboard, entry) = (clipboard.clone(), entry.clone());
        copy.connect_clicked(move |_| {
            if let Some(text) = entry.borrow().as_ref().and_then(history_text) {
                let _ = clipboard.send(Incoming::Text(text));
                toast("Copied");
            }
        });
    }
    {
        let (hub, entry) = (hub.clone(), entry.clone());
        again.connect_clicked(move |_| {
            if let Some(id) = entry.borrow().as_ref().and_then(|e| e["id"].as_u64()) {
                act(
                    json!({ "cmd": "history_repush", "id": id, "target": null }),
                    Some("Sent"),
                    &hub,
                );
            }
        });
    }

    let show = {
        let (column, waiting, latest) = (column.clone(), waiting.clone(), latest.clone());
        Rc::new(move || column.set_visible(waiting.is_visible() || latest.is_visible()))
    };
    let reload: Rc<dyn Fn()> = {
        let (latest, copy, entry, show) =
            (latest.clone(), copy.clone(), entry.clone(), show.clone());
        Rc::new(move || {
            let (latest, copy, entry, show) =
                (latest.clone(), copy.clone(), entry.clone(), show.clone());
            glib::spawn_future_local(async move {
                let newest = ipc::call(json!({ "cmd": "history", "last": 1 }))
                    .await
                    .ok()
                    .and_then(|v| v.as_array().and_then(|a| a.first().cloned()));
                if let Some(e) = &newest {
                    let from = e["source_device"].as_str().unwrap_or("this computer");
                    let when = ago(e["timestamp"].as_u64().unwrap_or(0));
                    latest.set_title(&clip_title(e));
                    latest.set_subtitle(&if e["sensitive_kind"].is_string() {
                        format!("Hidden here · from {from} · {when}")
                    } else {
                        format!("From {from} · {when}")
                    });
                    copy.set_sensitive(history_text(e).is_some());
                }
                latest.set_visible(newest.is_some());
                *entry.borrow_mut() = newest;
                show();
            });
        })
    };
    {
        let reload = reload.clone();
        hub.on_activity(move |_, e| {
            if e["kind"]
                .as_str()
                .is_some_and(|k| k.starts_with("clipboard") || k == "remote_clipboard_available")
            {
                reload();
            }
        });
    }
    {
        let reload = reload.clone();
        list.connect_map(move |_| reload());
    }
    let pending = Cell::new(u64::MAX);
    hub.on_status(move |hub| {
        let n = hub.status.borrow()["pending_clipboard_count"]
            .as_u64()
            .unwrap_or(0);
        if pending.replace(n) == n {
            return;
        }
        waiting.set_title(&match n {
            1 => "1 clip waiting to paste".to_string(),
            n => format!("{n} clips waiting to paste"),
        });
        waiting.set_visible(n > 0);
        show();
    });
    column
}

/// Transfers moving now, with their controls; hidden while nothing moves.
fn moving(hub: &Hub) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
    column.set_visible(false);
    let (head, _) = section_header(
        "Transferring",
        Some(("See all", Box::new(|| open_page("files")))),
    );
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    column.append(&head);
    column.append(&list);
    let c = column.clone();
    transfers::follow_active(hub, &list, move |n| c.set_visible(n > 0));
    column
}

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
    row.add_prefix(&small_well(icon, tint));
    row.add_suffix(&label(
        &ago(e["timestamp_ms"].as_u64().unwrap_or(0) / 1000),
        &["caption", "muted"],
    ));
    row
}

/// The last few things that happened; Activity has everything.
fn recent(hub: &Hub) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let (head, _) = section_header(
        "Recent",
        Some(("See all", Box::new(|| open_page("activity")))),
    );
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    column.append(&head);
    column.append(&list);
    {
        let list = list.clone();
        glib::spawn_future_local(async move {
            let entries = ipc::call(json!({ "cmd": "activity_recent", "limit": 120 }))
                .await
                .ok()
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
            list.remove_all();
            let mut shown = 0;
            for e in entries
                .iter()
                .filter(|e| is_recent_worthy(e))
                .take(RECENT as usize)
            {
                list.append(&recent_row(e));
                shown += 1;
            }
            if shown == 0 {
                let row = placeholder_row(
                    "Copies, files and notifications from your devices show up here",
                );
                row.add_css_class("dim");
                list.append(&row);
            }
        });
    }
    hub.on_activity(move |_, e| {
        if !is_recent_worthy(e) {
            return;
        }
        if let Some(first) = list.row_at_index(0) {
            if first.has_css_class("dim") {
                list.remove(&first);
            }
        }
        list.prepend(&recent_row(e));
        while let Some(r) = list.row_at_index(RECENT) {
            list.remove(&r);
        }
    });
    column
}
