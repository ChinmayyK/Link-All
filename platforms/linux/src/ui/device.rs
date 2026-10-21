//! A device's page: who it is and how it's linked, what it plays, and
//! everything this computer can do with it, each action already aimed at it.
//! Opened from its card on Home, the sidebar, Files or the command palette.

use super::{
    act, ago, extras, files, human_bytes, label, now_playing, pairing, send, toast, window,
};
use crate::{hub::Hub, ipc, theme};
use adw::prelude::*;
use gtk::glib;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Offline because the phone's battery settings pause Link All in the
/// background (it said so when it last connected), not because of the network.
fn paused_by_phone(p: &Value) -> bool {
    p["status"] != "connected"
        && p["explicit_disconnect"] != true
        && p["background_restricted"] == true
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

/// How the last pairing request with an unpaired device ended, if it says something.
pub fn pairing_outcome(p: &Value) -> Option<&'static str> {
    if p["trusted"] == true
        || p["pairing_requested"] == true
        || p["outgoing_pairing_waiting"] == true
    {
        return None;
    }
    match p["pairing_outcome"].as_str()? {
        "declined" => Some("Declined, try again"),
        "expired" => Some("No answer, try again"),
        "cancelled" => Some("Request withdrawn"),
        "update_needed" => Some("Update Link All on it, then try again"),
        _ => None,
    }
}

/// A paired device in one line: connected with its battery and free space, or
/// when it was last seen.
pub fn describe(p: &Value, status: &Value) -> String {
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
    if let Some(free) = storage(status, id).and_then(|s| s["free_bytes"].as_u64()) {
        parts.push(format!("{} free", human_bytes(free)));
    }
    parts.join(" · ")
}

/// The line under the device's name, and the style it is shown in.
fn state_line(p: &Value, testing: bool) -> (String, &'static str) {
    let trusted = p["trusted"] == true;
    let connected = p["status"] == "connected";
    let name = p["friendly_name"].as_str().unwrap_or("the device");
    let platform = p["platform"].as_str().filter(|s| !s.is_empty());
    if testing {
        return ("Testing the connection…".into(), "accent");
    }
    if p["pairing_requested"] == true && !trusted {
        return ("Wants to pair".into(), "accent");
    }
    if trusted && connected {
        let state = if p["sync_enabled"] == false {
            "Connected · sync paused"
        } else {
            "Connected"
        };
        return (
            match platform {
                Some(os) => format!("{state} · {os}"),
                None => state.to_string(),
            },
            "live-text",
        );
    }
    if p["outgoing_pairing_waiting"] == true && !trusted {
        return (format!("Waiting for {name} to accept"), "warning");
    }
    if let Some(outcome) = pairing_outcome(p) {
        return (outcome.into(), "warning");
    }
    if p["status"] == "connecting" {
        return ("Connecting…".into(), "muted");
    }
    if trusted {
        if paused_by_phone(p) {
            return ("Paused by the phone".into(), "muted");
        }
        return (
            match p["last_seen"].as_u64() {
                Some(t) if t > 0 => format!("Not connected · seen {}", ago(t)),
                _ => "Not connected".into(),
            },
            "muted",
        );
    }
    ("Not paired".into(), "warning")
}

/// Its information rows: a value at the end, or below for long ones.
struct About {
    status: (adw::ActionRow, gtk::Label),
    system: (adw::ActionRow, gtk::Label),
    version: (adw::ActionRow, gtk::Label),
    address: (adw::ActionRow, gtk::Label),
    battery: (adw::ActionRow, gtk::Label),
    battery_bar: gtk::LevelBar,
    storage: (adw::ActionRow, gtk::Label),
    storage_bar: gtk::LevelBar,
    seen: (adw::ActionRow, gtk::Label),
    fingerprint: adw::ActionRow,
    speed: adw::ActionRow,
}

struct Page {
    hub: Hub,
    nav: adw::NavigationPage,
    id: RefCell<String>,
    well: gtk::Box,
    icon: gtk::Image,
    name: gtk::Label,
    dot: gtk::Box,
    state: gtk::Label,
    panel: gtk::Box,
    panel_key: RefCell<String>,
    playing: Rc<now_playing::Panel>,
    actions: gtk::Box,
    actions_key: RefCell<String>,
    about: About,
    manage: adw::PreferencesGroup,
    sync_row: adw::SwitchRow,
    auto_row: adw::SwitchRow,
    connect_label: gtk::Label,
    /// Set while the page moves its own switches, so they don't send anything.
    quiet: Cell<bool>,
    /// The hero's styles as last set, so a refresh only restyles on a change.
    styled: RefCell<String>,
}

