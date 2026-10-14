//! Clipboard history: everything copied on any device, searchable, with copy,
//! send again, pin and delete.

use super::{act, ago, icon_button, page_column, placeholder_row, toast, window};
use crate::agent::{history_text, ClipboardTx, Incoming};
use crate::{hub::Hub, ipc};
use adw::prelude::*;
use gtk::glib;
use serde_json::{json, Value};
use std::cell::Cell;
use std::rc::Rc;

pub fn page(hub: &Hub, clipboard: &ClipboardTx) -> gtk::ScrolledWindow {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search clipboard history")
        .hexpand(true)
        .build();
    let clear = gtk::Button::builder()
        .label("Clear All")
        .css_classes(["destructive-action"])
        .build();
    bar.append(&search);
    bar.append(&clear);
    column.append(&bar);
    // Clips from other devices that wait for "Paste Here" (when automatic pasting is off).
    let pending_title = gtk::Label::builder()
        .label("Waiting to Paste")
        .xalign(0.0)
        .css_classes(["section-title"])
        .visible(false)
        .build();
    let pending = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .visible(false)
        .build();
    column.append(&pending_title);
    column.append(&pending);
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    column.append(&list);
    {
        let (hub2, clipboard) = (hub.clone(), clipboard.clone());
        let signature = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        hub.on_status(move |hub| {
            if hub.status.borrow()["pending_clipboard_count"]
                .as_u64()
                .unwrap_or(0)
                == 0
                && signature.borrow().is_empty()
            {
                return;
            }
            let (pending, pending_title, hub, clipboard, signature) = (
                pending.clone(),
                pending_title.clone(),
                hub2.clone(),
                clipboard.clone(),
                signature.clone(),
            );
            glib::spawn_future_local(async move {
                let items = ipc::call(json!({ "cmd": "pending_remote_clipboards" }))
                    .await
                    .ok()
                    .and_then(|v| v.as_array().cloned())
                    .unwrap_or_default();
                let sig: String = items.iter().map(|e| e["id"].to_string()).collect();
                if *signature.borrow() == sig {
                    return;
                }
                *signature.borrow_mut() = sig;
                pending.remove_all();
                for e in items.iter().take(10) {
                    pending.append(&pending_row(&hub, e, &clipboard));
                }
                pending.set_visible(!items.is_empty());
                pending_title.set_visible(!items.is_empty());
            });
        });
    }

    // Results from an older search can arrive after a newer one; keep only the latest.
    let generation = Rc::new(Cell::new(0u32));
    let reload: Rc<dyn Fn()> = {
        let (hub, list, search, clipboard, generation) = (
            hub.clone(),
            list.clone(),
            search.clone(),
            clipboard.clone(),
            generation.clone(),
        );
        Rc::new(move || {
            let query = search.text().trim().to_string();
            let req = if query.is_empty() {
                json!({ "cmd": "history", "last": 200 })
            } else {
                json!({ "cmd": "history_search", "query": query, "limit": 200 })
            };
            let mine = generation.get() + 1;
            generation.set(mine);
            let (hub, list, clipboard, generation) = (
                hub.clone(),
                list.clone(),
                clipboard.clone(),
                generation.clone(),
            );
            glib::spawn_future_local(async move {
                let entries = ipc::call(req)
                    .await
                    .ok()
                    .and_then(|v| v.as_array().cloned())
                    .unwrap_or_default();
                if generation.get() != mine {
                    return;
                }
                list.remove_all();
                let mut entries = entries;
                // Pinned first, then newest first.
                entries.sort_by_key(|e| {
                    (
                        e["pinned"] != true,
                        std::cmp::Reverse(e["timestamp"].as_u64().unwrap_or(0)),
                    )
                });
                for e in &entries {
                    list.append(&row(&hub, e, &clipboard));
                }
                if entries.is_empty() {
                    list.append(&placeholder_row(
                        "Nothing copied yet. Copy something on any paired device.",
                    ));
                }
            });
        })
    };

    {
        let reload = reload.clone();
        search.connect_search_changed(move |_| reload());
    }
    {
        let (hub, reload) = (hub.clone(), reload.clone());
        clear.connect_clicked(move |_| {
            let dialog = adw::AlertDialog::builder()
                .heading("Clear Clipboard History?")
                .body("Pinned items are removed too. Other devices keep their own history.")
                .close_response("cancel")
                .build();
            dialog.add_responses(&[("cancel", "Cancel"), ("clear", "Clear")]);
            dialog.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
            let (hub, reload) = (hub.clone(), reload.clone());
            dialog.connect_response(Some("clear"), move |_, _| {
                act(json!({ "cmd": "history_clear" }), None, &hub);
                let reload = reload.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                    reload()
                });
            });
            dialog.present(window().as_ref());
        });
    }
    // New clips appear as they arrive.
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

    page_column(&column)
}

