//! Now Playing: what a paired device plays, with its cover and controls. A
//! small player on the device's card on Home, the full one on its page. One
//! poll feeds both.

use super::name_button;
use crate::ipc;
use adw::prelude::*;
use base64::Engine as _;
use gtk::{gdk, glib};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

thread_local! {
    static ITEMS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static SHOWN: RefCell<String> = const { RefCell::new(String::new()) };
    static WATCHERS: RefCell<Vec<Rc<dyn Fn()>>> = const { RefCell::new(Vec::new()) };
    /// Covers by track, so a refresh doesn't decode the same picture again.
    static COVERS: RefCell<HashMap<String, gdk::Texture>> = RefCell::new(HashMap::new());
    static POLLING: Cell<bool> = const { Cell::new(false) };
}

fn clock(ms: u64) -> String {
    format!("{}:{:02}", ms / 60_000, (ms / 1000) % 60)
}

fn track_key(item: &Value) -> String {
    format!(
        "{}|{}|{}|{}",
        item["device_id"], item["title"], item["artist"], item["album"]
    )
}

/// Calls `f` after each change to what the devices play. The first watcher
/// starts the poll.
pub fn watch(f: impl Fn() + 'static) {
    WATCHERS.with(|w| w.borrow_mut().push(Rc::new(f)));
    if !POLLING.replace(true) {
        poll();
        glib::timeout_add_seconds_local(2, || {
            poll();
            glib::ControlFlow::Continue
        });
    }
}

/// What `device` plays now, if anything.
pub fn playing_on(device: &str) -> Option<Value> {
    ITEMS.with(|items| {
        items
            .borrow()
            .iter()
            .find(|i| i["device_id"] == device)
            .cloned()
    })
}

fn poll() {
    // Nothing shows it while the window is hidden in the tray.
    if super::window().is_some_and(|w| !w.is_visible()) {
        return;
    }
    glib::spawn_future_local(async {
        let list: Vec<Value> = ipc::call(json!({ "cmd": "now_playing_list" }))
            .await
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter(|i| i["title"].as_str().is_some_and(|t| !t.is_empty()))
            .collect();
        let key: String = list
            .iter()
            .map(|i| {
                format!(
                    "{}|{}|{}|{};",
                    track_key(i),
                    i["playing"],
                    i["position_ms"].as_u64().unwrap_or(0) / 1000,
                    i["duration_ms"]
                )
            })
            .collect();
        if SHOWN.with(|s| *s.borrow() == key) {
            return;
        }
        SHOWN.with(|s| *s.borrow_mut() = key);
        let playing: Vec<String> = list.iter().map(track_key).collect();
        COVERS.with(|c| c.borrow_mut().retain(|k, _| playing.contains(k)));
        ITEMS.with(|items| *items.borrow_mut() = list);
        for f in WATCHERS.with(|w| w.borrow().clone()) {
            f();
        }
    });
}

/// Looks again shortly, so a press shows without waiting for the next poll.
fn poll_soon() {
    glib::timeout_add_local_once(Duration::from_millis(450), poll);
}

fn command(device: &str, action: &'static str, value: i64) {
    let device = device.to_string();
    glib::spawn_future_local(async move {
        let req =
            json!({ "cmd": "media_command", "target": device, "action": action, "value": value });
        if let Err(e) = ipc::call(req).await {
            super::toast(&e.to_string());
        }
        poll_soon();
    });
}

fn cover_of(item: &Value) -> Option<gdk::Texture> {
    let key = track_key(item);
    if let Some(t) = COVERS.with(|c| c.borrow().get(&key).cloned()) {
        return Some(t);
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(item["art_base64"].as_str()?)
        .ok()?;
    let texture = gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)).ok()?;
    COVERS.with(|c| c.borrow_mut().insert(key, texture.clone()));
    Some(texture)
}

/// The cover, or a note on a tile when there is none.
struct Cover {
    stack: gtk::Stack,
    picture: gtk::Picture,
}