thread_local! {
    static PAGE: RefCell<Option<Rc<Page>>> = const { RefCell::new(None) };
}

fn current() -> Option<Rc<Page>> {
    PAGE.with(|p| p.borrow().clone())
}

/// The device shown and its name now, for an action pressed on the page.
fn target() -> Option<(Hub, String, String)> {
    let page = current()?;
    let id = page.id.borrow().clone();
    let name = page
        .hub
        .peer(&id)
        .and_then(|p| p["friendly_name"].as_str().map(str::to_string))
        .unwrap_or_else(|| "the device".into());
    Some((page.hub.clone(), id, name))
}

/// Opens `id`'s page over the current section.
pub fn open(id: &str) {
    let Some(page) = current() else { return };
    if *page.id.borrow() != id {
        *page.id.borrow_mut() = id.to_string();
        page.panel_key.borrow_mut().clear();
        page.actions_key.borrow_mut().clear();
    }
    if page.update() {
        super::show_over_section(&page.nav);
    }
}

fn value_row(title: &str) -> (adw::ActionRow, gtk::Label) {
    let row = adw::ActionRow::builder().title(title).build();
    let value = gtk::Label::builder()
        .xalign(1.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(28)
        .css_classes(["dim-label"])
        .build();
    row.add_suffix(&value);
    (row, value)
}

/// A row that does one thing, its title centred like a button.
fn action_row(title: &str, destructive: bool) -> (adw::ActionRow, gtk::Label) {
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
    (row, l)
}

/// An action on the page: a tinted icon, what it does and a line of help.
fn tile(icon: &str, tint: &str, title: &str, sub: &str, run: impl Fn() + 'static) -> gtk::Button {
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let w = theme::well(icon, tint);
    w.add_css_class("small");
    inner.append(&w);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_valign(gtk::Align::Center);
    text.set_hexpand(true);
    let t = label(title, &["tile-title"]);
    t.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&t);
    let s = label(sub, &["tile-sub"]);
    s.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&s);
    inner.append(&text);
    let button = gtk::Button::builder()
        .child(&inner)
        .css_classes(["tile"])
        .tooltip_text(sub)
        .width_request(220)
        .build();
    button.connect_clicked(move |_| run());
    button
}

/// A titled group of tiles, three across at most.
fn tiles(title: &str, items: Vec<gtk::Button>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title(title).build();
    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(3)
        .column_spacing(12)
        .row_spacing(12)
        .build();
    for t in items {
        flow.append(
            &gtk::FlowBoxChild::builder()
                .child(&t)
                .focusable(false)
                .build(),
        );
    }
    group.add(&flow);
    group
}

/// Runs `f` with the device shown and its name.
fn on_target(f: impl Fn(&Hub, &str, &str) + 'static) -> impl Fn() + 'static {
    move || {
        if let Some((hub, id, name)) = target() {
            f(&hub, &id, &name);
        }
    }
}

/// A state panel: a small heading, what is going on, and its buttons below.
fn panel(eyebrow: &str, heading: &str, body: &str, accent: bool) -> (gtk::Box, gtk::Box) {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["card", "la", "state-panel"])
        .build();
    if accent {
        b.add_css_class("accent-edge");
    }
    b.append(&label(eyebrow, &["eyebrow"]));
    let h = label(heading, &["title-3"]);
    h.set_wrap(true);
    b.append(&h);
    let body_label = label(body, &["muted"]);
    body_label.set_wrap(true);
    body_label.set_margin_top(2);
    let buttons = gtk::Box::builder()
        .spacing(10)
        .margin_top(10)
        .halign(gtk::Align::Start)
        .build();
    b.append(&body_label);
    b.append(&buttons);
    (b, buttons)
}

fn pill(text: &str, suggested: bool, run: impl Fn() + 'static) -> gtk::Button {
    let button = gtk::Button::builder()
        .label(text)
        .css_classes(if suggested {
            vec!["pill", "suggested-action"]
        } else {
            vec!["pill"]
        })
        .build();
    button.connect_clicked(move |_| run());
    button
}