fn row(hub: &Hub, e: &Value, clipboard: &ClipboardTx) -> adw::ActionRow {
    let id = e["id"].as_u64().unwrap_or(0);
    let text = history_text(e);
    let title = match &text {
        Some(t) => t
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(200)
            .collect::<String>(),
        None => format!(
            "{} item",
            e["payload"]["type"].as_str().unwrap_or("Clipboard")
        ),
    };
    let from = e["source_device"].as_str().unwrap_or("This computer");
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(&title)
        .title_lines(2)
        .subtitle(format!(
            "{from} · {}",
            ago(e["timestamp"].as_u64().unwrap_or(0))
        ))
        .build();
    if e["pinned"] == true {
        row.add_prefix(&gtk::Image::from_icon_name("view-pin-symbolic"));
    }

    let copy = icon_button("edit-copy-symbolic", "Copy");
    {
        let (clipboard, text) = (clipboard.clone(), text.clone());
        copy.connect_clicked(move |_| {
            if let Some(t) = &text {
                let _ = clipboard.send(Incoming::Text(t.clone()));
                toast("Copied");
            }
        });
    }
    copy.set_sensitive(text.is_some());
    row.add_suffix(&copy);

    let resend = icon_button("document-send-symbolic", "Send to Devices Again");
    {
        let hub = hub.clone();
        resend.connect_clicked(move |_| {
            act(
                json!({ "cmd": "history_repush", "id": id, "target": null }),
                Some("Sent"),
                &hub,
            )
        });
    }
    row.add_suffix(&resend);

    let pin = icon_button(
        "view-pin-symbolic",
        if e["pinned"] == true { "Unpin" } else { "Pin" },
    );
    {
        let (hub, pinned) = (hub.clone(), e["pinned"] == true);
        pin.connect_clicked(move |_| {
            act(
                json!({ "cmd": "history_pin", "id": id, "pinned": !pinned }),
                None,
                &hub,
            )
        });
    }
    row.add_suffix(&pin);

    let delete = icon_button("user-trash-symbolic", "Delete");
    {
        let (hub, row2) = (hub.clone(), row.clone());
        delete.connect_clicked(move |_| {
            act(json!({ "cmd": "history_delete", "id": id }), None, &hub);
            if let Some(list) = row2.parent().and_downcast::<gtk::ListBox>() {
                list.remove(&row2);
            }
        });
    }
    row.add_suffix(&delete);
    row
}

fn pending_row(hub: &Hub, e: &Value, clipboard: &ClipboardTx) -> adw::ActionRow {
    let preview: String = e["text_preview"]
        .as_str()
        .unwrap_or("Image")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect();
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(preview)
        .title_lines(2)
        .subtitle(format!(
            "From {} · {}",
            e["device_name"].as_str().unwrap_or("a device"),
            ago(e["timestamp_ms"].as_u64().unwrap_or(0) / 1000)
        ))
        .build();
    let w = crate::theme::well("edit-paste-symbolic", "red");
    w.add_css_class("small");
    row.add_prefix(&w);
    let paste = gtk::Button::builder()
        .label("Paste Here")
        .valign(gtk::Align::Center)
        .css_classes(["pill", "suggested-action"])
        .build();
    let (hub, clipboard, id, hash) = (
        hub.clone(),
        clipboard.clone(),
        e["id"].as_u64().unwrap_or(0),
        e["content_hash"].as_str().unwrap_or_default().to_string(),
    );
    paste.connect_clicked(move |_| {
        let (hub, clipboard, hash) = (hub.clone(), clipboard.clone(), hash.clone());
        glib::spawn_future_local(async move {
            if let Ok(clip) = ipc::call(json!({ "cmd": "incoming_clipboard", "id": id })).await {
                if let Some(text) = clip["text"].as_str() {
                    let _ = clipboard.send(Incoming::Text(text.to_string()));
                } else if let Some(png) = clip["data_base64"].as_str().and_then(|b| {
                    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b).ok()
                }) {
                    let _ = clipboard.send(Incoming::Png(png));
                }
            }
            let _ = ipc::call(json!({ "cmd": "apply_clipboard", "content_hash": hash })).await;
            toast("Pasted to this computer's clipboard");
            hub.poke();
        });
    });
    row.add_suffix(&paste);
    row
}