impl Cover {
    fn new(size: i32) -> Cover {
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .width_request(size)
            .height_request(size)
            .overflow(gtk::Overflow::Hidden)
            .css_classes(["cover"])
            .build();
        let note = gtk::Image::builder()
            .icon_name("audio-x-generic-symbolic")
            .pixel_size(size / 2)
            .width_request(size)
            .height_request(size)
            .build();
        let placeholder = gtk::Box::builder()
            .css_classes(["well", "teal", "cover"])
            .build();
        placeholder.append(&note);
        let stack = gtk::Stack::builder().valign(gtk::Align::Center).build();
        stack.add_named(&picture, Some("art"));
        stack.add_named(&placeholder, Some("none"));
        Cover { stack, picture }
    }

    fn show(&self, item: &Value) {
        match cover_of(item) {
            Some(texture) => {
                self.picture.set_paintable(Some(&texture));
                self.stack.set_visible_child_name("art");
            }
            None => self.stack.set_visible_child_name("none"),
        }
    }
}

fn set_play_button(button: &gtk::Button, playing: bool) {
    button.set_icon_name(if playing {
        "media-playback-pause-symbolic"
    } else {
        "media-playback-start-symbolic"
    });
    name_button(button, if playing { "Pause" } else { "Play" });
}

/// Flips the button at once on a press; the next poll confirms it.
fn play_pause(button: &gtk::Button, device: &RefCell<String>) {
    let playing = button.icon_name().is_some_and(|i| i.contains("pause"));
    set_play_button(button, !playing);
    command(&device.borrow(), "play_pause", 0);
}

/// One line on a device's card: the cover, the track and play/pause.
pub struct Mini {
    pub widget: gtk::Box,
    cover: Cover,
    title: gtk::Label,
    detail: gtk::Label,
    play: gtk::Button,
    device: RefCell<String>,
}

impl Mini {
    pub fn new() -> Rc<Mini> {
        let widget = gtk::Box::builder()
            .spacing(10)
            .css_classes(["now-playing-mini"])
            .visible(false)
            .build();
        let cover = Cover::new(40);
        widget.append(&cover.stack);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        let line = |classes: &[&str]| {
            gtk::Label::builder()
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(classes.to_vec())
                .build()
        };
        let title = line(&["caption-heading"]);
        let detail = line(&["caption", "dim-label"]);
        text.append(&title);
        text.append(&detail);
        widget.append(&text);
        let play = gtk::Button::builder()
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        set_play_button(&play, false);
        widget.append(&play);
        let mini = Rc::new(Mini {
            widget,
            cover,
            title,
            detail,
            play,
            device: RefCell::default(),
        });
        let weak = Rc::downgrade(&mini);
        mini.play.connect_clicked(move |b| {
            if let Some(mini) = weak.upgrade() {
                play_pause(b, &mini.device);
            }
        });
        mini
    }

    pub fn update(&self, item: Option<&Value>) {
        let Some(item) = item else {
            self.widget.set_visible(false);
            return;
        };
        *self.device.borrow_mut() = item["device_id"].as_str().unwrap_or_default().to_string();
        self.cover.show(item);
        self.title
            .set_label(item["title"].as_str().unwrap_or_default());
        let detail: Vec<&str> = [item["artist"].as_str(), item["app"].as_str()]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect();
        self.detail.set_label(&detail.join(" · "));
        set_play_button(&self.play, item["playing"] == true);
        self.widget.set_visible(true);
    }
}

/// The full player for a device's page: cover, track, a scrub bar and the controls.
pub struct Panel {
    pub widget: gtk::Box,
    cover: Cover,
    title: gtk::Label,
    artist: gtk::Label,
    app: gtk::Label,
    scale: gtk::Scale,
    times: gtk::Box,
    position: gtk::Label,
    duration: gtk::Label,
    play: gtk::Button,
    device: RefCell<String>,
    /// When the user last moved the scrub bar; polls leave it alone for a moment.
    scrubbed: Cell<Option<Instant>>,
    seek: RefCell<Option<glib::SourceId>>,
}

