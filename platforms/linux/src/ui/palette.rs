//! The command palette (Ctrl+K): one search box for actions, pages, devices
//! and clipboard history, as on the Mac.

use super::{open_page, toast, window};
use crate::agent::{history_text, ClipboardTx, Incoming};
use crate::{hub::Hub, ipc, theme};
use adw::prelude::*;
use gtk::glib;
use serde_json::json;
use std::cell::Cell;
use std::rc::Rc;

type Run = Rc<dyn Fn()>;

struct Item {
    icon: String,
    tint: &'static str,
    title: String,
    sub: String,
    run: Run,
}

fn fixed_items(hub: &Hub) -> Vec<Item> {
    let mut items = Vec::new();
    let mut add = |icon: &str, tint: &'static str, title: &str, sub: &str, run: Run| {
        items.push(Item {
            icon: icon.into(),
            tint,
            title: title.into(),
            sub: sub.into(),
            run,
        });
    };
    {
        let hub = hub.clone();
        add(
            "document-send-symbolic",
            "blue",
            "Send Files…",
            "Action",
            Rc::new(move || super::send::pick_files(&hub, None)),
        );
    }
    {
        let hub = hub.clone();
        add(
            "folder-symbolic",
            "blue",
            "Send a Folder…",
            "Action",
            Rc::new(move || super::send::pick_folder(&hub, None)),
        );
    }
    {
        let hub = hub.clone();
        add(
            "view-grid-symbolic",
            "violet",
            "Pair a Phone…",
            "Action",
            Rc::new(move || super::pairing::show_qr(&hub)),
        );
    }
    {
        let hub = hub.clone();
        let paused = hub.setting("sync_enabled") == false;
        add(
            if paused {
                "media-playback-start-symbolic"
            } else {
                "media-playback-pause-symbolic"
            },
            "amber",
            if paused { "Resume Sync" } else { "Pause Sync" },
            "Action",
            Rc::new(move || {
                super::act(
                    json!({ "cmd": "set_sync_enabled", "enabled": paused }),
                    Some(if paused {
                        "Sync resumed"
                    } else {
                        "Sync paused"
                    }),
                    &hub,
                )
            }),
        );
    }
    for (id, title, icon) in [
        ("devices", "Devices", "computer-symbolic"),
        ("clipboard", "Clipboard History", "edit-paste-symbolic"),
        ("transfers", "Transfers", "folder-download-symbolic"),
        ("files", "Browse Files", "folder-remote-symbolic"),
        ("activity", "Activity", "document-open-recent-symbolic"),
        ("settings", "Settings", "emblem-system-symbolic"),
    ] {
        add(icon, "", title, "Go to", Rc::new(move || open_page(id)));
    }
    for p in hub.peers().into_iter().filter(|p| p["trusted"] == true) {
        let id = p["id"].as_str().unwrap_or_default().to_string();
        let name = p["friendly_name"].as_str().unwrap_or("Device").to_string();
        let connected = p["status"] == "connected";
        let hub = hub.clone();
        add(
            theme::os_icon(p["platform"].as_str().unwrap_or_default()),
            if connected { "blue" } else { "" },
            &name,
            if connected {
                "Device · connected"
            } else {
                "Device"
            },
            Rc::new(move || super::devices::open_device(&hub, &id)),
        );
    }
    items
}

fn row(item: &Item) -> gtk::ListBoxRow {
    let line = gtk::Box::builder()
        .spacing(12)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();
    let w = theme::well(&item.icon, item.tint);
    w.add_css_class("small");
    line.append(&w);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text.set_valign(gtk::Align::Center);
    text.append(
        &gtk::Label::builder()
            .label(&item.title)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(60)
            .css_classes(["heading"])
            .build(),
    );
    text.append(
        &gtk::Label::builder()
            .label(&item.sub)
            .xalign(0.0)
            .css_classes(["caption", "muted"])
            .build(),
    );
    line.append(&text);
    gtk::ListBoxRow::builder().child(&line).build()
}

