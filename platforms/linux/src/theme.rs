//! Link All's look: brand colours for light and dark, the OS logos used for
//! devices, and the app icon.

use adw::prelude::*;
use gtk::gdk;

const SHEET: &str = include_str!("style.css");

/// The same palette as the Android app (ui/Kit.kt): warm paper and one ink blue.
fn palette(dark: bool) -> String {
    let c = if dark {
        [
            ("blue", "#7c9bff"),
            ("red", "#f87171"),
            ("violet", "#9d8bff"),
            ("teal", "#3cc6d8"),
            ("live", "#34d399"),
            ("warn", "#fbbf24"),
            ("muted", "#a19d95"),
            ("surface", "#17181b"),
            ("sunk", "#1d1e22"),
            ("line", "#2a2b30"),
        ]
    } else {
        [
            ("blue", "#2f5bff"),
            ("red", "#dc2626"),
            ("violet", "#6a52e8"),
            ("teal", "#0f8fa3"),
            ("live", "#16a34a"),
            ("warn", "#b45309"),
            ("muted", "#6b665e"),
            ("surface", "#ffffff"),
            ("sunk", "#f2efea"),
            ("line", "#e7e2da"),
        ]
    };
    let mut s: String = c
        .iter()
        .map(|(k, v)| format!("@define-color la_{k} {v};\n"))
        .collect();
    // libadwaita's own accent follows the brand blue.
    s += &format!("@define-color accent_bg_color {};\n@define-color accent_color {};\n@define-color accent_fg_color {};\n", if dark { "#5f82ff" } else { "#2f5bff" }, c[0].1, if dark { "#0b1022" } else { "white" });
    if dark {
        s += "@define-color window_bg_color #0f1012;\n@define-color view_bg_color #0f1012;\n@define-color sidebar_bg_color #141517;\n@define-color headerbar_bg_color #0f1012;\n@define-color card_bg_color #17181b;\n@define-color dialog_bg_color #17181b;\n@define-color popover_bg_color #1d1e22;\n@define-color window_fg_color #f1efea;\n@define-color view_fg_color #f1efea;\n@define-color card_fg_color #f1efea;\n";
    } else {
        s += "@define-color window_bg_color #faf8f5;\n@define-color view_bg_color #faf8f5;\n@define-color headerbar_bg_color #faf8f5;\n@define-color window_fg_color #1b1a17;\n@define-color view_fg_color #1b1a17;\n@define-color card_fg_color #1b1a17;\n";
    }
    s
}

/// "system", "light" or "dark".
pub fn apply_scheme(scheme: &str) {
    adw::StyleManager::default().set_color_scheme(match scheme {
        "light" => adw::ColorScheme::ForceLight,
        "dark" => adw::ColorScheme::ForceDark,
        _ => adw::ColorScheme::Default,
    });
}

pub fn install() {
    if std::env::var_os("ADW_DEBUG_COLOR_SCHEME").is_none() {
        apply_scheme(&crate::prefs::get_string(crate::prefs::COLOR_SCHEME).unwrap_or_default());
    }
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let manager = adw::StyleManager::default();
    let apply =
        move |m: &adw::StyleManager| provider.load_from_string(&(palette(m.is_dark()) + SHEET));
    apply(&manager);
    manager.connect_dark_notify(apply);
    install_icons(&display);
}

/// Puts the OS logos and the app icon where the icon theme finds them, so
/// they recolour like any other symbolic icon.
fn install_icons(display: &gdk::Display) {
    let Some(base) = dirs::cache_dir().map(|d| d.join("linkall").join("icons")) else {
        return;
    };
    let symbolic = base.join("hicolor").join("scalable").join("apps");
    let app = base.join("hicolor").join("256x256").join("apps");
    let files: [(&std::path::Path, &str, &[u8]); 5] = [
        (
            &symbolic,
            "linkall-android-symbolic.svg",
            include_bytes!("../icons/symbolic/linkall-android-symbolic.svg"),
        ),
        (
            &symbolic,
            "linkall-apple-symbolic.svg",
            include_bytes!("../icons/symbolic/linkall-apple-symbolic.svg"),
        ),
        (
            &symbolic,
            "linkall-windows-symbolic.svg",
            include_bytes!("../icons/symbolic/linkall-windows-symbolic.svg"),
        ),
        (
            &symbolic,
            "linkall-linux-symbolic.svg",
            include_bytes!("../icons/symbolic/linkall-linux-symbolic.svg"),
        ),
        (
            &app,
            "app.linkall.LinkAll.png",
            include_bytes!("../icons/hicolor/256x256/apps/app.linkall.LinkAll.png"),
        ),
    ];
    for (dir, name, bytes) in files {
        let path = dir.join(name);
        if std::fs::read(&path).ok().as_deref() != Some(bytes) {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(&path, bytes);
        }
    }
    gtk::IconTheme::for_display(display).add_search_path(&base);
}

/// The symbolic logo for a device's platform.
pub fn os_icon(platform: &str) -> &'static str {
    match platform.to_ascii_lowercase().as_str() {
        "android" => "linkall-android-symbolic",
        "macos" | "ios" | "mac" => "linkall-apple-symbolic",
        "windows" => "linkall-windows-symbolic",
        "linux" => "linkall-linux-symbolic",
        _ => "computer-symbolic",
    }
}

/// An icon on a tinted rounded tile.
pub fn well(icon: &str, tint: &str) -> gtk::Box {
    let b = gtk::Box::builder()
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Center)
        .css_classes(["well", tint])
        .build();
    let image = gtk::Image::from_icon_name(icon);
    // The well is a fixed square (style.css); centre the glyph without letting it stretch the row.
    image.set_size_request(40, 40);
    b.append(&image);
    b
}

/// A small round dot showing whether something is live.
pub fn dot(live: bool) -> gtk::Box {
    let d = gtk::Box::builder()
        .valign(gtk::Align::Center)
        .css_classes(["status-dot"])
        .build();
    if live {
        d.add_css_class("live");
    }
    d
}

/// The app logo as a texture, for the sidebar header.
pub fn logo(size: i32) -> gtk::Image {
    let image = gtk::Image::from_icon_name(crate::APP_ID);
    image.set_pixel_size(size);
    image
}

#[cfg(test)]
mod tests {
    #[test]
    fn each_platform_gets_its_logo() {
        assert_eq!(super::os_icon("Android"), "linkall-android-symbolic");
        assert_eq!(super::os_icon("macOS"), "linkall-apple-symbolic");
        assert_eq!(super::os_icon("Windows"), "linkall-windows-symbolic");
        assert_eq!(super::os_icon("Linux"), "linkall-linux-symbolic");
        assert_eq!(super::os_icon("Something"), "computer-symbolic");
    }
}
