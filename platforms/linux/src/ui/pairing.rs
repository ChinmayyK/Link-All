//! Pairing: a device asking to pair (confirm the PIN), this computer asking a
//! device (wait for it to accept), and a QR code for a phone to scan.

use super::{act, toast, window};
use crate::{hub::Hub, ipc};
use adw::prelude::*;
use gtk::{gdk, glib};
use serde_json::json;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Open "Pair with …?" prompts, by device id.
    static INCOMING: RefCell<HashMap<String, adw::AlertDialog>> = RefCell::new(HashMap::new());
    /// The device this computer asked to pair, its dialog and the label showing the code.
    static OUTGOING: RefCell<Option<(String, adw::AlertDialog, gtk::Label)>> = const { RefCell::new(None) };
    static QR: RefCell<Option<(adw::AlertDialog, usize)>> = const { RefCell::new(None) };
}

fn pin_label(pin: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(pin));
    label.add_css_class("title-1");
    label.add_css_class("numeric");
    label
}

fn present_window() -> Option<adw::ApplicationWindow> {
    let w = window()?;
    w.present();
    Some(w)
}

/// Follows pairing state in every status refresh.
pub fn watch(hub: &Hub) {
    hub.on_status(|hub| {
        for peer in hub.peers() {
            let id = peer["id"].as_str().unwrap_or_default().to_string();
            let name = peer["friendly_name"]
                .as_str()
                .unwrap_or("A device")
                .to_string();
            let trusted = peer["trusted"] == true;
            let asking = peer["pairing_requested"] == true && !trusted;
            let pin = peer["pairing_pin"].as_str().unwrap_or_default().to_string();

            let open = INCOMING.with(|m| m.borrow().contains_key(&id));
            let is_outgoing =
                OUTGOING.with(|o| o.borrow().as_ref().is_some_and(|(oid, ..)| *oid == id));
            if asking && !open && !is_outgoing && !pin.is_empty() {
                ask(hub, &id, &name, &pin);
            } else if !asking && open {
                if let Some(d) = INCOMING.with(|m| m.borrow_mut().remove(&id)) {
                    d.force_close();
                }
            }

            if is_outgoing {
                OUTGOING.with(|o| {
                    if let Some((_, _, label)) = o.borrow().as_ref() {
                        if !pin.is_empty() {
                            label.set_label(&pin);
                        }
                    }
                });
                let ended = match peer["pairing_outcome"].as_str() {
                    Some("declined") => Some(format!("{name} declined")),
                    Some("expired") => Some(format!("{name} didn't answer in time")),
                    Some("update_needed") => {
                        Some(format!("Update Link All on {name}, then try again"))
                    }
                    _ => None,
                };
                if trusted || ended.is_some() {
                    // Take it out first: closing runs the dialog's response handler, which looks here too.
                    if let Some((_, dialog, _)) = OUTGOING.with(|o| o.borrow_mut().take()) {
                        dialog.force_close();
                    }
                    toast(&ended.unwrap_or_else(|| format!("Paired with {name}")));
                }
            }
        }

        // The QR dialog closes itself once a new device is paired.
        let trusted_now = hub.peers().iter().filter(|p| p["trusted"] == true).count();
        let paired_new = QR.with(|q| {
            q.borrow()
                .as_ref()
                .is_some_and(|(_, before)| trusted_now > *before)
        });
        if paired_new {
            if let Some((dialog, _)) = QR.with(|q| q.borrow_mut().take()) {
                dialog.force_close();
            }
            toast("Paired");
        }
    });
}

fn ask(hub: &Hub, id: &str, name: &str, pin: &str) {
    // The window may be hidden in the tray: say so on the desktop too.
    if let Some(app) = gtk::gio::Application::default() {
        let n = gtk::gio::Notification::new(&format!("{name} wants to pair"));
        n.set_body(Some(&format!("Confirm the code {pin} in Link All")));
        n.set_priority(gtk::gio::NotificationPriority::High);
        n.set_default_action("app.show");
        app.send_notification(Some("pairing"), &n);
    }
    let Some(win) = present_window() else { return };
    let dialog = adw::AlertDialog::builder()
        .heading(format!("Pair with {name}?"))
        .body(format!("Pair only if {name} shows the same code."))
        .extra_child(&pin_label(pin))
        .close_response("decline")
        .default_response("accept")
        .build();
    dialog.add_responses(&[("decline", "Decline"), ("accept", "Pair")]);
    dialog.set_response_appearance("accept", adw::ResponseAppearance::Suggested);
    let (hub, device) = (hub.clone(), id.to_string());
    dialog.connect_response(None, move |_, response| {
        if let Some(app) = gtk::gio::Application::default() {
            app.withdraw_notification("pairing");
        }
        // Answered on the device's page already: that sent the answer.
        if INCOMING.with(|m| m.borrow_mut().remove(&device)).is_none() {
            return;
        }
        let accepted = response == "accept";
        act(
            json!({ "cmd": "respond_to_pairing", "device_id": device, "accepted": accepted }),
            None,
            &hub,
        );
    });
    INCOMING.with(|m| m.borrow_mut().insert(id.to_string(), dialog.clone()));
    dialog.present(Some(&win));
}

