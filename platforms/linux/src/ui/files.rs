//! Browse Files: the files a connected device shares, with previews, filters
//! and search. Clicking a file downloads it to this computer.

use super::{human_bytes, open_page, toast};
use crate::{hub::Hub, ipc};
use adw::prelude::*;
use base64::Engine as _;
use gtk::{gdk, glib};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

const PAGE: u32 = 120;
const FILTERS: [(&str, &str, Option<&str>, Option<&str>); 9] = [
    ("All", "view-grid-symbolic", None, None),
    ("Photos", "image-x-generic-symbolic", Some("Images"), None),
    ("Videos", "video-x-generic-symbolic", Some("Videos"), None),
    ("Music", "audio-x-generic-symbolic", Some("Audio"), None),
    (
        "Documents",
        "x-office-document-symbolic",
        Some("Documents"),
        None,
    ),
    (
        "Archives",
        "package-x-generic-symbolic",
        Some("Archives"),
        None,
    ),
    (
        "Apps",
        "application-x-executable-symbolic",
        Some("Apks"),
        None,
    ),
    ("Camera", "camera-photo-symbolic", None, Some("Camera")),
    (
        "Downloads",
        "folder-download-symbolic",
        None,
        Some("Downloads"),
    ),
];

struct State {
    hub: Hub,
    device_ids: RefCell<Vec<String>>,
    devices: gtk::DropDown,
    filter: Cell<usize>,
    search: gtk::SearchEntry,
    grid: gtk::FlowBox,
    stack: gtk::Stack,
    empty: adw::StatusPage,
    more: gtk::Button,
    offset: Cell<u32>,
    generation: Cell<u32>,
    wanted: RefCell<Option<String>>,
}

thread_local! {
    static STATE: RefCell<Option<Rc<State>>> = const { RefCell::new(None) };
    /// Thumbnails being fetched now; a few at a time is all a phone should serve.
    static IN_FLIGHT: Cell<usize> = const { Cell::new(0) };
}
const MAX_THUMBNAILS: usize = 4;

/// Opens Browse Files on `device`.
pub fn browse(device: &str) {
    STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            *state.wanted.borrow_mut() = Some(device.to_string());
            sync_devices(state);
        }
    });
    open_page("files");
}

pub fn page(hub: &Hub) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);

    let top = gtk::Box::builder()
        .spacing(8)
        .margin_top(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    let devices = gtk::DropDown::from_strings(&[]);
    devices.set_tooltip_text(Some("Device"));
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search files")
        .hexpand(true)
        .build();
    let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh.set_tooltip_text(Some("Refresh"));
    top.append(&devices);
    top.append(&search);
    top.append(&refresh);
    root.append(&top);

    let chips = gtk::Box::builder()
        .spacing(6)
        .margin_top(10)
        .margin_bottom(6)
        .margin_start(12)
        .margin_end(12)
        .build();
    let chip_scroll = gtk::ScrolledWindow::builder()
        .vscrollbar_policy(gtk::PolicyType::Never)
        .child(&chips)
        .build();
    root.append(&chip_scroll);

    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(2)
        .max_children_per_line(10)
        .column_spacing(6)
        .row_spacing(6)
        .valign(gtk::Align::Start)
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .build();
    let more = gtk::Button::builder()
        .label("Load More")
        .halign(gtk::Align::Center)
        .margin_top(12)
        .margin_bottom(18)
        .css_classes(["pill"])
        .visible(false)
        .build();
    let grid_column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    grid_column.append(&grid);
    grid_column.append(&more);
    let grid_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&grid_column)
        .build();

    let empty = adw::StatusPage::builder()
        .icon_name("folder-symbolic")
        .title("No Device Connected")
        .build();
    let spinner = gtk::Spinner::builder()
        .spinning(true)
        .width_request(32)
        .height_request(32)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    let stack = gtk::Stack::builder()
        .vexpand(true)
        .hhomogeneous(false)
        .build();
    stack.add_named(&grid_scroll, Some("grid"));
    stack.add_named(&empty, Some("empty"));
    stack.add_named(&spinner, Some("loading"));
    stack.set_visible_child_name("empty");
    root.append(&stack);

    let state = Rc::new(State {
        hub: hub.clone(),
        device_ids: RefCell::default(),
        devices: devices.clone(),
        filter: Cell::new(0),
        search: search.clone(),
        grid,
        stack,
        empty,
        more: more.clone(),
        offset: Cell::new(0),
        generation: Cell::new(0),
        wanted: RefCell::default(),
    });

    let mut first: Option<gtk::ToggleButton> = None;
    for (i, (label, icon, _, _)) in FILTERS.iter().enumerate() {
        let content = adw::ButtonContent::builder()
            .label(*label)
            .icon_name(*icon)
            .build();
        let chip = gtk::ToggleButton::builder()
            .child(&content)
            .css_classes(["chip", "flat"])
            .active(i == 0)
            .build();
        if let Some(f) = &first {
            chip.set_group(Some(f));
        } else {
            first = Some(chip.clone());
        }
        let state = state.clone();
        chip.connect_toggled(move |c| {
            if c.is_active() {
                state.filter.set(i);
                load(&state, true);
            }
        });
        chips.append(&chip);
    }

    {
        let state = state.clone();
        devices.connect_selected_notify(move |_| load(&state, true));
    }
    {
        // Wait for a pause in typing before asking the device.
        let state = state.clone();
        let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
        search.connect_search_changed(move |_| {
            if let Some(id) = pending.borrow_mut().take() {
                id.remove();
            }
            let (state, pending2) = (state.clone(), pending.clone());
            *pending.borrow_mut() = Some(glib::timeout_add_local_once(
                Duration::from_millis(350),
                move || {
                    pending2.borrow_mut().take();
                    load(&state, true);
                },
            ));
        });
    }
    {
        let state = state.clone();
        refresh.connect_clicked(move |_| load(&state, true));
    }
    {
        let state = state.clone();
        more.connect_clicked(move |_| load(&state, false));
    }
    {
        let state = state.clone();
        hub.on_status(move |_| sync_devices(&state));
    }
    STATE.with(|s| *s.borrow_mut() = Some(state));
    root
}

