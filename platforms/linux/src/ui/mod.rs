//! The Link All window: a sidebar of sections, and a navigation view that
//! pushes a device's page, its files, Notes and each settings category over
//! them, each with a back button.

mod activity;
mod clipboard;
mod device;
mod extras;
mod files;
mod home;
mod now_playing;
pub mod pairing;
pub mod palette;
mod scratchpad;
mod share;
pub mod send;
mod settings;
mod transfers;

pub use extras::{call_from_phone, receive_from_guest};
pub use pairing::add_by_address;

use crate::{agent::ClipboardTx, hub::Hub, ipc};
use adw::prelude::*;
use gtk::{gio, glib};
use serde_json::Value;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

/// The window's navigation: the sidebar, the sections in the root page, and
/// the view pages are pushed onto.
#[derive(Clone)]
struct Nav {
    sidebar: gtk::ListBox,
    split: adw::NavigationSplitView,
    view: adw::NavigationView,
    root: adw::NavigationPage,
    content: adw::NavigationPage,
    stack: gtk::Stack,
    notes: adw::NavigationPage,
}

thread_local! {
    static TOASTS: RefCell<Option<adw::ToastOverlay>> = const { RefCell::new(None) };
    static WINDOW: RefCell<Option<adw::ApplicationWindow>> = const { RefCell::new(None) };
    static NAV: RefCell<Option<Nav>> = const { RefCell::new(None) };
}

/// Runs `f` with the navigation, cloned out first so `f` may navigate again.
fn with_nav(f: impl FnOnce(&Nav)) {
    if let Some(nav) = NAV.with(|n| n.borrow().clone()) {
        f(&nav);
    }
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

/// Opens a file in the app that handles its type.
pub fn open_file(path: &Path) {
    if !path.exists() {
        return toast("Can't find this file any more");
    }
    if let Err(e) = gio::AppInfo::launch_default_for_uri(
        &gio::File::for_path(path).uri(),
        None::<&gio::AppLaunchContext>,
    ) {
        toast(&e.to_string());
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

/// Gives an icon-only button a name screen readers read out, and the same tooltip.
pub fn name_button(button: &impl IsA<gtk::Widget>, name: &str) {
    let widget = button.upcast_ref::<gtk::Widget>();
    widget.set_tooltip_text(Some(name));
    widget.update_property(&[gtk::accessible::Property::Label(name)]);
}

/// This computer's name, as its paired devices see it.
pub fn device_name(hub: &Hub) -> String {
    hub.status.borrow()["device_name"]
        .as_str()
        .unwrap_or("This computer")
        .to_string()
}

/// How sync stands, in a few words, with the style class that colours it.
pub fn sync_state(hub: &Hub) -> (&'static str, &'static str) {
    if !hub.online() {
        ("Not running", "warning")
    } else if hub.setting("sync_enabled") == false {
        ("Sync paused", "warning")
    } else if hub.connected().is_empty() {
        ("Waiting for devices", "muted")
    } else {
        ("Syncing", "live-text")
    }
}

pub fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    name_button(&b, tooltip);
    b.set_valign(gtk::Align::Center);
    b.add_css_class("flat");
    b
}

pub fn label(text: &str, classes: &[&str]) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .css_classes(classes.to_vec())
        .build()
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

/// A header's link on the right, e.g. "See all", and what it does.
pub type HeaderAction<'a> = Option<(&'a str, Box<dyn Fn()>)>;

/// A section's title, with an optional link-style action at its end ("See all").
pub fn section_header(title: &str, action: HeaderAction) -> (gtk::Box, gtk::Label) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let heading = label(title, &["section-title"]);
    heading.set_hexpand(true);
    row.append(&heading);
    if let Some((text, f)) = action {
        let b = gtk::Button::builder()
            .label(text)
            .valign(gtk::Align::Center)
            .css_classes(["flat", "accent"])
            .build();
        b.connect_clicked(move |_| f());
        row.append(&b);
    }
    (row, heading)
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

/// A page pushed over the sections: a header bar, which shows the back
/// button, over `content`. The header is returned for buttons of its own.
pub fn pushed_page(
    tag: &str,
    title: &str,
    content: &impl IsA<gtk::Widget>,
) -> (adw::NavigationPage, adw::HeaderBar) {
    let header = adw::HeaderBar::new();
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(content));
    let page = adw::NavigationPage::builder()
        .tag(tag)
        .title(title)
        .child(&view)
        .build();
    (page, header)
}