fn pin(text: &str) -> gtk::Label {
    let l = label(text, &["pin"]);
    l.set_margin_top(8);
    l.set_margin_bottom(4);
    l
}

impl Page {
    /// Fills the page in for its device; false once that device is gone.
    fn update(&self) -> bool {
        let id = self.id.borrow().clone();
        // Forgotten while open, or gone from the network unpaired. Closed
        // from idle, as this can run while the page is still being shown.
        let Some(p) = self.hub.peer(&id) else {
            let nav = self.nav.clone();
            glib::idle_add_local_once(move || super::close(&nav));
            return false;
        };
        let status = self.hub.status.borrow().clone();
        let name = p["friendly_name"].as_str().unwrap_or("Device");
        let linked = p["trusted"] == true && p["status"] == "connected";
        let testing = status["active_speed_tests"]
            .as_array()
            .is_some_and(|t| t.iter().any(|t| t["peer_id"] == id.as_str()));

        self.nav.set_title(name);
        self.name.set_label(name);
        self.icon.set_icon_name(Some(theme::os_icon(
            p["platform"].as_str().unwrap_or_default(),
        )));
        let (line, class) = state_line(&p, testing);
        self.state.set_label(&line);
        let styled = format!("{linked}|{class}");
        if *self.styled.borrow() != styled {
            *self.styled.borrow_mut() = styled;
            self.well.set_css_classes(if linked {
                &["well", "large", "blue"]
            } else {
                &["well", "large"]
            });
            self.state.set_css_classes(&[class]);
            self.dot.set_css_classes(if linked {
                &["status-dot", "live"]
            } else {
                &["status-dot"]
            });
        }

        self.update_panel(&p);
        self.update_actions(&p, linked);
        self.update_playing(linked);
        self.update_about(&p, &status, &line, linked, testing);
        self.update_manage(&p);
        true
    }

    /// What needs doing first, when the device isn't simply linked: a pairing
    /// request to answer or wait on, pairing to start, or a link to bring back.
    fn update_panel(&self, p: &Value) {
        let name = p["friendly_name"].as_str().unwrap_or("the device");
        let trusted = p["trusted"] == true;
        let pin_code = p["pairing_pin"].as_str().unwrap_or_default();
        let key = if p["pairing_requested"] == true && !trusted {
            format!("ask|{name}|{pin_code}")
        } else if p["outgoing_pairing_waiting"] == true && !trusted {
            format!("wait|{name}|{pin_code}")
        } else if !trusted {
            format!("pair|{name}")
        } else if p["status"] != "connected" {
            format!(
                "away|{name}|{}|{}",
                paused_by_phone(p),
                p["explicit_disconnect"] == true
            )
        } else {
            String::new()
        };
        if *self.panel_key.borrow() == key {
            return;
        }
        *self.panel_key.borrow_mut() = key.clone();
        while let Some(child) = self.panel.first_child() {
            self.panel.remove(&child);
        }
        let Some((kind, _)) = key.split_once('|') else {
            self.panel.set_visible(false);
            return;
        };
        let built = match kind {
            "ask" => {
                let (b, buttons) = panel(
                    "PAIRING REQUEST",
                    &format!("{name} wants to pair"),
                    &format!("Only accept if this code matches the one on {name}."),
                    true,
                );
                b.insert_child_after(
                    &pin(pin_code),
                    b.first_child().and_then(|c| c.next_sibling()).as_ref(),
                );
                buttons.append(&pill(
                    "Decline",
                    false,
                    on_target(|hub, id, _| pairing::respond(hub, id, false)),
                ));
                buttons.append(&pill(
                    "Accept",
                    true,
                    on_target(|hub, id, _| pairing::respond(hub, id, true)),
                ));
                b
            }
            "wait" => {
                let (b, buttons) = panel(
                    "WAITING FOR APPROVAL",
                    &format!("Accept on {name}"),
                    &if pin_code.is_empty() {
                        format!("Connecting to {name}…")
                    } else {
                        format!("{name} shows a request with a code. Accept it there if it matches this one.")
                    },
                    true,
                );
                if !pin_code.is_empty() {
                    b.insert_child_after(
                        &pin(pin_code),
                        b.first_child().and_then(|c| c.next_sibling()).as_ref(),
                    );
                }
                buttons.append(&pill(
                    "Cancel request",
                    false,
                    on_target(|hub, id, _| pairing::cancel(hub, id)),
                ));
                b
            }
            "pair" => {
                let (b, buttons) = panel(
                    "NOT PAIRED",
                    "Not paired yet",
                    &format!("Pair to share your clipboard and files with {name}. Both devices show a code to compare first."),
                    false,
                );
                buttons.append(&pill(
                    &format!("Pair with {name}"),
                    true,
                    on_target(pairing::request),
                ));
                b
            }
            _ => {
                let body = if paused_by_phone(p) {
                    "Your phone stops Link All in the background to save battery, so it drops off when the screen is off. On the phone, open Link All and tap Allow on the battery notice.".to_string()
                } else if p["explicit_disconnect"] == true {
                    "You disconnected it. It stays disconnected until you connect again."
                        .to_string()
                } else {
                    format!("It reconnects on its own when both are on the same network and Link All is open on {name}.")
                };
                let (b, buttons) = panel(
                    "NOT CONNECTED",
                    &format!("{name} isn't connected"),
                    &body,
                    false,
                );
                buttons.append(&pill(
                    "Connect",
                    true,
                    on_target(|hub, id, _| {
                        act(
                            json!({ "cmd": "reconnect_peer", "device_id": id }),
                            Some("Connecting…"),
                            hub,
                        )
                    }),
                ));
                b
            }
        };
        self.panel.append(&built);
        self.panel.set_visible(true);
    }

