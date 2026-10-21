//! Scratchpad: notes, links and snippets shared with your own devices. They
//! are kept on each device and sent only to paired ones.

use super::{ago, page_column, placeholder_row, toast, window};
use crate::ipc;
use adw::prelude::*;
use gtk::{gdk, glib};
use serde_json::{json, Value};

/// The first line of a note, shortened, for a row's title.
fn title_of(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let line = line.trim_start_matches(['#', '-', '*', ' ']);
    let mut s: String = line.chars().take(120).collect();
    if line.chars().count() > 120 {
        s.push('…');
    }
    s
}

/// The rest of a note after its first line, for the row's subtitle.
fn body_of(text: &str) -> String {
    let rest: Vec<&str> = text
        .lines()
        .skip_while(|l| l.trim().is_empty())
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .take(3)
        .collect();
    rest.join("\n")
}

/// Runs a Scratchpad request, shows a failure, and refreshes the list.
fn send(req: Value, refresh: impl Fn() + 'static) {
    glib::spawn_future_local(async move {
        if let Err(e) = ipc::call(req).await {
            toast(&e.to_string());
        }
        refresh();
    });
}

fn edit_dialog(note: Value, refresh: impl Fn() + Clone + 'static) {
    let view = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .monospace(true)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .build();
    view.buffer()
        .set_text(note["text"].as_str().unwrap_or_default());
    let scroll = gtk::ScrolledWindow::builder()
        .min_content_height(220)
        .min_content_width(380)
        .css_classes(["card"])
        .child(&view)
        .build();
    let dialog = adw::AlertDialog::builder()
        .heading("Edit Note")
        .body("Markdown works: # heading, **bold**, `code`, - list")
        .extra_child(&scroll)
        .close_response("cancel")
        .default_response("save")
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("save", "Save")]);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.connect_response(Some("save"), move |_, _| {
        let buffer = view.buffer();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .trim()
            .to_string();
        if text.is_empty() {
            return;
        }
        send(
            json!({
                "cmd": "scratchpad_save",
                "id": note["id"],
                "text": text,
                "pinned": note["pinned"].as_bool().unwrap_or(false),
            }),
            refresh.clone(),
        );
    });
    dialog.present(window().as_ref());
}

pub fn page() -> gtk::ScrolledWindow {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 14);

    let entry = adw::EntryRow::builder()
        .title("A link, a command, a thought…")
        .show_apply_button(true)
        .build();
    let compose = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    compose.append(&entry);
    column.append(&compose);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    column.append(&list);

    let clear = gtk::Button::builder()
        .label("Clear All")
        .halign(gtk::Align::End)
        .css_classes(["flat"])
        .visible(false)
        .build();
    column.append(&clear);

    let refresh = {
        let list = list.clone();
        let clear = clear.clone();
        std::rc::Rc::new(move || {
            let (list, clear) = (list.clone(), clear.clone());
            glib::spawn_future_local(async move {
                let Ok(notes) = ipc::call(json!({ "cmd": "scratchpad_list" })).await else {
                    return;
                };
                let notes = notes.as_array().cloned().unwrap_or_default();
                while let Some(row) = list.first_child() {
                    list.remove(&row);
                }
                clear.set_visible(
                    notes
                        .iter()
                        .any(|n| !n["pinned"].as_bool().unwrap_or(false)),
                );
                if notes.is_empty() {
                    list.append(&placeholder_row(
                        "Nothing here yet. Add something and it appears on your other devices.",
                    ));
                }
                for note in &notes {
                    list.append(&note_row(note));
                }
            });
        })
    };
    // `note_row` needs a refresh to call after a change; it reads the shared one.
    thread_local! {
        static REFRESH: std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>> = const { std::cell::RefCell::new(None) };
    }
    REFRESH.with(|r| *r.borrow_mut() = Some(refresh.clone()));

    fn after_change() -> impl Fn() + Clone + 'static {
        || {
            REFRESH.with(|r| {
                if let Some(f) = r.borrow().as_ref() {
                    f();
                }
            })
        }
    }

    fn note_row(note: &Value) -> adw::ActionRow {
        let text = note["text"].as_str().unwrap_or_default().to_string();
        let pinned = note["pinned"].as_bool().unwrap_or(false);
        let row = adw::ActionRow::builder()
            .use_markup(false)
            .title(title_of(&text))
            .title_lines(2)
            .subtitle(format!(
                "{}{}",
                {
                    let body = body_of(&text);
                    if body.is_empty() {
                        String::new()
                    } else {
                        format!("{body}\n")
                    }
                },
                ago(note["updated_at_ms"].as_u64().unwrap_or(0) / 1000)
            ))
            .subtitle_lines(5)
            .build();
        if pinned {
            row.add_prefix(&gtk::Image::from_icon_name("view-pin-symbolic"));
        }
        let button = |icon: &str, tip: &str| {
            let b = gtk::Button::from_icon_name(icon);
            b.set_tooltip_text(Some(tip));
            b.set_valign(gtk::Align::Center);
            b.add_css_class("flat");
            b
        };
        let copy = button("edit-copy-symbolic", "Copy");
        {
            let text = text.clone();
            copy.connect_clicked(move |_| {
                if let Some(display) = gdk::Display::default() {
                    display.clipboard().set_text(&text);
                    toast("Copied");
                }
            });
        }
        let pin = button("view-pin-symbolic", if pinned { "Unpin" } else { "Pin" });
        {
            let note = note.clone();
            pin.connect_clicked(move |_| {
                send(
                    json!({
                        "cmd": "scratchpad_save",
                        "id": note["id"],
                        "text": note["text"],
                        "pinned": !pinned,
                    }),
                    after_change(),
                );
            });
        }
        let edit = button("document-edit-symbolic", "Edit");
        {
            let note = note.clone();
            edit.connect_clicked(move |_| edit_dialog(note.clone(), after_change()));
        }
        let delete = button("user-trash-symbolic", "Delete");
        {
            let id = note["id"].clone();
            delete.connect_clicked(move |_| {
                send(
                    json!({ "cmd": "scratchpad_delete", "id": id }),
                    after_change(),
                );
            });
        }
        for b in [&delete, &edit, &pin, &copy] {
            row.add_suffix(b);
        }
        row
    }

    {
        let refresh = refresh.clone();
        entry.connect_apply(move |entry| {
            let text = entry.text().trim().to_string();
            if text.is_empty() {
                return;
            }
            entry.set_text("");
            let refresh = refresh.clone();
            send(
                json!({ "cmd": "scratchpad_save", "text": text, "pinned": false }),
                move || refresh(),
            );
        });
    }
    {
        let refresh = refresh.clone();
        clear.connect_clicked(move |_| {
            let dialog = adw::AlertDialog::builder()
                .heading("Clear All Notes?")
                .body("This deletes every note that isn't pinned, here and on your paired devices.")
                .close_response("cancel")
                .default_response("cancel")
                .build();
            dialog.add_responses(&[("cancel", "Cancel"), ("clear", "Clear All")]);
            dialog.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
            let refresh = refresh.clone();
            dialog.connect_response(Some("clear"), move |_, _| {
                let refresh = refresh.clone();
                send(json!({ "cmd": "scratchpad_clear" }), move || refresh());
            });
            dialog.present(window().as_ref());
        });
    }

    // Notes written on another device show up while the page is open.
    refresh();
    {
        let refresh = refresh.clone();
        glib::timeout_add_seconds_local(3, move || {
            refresh();
            glib::ControlFlow::Continue
        });
    }

    page_column(&column)
}