impl Panel {
    pub fn new() -> Rc<Panel> {
        let widget = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .css_classes(["card", "la", "now-playing"])
            .visible(false)
            .build();
        widget.append(&super::label("NOW PLAYING", &["eyebrow"]));
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        let cover = Cover::new(72);
        top.append(&cover.stack);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        let title = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .lines(2)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["title-4"])
            .build();
        let line = |classes: &[&str]| {
            gtk::Label::builder()
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(classes.to_vec())
                .build()
        };
        let artist = line(&["dim-label"]);
        let app = line(&["caption", "dim-label"]);
        text.append(&title);
        text.append(&artist);
        text.append(&app);
        top.append(&text);
        widget.append(&top);

        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1000.0);
        scale.set_draw_value(false);
        scale.update_property(&[gtk::accessible::Property::Label("Position")]);
        widget.append(&scale);
        let times = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let position = line(&["caption", "numeric", "dim-label"]);
        position.set_hexpand(true);
        let duration = line(&["caption", "numeric", "dim-label"]);
        times.append(&position);
        times.append(&duration);
        widget.append(&times);

        let controls = gtk::Box::builder()
            .spacing(18)
            .halign(gtk::Align::Center)
            .build();
        let previous = gtk::Button::builder()
            .icon_name("media-skip-backward-symbolic")
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        name_button(&previous, "Previous");
        let play = gtk::Button::builder()
            .valign(gtk::Align::Center)
            .css_classes(["circular", "suggested-action", "play-button"])
            .build();
        set_play_button(&play, false);
        let next = gtk::Button::builder()
            .icon_name("media-skip-forward-symbolic")
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        name_button(&next, "Next");
        controls.append(&previous);
        controls.append(&play);
        controls.append(&next);
        widget.append(&controls);

        let panel = Rc::new(Panel {
            widget,
            cover,
            title,
            artist,
            app,
            scale,
            times,
            position,
            duration,
            play,
            device: RefCell::default(),
            scrubbed: Cell::new(None),
            seek: RefCell::default(),
        });
        for (button, action) in [(&previous, "previous"), (&next, "next")] {
            let weak = Rc::downgrade(&panel);
            button.connect_clicked(move |_| {
                if let Some(panel) = weak.upgrade() {
                    command(&panel.device.borrow(), action, 0);
                }
            });
        }
        {
            let weak = Rc::downgrade(&panel);
            panel.play.connect_clicked(move |b| {
                if let Some(panel) = weak.upgrade() {
                    play_pause(b, &panel.device);
                }
            });
        }
        // Seek once the bar rests, not on every step of a drag.
        let weak = Rc::downgrade(&panel);
        panel.scale.connect_change_value(move |_, _, value| {
            let Some(panel) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            panel.scrubbed.set(Some(Instant::now()));
            panel.position.set_label(&clock(value.max(0.0) as u64));
            if let Some(pending) = panel.seek.borrow_mut().take() {
                pending.remove();
            }
            let weak = Rc::downgrade(&panel);
            let to = value.max(0.0) as i64;
            *panel.seek.borrow_mut() = Some(glib::timeout_add_local_once(
                Duration::from_millis(250),
                move || {
                    if let Some(panel) = weak.upgrade() {
                        panel.seek.borrow_mut().take();
                        command(&panel.device.borrow(), "seek", to);
                    }
                },
            ));
            glib::Propagation::Proceed
        });
        panel
    }

    pub fn update(&self, item: Option<&Value>) {
        let Some(item) = item else {
            self.widget.set_visible(false);
            return;
        };
        *self.device.borrow_mut() = item["device_id"].as_str().unwrap_or_default().to_string();
        self.cover.show(item);
        self.title
            .set_label(item["title"].as_str().unwrap_or_default());
        let artist = item["artist"]
            .as_str()
            .filter(|a| !a.is_empty())
            .or(item["album"].as_str())
            .unwrap_or_default();
        self.artist.set_label(artist);
        self.artist.set_visible(!artist.is_empty());
        let app = item["app"].as_str().unwrap_or_default();
        self.app.set_label(app);
        self.app.set_visible(!app.is_empty());
        set_play_button(&self.play, item["playing"] == true);

        let duration = item["duration_ms"].as_u64().unwrap_or(0);
        self.scale.set_visible(duration > 0);
        self.times.set_visible(duration > 0);
        let resting = self
            .scrubbed
            .get()
            .is_none_or(|t| t.elapsed() > Duration::from_millis(2500));
        if duration > 0 && resting {
            let position = item["position_ms"].as_u64().unwrap_or(0).min(duration);
            self.scale.set_range(0.0, duration as f64);
            self.scale.set_value(position as f64);
            self.position.set_label(&clock(position));
        }
        self.duration.set_label(&clock(duration));
        self.widget.set_visible(true);
    }
}
