//! The Link All window: a sidebar of pages, each built from the shared hub.

mod activity;
mod clipboard;
pub mod devices;
mod files;
pub mod pairing;
pub mod palette;
mod scratchpad;
pub use devices::add_by_address;
pub mod send;
mod settings;
mod transfers;

use crate::{agent::ClipboardTx, hub::Hub, ipc};
use adw::prelude::*;
use gtk::{gio, glib};
use serde_json::Value;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

thread_local! {
    static TOASTS: RefCell<Option<adw::ToastOverlay>> = const { RefCell::new(None) };
    static WINDOW: RefCell<Option<adw::ApplicationWindow>> = const { RefCell::new(None) };
    static SIDEBAR: RefCell<Option<gtk::ListBox>> = const { RefCell::new(None) };
}

/// Shows the banner while a newer Link All exists, checking at start and every
/// six hours; the check itself runs at most once a day (see `update`).
fn watch_updates(banner: &adw::Banner) {
    let shown: Rc<RefCell<Option<crate::update::Update>>> = Rc::new(RefCell::new(None));
    {
        let shown = shown.clone();
        banner.connect_button_clicked(move |_| {
            if let Some(update) = shown.borrow().as_ref() {
                let _ = gio::AppInfo::launch_default_for_uri(
                    &update.url,
                    None::<&gio::AppLaunchContext>,
                );
            }
        });
    }
    fn check(banner: &adw::Banner, shown: &Rc<RefCell<Option<crate::update::Update>>>) {
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            crate::update::refresh_if_due();
            let _ = tx.send_blocking(crate::update::known());
        });
        let (banner, shown) = (banner.clone(), shown.clone());
        glib::spawn_future_local(async move {
            if let Ok(found) = rx.recv().await {
                if let Some(update) = &found {
                    banner.set_title(&format!("Link All {} is out", update.version));
                }
                banner.set_revealed(found.is_some());
                *shown.borrow_mut() = found;
            }
        });
    }
    check(banner, &shown);
    let banner = banner.clone();
    glib::timeout_add_seconds_local(6 * 60 * 60, move || {
        check(&banner, &shown);
        glib::ControlFlow::Continue
    });
}

/// Shows a short message at the bottom of the window.
pub fn toast(message: &str) {
    TOASTS.with(|t| {
        if let Some(overlay) = t.borrow().as_ref() {
            overlay.add_toast(
                adw::Toast::builder()
                    .title(glib::markup_escape_text(message))
                    .timeout(3)
                    .build(),
            );
        }
    });
}

pub fn window() -> Option<adw::ApplicationWindow> {
    WINDOW.with(|w| w.borrow().clone())
}

/// Sends a request and reports failure (or `done`, if given) as a toast.
pub fn act(req: Value, done: Option<&'static str>, hub: &Hub) {
    let hub = hub.clone();
    glib::spawn_future_local(async move {
        match ipc::call(req).await {
            Ok(_) => {
                if let Some(done) = done {
                    toast(done);
                }
            }
            Err(error) => toast(&error.to_string()),
        }
        hub.poke();
    });
}

pub fn show_in_folder(path: &Path) {
    // The FileManager1 D-Bus interface selects the file; opening the folder is the fallback.
    let uri = gio::File::for_path(path).uri();
    let shown = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)
        .ok()
        .and_then(|bus| {
            bus.call_sync(
                Some("org.freedesktop.FileManager1"),
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1",
                "ShowItems",
                Some(&(vec![uri.to_string()], "").to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                2000,
                None::<&gio::Cancellable>,
            )
            .ok()
        });
    if shown.is_none() {
        if let Some(dir) = path.parent() {
            let _ = gio::AppInfo::launch_default_for_uri(
                &gio::File::for_path(dir).uri(),
                None::<&gio::AppLaunchContext>,
            );
        }
    }
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// "just now", "5 min ago", "3 h ago", or a date.
pub fn ago(epoch_secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let d = now.saturating_sub(epoch_secs);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", d / 60),
        3600..=86399 => format!("{} h ago", d / 3600),
        _ => glib::DateTime::from_unix_local(epoch_secs as i64)
            .and_then(|t| t.format("%e %b"))
            .map(|s| s.trim().to_string())
            .unwrap_or_default(),
    }
}