    /// The actions, grouped, rebuilt only when what the device can do changes.
    fn update_actions(&self, p: &Value, linked: bool) {
        let phone = p["platform"] == "Android";
        // Ringing and calls need Link All 2.1 or later on the phone.
        let up_to_date =
            linkall_core::device_choice::understands_remote_control(p["app_version"].as_str());
        let key = format!("{linked}|{phone}|{up_to_date}");
        if *self.actions_key.borrow() == key {
            return;
        }
        *self.actions_key.borrow_mut() = key;
        while let Some(child) = self.actions.first_child() {
            self.actions.remove(&child);
        }
        self.actions.set_visible(linked);
        if !linked {
            return;
        }
        self.actions.append(&tiles(
            "Share",
            vec![
                tile(
                    "document-send-symbolic",
                    "blue",
                    "Send files",
                    "Or drop them on this page",
                    on_target(|hub, id, _| send::pick_files(hub, Some(id.to_string()))),
                ),
                tile(
                    "folder-symbolic",
                    "blue",
                    "Send a folder",
                    "Keeps its structure",
                    on_target(|hub, id, _| send::pick_folder(hub, Some(id.to_string()))),
                ),
                tile(
                    "edit-paste-symbolic",
                    "blue",
                    "Send clipboard",
                    "What you last copied here",
                    on_target(|hub, id, _| send::send_clipboard(hub, Some(id.to_string()))),
                ),
                tile(
                    "web-browser-symbolic",
                    "blue",
                    "Open a link",
                    "Opens it in its browser",
                    on_target(open_link),
                ),
            ],
        ));
        self.actions.append(&tiles(
            "Files",
            vec![tile(
                "folder-remote-symbolic",
                "green",
                "Browse files",
                "Open and download its files",
                on_target(|_, id, _| files::browse(id)),
            )],
        ));
        if phone {
            let call = tile(
                "call-start-symbolic",
                "violet",
                "Call a number",
                "Your phone dials it",
                on_target(|_, id, name| extras::call_on(id, name)),
            );
            let photo = tile(
                "camera-photo-symbolic",
                "violet",
                "Take a photo",
                "Pasted where you were typing",
                on_target(|hub, id, _| capture(hub, id, "photo")),
            );
            let scan = tile(
                "scanner-symbolic",
                "violet",
                "Scan a document",
                "Pasted where you were typing",
                on_target(|hub, id, _| capture(hub, id, "document")),
            );
            let ring = tile(
                "audio-volume-high-symbolic",
                "amber",
                "Ring",
                "Rings at full volume, even on silent",
                on_target(find_phone),
            );
            if !up_to_date {
                for t in [&call, &ring] {
                    t.set_sensitive(false);
                    t.set_tooltip_text(Some("Needs Link All 2.1 or later on the phone"));
                }
            }
            self.actions
                .append(&tiles("Phone", vec![call, photo, scan]));
            self.actions.append(&tiles("Find", vec![ring]));
        }
    }

