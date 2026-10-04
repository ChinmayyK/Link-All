//! Link All's Windows system tray helper.
//!
//! A separate process from the WinUI app so the icon survives UI crashes and
//! the app can drop its window while sitting in the tray. It replaces the old
//! WinForms `LinkAll.Tray`, which carried a second self-contained .NET
//! runtime (~110 MB) just to draw one icon.
//!
//! It keeps that helper's contract with `TrayService.cs` exactly:
//! - runs as `tray\LinkAll.Tray.exe` (the process name the watchdog checks);
//! - reads one line per connection on `\\.\pipe\LinkAll_Tray_Pipe`:
//!   `TIP:<text>` sets the tooltip, `NOTIFY:<text>` shows a balloon,
//!   `QUIT` removes the icon and exits;
//! - writes menu commands (`QUICKACCESS`, `SETTINGS`, `RESCAN`) as one line
//!   to `\\.\pipe\LinkAll_Main_Commands`, falling back to opening the app.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    win::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("linkall-tray only runs on Windows");
}