pub fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tooltip));
    b.set_valign(gtk::Align::Center);
    b.add_css_class("flat");
    b
}

/// A scrolling, width-limited column for a page's content.
pub fn page_column(content: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let clamp = adw::Clamp::builder()
        .maximum_size(860)
        .child(content)
        .build();
    clamp.set_margin_top(18);
    clamp.set_margin_bottom(24);
    clamp.set_margin_start(12);
    clamp.set_margin_end(12);
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build()
}

/// A titled boxed list, the building block of most pages.
pub fn section(title: &str) -> (gtk::Box, gtk::ListBox) {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
    if !title.is_empty() {
        let label = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["heading"])
            .build();
        column.append(&label);
    }
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    column.append(&list);
    (column, list)
}

pub fn placeholder_row(text: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(text)
        .use_markup(false)
        .sensitive(false)
        .build();
    row.add_css_class("dim-label");
    row
}

struct PageSpec {
    id: &'static str,
    title: &'static str,
    icon: &'static str,
}

const PAGES: [PageSpec; 7] = [
    PageSpec {
        id: "devices",
        title: "Devices",
        icon: "computer-symbolic",
    },
    PageSpec {
        id: "clipboard",
        title: "Clipboard",
        icon: "edit-paste-symbolic",
    },
    PageSpec {
        id: "transfers",
        title: "Transfers",
        icon: "folder-download-symbolic",
    },
    PageSpec {
        id: "files",
        title: "Browse Files",
        icon: "folder-symbolic",
    },
    PageSpec {
        id: "activity",
        title: "Activity",
        icon: "document-open-recent-symbolic",
    },
    PageSpec {
        id: "scratchpad",
        title: "Scratchpad",
        icon: "document-edit-symbolic",
    },
    PageSpec {
        id: "settings",
        title: "Settings",
        icon: "emblem-system-symbolic",
    },
];

fn nav_row(icon: &str, title: &str, name: &str) -> gtk::ListBoxRow {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.append(&gtk::Image::from_icon_name(icon));
    row.append(
        &gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build(),
    );
    gtk::ListBoxRow::builder().child(&row).name(name).build()
}

/// The sidebar's "Your devices" list: paired devices with a live dot; a click opens the device.
fn sidebar_devices(hub: &Hub) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let heading = gtk::Label::builder()
        .label("YOUR DEVICES")
        .xalign(0.0)
        .css_classes(["eyebrow", "sidebar-heading"])
        .build();
    let list = gtk::ListBox::builder()
        .css_classes(["navigation-sidebar"])
        .selection_mode(gtk::SelectionMode::None)
        .build();
    column.append(&heading);
    column.append(&list);
    {
        let hub = hub.clone();
        list.connect_row_activated(move |_, row| {
            devices::open_device(&hub, row.widget_name().as_str())
        });
    }
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
        heading.set_visible(!paired.is_empty());
        for p in &paired {
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            line.append(&crate::theme::dot(p["status"] == "connected"));
            line.append(&gtk::Image::from_icon_name(crate::theme::os_icon(
                p["platform"].as_str().unwrap_or_default(),
            )));
            line.append(
                &gtk::Label::builder()
                    .label(p["friendly_name"].as_str().unwrap_or("Device"))
                    .xalign(0.0)
                    .hexpand(true)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .build(),
            );
            list.append(
                &gtk::ListBoxRow::builder()
                    .child(&line)
                    .name(p["id"].as_str().unwrap_or_default())
                    .build(),
            );
        }
    });
    column
}