/// Keeps the device list matching what's connected, and keeps the choice.
fn sync_devices(state: &Rc<State>) {
    let connected = state.hub.connected();
    let ids: Vec<String> = connected
        .iter()
        .filter_map(|p| p["id"].as_str().map(str::to_string))
        .collect();
    let wanted = state.wanted.borrow_mut().take();
    if ids != *state.device_ids.borrow() {
        let current = state
            .device_ids
            .borrow()
            .get(state.devices.selected() as usize)
            .cloned();
        let names: Vec<&str> = connected
            .iter()
            .map(|p| p["friendly_name"].as_str().unwrap_or("Device"))
            .collect();
        *state.device_ids.borrow_mut() = ids.clone();
        state.devices.set_model(Some(&gtk::StringList::new(&names)));
        let keep = wanted
            .clone()
            .or(current)
            .and_then(|id| ids.iter().position(|i| *i == id))
            .unwrap_or(0);
        state.devices.set_selected(keep as u32);
        if ids.is_empty() {
            show_message(
                state,
                "folder-symbolic",
                "No Device Connected",
                "Connect a paired device to browse its files.",
            );
        } else {
            load(state, true);
        }
    } else if let Some(id) = wanted {
        if let Some(pos) = ids.iter().position(|i| *i == id) {
            if pos as u32 != state.devices.selected() {
                state.devices.set_selected(pos as u32);
            }
        }
    }
}

fn show_message(state: &State, icon: &str, title: &str, body: &str) {
    state.empty.set_icon_name(Some(icon));
    state.empty.set_title(title);
    state
        .empty
        .set_description(Some(&glib::markup_escape_text(body)));
    state.stack.set_visible_child_name("empty");
    state.more.set_visible(false);
}