fn in_stack(view: &adw::NavigationView, page: &adw::NavigationPage) -> bool {
    let stack = view.navigation_stack();
    (0..stack.n_items()).any(|i| {
        stack
            .item(i)
            .is_some_and(|p| p == *page.upcast_ref::<glib::Object>())
    })
}

/// Drops whatever was pushed over the sections.
fn back_to_sections(nav: &Nav) {
    if nav.view.visible_page().as_ref() != Some(&nav.root) {
        nav.view.replace(std::slice::from_ref(&nav.root));
    }
}

/// Shows `page` over the current section, in place of anything pushed before.
pub fn show_over_section(page: &adw::NavigationPage) {
    with_nav(|nav| {
        nav.split.set_show_content(true);
        match nav.view.visible_page() {
            Some(p) if &p == page => {}
            Some(p) if p == nav.root => nav.view.push(page),
            _ => nav.view.replace(&[nav.root.clone(), page.clone()]),
        }
    });
}

/// Pushes `page` over what is showing, e.g. a device's files over its page.
pub fn push_over(page: &adw::NavigationPage) {
    with_nav(|nav| {
        nav.split.set_show_content(true);
        if nav.view.visible_page().as_ref() == Some(page) {
            return;
        }
        if in_stack(&nav.view, page) {
            nav.view.pop_to_page(page);
        } else {
            nav.view.push(page);
        }
    });
}

/// Takes `page` away if it is pushed, e.g. the page of a device just forgotten.
pub fn close(page: &adw::NavigationPage) {
    with_nav(|nav| {
        if in_stack(&nav.view, page) {
            back_to_sections(nav);
        }
    });
}

struct PageSpec {
    id: &'static str,
    title: &'static str,
    icon: &'static str,
}

const PAGES: [PageSpec; 6] = [
    PageSpec {
        id: "home",
        title: "Home",
        icon: "go-home-symbolic",
    },
    PageSpec {
        id: "clipboard",
        title: "Clipboard",
        icon: "edit-paste-symbolic",
    },
    PageSpec {
        id: "files",
        title: "Files",
        icon: "folder-symbolic",
    },
    PageSpec {
        id: "share",
        title: "Keyboard & Mouse",
        icon: "input-keyboard-symbolic",
    },
    PageSpec {
        id: "activity",
        title: "Activity",
        icon: "document-open-recent-symbolic",
    },
    PageSpec {
        id: "settings",
        title: "Settings",
        icon: "emblem-system-symbolic",
    },
];

fn show_section(nav: &Nav, id: &str) {
    let Some(spec) = PAGES.iter().find(|p| p.id == id) else {
        return;
    };
    back_to_sections(nav);
    if nav.stack.visible_child_name().as_deref() != Some(spec.id) {
        nav.stack.set_visible_child_name(spec.id);
    }
    nav.root.set_title(spec.title);
    nav.content.set_title(spec.title);
    nav.split.set_show_content(true);
}

/// Opens a section by id, e.g. "activity" from "See all" on Home. Ids from
/// before Home and Files still work: "devices" is Home, "transfers" is Files
/// and "scratchpad" opens Notes.
pub fn open_page(id: &str) {
    let id = match id {
        "devices" => "home",
        "transfers" => "files",
        "scratchpad" | "notes" => return open_notes(),
        other => other,
    };
    with_nav(|nav| {
        let mut i = 0;
        while let Some(row) = nav.sidebar.row_at_index(i) {
            if row.widget_name() == id {
                nav.sidebar.select_row(Some(&row));
                break;
            }
            i += 1;
        }
        show_section(nav, id);
    });
}

/// Notes, over whatever section is showing.
pub fn open_notes() {
    with_nav(|nav| show_over_section(&nav.notes));
}

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
    list.connect_row_activated(|_, row| device::open(row.widget_name().as_str()));
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