/// This computer, pinned to the bottom of the sidebar.
fn this_device(hub: &Hub) -> gtk::Box {
    let card = gtk::Box::builder()
        .spacing(10)
        .css_classes(["this-device"])
        .build();
    card.append(&crate::theme::well("computer-symbolic", "blue"));
    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text.set_valign(gtk::Align::Center);
    let name = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["heading"])
        .build();
    let state = gtk::Label::builder()
        .xalign(0.0)
        .css_classes(["caption", "muted"])
        .build();
    text.append(&name);
    text.append(&state);
    card.append(&text);
    hub.on_status(move |hub| {
        let status = hub.status.borrow();
        name.set_label(status["device_name"].as_str().unwrap_or("This computer"));
        state.set_label(&if !hub.online() {
            "Not running".to_string()
        } else if hub.setting("sync_enabled") == false {
            "Sync paused".to_string()
        } else {
            match hub.connected().len() {
                0 => "Waiting for devices".to_string(),
                1 => "1 device connected".to_string(),
                n => format!("{n} devices connected"),
            }
        });
    });
    card
}

/// Builds the window once; later calls just return it.
pub fn build(app: &adw::Application, hub: &Hub, clipboard: &ClipboardTx) -> adw::ApplicationWindow {
    if let Some(w) = window() {
        return w;
    }

    // Each page sizes itself; a wide page mustn't widen the others.
    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .hhomogeneous(false)
        .vhomogeneous(false)
        .build();
    let pages: [gtk::Widget; 7] = [
        devices::page(hub).upcast(),
        clipboard::page(hub, clipboard).upcast(),
        transfers::page(hub).upcast(),
        files::page(hub).upcast(),
        activity::page(hub).upcast(),
        scratchpad::page().upcast(),
        settings::page(hub).upcast(),
    ];
    for (spec, widget) in PAGES.iter().zip(pages) {
        stack.add_named(&widget, Some(spec.id));
    }

    let nav = gtk::ListBox::builder()
        .css_classes(["navigation-sidebar"])
        .build();
    for spec in &PAGES {
        nav.append(&nav_row(spec.icon, spec.title, spec.id));
    }

    let content_page = adw::NavigationPage::builder()
        .title("Devices")
        .tag("content")
        .build();
    let banner = adw::Banner::builder()
        .title("Link All's background service isn't running")
        .button_label("Start")
        .revealed(false)
        .build();
    banner.connect_button_clicked(|b| {
        if b.action_name().is_none() {
            std::thread::spawn(|| {
                crate::daemon::ensure_running();
            });
        }
    });
    let update_banner = adw::Banner::builder()
        .button_label("See What's New")
        .revealed(false)
        .build();
    watch_updates(&update_banner);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&stack));
    let content_view = adw::ToolbarView::new();
    let content_header = adw::HeaderBar::new();
    let send_button = gtk::Button::builder()
        .icon_name("document-send-symbolic")
        .tooltip_text("Send Files")
        .action_name("app.send-files")
        .build();
    content_header.pack_end(&send_button);
    let search_button = gtk::Button::builder()
        .icon_name("system-search-symbolic")
        .tooltip_text("Search (Ctrl+K)")
        .action_name("app.palette")
        .build();
    content_header.pack_start(&search_button);
    content_view.add_top_bar(&content_header);
    content_view.add_top_bar(&banner);
    content_view.add_top_bar(&update_banner);
    content_view.set_content(Some(&toasts));
    content_page.set_child(Some(&content_view));

    let sidebar_view = adw::ToolbarView::new();
    let sidebar_header = adw::HeaderBar::new();
    let brand = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    brand.append(&crate::theme::logo(22));
    brand.append(
        &gtk::Label::builder()
            .label("Link All")
            .css_classes(["brand-title"])
            .build(),
    );
    sidebar_header.set_title_widget(Some(&brand));
    let menu = gio::Menu::new();
    let actions = gio::Menu::new();
    actions.append(Some("Send Files…"), Some("app.send-files"));
    actions.append(Some("Pair a Phone…"), Some("app.pair-phone"));
    menu.append_section(None, &actions);
    let more = gio::Menu::new();
    more.append(Some("About Link All"), Some("app.about"));
    more.append(Some("Quit"), Some("app.quit"));
    menu.append_section(None, &more);
    sidebar_header.pack_end(
        &gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .tooltip_text("Main Menu")
            .primary(true)
            .build(),
    );
    sidebar_view.add_top_bar(&sidebar_header);
    let sidebar_column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar_column.append(&nav);
    sidebar_column.append(&sidebar_devices(hub));
    sidebar_view.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&sidebar_column)
            .build(),
    ));
    sidebar_view.add_bottom_bar(&this_device(hub));
    sidebar_view.add_css_class("la-sidebar");
    let sidebar_page = adw::NavigationPage::builder()
        .title("Link All")
        .child(&sidebar_view)
        .build();

    let split = adw::NavigationSplitView::builder()
        .sidebar(&sidebar_page)
        .content(&content_page)
        .build();
    split.set_min_sidebar_width(230.0);
    split.set_max_sidebar_width(280.0);

    {
        let (stack, split, content_page) = (stack.clone(), split.clone(), content_page.clone());
        nav.connect_row_selected(move |_, row| {
            let Some(row) = row else { return };
            let id = row.widget_name();
            if let Some(spec) = PAGES.iter().find(|p| p.id == id.as_str()) {
                stack.set_visible_child_name(spec.id);
                content_page.set_title(spec.title);
                split.set_show_content(true);
            }
        });
    }
    nav.select_row(nav.row_at_index(0).as_ref());

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Link All")
        .icon_name(crate::APP_ID)
        .default_width(1100)
        .default_height(760)
        .width_request(360)
        .height_request(480)
        .hide_on_close(true)
        .content(&split)
        .build();
    let narrow = adw::Breakpoint::new(
        adw::BreakpointCondition::parse("max-width: 680sp").expect("breakpoint"),
    );
    narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
    window.add_breakpoint(narrow);

    // The service being down comes first; otherwise the engine's first health issue.
    hub.on_status(move |hub| {
        if !hub.online() {
            banner.set_title("Link All's background service isn't running");
            banner.set_button_label(Some("Start"));
            banner.set_action_name(None);
            banner.set_revealed(true);
            return;
        }
        let issue = hub.status.borrow()["health"]
            .as_array()
            .and_then(|a| a.first().cloned());
        match issue {
            Some(issue) => {
                let title = issue["title"].as_str().unwrap_or_default();
                let detail = issue["detail"].as_str().unwrap_or_default();
                banner.set_title(&glib::markup_escape_text(&if detail.is_empty() {
                    title.to_string()
                } else {
                    format!("{title}. {detail}")
                }));
                let (label, action) = match issue["kind"].as_str() {
                    Some("sync_paused") => (Some("Resume"), Some("app.resume-sync")),
                    Some("devices_not_found") => (Some("Scan"), Some("app.rescan")),
                    _ => (None, None),
                };
                banner.set_button_label(label);
                banner.set_action_name(action);
                banner.set_revealed(true);
            }
            None => banner.set_revealed(false),
        }
    });
    pairing::watch(hub);

    TOASTS.with(|t| *t.borrow_mut() = Some(toasts));
    SIDEBAR.with(|s| *s.borrow_mut() = Some(nav));
    WINDOW.with(|w| *w.borrow_mut() = Some(window.clone()));
    window
}

/// Opens a page by id, e.g. from a device row's "Browse files".
pub fn open_page(id: &str) {
    SIDEBAR.with(|s| {
        let Some(list) = s.borrow().clone() else {
            return;
        };
        let mut i = 0;
        while let Some(row) = list.row_at_index(i) {
            if row.widget_name() == id {
                list.select_row(Some(&row));
                break;
            }
            i += 1;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_like_a_file_manager() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(999), "999 B");
        assert_eq!(human_bytes(1_500), "1.5 KB");
        assert_eq!(human_bytes(30_000_000), "30.0 MB");
        assert_eq!(human_bytes(2_500_000_000), "2.5 GB");
    }

    #[test]
    fn recent_times_are_relative() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(ago(now), "just now");
        assert_eq!(ago(now - 5 * 60), "5 min ago");
        assert_eq!(ago(now - 3 * 3600), "3 h ago");
    }
}
