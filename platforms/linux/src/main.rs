//! Link All for Linux: the window, the tray icon and the desktop agent. The
//! syncing itself happens in `linkall-daemon`, the same background service the
//! command line uses; this app talks to it over its local socket.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This is the Link All app for Linux.");
}

#[cfg(target_os = "linux")]
mod agent;
#[cfg(target_os = "linux")]
mod autostart;
#[cfg(target_os = "linux")]
mod daemon;
#[cfg(target_os = "linux")]
mod hub;
#[cfg(target_os = "linux")]
mod ipc;
#[cfg(target_os = "linux")]
mod prefs;
#[cfg(target_os = "linux")]
mod theme;
#[cfg(target_os = "linux")]
mod tray;
#[cfg(target_os = "linux")]
mod ui;
#[cfg(target_os = "linux")]
mod update;

#[cfg(target_os = "linux")]
pub const APP_ID: &str = "app.linkall.LinkAll";

#[cfg(target_os = "linux")]
fn main() -> gtk::glib::ExitCode {
    use adw::prelude::*;
    use gtk::{gio, glib};
    use serde_json::json;
    use std::cell::Cell;
    use std::rc::Rc;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("LINKALL_LOG")
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // `--background` (used at login) starts hidden in the tray.
    let mut args: Vec<String> = std::env::args().collect();
    let background = args.iter().any(|a| a == "--background");
    // Launcher actions: run one action in the app (starting it if needed).
    let action = [
        ("--send-files", "send-files"),
        ("--pair", "pair-phone"),
        ("--search", "palette"),
    ]
    .into_iter()
    .find(|(flag, _)| args.iter().any(|a| a == flag))
    .map(|(_, action)| action);
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: linkall [--background | --send-files | --pair | --search] [FILE…]\n\nFiles given are sent to one of your devices.");
        return glib::ExitCode::SUCCESS;
    }
    args.retain(|a| {
        !matches!(
            a.as_str(),
            "--background" | "--send-files" | "--pair" | "--search"
        )
    });

    // Start the background service early, off the GTK thread.
    std::thread::spawn(|| {
        if !daemon::ensure_running() {
            tracing::warn!("Link All's background service isn't running");
        }
    });

    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    let hub = hub::Hub::default();
    let skip_present = Rc::new(Cell::new(background));

    {
        let hub = hub.clone();
        app.connect_startup(move |app| {
            // Keep running in the tray after the window closes.
            std::mem::forget(app.hold());
            theme::install();
            gtk::Window::set_default_icon_name(APP_ID);

            let clipboard = agent::start_clipboard();
            agent::start(app, &hub, clipboard.clone());
            ui::build(app, &hub, &clipboard);
            install_actions(app, &hub);
            let palette = gio::SimpleAction::new("palette", None);
            {
                let (hub, clipboard) = (hub.clone(), clipboard.clone());
                palette.connect_activate(move |_, _| ui::palette::open(&hub, &clipboard));
            }
            app.add_action(&palette);
            app.set_accels_for_action("app.palette", &["<Control>k", "<Control>f"]);

            let (commands, tray) = tray::start();
            let last = std::cell::RefCell::new(String::new());
            hub.on_status(move |hub| {
                let devices: Vec<(String, String)> = hub
                    .connected()
                    .iter()
                    .map(|p| {
                        (
                            p["id"].as_str().unwrap_or_default().to_string(),
                            p["friendly_name"].as_str().unwrap_or("Device").to_string(),
                        )
                    })
                    .collect();
                let paused = hub.setting("sync_enabled") == false;
                let key = format!("{paused}{devices:?}");
                if *last.borrow() != key {
                    *last.borrow_mut() = key;
                    tray.set_state(paused, devices);
                }
            });
            let (app, hub2) = (app.clone(), hub.clone());
            glib::spawn_future_local(async move {
                while let Ok(command) = commands.recv().await {
                    match command {
                        tray::Command::Show => app.activate(),
                        tray::Command::SendFiles => {
                            app.activate();
                            ui::send::pick_files(&hub2, None);
                        }
                        tray::Command::SendFilesTo(id) => {
                            app.activate();
                            ui::send::pick_files(&hub2, Some(id));
                        }
                        tray::Command::SendClipboard => ui::send::send_clipboard(&hub2, None),
                        tray::Command::Palette => {
                            app.activate();
                            app.activate_action("palette", None);
                        }
                        tray::Command::ToggleSync => {
                            let on = hub2.setting("sync_enabled") == false;
                            ui::act(
                                json!({ "cmd": "set_sync_enabled", "enabled": on }),
                                Some(if on { "Sync resumed" } else { "Sync paused" }),
                                &hub2,
                            );
                        }
                        tray::Command::Quit => app.quit(),
                    }
                }
            });
            hub.start();
        });
    }
    {
        let skip = skip_present.clone();
        app.connect_activate(move |_| {
            if skip.replace(false) {
                return;
            }
            if let Some(w) = ui::window() {
                w.present();
            }
        });
    }
    {
        let hub = hub.clone();
        app.connect_open(move |app, files, _| {
            skip_present.set(false);
            app.activate();
            // A tel: link goes to the phone; anything else is files to send.
            let (calls, rest): (Vec<_>, Vec<_>) =
                files.iter().partition(|f| f.uri().starts_with("tel:"));
            for call in calls {
                let uri = call.uri().to_string();
                let number =
                    glib::uri_unescape_string(uri.trim_start_matches("tel:"), None::<&str>)
                        .map(|s| s.to_string())
                        .unwrap_or_default();
                let valid = (3..=32).contains(&number.chars().count())
                    && number
                        .chars()
                        .all(|c| c.is_ascii_digit() || "+-().*# ".contains(c));
                if valid {
                    ui::call_from_phone(Some(number));
                } else {
                    ui::toast("That doesn't look like a phone number");
                }
            }
            let paths: Vec<_> = rest.iter().filter_map(|f| f.path()).collect();
            if !paths.is_empty() {
                ui::send::send_paths(&hub, paths, None);
            }
        });
    }
    if let Some(action) = action {
        // In the running app if there is one; otherwise this becomes the app.
        if app.register(None::<&gio::Cancellable>).is_ok() {
            if app.is_remote() {
                app.activate_action(action, None);
                return glib::ExitCode::SUCCESS;
            }
            let app2 = app.clone();
            glib::idle_add_local_once(move || {
                app2.activate();
                app2.activate_action(action, None);
            });
        }
    }
    app.run_with_args(&args)
}

