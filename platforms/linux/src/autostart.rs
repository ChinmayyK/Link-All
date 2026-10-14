//! "Start at login": an XDG autostart entry that starts the app hidden in the tray.

use std::path::PathBuf;

fn entry() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("autostart")
            .join(format!("{}.desktop", crate::APP_ID)),
    )
}

pub fn enabled() -> bool {
    entry().is_some_and(|p| p.exists())
}

pub fn set(on: bool) -> std::io::Result<()> {
    let Some(path) = entry() else { return Ok(()) };
    if !on {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Inside Flatpak the host launches the app through `flatpak run`.
    let exec = if std::env::var_os("FLATPAK_ID").is_some() {
        format!("flatpak run {} --background", crate::APP_ID)
    } else {
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "linkall".into());
        format!("{exe} --background")
    };
    std::fs::write(
        path,
        format!(
            "[Desktop Entry]\nType=Application\nName=Link All\nComment=Keeps your devices in sync\nExec={exec}\nIcon={}\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n",
            crate::APP_ID
        ),
    )
}