/// Answers `id`'s request to pair, from its page; closes the dialog asking the same.
pub fn respond(hub: &Hub, id: &str, accepted: bool) {
    if let Some(app) = gtk::gio::Application::default() {
        app.withdraw_notification("pairing");
    }
    if let Some(dialog) = INCOMING.with(|m| m.borrow_mut().remove(id)) {
        dialog.force_close();
    }
    act(
        json!({ "cmd": "respond_to_pairing", "device_id": id, "accepted": accepted }),
        None,
        hub,
    );
}

/// Withdraws this computer's request to pair with `id`.
pub fn cancel(hub: &Hub, id: &str) {
    if let Some((_, dialog, _)) = OUTGOING.with(|o| o.borrow_mut().take()) {
        dialog.force_close();
    }
    act(
        json!({ "cmd": "cancel_pairing_request", "device_id": id }),
        None,
        hub,
    );
}

/// Asks a nearby device to pair and waits for it to accept.
pub fn request(hub: &Hub, id: &str, name: &str) {
    let Some(win) = present_window() else { return };
    let label = pin_label("··· ···");
    let dialog = adw::AlertDialog::builder()
        .heading(format!("Accept on {name}"))
        .body(format!(
            "{name} will ask to pair. Accept there if it shows this code."
        ))
        .extra_child(&label)
        .close_response("cancel")
        .build();
    dialog.add_response("cancel", "Cancel");
    let (hub2, device) = (hub.clone(), id.to_string());
    dialog.connect_response(None, move |_, _| {
        let still_waiting = OUTGOING.with(|o| o.borrow_mut().take().is_some());
        if still_waiting {
            act(
                json!({ "cmd": "cancel_pairing_request", "device_id": device }),
                None,
                &hub2,
            );
        }
    });
    OUTGOING.with(|o| *o.borrow_mut() = Some((id.to_string(), dialog.clone(), label)));
    dialog.present(Some(&win));
    act(
        json!({ "cmd": "send_pairing_request", "device_id": id }),
        None,
        hub,
    );
}

/// A fresh pairing QR code for this computer, as a texture.
pub async fn qr(hub: &Hub) -> Option<gdk::MemoryTexture> {
    let token = match ipc::call(json!({ "cmd": "generate_qr_token" })).await {
        Ok(v) => v["token"].as_str().unwrap_or_default().to_string(),
        Err(e) => {
            toast(&e.to_string());
            return None;
        }
    };
    let status = hub.status.borrow().clone();
    let url = pairing_url(&[
        ("id", status["local_device_id"].as_str().unwrap_or_default()),
        (
            "name",
            status["device_name"]
                .as_str()
                .or(status["local_device_name"].as_str())
                .unwrap_or("Linux"),
        ),
        ("token", &token),
        ("ip", &local_address()),
        (
            "port",
            &status["port"].as_u64().unwrap_or(47823).to_string(),
        ),
        (
            "fingerprint",
            status["local_fingerprint"].as_str().unwrap_or_default(),
        ),
    ]);
    qr_texture(&url)
}

/// A QR code for the Devices page while nothing is paired yet. Tokens last ten
/// minutes, so the code is replaced well before then.
pub fn inline_qr(hub: &Hub) -> gtk::Picture {
    let picture = gtk::Picture::builder()
        .can_shrink(false)
        .width_request(184)
        .height_request(184)
        .css_classes(["thumb"])
        .build();
    let (hub, p) = (hub.clone(), picture.clone());
    glib::spawn_future_local(async move {
        loop {
            if p.is_mapped() || p.paintable().is_none() {
                if let Some(t) = qr(&hub).await {
                    p.set_paintable(Some(&t));
                }
            }
            glib::timeout_future_seconds(8 * 60).await;
        }
    });
    picture
}

