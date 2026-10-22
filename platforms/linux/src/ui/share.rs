//! Keyboard & Mouse: use this computer's keyboard and mouse on the one next
//! to it. Whether it is on, where each computer sits, what is happening now,
//! and whether each linked computer is ready.

use super::page_column;
use crate::{hub::Hub, ipc, prefs};
use adw::prelude::*;
use gtk::glib;
use serde_json::{json, Value};
use std::time::Duration;

pub fn page(hub: &Hub) -> gtk::ScrolledWindow {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 18);

    let heading = gtk::Label::builder()
        .label("Keyboard & Mouse")
        .xalign(0.0)
        .css_classes(["title-1"])
        .build();
    let sub = gtk::Label::builder()
        .label("Use this computer's keyboard and mouse on the one next to it")
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build();
    column.append(&heading);
    column.append(&sub);

    // What is happening now.
    let status_group = adw::PreferencesGroup::new();
    let status = adw::ActionRow::builder().title("Off").build();
    let end_button = gtk::Button::builder()
        .label("Take It Back")
        .valign(gtk::Align::Center)
        .visible(false)
        .build();
    end_button.connect_clicked(|_| {
        glib::spawn_future_local(async {
            let _ = ipc::call(json!({ "cmd": "edge_end" })).await;
        });
    });
    status.add_suffix(&end_button);
    status_group.add(&status);
    column.append(&status_group);

    // The switch.
    let switch_group = adw::PreferencesGroup::new();
    let on = adw::SwitchRow::builder()
        .title("Share This Keyboard and Mouse")
        .subtitle(
            "Push the pointer against a screen edge for about a quarter of a second to move to \
             the computer on that side. Press Ctrl+Alt+Esc to bring it back at any time. \
             Works on X11 sessions, not Wayland.",
        )
        .active(prefs::get_bool(crate::edge::ENABLED, false))
        .build();
    on.connect_active_notify(|r| {
        prefs::set(crate::edge::ENABLED, r.is_active().into());
        crate::edge::reload();
    });
    switch_group.add(&on);
    column.append(&switch_group);

    // Where each computer sits.
    let layout = adw::PreferencesGroup::builder()
        .title("Where Each Computer Sits")
        .build();
    let combos: Vec<adw::ComboRow> = (0..4)
        .map(|edge| {
            let row = adw::ComboRow::builder()
                .title(format!("On the {}", crate::edge::EDGE_NAMES[edge]))
                .build();
            layout.add(&row);
            row
        })
        .collect();
    column.append(&layout);

    // Is each linked computer ready?
    let readiness = adw::PreferencesGroup::builder()
        .title("Ready to Share?")
        .visible(false)
        .build();
    column.append(&readiness);

    // The choices follow who is paired, so they are filled in as that changes.
    let ids: std::rc::Rc<std::cell::RefCell<Vec<Vec<String>>>> = Default::default();
    let fill = {
        let (combos, ids) = (combos.clone(), ids.clone());
        move |hub: &Hub| {
            let computers = shareable(hub);
            let mut all = ids.borrow_mut();
            all.clear();
            for (edge, row) in combos.iter().enumerate() {
                let names = gtk::StringList::new(&["Nobody"]);
                for (_, name) in &computers {
                    names.append(name);
                }
                let saved = prefs::get_string(&crate::edge::link_key(edge)).unwrap_or_default();
                row.set_model(Some(&names));
                row.set_selected(
                    computers
                        .iter()
                        .position(|(id, _)| *id == saved)
                        .map_or(0, |i| i as u32 + 1),
                );
                all.push(computers.iter().map(|(id, _)| id.clone()).collect());
            }
        }
    };
    fill(hub);
    for (edge, row) in combos.iter().enumerate() {
        let ids = ids.clone();
        row.connect_selected_notify(move |r| {
            let i = r.selected() as usize;
            let id = if i == 0 {
                String::new()
            } else {
                ids.borrow()
                    .get(edge)
                    .and_then(|v| v.get(i - 1))
                    .cloned()
                    .unwrap_or_default()
            };
            // Filling the boxes selects too; only a real change is saved.
            if prefs::get_string(&crate::edge::link_key(edge)).unwrap_or_default() != id {
                prefs::set(&crate::edge::link_key(edge), id.into());
                crate::edge::reload();
            }
        });
    }
    {
        let fill = fill.clone();
        hub.on_status(move |hub| fill(hub));
    }

    // What is happening now, and what to fix, looked at once a second.
    let rows: std::rc::Rc<std::cell::RefCell<Vec<adw::ActionRow>>> = Default::default();
    {
        let hub = hub.clone();
        let (status, end_button, readiness, rows) =
            (status.clone(), end_button.clone(), readiness.clone(), rows.clone());
        glib::timeout_add_local(Duration::from_secs(1), move || {
            let (status, end_button, readiness, rows, hub) = (
                status.clone(),
                end_button.clone(),
                readiness.clone(),
                rows.clone(),
                hub.clone(),
            );
            glib::spawn_future_local(async move {
                let reply = ipc::call(json!({ "cmd": "edge_status", "peek": true })).await.ok();
                refresh(&hub, reply.as_ref(), &status, &end_button, &readiness, &rows);
            });
            glib::ControlFlow::Continue
        });
    }
    page_column(&column)
}

