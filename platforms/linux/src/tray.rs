//! The tray icon (StatusNotifierItem): built into KDE, Cinnamon, XFCE and
//! others; GNOME shows it with the AppIndicator extension.

use crate::APP_ID;

pub enum Command {
    Show,
    SendFiles,
    SendFilesTo(String),
    SendClipboard,
    Palette,
    ToggleSync,
    Quit,
}

pub struct Tray {
    tx: async_channel::Sender<Command>,
    icon: Vec<ksni::Icon>,
    paused: bool,
    /// Connected devices, as (id, name).
    devices: Vec<(String, String)>,
}

impl Tray {
    fn send(&self, c: Command) {
        let _ = self.tx.send_blocking(c);
    }
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        APP_ID.into()
    }
    fn title(&self) -> String {
        "Link All".into()
    }
    fn icon_name(&self) -> String {
        APP_ID.into()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icon.clone()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Link All".into(),
            description: if self.paused {
                "Sync is paused".into()
            } else {
                "Your devices are in sync".into()
            },
            ..Default::default()
        }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(Command::Show);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{StandardItem, SubMenu};
        let item = |label: &str, f: fn(&mut Self)| -> ksni::MenuItem<Self> {
            StandardItem {
                label: label.into(),
                activate: Box::new(f),
                ..Default::default()
            }
            .into()
        };
        let mut menu = vec![
            item("Open Link All", |t| t.send(Command::Show)),
            item("Search…", |t| t.send(Command::Palette)),
            ksni::MenuItem::Separator,
        ];
        if self.devices.len() > 1 {
            let mut sub: Vec<ksni::MenuItem<Self>> =
                vec![item("Choose Device…", |t| t.send(Command::SendFiles))];
            for (id, name) in &self.devices {
                let id = id.clone();
                sub.push(
                    StandardItem {
                        label: name.clone(),
                        activate: Box::new(move |t: &mut Self| {
                            t.send(Command::SendFilesTo(id.clone()))
                        }),
                        ..Default::default()
                    }
                    .into(),
                );
            }
            menu.push(
                SubMenu {
                    label: "Send Files To".into(),
                    submenu: sub,
                    ..Default::default()
                }
                .into(),
            );
        } else {
            let label = match self.devices.first() {
                Some((_, name)) => format!("Send Files to {name}…"),
                None => "Send Files…".into(),
            };
            menu.push(
                StandardItem {
                    label,
                    enabled: !self.devices.is_empty(),
                    activate: Box::new(|t: &mut Self| t.send(Command::SendFiles)),
                    ..Default::default()
                }
                .into(),
            );
        }
        menu.push(
            StandardItem {
                label: "Send Clipboard".into(),
                enabled: !self.devices.is_empty(),
                activate: Box::new(|t: &mut Self| t.send(Command::SendClipboard)),
                ..Default::default()
            }
            .into(),
        );
        menu.push(ksni::MenuItem::Separator);
        menu.push(item(
            if self.paused {
                "Resume Sync"
            } else {
                "Pause Sync"
            },
            |t| t.send(Command::ToggleSync),
        ));
        menu.push(item("Quit Link All", |t| t.send(Command::Quit)));
        menu
    }
}

/// The app icon as ARGB32, for trays that can't look the icon up by name.
fn pixmap() -> Vec<ksni::Icon> {
    let Ok(img) = image::load_from_memory(include_bytes!(
        "../icons/hicolor/64x64/apps/app.linkall.LinkAll.png"
    )) else {
        return vec![];
    };
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let data = rgba
        .pixels()
        .flat_map(|p| [p[3], p[0], p[1], p[2]])
        .collect();
    vec![ksni::Icon {
        width: w as i32,
        height: h as i32,
        data,
    }]
}

/// Updates the tray's menu from the GTK thread.
pub struct TrayHandle(ksni::Handle<Tray>);

impl TrayHandle {
    pub fn set_state(&self, paused: bool, devices: Vec<(String, String)>) {
        self.0.update(|t| {
            t.paused = paused;
            t.devices = devices;
        });
    }
}

/// Starts the tray on its own thread; returns the commands it sends.
pub fn start() -> (async_channel::Receiver<Command>, TrayHandle) {
    let (tx, rx) = async_channel::unbounded();
    let service = ksni::TrayService::new(Tray {
        tx,
        icon: pixmap(),
        paused: false,
        devices: vec![],
    });
    let handle = TrayHandle(service.handle());
    service.spawn();
    (rx, handle)
}