fn load(state: &Rc<State>, fresh: bool) {
    let Some(device) = state
        .device_ids
        .borrow()
        .get(state.devices.selected() as usize)
        .cloned()
    else {
        return;
    };
    let (_, _, category, source) = FILTERS[state.filter.get()];
    let query = state.search.text().trim().to_string();
    if fresh {
        state.offset.set(0);
        state.stack.set_visible_child_name("loading");
    }
    let generation = state.generation.get() + 1;
    state.generation.set(generation);
    let offset = state.offset.get();
    let req = json!({
        "cmd": "remote_files_query",
        "target_device": device,
        "summary_only": false,
        "category": category,
        "source": source,
        "search_query": if query.is_empty() { Value::Null } else { query.into() },
        "offset": offset,
        "limit": PAGE,
        "timeout_secs": 10,
    });
    let state = state.clone();
    glib::spawn_future_local(async move {
        let result = ipc::call_with(req, Duration::from_secs(13)).await;
        if state.generation.get() != generation {
            return;
        }
        match result {
            Err(error) => {
                let msg = error.to_string();
                let msg = if msg.contains("timed out") {
                    "The device didn't answer. Check it's awake and on the same network."
                        .to_string()
                } else {
                    msg
                };
                show_message(
                    &state,
                    "dialog-warning-symbolic",
                    "Can't Browse This Device",
                    &msg,
                );
            }
            Ok(res) => {
                if fresh {
                    state.grid.remove_all();
                }
                let files = res["files"].as_array().cloned().unwrap_or_default();
                for f in &files {
                    state.grid.append(&tile(&state, &device, f, generation));
                }
                let shown = offset + files.len() as u32;
                state.offset.set(shown);
                let total = res["total_matching"].as_u64().unwrap_or(shown as u64) as u32;
                state.more.set_visible(shown < total);
                if shown == 0 {
                    show_message(
                        &state,
                        "folder-symbolic",
                        "No Files Here",
                        "Try another filter, or a different search.",
                    );
                } else {
                    state.stack.set_visible_child_name("grid");
                }
            }
        }
    });
}

fn icon_for(mime: &str) -> &'static str {
    match mime.split('/').next().unwrap_or_default() {
        "image" => "image-x-generic",
        "video" => "video-x-generic",
        "audio" => "audio-x-generic",
        "text" => "text-x-generic",
        _ if mime.contains("pdf") => "x-office-document",
        _ if mime.contains("zip") || mime.contains("compressed") || mime.contains("tar") => {
            "package-x-generic"
        }
        _ if mime.contains("android") => "application-x-executable",
        _ => "text-x-generic",
    }
}

