//! Now Playing: what another device is playing, with its cover and controls,
//! at the top of the Devices page while something plays.

use crate::ipc;
use adw::prelude::*;
use base64::Engine as _;
use gtk::{gdk, glib};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::rc::Rc;

fn clock(ms: u64) -> String {
    format!("{}:{:02}", ms / 60_000, (ms / 1000) % 60)
}

fn texture_of(item: &Value) -> Option<gdk::Texture> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(item["art_base64"].as_str()?)
        .ok()?;
    gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)).ok()
}

fn control(icon: &str, tip: &str, target: String, action: &'static str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.set_tooltip_text(Some(tip));
    button.add_css_class("flat");
    button.add_css_class("circular");
    button.connect_clicked(move |_| {
        let target = target.clone();
        glib::spawn_future_local(async move {
            let _ = ipc::call(
                json!({ "cmd": "media_command", "target": target, "action": action, "value": 0 }),
            )
            .await;
        });
    });
    button
}

fn card_for(item: &Value) -> gtk::Box {
    let device = item["device_id"].as_str().unwrap_or_default().to_string();
    let row = gtk::Box::builder()
        .spacing(14)
        .css_classes(["card"])
        .margin_top(2)
        .margin_bottom(2)
        .build();
    row.set_margin_start(2);
    row.set_margin_end(2);

    let cover = gtk::Picture::new();
    cover.set_size_request(72, 72);
    cover.set_content_fit(gtk::ContentFit::Cover);
    cover.add_css_class("card");
    cover.set_margin_start(10);
    cover.set_margin_top(10);
    cover.set_margin_bottom(10);
    match texture_of(item) {
        Some(texture) => cover.set_paintable(Some(&texture)),
        None => cover.set_paintable(None::<&gdk::Paintable>),
    }
    row.append(&cover);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 4);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let label = |s: &str, classes: &[&str]| {
        gtk::Label::builder()
            .label(s)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(classes.iter().map(|c| c.to_string()).collect::<Vec<_>>())
            .build()
    };
    let artist = item["artist"].as_str().unwrap_or_default();
    text.append(&label(
        item["title"].as_str().unwrap_or_default(),
        &["heading"],
    ));
    text.append(&label(
        if artist.is_empty() {
            item["album"].as_str().unwrap_or_default()
        } else {
            artist
        },
        &["dim-label"],
    ));
    text.append(&label(
        &format!(
            "{} · on {} · {} / {}",
            item["app"]
                .as_str()
                .filter(|a| !a.is_empty())
                .unwrap_or("Playing"),
            item["device_name"].as_str().unwrap_or("a device"),
            clock(item["position_ms"].as_u64().unwrap_or(0)),
            clock(item["duration_ms"].as_u64().unwrap_or(0)),
        ),
        &["caption", "dim-label"],
    ));
    let duration = item["duration_ms"].as_u64().unwrap_or(0);
    if duration > 0 {
        let scale =
            gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, duration as f64, 1000.0);
        scale.set_draw_value(false);
        scale.set_value(item["position_ms"].as_u64().unwrap_or(0) as f64);
        let target = device.clone();
        scale.connect_change_value(move |_, _, value| {
            let (target, to) = (target.clone(), value.max(0.0) as i64);
            glib::spawn_future_local(async move {
                let _ = ipc::call(json!({ "cmd": "media_command", "target": target, "action": "seek", "value": to })).await;
            });
            glib::Propagation::Proceed
        });
        text.append(&scale);
    }
    row.append(&text);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    buttons.set_valign(gtk::Align::Center);
    buttons.set_margin_end(10);
    buttons.append(&control(
        "media-skip-backward-symbolic",
        "Previous",
        device.clone(),
        "previous",
    ));
    let playing = item["playing"].as_bool().unwrap_or(false);
    buttons.append(&control(
        if playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        },
        if playing { "Pause" } else { "Play" },
        device.clone(),
        "play_pause",
    ));
    buttons.append(&control(
        "media-skip-forward-symbolic",
        "Next",
        device,
        "next",
    ));
    row.append(&buttons);
    row
}

/// The card, empty and hidden until a paired device plays something.
pub fn card() -> gtk::Box {
    let holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
    holder.set_visible(false);
    let shown = Rc::new(RefCell::new(String::new()));
    let refresh = {
        let holder = holder.clone();
        let shown = shown.clone();
        move || {
            let (holder, shown) = (holder.clone(), shown.clone());
            glib::spawn_future_local(async move {
                let list = ipc::call(json!({ "cmd": "now_playing_list" }))
                    .await
                    .ok()
                    .and_then(|v| v.as_array().cloned())
                    .unwrap_or_default();
                let key = list
                    .first()
                    .map(|i| {
                        format!(
                            "{}{}{}{}",
                            i["device_id"],
                            i["title"],
                            i["playing"],
                            i["position_ms"].as_u64().unwrap_or(0) / 2000
                        )
                    })
                    .unwrap_or_default();
                if *shown.borrow() == key {
                    return;
                }
                *shown.borrow_mut() = key;
                while let Some(child) = holder.first_child() {
                    holder.remove(&child);
                }
                match list.first() {
                    Some(item) => {
                        holder.append(&card_for(item));
                        holder.set_visible(true);
                    }
                    None => holder.set_visible(false),
                }
            });
        }
    };
    refresh();
    glib::timeout_add_seconds_local(2, move || {
        refresh();
        glib::ControlFlow::Continue
    });
    holder
}
