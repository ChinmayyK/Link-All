//! Guest Drop and click-to-call: two things to do from this computer that aren't
//! about one paired device.

use super::{toast, window};
use crate::ipc;
use adw::prelude::*;
use gtk::glib;
use serde_json::{json, Value};

/// "Receive from a Guest": a link, QR code and PIN that someone who isn't
/// paired can use to send a file here. It ends after five minutes or on Stop.
pub fn receive_from_guest() {
    glib::spawn_future_local(async move {
        let session = match ipc::call(json!({ "cmd": "guest_drop_start" })).await {
            Ok(v) => v,
            Err(e) => {
                toast(&e.to_string());
                return;
            }
        };
        let url = session["url"].as_str().unwrap_or_default().to_string();
        let pin = session["pin"].as_str().unwrap_or_default().to_string();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_halign(gtk::Align::Center);
        if let Some(texture) = super::pairing::qr_texture(&url) {
            let picture = gtk::Picture::for_paintable(&texture);
            picture.set_size_request(200, 200);
            content.append(&picture);
        }
        content.append(
            &gtk::Label::builder()
                .label(&url)
                .selectable(true)
                .wrap(true)
                .max_width_chars(40)
                .css_classes(["monospace", "dim-label"])
                .build(),
        );
        content.append(
            &gtk::Label::builder()
                .label(format!("PIN  {pin}"))
                .selectable(true)
                .css_classes(["title-1"])
                .build(),
        );
        content.append(
            &gtk::Label::builder()
                .label("Works for 5 minutes. Files go to Downloads › Link All › Guest. The page is not encrypted: use it on a network you trust.")
                .wrap(true)
                .max_width_chars(44)
                .css_classes(["caption", "dim-label"])
                .build(),
        );
        let dialog = adw::AlertDialog::builder()
            .heading("Receive from a Guest")
            .body("They scan this, type the PIN and send a file. Nothing is paired or saved.")
            .extra_child(&content)
            .close_response("stop")
            .default_response("stop")
            .build();
        dialog.add_responses(&[("copy", "Copy Link"), ("stop", "Stop")]);
        dialog.set_response_appearance("copy", adw::ResponseAppearance::Suggested);
        dialog.connect_response(None, move |dialog, response| {
            if response == "copy" {
                if let Some(display) = gtk::gdk::Display::default() {
                    display.clipboard().set_text(&url);
                    toast("Link copied");
                }
                // Keep the dialog up: present again after it closes.
                let again = dialog.clone();
                glib::idle_add_local_once(move || again.present(window().as_ref()));
                return;
            }
            glib::spawn_future_local(async {
                let _ = ipc::call(json!({ "cmd": "guest_drop_stop" })).await;
            });
        });
        dialog.present(window().as_ref());
    });
}

/// The first connected phone that can be asked, as (id, name).
async fn pick_phone() -> Option<(String, String)> {
    let choices = ipc::call(json!({ "cmd": "device_choices", "purpose": "find" }))
        .await
        .ok()?;
    choices["choices"].as_array()?.iter().find_map(|c| {
        (c["available"] == true).then(|| {
            (
                c["id"].as_str().unwrap_or_default().to_string(),
                c["name"].as_str().unwrap_or("your phone").to_string(),
            )
        })
    })
}

/// Offers `number` to a paired phone, which shows a notification and opens its
/// dialer when its owner taps it. Without a number, asks for one first.
pub fn call_from_phone(number: Option<String>) {
    if let Some(number) = number {
        send_number(number);
        return;
    }
    let entry = adw::EntryRow::builder().title("Phone number").build();
    let list = gtk::ListBox::builder()
        .css_classes(["boxed-list"])
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.append(&entry);
    let dialog = adw::AlertDialog::builder()
        .heading("Call from Your Phone")
        .body("Your phone shows a notification. Tap it to open the dialer.")
        .extra_child(&list)
        .close_response("cancel")
        .default_response("send")
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("send", "Send to Phone")]);
    dialog.set_response_appearance("send", adw::ResponseAppearance::Suggested);
    dialog.connect_response(Some("send"), move |_, _| {
        let number = entry.text().trim().to_string();
        if !number.is_empty() {
            send_number(number);
        }
    });
    dialog.present(window().as_ref());
}

fn send_number(number: String) {
    glib::spawn_future_local(async move {
        let Some((id, name)) = pick_phone().await else {
            toast("Connect a paired phone that runs Link All 2.1 or later");
            return;
        };
        dial(&id, &name, &number).await;
    });
}

/// Asks for a number and offers it to the phone `id`, from that phone's page.
pub fn call_on(id: &str, name: &str) {
    let entry = adw::EntryRow::builder().title("Phone number").build();
    let list = gtk::ListBox::builder()
        .css_classes(["boxed-list"])
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.append(&entry);
    let dialog = adw::AlertDialog::builder()
        .heading(format!("Call from {name}"))
        .body(format!(
            "{name} shows a notification. Tap it to open the dialer."
        ))
        .extra_child(&list)
        .close_response("cancel")
        .default_response("send")
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("send", "Send to Phone")]);
    dialog.set_response_appearance("send", adw::ResponseAppearance::Suggested);
    let (id, name) = (id.to_string(), name.to_string());
    dialog.connect_response(Some("send"), move |_, _| {
        let number = entry.text().trim().to_string();
        if number.is_empty() {
            return;
        }
        let (id, name) = (id.clone(), name.clone());
        glib::spawn_future_local(async move { dial(&id, &name, &number).await });
    });
    dialog.present(window().as_ref());
}

async fn dial(id: &str, name: &str, number: &str) {
    let reply: Result<Value, _> =
        ipc::call(json!({ "cmd": "dial_number", "target": id, "number": number })).await;
    match reply {
        Ok(_) => toast(&format!("Check {name} and tap the notification")),
        Err(e) => toast(&e.to_string()),
    }
}