/// Paired computers: a phone can't take a keyboard and mouse.
fn shareable(hub: &Hub) -> Vec<(String, String)> {
    hub.peers()
        .into_iter()
        .filter(|p| {
            p["trusted"] == true
                && !p["platform"].as_str().unwrap_or("").to_lowercase().contains("android")
        })
        .filter_map(|p| {
            Some((
                p["id"].as_str()?.to_string(),
                p["friendly_name"].as_str().unwrap_or("Computer").to_string(),
            ))
        })
        .collect()
}

fn refresh(
    hub: &Hub,
    reply: Option<&Value>,
    status: &adw::ActionRow,
    end_button: &gtk::Button,
    readiness: &adw::PreferencesGroup,
    rows: &std::rc::Rc<std::cell::RefCell<Vec<adw::ActionRow>>>,
) {
    let name_of = |id: &str| {
        hub.peer(id)
            .and_then(|p| p["friendly_name"].as_str().map(str::to_string))
            .unwrap_or_else(|| "another computer".to_string())
    };
    let get = |key: &str| reply.map(|r| &r[key]);
    let hosting = get("hosting").is_some_and(|v| v == true);
    let controlled = get("controlled").is_some_and(|v| v == true);
    let peer = get("peer").and_then(Value::as_str).map(name_of);
    let enabled = prefs::get_bool(crate::edge::ENABLED, false);

    end_button.set_visible(hosting);
    if hosting {
        status.set_title(&format!("Controlling {}", peer.unwrap_or_default()));
        status.set_subtitle(
            "This computer's keyboard and mouse are on that computer. Push the pointer back \
             past its edge, or press Ctrl+Alt+Esc.",
        );
    } else if controlled {
        status.set_title(&format!("Being controlled by {}", peer.unwrap_or_default()));
        status.set_subtitle("That computer's keyboard and mouse are driving this one.");
    } else if enabled {
        status.set_title("Ready");
        status.set_subtitle("Push the pointer against a screen edge that has a computer next to it.");
    } else {
        status.set_title("Off");
        status.set_subtitle("Turn on sharing to move between computers with one keyboard and mouse.");
    }
    if let Some(note) = crate::edge::take_note() {
        super::toast(&note);
    }

    // One row per linked computer: ready, or what to fix.
    for old in rows.borrow_mut().drain(..) {
        readiness.remove(&old);
    }
    let refused_target = get("refused_target").and_then(Value::as_str);
    let refused_reason = get("refused_reason").and_then(Value::as_str);
    let mut shown = false;
    for edge in 0..4 {
        let Some(id) = prefs::get_string(&crate::edge::link_key(edge)).filter(|s| !s.is_empty())
        else {
            continue;
        };
        let peer = hub.peer(&id);
        let (ok, verdict) = match &peer {
            None => (false, "Not paired any more. Choose another computer.".to_string()),
            Some(p) if p["trusted"] != true => (false, "Not paired yet.".to_string()),
            Some(p) if p["status"] != "connected" => (
                false,
                "Not connected right now. Open Link All on it, on the same Wi-Fi.".to_string(),
            ),
            Some(_) if refused_target == Some(id.as_str()) && refused_reason.is_some() => {
                (false, refused_reason.unwrap_or_default().to_string())
            }
            Some(_) => (
                true,
                "Connected. If it does not take over, turn on Remote control in Link All on that computer."
                    .to_string(),
            ),
        };
        let name = peer
            .as_ref()
            .and_then(|p| p["friendly_name"].as_str())
            .unwrap_or("A computer");
        let row = adw::ActionRow::builder()
            .title(format!("{name} ({})", crate::edge::EDGE_NAMES[edge]))
            .subtitle(verdict)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name(if ok {
            "emblem-ok-symbolic"
        } else {
            "dialog-warning-symbolic"
        }));
        readiness.add(&row);
        rows.borrow_mut().push(row);
        shown = true;
    }
    readiness.set_visible(enabled && shown);
}
