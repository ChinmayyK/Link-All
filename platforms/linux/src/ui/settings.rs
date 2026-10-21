//! Settings: this computer's name, what syncs, files and startup.

use super::toast;
use crate::{autostart, hub::Hub, ipc, prefs};
use adw::prelude::*;
use gtk::glib;
use serde_json::json;
use std::cell::Cell;
use std::rc::Rc;

/// Writes one setting; the next refresh brings the switch in line if it failed.
fn patch(hub: &Hub, key: &'static str, value: serde_json::Value) {
    let hub = hub.clone();
    let patch = json!({ key: value }).to_string();
    glib::spawn_future_local(async move {
        if let Err(e) = ipc::call(json!({ "cmd": "patch_settings", "patch": patch })).await {
            toast(&e.to_string());
        }
        hub.poke();
    });
}

fn switch(
    hub: &Hub,
    group: &adw::PreferencesGroup,
    key: &'static str,
    title: &str,
    subtitle: &str,
    syncing: &Rc<Cell<bool>>,
) {
    let row = adw::SwitchRow::builder().title(title).build();
    if !subtitle.is_empty() {
        row.set_subtitle(subtitle);
    }
    group.add(&row);
    {
        let (hub, syncing) = (hub.clone(), syncing.clone());
        row.connect_active_notify(move |r| {
            if !syncing.get() {
                patch(&hub, key, r.is_active().into());
            }
        });
    }
    let syncing = syncing.clone();
    hub.on_status(move |hub| {
        if let Some(v) = hub.setting(key).as_bool() {
            if v != row.is_active() {
                syncing.set(true);
                row.set_active(v);
                syncing.set(false);
            }
        }
    });
}

/// A number setting kept in the daemon, e.g. history size.
#[allow(clippy::too_many_arguments)]
fn spin(
    hub: &Hub,
    group: &adw::PreferencesGroup,
    key: &'static str,
    title: &str,
    subtitle: &str,
    min: f64,
    max: f64,
    step: f64,
    syncing: &Rc<Cell<bool>>,
) {
    let row = adw::SpinRow::builder()
        .title(title)
        .subtitle(subtitle)
        .adjustment(&gtk::Adjustment::new(min, min, max, step, step * 10.0, 0.0))
        .build();
    group.add(&row);
    {
        let (hub, syncing) = (hub.clone(), syncing.clone());
        row.connect_value_notify(move |r| {
            if !syncing.get() {
                patch(&hub, key, (r.value() as u64).into());
            }
        });
    }
    let syncing = syncing.clone();
    hub.on_status(move |hub| {
        if let Some(v) = hub.setting(key).as_f64() {
            if (v - row.value()).abs() > 0.5 && !row.has_focus() {
                syncing.set(true);
                row.set_value(v);
                syncing.set(false);
            }
        }
    });
}