/// Shows a QR code a phone running Link All can scan to pair.
pub fn show_qr(hub: &Hub) {
    let hub = hub.clone();
    glib::spawn_future_local(async move {
        let Some(texture) = qr(&hub).await else {
            return;
        };
        let picture = gtk::Picture::for_paintable(&texture);
        picture.set_size_request(240, 240);
        picture.set_can_shrink(false);
        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.set_halign(gtk::Align::Center);
        frame.append(&picture);
        let dialog = adw::AlertDialog::builder()
            .heading("Pair a Phone")
            .body("On the phone, open Link All, tap + and scan this code.")
            .extra_child(&frame)
            .close_response("close")
            .build();
        dialog.add_response("close", "Close");
        dialog.connect_response(None, |_, _| QR.with(|q| *q.borrow_mut() = None));
        let paired = hub.peers().iter().filter(|p| p["trusted"] == true).count();
        QR.with(|q| *q.borrow_mut() = Some((dialog.clone(), paired)));
        if let Some(win) = present_window() {
            dialog.present(Some(&win));
        }
    });
}

fn pairing_url(params: &[(&str, &str)]) -> String {
    let query: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{k}={}", glib::Uri::escape_string(v, None, false)))
        .collect();
    format!("linkall://pair?{}", query.join("&"))
}

/// The address phones on the LAN reach this computer at. Connecting a UDP
/// socket sends nothing; it only picks the outgoing interface.
pub fn local_address() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.0.2.1:9").map(|_| s))
        .and_then(|s| s.local_addr())
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}

pub(crate) fn qr_texture(text: &str) -> Option<gdk::MemoryTexture> {
    let code = qrcode::QrCode::with_error_correction_level(text, qrcode::EcLevel::M).ok()?;
    let modules = code.width();
    let colors = code.to_colors();
    const SCALE: usize = 8;
    const QUIET: usize = 4;
    let side = (modules + QUIET * 2) * SCALE;
    let mut pixels = vec![255u8; side * side * 4];
    for (i, color) in colors.iter().enumerate() {
        if *color != qrcode::Color::Dark {
            continue;
        }
        let (mx, my) = (i % modules + QUIET, i / modules + QUIET);
        for y in my * SCALE..(my + 1) * SCALE {
            for x in mx * SCALE..(mx + 1) * SCALE {
                let p = (y * side + x) * 4;
                pixels[p..p + 3].copy_from_slice(&[0, 0, 0]);
            }
        }
    }
    Some(gdk::MemoryTexture::new(
        side as i32,
        side as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from_owned(pixels),
        side * 4,
    ))
}

/// Connects to a device by IP address or host name, for networks where discovery is blocked.
pub fn add_by_address(hub: &Hub) {
    let entry = adw::EntryRow::builder()
        .title("IP address or host name")
        .build();
    let list = gtk::ListBox::builder()
        .css_classes(["boxed-list"])
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.append(&entry);
    let dialog = adw::AlertDialog::builder()
        .heading("Add by IP Address")
        .body("For a device on another part of your network. Link All must be open on it.")
        .extra_child(&list)
        .close_response("cancel")
        .default_response("connect")
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("connect", "Connect")]);
    dialog.set_response_appearance("connect", adw::ResponseAppearance::Suggested);
    let hub = hub.clone();
    dialog.connect_response(Some("connect"), move |_, _| {
        let text = entry.text().trim().to_string();
        let (host, port) = match text.rsplit_once(':') {
            Some((h, p)) if p.parse::<u16>().is_ok() && !h.contains(':') => {
                (h.to_string(), p.parse::<u16>().ok())
            }
            _ => (text.clone(), None),
        };
        if host.is_empty() {
            return toast("Enter an address");
        }
        act(
            json!({ "cmd": "connect_manual", "host": host, "port": port }),
            Some("Connecting…"),
            &hub,
        );
    });
    dialog.present(window().as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_url_escapes_every_value() {
        let url = pairing_url(&[
            ("id", "abc"),
            ("name", "Chinmay's PC & laptop"),
            ("token", "a+b/c="),
        ]);
        assert!(url.starts_with("linkall://pair?id=abc&name="));
        assert!(!url.contains(' ') && !url.contains("& ") && !url.contains("+b"));
        let name = url
            .split("name=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap();
        assert_eq!(
            glib::Uri::unescape_string(name, None::<&str>).unwrap(),
            "Chinmay's PC & laptop"
        );
    }

    #[test]
    fn qr_code_is_square_with_a_quiet_zone() {
        let texture = qr_texture("linkall://pair?id=x&token=y").expect("QR");
        assert_eq!(texture.width(), texture.height());
        assert!(texture.width() >= (21 + 8) * 8);
    }
}