pub fn open(hub: &Hub, clipboard: &ClipboardTx) {
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search devices, clipboard and actions")
        .hexpand(true)
        .build();
    let list = gtk::ListBox::builder()
        .css_classes(["navigation-sidebar"])
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(360)
        .max_content_height(460)
        .child(&list)
        .build();
    let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
    column.set_margin_top(12);
    column.set_margin_bottom(12);
    column.set_margin_start(12);
    column.set_margin_end(12);
    column.append(&search);
    column.append(&scroll);
    let dialog = adw::Dialog::builder()
        .content_width(560)
        .child(&column)
        .build();

    let items: Rc<std::cell::RefCell<Vec<Item>>> = Rc::default();
    let generation = Rc::new(Cell::new(0u32));
    let refresh: Rc<dyn Fn()> = {
        let (hub, clipboard, list, search, items, generation) = (
            hub.clone(),
            clipboard.clone(),
            list.clone(),
            search.clone(),
            items.clone(),
            generation.clone(),
        );
        Rc::new(move || {
            let query = search.text().trim().to_lowercase();
            let mut found: Vec<Item> = fixed_items(&hub)
                .into_iter()
                .filter(|i| {
                    query.is_empty()
                        || i.title.to_lowercase().contains(&query)
                        || i.sub.to_lowercase().contains(&query)
                })
                .collect();
            let mine = generation.get() + 1;
            generation.set(mine);
            let (hub, clipboard, list, items, generation) = (
                hub.clone(),
                clipboard.clone(),
                list.clone(),
                items.clone(),
                generation.clone(),
            );
            glib::spawn_future_local(async move {
                if !query.is_empty() {
                    let hits =
                        ipc::call(json!({ "cmd": "history_search", "query": query, "limit": 8 }))
                            .await
                            .ok()
                            .and_then(|v| v.as_array().cloned())
                            .unwrap_or_default();
                    for e in hits {
                        let Some(text) = history_text(&e) else {
                            continue;
                        };
                        let clipboard = clipboard.clone();
                        let copy = text.clone();
                        found.push(Item {
                            icon: "edit-paste-symbolic".into(),
                            tint: "red",
                            title: text.split_whitespace().collect::<Vec<_>>().join(" "),
                            sub: format!(
                                "Clipboard · {} · Enter to copy",
                                e["source_device"].as_str().unwrap_or("this computer")
                            ),
                            run: Rc::new(move || {
                                let _ = clipboard.send(Incoming::Text(copy.clone()));
                                toast("Copied");
                            }),
                        });
                    }
                }
                if generation.get() != mine {
                    return;
                }
                let _ = &hub;
                list.remove_all();
                for i in &found {
                    list.append(&row(i));
                }
                list.select_row(list.row_at_index(0).as_ref());
                *items.borrow_mut() = found;
            });
        })
    };
    refresh();
    {
        let refresh = refresh.clone();
        search.connect_search_changed(move |_| refresh());
    }
    let run_selected = {
        let (items, dialog) = (items.clone(), dialog.clone());
        move |index: i32| {
            let run = items.borrow().get(index as usize).map(|i| i.run.clone());
            if let Some(run) = run {
                dialog.close();
                run();
            }
        }
    };
    {
        let run_selected = run_selected.clone();
        list.connect_row_activated(move |_, r| run_selected(r.index()));
    }
    {
        let list2 = list.clone();
        search.connect_activate(move |_| {
            if let Some(r) = list2.selected_row() {
                run_selected(r.index());
            }
        });
    }
    // Up and Down move through results while typing.
    let keys = gtk::EventControllerKey::new();
    {
        let list = list.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            let step = match key {
                gtk::gdk::Key::Down => 1,
                gtk::gdk::Key::Up => -1,
                _ => return glib::Propagation::Proceed,
            };
            let current = list.selected_row().map(|r| r.index()).unwrap_or(-1);
            if let Some(r) = list.row_at_index((current + step).max(0)) {
                list.select_row(Some(&r));
                r.grab_focus();
                list.grab_focus();
            }
            glib::Propagation::Stop
        });
    }
    search.add_controller(keys);
    dialog.present(window().as_ref());
    search.grab_focus();
}