pub fn page(hub: &Hub) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    let syncing = Rc::new(Cell::new(false));

    let device = adw::PreferencesGroup::builder()
        .title("This Computer")
        .build();
    let name = adw::EntryRow::builder()
        .title("Name on your other devices")
        .show_apply_button(true)
        .build();
    device.add(&name);
    {
        let hub = hub.clone();
        name.connect_apply(move |row| {
            let text = row.text().trim().to_string();
            if !text.is_empty() {
                patch(&hub, "device_name", text.into());
                toast("Name changed");
            }
        });
    }
    {
        let name = name.clone();
        hub.on_status(move |hub| {
            if name.text().is_empty() && !name.has_focus() {
                if let Some(n) = hub.status.borrow()["device_name"].as_str() {
                    name.set_text(n);
                }
            }
        });
    }
    page.add(&device);

    let look = adw::PreferencesGroup::builder().title("Appearance").build();
    let schemes = ["system", "light", "dark"];
    let theme = adw::ComboRow::builder()
        .title("Theme")
        .model(&gtk::StringList::new(&["Match System", "Light", "Dark"]))
        .build();
    let saved = prefs::get_string(prefs::COLOR_SCHEME).unwrap_or_else(|| "system".into());
    theme.set_selected(schemes.iter().position(|s| *s == saved).unwrap_or(0) as u32);
    theme.connect_selected_notify(move |r| {
        let scheme = schemes[r.selected() as usize];
        prefs::set(prefs::COLOR_SCHEME, scheme.into());
        crate::theme::apply_scheme(scheme);
    });
    look.add(&theme);
    page.add(&look);

    let sync = adw::PreferencesGroup::builder().title("Sync").build();
    switch(
        hub,
        &sync,
        "sync_enabled",
        "Sync",
        "Turn off to pause everything",
        &syncing,
    );
    switch(hub, &sync, "sync_text", "Text", "", &syncing);
    switch(hub, &sync, "sync_images", "Images", "", &syncing);
    switch(hub, &sync, "sync_files", "Files", "", &syncing);
    page.add(&sync);

    let clip = adw::PreferencesGroup::builder().title("Clipboard").build();
    switch(
        hub,
        &clip,
        "auto_apply_remote_clipboard",
        "Paste Here Automatically",
        "Clips from your other devices go straight to this computer's clipboard",
        &syncing,
    );
    switch(
        hub,
        &clip,
        "block_sensitive_text",
        "Keep Secrets on This Computer",
        "Passwords, one-time codes, card numbers and keys are not sent",
        &syncing,
    );
    switch(
        hub,
        &clip,
        "show_receive_notification",
        "Notify When a Clip Arrives",
        "",
        &syncing,
    );
    page.add(&clip);

    let files = adw::PreferencesGroup::builder().title("Files").build();
    switch(
        hub,
        &files,
        "allow_file_browsing",
        "Let Devices Browse Files",
        "Paired devices can see and download from Documents, Downloads, Pictures, Videos and Music",
        &syncing,
    );
    switch(
        hub,
        &files,
        "auto_accept_file_transfers",
        "Accept Files Automatically",
        "Files from paired devices are saved to Downloads",
        &syncing,
    );
    page.add(&files);

    let remote = adw::PreferencesGroup::builder()
        .title("Remote Control")
        .build();
    switch(
        hub,
        &remote,
        "allow_remote_control",
        "Let Phones Control This Computer",
        "Paired phones can press play/pause, next and previous, volume and mute, and the arrow keys for slides. Uses playerctl, wpctl or pactl, and xdotool or wtype when installed",
        &syncing,
    );
    page.add(&remote);

    let startup = adw::PreferencesGroup::builder().title("Startup").build();
    let login = adw::SwitchRow::builder()
        .title("Start at Login")
        .subtitle("Runs in the background with a tray icon")
        .active(autostart::enabled())
        .build();
    login.connect_active_notify(|r| {
        if let Err(e) = autostart::set(r.is_active()) {
            toast(&format!("Couldn't change startup: {e}"));
        }
    });
    startup.add(&login);
    let updates = adw::SwitchRow::builder()
        .title("Check for Updates")
        .subtitle("Asks GitHub once a day if a newer version exists. Never installs anything.")
        .active(crate::update::enabled())
        .build();
    updates.connect_active_notify(|r| {
        prefs::set(crate::update::ENABLED, r.is_active().into());
    });
    startup.add(&updates);
    page.add(&startup);

    let advanced = adw::PreferencesGroup::builder().title("Advanced").build();
    spin(
        hub,
        &advanced,
        "port",
        "Network Port",
        "Change only if another app uses 47823",
        1024.0,
        65535.0,
        1.0,
        &syncing,
    );
    switch(
        hub,
        &advanced,
        "require_tofu_confirmation",
        "Confirm New Devices",
        "Ask before a device pairs, with a matching code",
        &syncing,
    );
    let rescan = adw::ActionRow::builder()
        .title("Scan for Devices")
        .activatable(true)
        .build();
    rescan.set_action_name(Some("app.rescan"));
    rescan.add_suffix(&gtk::Image::from_icon_name("view-refresh-symbolic"));
    advanced.add(&rescan);
    let disconnect = adw::ActionRow::builder()
        .title("Disconnect All Devices")
        .activatable(true)
        .build();
    {
        let hub = hub.clone();
        disconnect.connect_activated(move |_| {
            for p in hub.connected() {
                if let Some(id) = p["id"].as_str() {
                    super::act(
                        json!({ "cmd": "disconnect_peer", "device_id": id }),
                        None,
                        &hub,
                    );
                }
            }
        });
    }
    advanced.add(&disconnect);
    let diagnostics = adw::ActionRow::builder()
        .title("Report a Problem")
        .subtitle("Copy or save a report for a bug report")
        .activatable(true)
        .build();
    diagnostics.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    diagnostics.connect_activated(|_| show_diagnostics());
    advanced.add(&diagnostics);
    page.add(&advanced);

    let about = adw::PreferencesGroup::new();
    let version = adw::ActionRow::builder()
        .title("Link All")
        .subtitle(format!(
            "Version {} · No cloud, no account, no telemetry",
            env!("CARGO_PKG_VERSION")
        ))
        .build();
    about.add(&version);
    page.add(&about);
    page
}

/// Distro, desktop and session: what only this app knows, for the report.
fn report_header() -> String {
    let distro = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| {
            t.lines().find_map(|l| {
                l.strip_prefix("PRETTY_NAME=")
                    .map(|v| v.trim_matches('"').to_string())
            })
        })
        .unwrap_or_else(|| "unknown".into());
    format!(
        "Linux: {distro}\nDesktop: {} ({})",
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".into()),
        std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into()),
    )
}

/// The daemon's redacted problem report, to copy or save.
fn show_diagnostics() {
    glib::spawn_future_local(async move {
        let header = report_header();
        let text = match ipc::call(json!({ "cmd": "diagnostic_report", "header": header })).await {
            Ok(data) => data["text"].as_str().unwrap_or_default().to_string(),
            Err(e) => format!("Link All problem report\n{header}\n\n{e}\n"),
        };
        let view = gtk::TextView::builder()
            .editable(false)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .top_margin(12)
            .bottom_margin(12)
            .left_margin(12)
            .right_margin(12)
            .build();
        view.buffer().set_text(&text);
        view.add_css_class("card");
        let scroller = gtk::ScrolledWindow::builder()
            .child(&view)
            .min_content_height(320)
            .min_content_width(520)
            .build();
        let dialog = adw::AlertDialog::builder()
            .heading("Report a Problem")
            .body("Versions, device states and recent log lines. No clipboard text, file names, device names or full addresses.")
            .extra_child(&scroller)
            .close_response("close")
            .build();
        dialog.add_responses(&[("copy", "Copy"), ("save", "Save…"), ("close", "Close")]);
        {
            let text = text.clone();
            dialog.connect_response(Some("copy"), move |_, _| {
                if let Some(display) = gtk::gdk::Display::default() {
                    display.clipboard().set_text(&text);
                    toast("Copied");
                }
            });
        }
        dialog.connect_response(Some("save"), move |_, _| {
            let text = text.clone();
            gtk::FileDialog::builder()
                .initial_name("link-all-report.txt")
                .build()
                .save(
                    super::window().as_ref(),
                    None::<&gtk::gio::Cancellable>,
                    move |picked| {
                        if let Some(path) = picked.ok().and_then(|f| f.path()) {
                            if let Err(e) = std::fs::write(path, &text) {
                                toast(&format!("Couldn't save the report: {e}"));
                            }
                        }
                    },
                );
        });
        dialog.present(super::window().as_ref());
    });
}