#[cfg(target_os = "linux")]
fn install_actions(app: &adw::Application, hub: &hub::Hub) {
    use adw::prelude::*;
    use gtk::gio;

    let show = gio::SimpleAction::new("show", None);
    {
        let app = app.clone();
        show.connect_activate(move |_, _| app.activate());
    }
    app.add_action(&show);

    let send = gio::SimpleAction::new("send-files", None);
    {
        let hub = hub.clone();
        send.connect_activate(move |_, _| ui::send::pick_files(&hub, None));
    }
    app.add_action(&send);

    let pair = gio::SimpleAction::new("pair-phone", None);
    {
        let hub = hub.clone();
        pair.connect_activate(move |_, _| ui::pairing::show_qr(&hub));
    }
    app.add_action(&pair);

    let by_address = gio::SimpleAction::new("add-by-address", None);
    {
        let hub = hub.clone();
        by_address.connect_activate(move |_, _| ui::add_by_address(&hub));
    }
    app.add_action(&by_address);

    let guest = gio::SimpleAction::new("receive-guest", None);
    guest.connect_activate(|_, _| ui::receive_from_guest());
    app.add_action(&guest);

    let call = gio::SimpleAction::new("call-phone", None);
    call.connect_activate(|_, _| ui::call_from_phone(None));
    app.add_action(&call);

    let rescan = gio::SimpleAction::new("rescan", None);
    {
        let hub = hub.clone();
        rescan.connect_activate(move |_, _| {
            ui::act(
                serde_json::json!({ "cmd": "rescan_peers" }),
                Some("Looking for devices…"),
                &hub,
            )
        });
    }
    app.add_action(&rescan);

    let resume = gio::SimpleAction::new("resume-sync", None);
    {
        let hub = hub.clone();
        resume.connect_activate(move |_, _| {
            ui::act(
                serde_json::json!({ "cmd": "set_sync_enabled", "enabled": true }),
                Some("Sync resumed"),
                &hub,
            )
        });
    }
    app.add_action(&resume);

    let about = gio::SimpleAction::new("about", None);
    about.connect_activate(|_, _| {
        let dialog = adw::AboutDialog::builder()
            .application_name("Link All")
            .application_icon(APP_ID)
            .version(env!("CARGO_PKG_VERSION"))
            .developer_name("Chinmay Kudalkar")
            .website("https://linkall.chinmaykudalkar.com")
            .issue_url("https://github.com/ChinmayyK/Link-All/issues")
            .license_type(gtk::License::Agpl30Only)
            .comments("Continuity for every device you own: one clipboard, files and notifications, straight over your own network.")
            .build();
        dialog.present(ui::window().as_ref());
    });
    app.add_action(&about);

    let quit = gio::SimpleAction::new("quit", None);
    {
        let app = app.clone();
        quit.connect_activate(move |_, _| app.quit());
    }
    app.add_action(&quit);
    app.set_accels_for_action("app.quit", &["<Control>q"]);
    app.set_accels_for_action("window.close", &["<Control>w"]);
}