    fn update_playing(&self, linked: bool) {
        let id = self.id.borrow().clone();
        let item = linked.then(|| now_playing::playing_on(&id)).flatten();
        self.playing.update(item.as_ref());
    }

    fn update_about(&self, p: &Value, status: &Value, line: &str, linked: bool, testing: bool) {
        let id = p["id"].as_str().unwrap_or_default();
        let a = &self.about;
        let set = |(row, value): &(adw::ActionRow, gtk::Label), text: Option<String>| {
            row.set_visible(text.is_some());
            if let Some(t) = text {
                value.set_label(&t);
            }
        };
        set(&a.status, Some(line.to_string()));
        set(
            &a.system,
            p["platform"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        );
        set(
            &a.version,
            p["app_version"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        );
        set(
            &a.address,
            p["ips"]
                .as_array()
                .and_then(|ips| ips.first())
                .and_then(|ip| ip.as_str())
                .map(str::to_string),
        );
        let level = battery(status, id).and_then(|b| {
            let level = b["level"].as_u64()?;
            Some((level, b["charging"] == true))
        });
        set(
            &a.battery,
            level.map(|(l, charging)| {
                if charging {
                    format!("{l}%, charging")
                } else {
                    format!("{l}%")
                }
            }),
        );
        if let Some((l, _)) = level {
            a.battery_bar.set_value(l as f64 / 100.0);
        }
        let space = storage(status, id).and_then(|s| {
            let total = s["total_bytes"].as_u64().filter(|t| *t > 0)?;
            Some((s, total))
        });
        set(
            &a.storage,
            space.map(|(s, total)| {
                format!(
                    "{} free of {}",
                    human_bytes(s["free_bytes"].as_u64().unwrap_or(0)),
                    human_bytes(total)
                )
            }),
        );
        if let Some((s, total)) = space {
            let free = s["free_bytes"].as_u64().unwrap_or(0).min(total);
            a.storage_bar
                .set_value((total - free) as f64 / total as f64);
            // A phone also says what fills it.
            let parts: Vec<String> = [
                ("Photos", "images_bytes"),
                ("Videos", "videos_bytes"),
                ("Apps", "apps_bytes"),
            ]
            .iter()
            .filter_map(|(what, key)| {
                let bytes = s[*key].as_u64().filter(|b| *b > 0)?;
                Some(format!("{what} {}", human_bytes(bytes)))
            })
            .collect();
            a.storage.0.set_subtitle(&parts.join(" · "));
        }
        let seen = if p["status"] == "connected" {
            p["last_sync"]
                .as_u64()
                .filter(|t| *t > 0)
                .map(|t| ("Last synced", ago(t)))
        } else {
            p["last_seen"]
                .as_u64()
                .filter(|t| *t > 0)
                .map(|t| ("Last seen", ago(t)))
        };
        if let Some((title, _)) = &seen {
            a.seen.0.set_title(title);
        }
        set(&a.seen, seen.map(|(_, when)| when));
        let fp = p["fingerprint_display"].as_str().unwrap_or_default();
        a.fingerprint.set_visible(!fp.is_empty());
        a.fingerprint.set_subtitle(fp);
        a.speed.set_visible(linked);
        a.speed.set_sensitive(!testing);
        a.speed.set_subtitle(if testing {
            "Testing…"
        } else {
            "Takes about 5 seconds"
        });
    }

    fn update_manage(&self, p: &Value) {
        let trusted = p["trusted"] == true;
        self.manage.set_visible(trusted);
        if !trusted {
            return;
        }
        self.quiet.set(true);
        self.sync_row.set_active(p["sync_enabled"] != false);
        self.auto_row.set_active(p["auto_connect"] != false);
        self.quiet.set(false);
        self.connect_label.set_label(if p["status"] == "connected" {
            "Disconnect"
        } else {
            "Connect"
        });
    }
}

pub fn page(hub: &Hub) -> adw::NavigationPage {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 24);

    let hero = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    let icon = gtk::Image::builder()
        .icon_name("computer-symbolic")
        .pixel_size(32)
        .width_request(64)
        .height_request(64)
        .build();
    let well = gtk::Box::builder()
        .valign(gtk::Align::Center)
        .css_classes(["well", "large"])
        .build();
    well.append(&icon);
    hero.append(&well);
    let hero_text = gtk::Box::new(gtk::Orientation::Vertical, 4);
    hero_text.set_valign(gtk::Align::Center);
    hero_text.set_hexpand(true);
    let name = label("", &["title-1"]);
    name.set_wrap(true);
    name.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    hero_text.append(&name);
    let state_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let dot = theme::dot(false);
    let state = label("", &["muted"]);
    state.set_ellipsize(gtk::pango::EllipsizeMode::End);
    state_box.append(&dot);
    state_box.append(&state);
    hero_text.append(&state_box);
    hero.append(&hero_text);
    column.append(&hero);

    let panel_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    panel_box.set_visible(false);
    column.append(&panel_box);
    let playing = now_playing::Panel::new();
    column.append(&playing.widget);
    let actions = gtk::Box::new(gtk::Orientation::Vertical, 24);
    column.append(&actions);

    let about_group = adw::PreferencesGroup::builder()
        .title("About this device")
        .build();
    let battery_bar = gtk::LevelBar::builder()
        .min_value(0.0)
        .max_value(1.0)
        .width_request(96)
        .valign(gtk::Align::Center)
        .build();
    let storage_bar = gtk::LevelBar::builder()
        .min_value(0.0)
        .max_value(1.0)
        .width_request(96)
        .valign(gtk::Align::Center)
        .build();
    // A fuller disk isn't "better": one colour for storage.
    for offset in ["low", "high", "full"] {
        storage_bar.remove_offset_value(Some(offset));
    }
    let fingerprint = adw::ActionRow::builder()
        .title("Key fingerprint")
        .subtitle_selectable(true)
        .css_classes(["fingerprint"])
        .build();
    let speed = adw::ActionRow::builder()
        .title("Test connection speed")
        .activatable(true)
        .build();
    speed.add_prefix(&{
        let w = theme::well("network-transmit-receive-symbolic", "teal");
        w.add_css_class("small");
        w
    });
    speed.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    speed.connect_activated(|_| {
        if let Some((hub, id, _)) = target() {
            act(
                json!({ "cmd": "start_speed_test", "device_id": id, "duration_secs": 5 }),
                Some("Testing the connection…"),
                &hub,
            );
        }
    });
    let about = About {
        status: value_row("Status"),
        system: value_row("System"),
        version: value_row("Link All"),
        address: value_row("Address"),
        battery: value_row("Battery"),
        battery_bar,
        storage: value_row("Storage"),
        storage_bar,
        seen: value_row("Last seen"),
        fingerprint,
        speed,
    };
    about.battery.0.add_suffix(&about.battery_bar);
    about.storage.0.add_suffix(&about.storage_bar);
    about.address.1.set_selectable(true);
    for (row, _) in [
        &about.status,
        &about.system,
        &about.version,
        &about.address,
        &about.battery,
        &about.storage,
        &about.seen,
    ] {
        about_group.add(row);
    }
    about_group.add(&about.fingerprint);
    about_group.add(&about.speed);
    column.append(&about_group);

    let manage = adw::PreferencesGroup::builder().title("Manage").build();
    let sync_row = adw::SwitchRow::builder()
        .title("Sync with this device")
        .build();
    let auto_row = adw::SwitchRow::builder()
        .title("Connect automatically")
        .subtitle("Reconnect whenever both devices are on the same network")
        .build();
    let (connect_row, connect_label) = action_row("Disconnect", false);
    let (rename_row, _) = action_row("Rename…", false);
    let (forget_row, _) = action_row("Forget this device…", true);
    manage.add(&sync_row);
    manage.add(&auto_row);
    manage.add(&connect_row);
    manage.add(&rename_row);
    manage.add(&forget_row);
    column.append(&manage);

    let scroll = super::page_column(&column);
    // Files dropped anywhere on the page go to this device.
    send::accept_drops(&scroll, hub, || target().map(|(_, id, _)| id));
    let (nav, _) = super::pushed_page("device", "Device", &scroll);

    let page = Rc::new(Page {
        hub: hub.clone(),
        nav: nav.clone(),
        id: RefCell::default(),
        well,
        icon,
        name,
        dot,
        state,
        panel: panel_box,
        panel_key: RefCell::default(),
        playing,
        actions,
        actions_key: RefCell::default(),
        about,
        manage,
        sync_row: sync_row.clone(),
        auto_row: auto_row.clone(),
        connect_label,
        quiet: Cell::new(false),
        styled: RefCell::default(),
    });

    sync_row.connect_active_notify(|r| {
        let Some(page) = current() else { return };
        if page.quiet.get() {
            return;
        }
        let cmd = if r.is_active() {
            "resume_sync_peer"
        } else {
            "pause_sync_peer"
        };
        let id = page.id.borrow().clone();
        act(json!({ "cmd": cmd, "device_id": id }), None, &page.hub);
    });
    auto_row.connect_active_notify(|r| {
        let Some(page) = current() else { return };
        if page.quiet.get() {
            return;
        }
        let id = page.id.borrow().clone();
        act(
            json!({ "cmd": "set_auto_connect", "device_id": id, "enabled": r.is_active() }),
            None,
            &page.hub,
        );
    });
    connect_row.connect_activated(|_| {
        let Some((hub, id, _)) = target() else { return };
        let connected = hub.peer(&id).is_some_and(|p| p["status"] == "connected");
        let cmd = if connected {
            "disconnect_peer"
        } else {
            "reconnect_peer"
        };
        act(json!({ "cmd": cmd, "device_id": id }), None, &hub);
    });
    rename_row.connect_activated(|_| {
        if let Some((hub, id, name)) = target() {
            rename(&hub, &id, &name);
        }
    });
    forget_row.connect_activated(|_| {
        if let Some((hub, id, name)) = target() {
            forget(&hub, &id, &name);
        }
    });

    // Live while it shows; brought up to date as it comes back into view.
    nav.connect_showing(|_| {
        if let Some(page) = current() {
            page.update();
        }
    });
    hub.on_status(|_| {
        if let Some(page) = current() {
            if page.nav.is_mapped() {
                page.update();
            }
        }
    });
    now_playing::watch(|| {
        if let Some(page) = current() {
            let id = page.id.borrow().clone();
            let linked = page
                .hub
                .peer(&id)
                .is_some_and(|p| p["trusted"] == true && p["status"] == "connected");
            page.update_playing(linked);
        }
    });
    PAGE.with(|p| *p.borrow_mut() = Some(page));
    nav
}

/// Asks a phone for a picture; it comes back and is pasted where you were typing.
fn capture(hub: &Hub, id: &str, mode: &str) {
    crate::agent::mark_capture_asked();
    act(
        json!({ "cmd": "capture_request", "target": id, "mode": mode }),
        Some("Check your phone and tap the notification"),
        hub,
    );
}

/// Rings a phone and keeps a dialog open with a Stop button; the phone also stops
/// by itself after a minute, or on "Found it".
fn find_phone(hub: &Hub, id: &str, name: &str) {
    let (hub, id, name) = (hub.clone(), id.to_string(), name.to_string());
    glib::spawn_future_local(async move {
        if let Err(e) = ipc::call(json!({ "cmd": "find_device", "target": id, "ring": true })).await
        {
            toast(&e.to_string());
            return;
        }
        let dialog = adw::AlertDialog::builder()
            .heading(format!("{name} is ringing"))
            .body("It rings at full volume for up to a minute.")
            .close_response("stop")
            .default_response("stop")
            .build();
        dialog.add_response("stop", "Stop ringing");
        dialog.set_response_appearance("stop", adw::ResponseAppearance::Suggested);
        dialog.connect_response(None, move |_, _| {
            let (hub, id) = (hub.clone(), id.clone());
            glib::spawn_future_local(async move {
                let _ =
                    ipc::call(json!({ "cmd": "find_device", "target": id, "ring": false })).await;
                hub.poke();
            });
        });
        dialog.present(window().as_ref());
    });
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
        .heading(format!("Open a link on {name}"))
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

fn rename(hub: &Hub, id: &str, name: &str) {
    let entry = adw::EntryRow::builder().title("Name").text(name).build();
    let list = gtk::ListBox::builder()
        .css_classes(["boxed-list"])
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.append(&entry);
    let dialog = adw::AlertDialog::builder()
        .heading("Rename device")
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
        .body("This computer stops trusting it. To use it again, pair again and compare the code.")
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("forget", "Forget")]);
    dialog.set_response_appearance("forget", adw::ResponseAppearance::Destructive);
    let (hub, id) = (hub.clone(), id.to_string());
    dialog.connect_response(Some("forget"), move |_, _| {
        act(
            json!({ "cmd": "forget_device", "device_id": id }),
            None,
            &hub,
        );
        if let Some(page) = current() {
            super::close(&page.nav);
        }
    });
    dialog.present(window().as_ref());
}