/// Builds the window once; later calls just return it.
pub fn build(app: &adw::Application, hub: &Hub, clipboard: &ClipboardTx) -> adw::ApplicationWindow {
    if let Some(w) = window() {
        return w;
    }

    // Each section sizes itself; a wide one mustn't widen the others.
    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .hhomogeneous(false)
        .vhomogeneous(false)
        .build();
    let (settings_root, settings_pages) = settings::page(hub);
    let sections: [gtk::Widget; 6] = [
        home::page(hub, clipboard).upcast(),
        clipboard::page(hub, clipboard).upcast(),
        transfers::page(hub).upcast(),
        share::page(hub).upcast(),
        activity::page(hub).upcast(),
        settings_root.upcast(),
    ];
    for (spec, widget) in PAGES.iter().zip(sections) {
        stack.add_named(&widget, Some(spec.id));
    }

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
        .button_label("See what's new")
        .revealed(false)
        .build();
    watch_updates(&update_banner);
    let header = adw::HeaderBar::new();
    let send_button = gtk::Button::builder()
        .icon_name("document-send-symbolic")
        .action_name("app.send-files")
        .build();
    name_button(&send_button, "Send files");
    header.pack_end(&send_button);
    let search_button = gtk::Button::builder()
        .icon_name("system-search-symbolic")
        .action_name("app.palette")
        .build();
    name_button(&search_button, "Search (Ctrl+K)");
    header.pack_start(&search_button);
    let sections_view = adw::ToolbarView::new();
    sections_view.add_top_bar(&header);
    sections_view.add_top_bar(&banner);
    sections_view.add_top_bar(&update_banner);
    sections_view.set_content(Some(&stack));
    let root = adw::NavigationPage::builder()
        .title("Home")
        .tag("sections")
        .child(&sections_view)
        .build();

    // Pages pushed over the sections stay built; showing one just fills it in.
    let view = adw::NavigationView::new();
    view.add(&root);
    view.add(&device::page(hub));
    view.add(&files::page(hub));
    let (notes, _) = pushed_page("notes", "Notes", &scratchpad::page());
    view.add(&notes);
    for page in &settings_pages {
        view.add(page);
    }
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&view));
    let content_page = adw::NavigationPage::builder()
        .title("Home")
        .tag("content")
        .child(&toasts)
        .build();

    let sidebar_list = gtk::ListBox::builder()
        .css_classes(["navigation-sidebar"])
        .build();
    for spec in &PAGES {
        sidebar_list.append(&nav_row(spec.icon, spec.title, spec.id));
    }
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
    actions.append(Some("Send files…"), Some("app.send-files"));
    actions.append(Some("Pair a phone…"), Some("app.pair-phone"));
    menu.append_section(None, &actions);
    let more = gio::Menu::new();
    more.append(Some("About Link All"), Some("app.about"));
    more.append(Some("Quit"), Some("app.quit"));
    menu.append_section(None, &more);
    sidebar_header.pack_end(
        &gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .tooltip_text("Main menu")
            .primary(true)
            .build(),
    );
    sidebar_view.add_top_bar(&sidebar_header);
    let sidebar_column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar_column.append(&sidebar_list);
    sidebar_column.append(&sidebar_devices(hub));
    sidebar_view.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&sidebar_column)
            .build(),
    ));
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

    // Selecting a section shows it; clicking the one already selected also
    // leaves a page pushed over it.
    sidebar_list.connect_row_selected(|_, row| {
        if let Some(row) = row {
            let id = row.widget_name();
            with_nav(|nav| show_section(nav, id.as_str()));
        }
    });
    sidebar_list.connect_row_activated(|_, row| {
        let id = row.widget_name();
        with_nav(|nav| show_section(nav, id.as_str()));
    });

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
                let (label, action) = fix_for(issue["kind"].as_str().unwrap_or_default());
                banner.set_button_label(label);
                banner.set_action_name(action);
                banner.set_revealed(true);
            }
            None => banner.set_revealed(false),
        }
    });
    pairing::watch(hub);

    TOASTS.with(|t| *t.borrow_mut() = Some(toasts));
    NAV.with(|n| {
        *n.borrow_mut() = Some(Nav {
            sidebar: sidebar_list.clone(),
            split,
            view,
            root,
            content: content_page,
            stack,
            notes,
        })
    });
    WINDOW.with(|w| *w.borrow_mut() = Some(window.clone()));
    sidebar_list.select_row(sidebar_list.row_at_index(0).as_ref());
    window
}

/// The button that fixes a health issue of this `kind`, as (label, action), if one does.
pub fn fix_for(kind: &str) -> (Option<&'static str>, Option<&'static str>) {
    match kind {
        "sync_paused" => (Some("Resume"), Some("app.resume-sync")),
        "devices_not_found" | "connection_blocked" => (Some("Scan"), Some("app.rescan")),
        _ => (None, None),
    }
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