fn tile(state: &Rc<State>, device: &str, f: &Value, generation: u32) -> gtk::FlowBoxChild {
    let name = f["display_name"].as_str().unwrap_or("File").to_string();
    let mime = f["mime_type"].as_str().unwrap_or_default().to_string();
    let file_id = f["file_id"].as_u64().unwrap_or(0);

    let picture = gtk::Image::builder()
        .icon_name(icon_for(&mime))
        .pixel_size(64)
        .width_request(128)
        .height_request(112)
        .build();
    let label = gtk::Label::builder()
        .label(&name)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .width_chars(14)
        .max_width_chars(14)
        .justify(gtk::Justification::Center)
        .css_classes(["file-name"])
        .build();
    let size = gtk::Label::builder()
        .label(human_bytes(f["size_bytes"].as_u64().unwrap_or(0)))
        .css_classes(["caption", "dim-label"])
        .build();
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(6)
        .margin_end(6)
        .build();
    column.append(&picture);
    column.append(&label);
    column.append(&size);
    let button = gtk::Button::builder()
        .child(&column)
        .css_classes(["flat"])
        .tooltip_text(format!("Download {name}"))
        .build();
    {
        let (hub, device, name) = (state.hub.clone(), device.to_string(), name.clone());
        button.connect_clicked(move |_| {
            super::act(json!({ "cmd": "remote_file_pull_request", "target_device": device, "file_id": file_id }), None, &hub);
            toast(&format!("Downloading {name} to Downloads"));
        });
    }

    if mime.starts_with("image/") || mime.starts_with("video/") {
        let (state, device, picture) = (state.clone(), device.to_string(), picture.clone());
        glib::spawn_future_local(async move {
            while IN_FLIGHT.with(Cell::get) >= MAX_THUMBNAILS {
                glib::timeout_future(Duration::from_millis(60)).await;
                if state.generation.get() != generation {
                    return;
                }
            }
            IN_FLIGHT.with(|n| n.set(n.get() + 1));
            let req = json!({ "cmd": "remote_thumbnail_request", "target_device": device, "file_id": file_id, "size_px": 256 });
            let result = ipc::call_with(req, Duration::from_secs(13)).await;
            IN_FLIGHT.with(|n| n.set(n.get() - 1));
            let Ok(res) = result else { return };
            if state.generation.get() != generation {
                return;
            }
            let Some(bytes) = res["data_base64"]
                .as_str()
                .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
            else {
                return;
            };
            if bytes.is_empty() {
                return;
            }
            if let Ok(texture) = gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)) {
                picture.set_paintable(Some(&texture));
                picture.set_pixel_size(112);
            }
        });
    }

    // Right-click (or long-press) for more.
    let menu = gtk::gio::Menu::new();
    menu.append(Some("Download"), Some("file.download"));
    menu.append(Some("Rename…"), Some("file.rename"));
    menu.append(Some("Delete…"), Some("file.delete"));
    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    popover.set_parent(&button);
    popover.set_has_arrow(false);
    let actions = gtk::gio::SimpleActionGroup::new();
    {
        let b = button.clone();
        let a = gtk::gio::SimpleAction::new("download", None);
        a.connect_activate(move |_, _| b.emit_clicked());
        actions.add_action(&a);
    }
    {
        let (state, device, name) = (state.clone(), device.to_string(), name.clone());
        let a = gtk::gio::SimpleAction::new("rename", None);
        a.connect_activate(move |_, _| rename_file(&state, &device, file_id, &name));
        actions.add_action(&a);
    }
    {
        let (state, device, name) = (state.clone(), device.to_string(), name.clone());
        let a = gtk::gio::SimpleAction::new("delete", None);
        a.connect_activate(move |_, _| delete_file(&state, &device, file_id, &name));
        actions.add_action(&a);
    }
    button.insert_action_group("file", Some(&actions));
    let click = gtk::GestureClick::builder().button(3).build();
    {
        let popover = popover.clone();
        click.connect_pressed(move |_, _, x, y| {
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
    }
    button.add_controller(click);
    let press = gtk::GestureLongPress::new();
    press.connect_pressed(move |_, x, y| {
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.popup();
    });
    button.add_controller(press);

    gtk::FlowBoxChild::builder().child(&button).build()
}

fn file_action(
    state: &Rc<State>,
    device: &str,
    file_id: u64,
    action: &str,
    new_name: Option<String>,
    done: String,
) {
    let req = json!({ "cmd": "remote_file_action_request", "target_device": device, "file_id": file_id, "action": action, "new_name": new_name });
    let state = state.clone();
    glib::spawn_future_local(async move {
        match ipc::call(req).await {
            Ok(_) => {
                toast(&done);
                // Give the device a moment, then show the folder as it is now.
                glib::timeout_future(Duration::from_millis(600)).await;
                load(&state, true);
            }
            Err(e) => toast(&e.to_string()),
        }
    });
}

fn rename_file(state: &Rc<State>, device: &str, file_id: u64, name: &str) {
    let entry = adw::EntryRow::builder().title("Name").text(name).build();
    let list = gtk::ListBox::builder()
        .css_classes(["boxed-list"])
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.append(&entry);
    let dialog = adw::AlertDialog::builder()
        .heading("Rename File")
        .extra_child(&list)
        .close_response("cancel")
        .default_response("rename")
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("rename", "Rename")]);
    dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
    let (state, device) = (state.clone(), device.to_string());
    dialog.connect_response(Some("rename"), move |_, _| {
        let new_name = entry.text().trim().to_string();
        if !new_name.is_empty() {
            file_action(
                &state,
                &device,
                file_id,
                "rename",
                Some(new_name.clone()),
                format!("Renamed to {new_name}"),
            );
        }
    });
    dialog.present(super::window().as_ref());
}

fn delete_file(state: &Rc<State>, device: &str, file_id: u64, name: &str) {
    let dialog = adw::AlertDialog::builder()
        .heading(format!("Delete {name}?"))
        .body("It is deleted from the other device. This can't be undone.")
        .close_response("cancel")
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    let (state, device, name) = (state.clone(), device.to_string(), name.to_string());
    dialog.connect_response(Some("delete"), move |_, _| {
        file_action(
            &state,
            &device,
            file_id,
            "delete",
            None,
            format!("Deleted {name}"),
        )
    });
    dialog.present(super::window().as_ref());
}
