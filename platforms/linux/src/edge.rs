//! Sharing this computer's keyboard and mouse with the one next to it. The
//! capture (watching the screen edge, grabbing the pointer and keyboard) is
//! `linkall_core::edge_x11`; this file connects it to the daemon and to the
//! settings the user picks.

use crate::{ipc, prefs};
use linkall_core::edge_x11::{self, Batch, Control, Host, Status};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};

pub const ENABLED: &str = "edge_control_enabled";

/// The preference that holds the device id linked to `edge` (0 left, 1 right,
/// 2 above, 3 below).
pub fn link_key(edge: usize) -> String {
    format!("edge_link_{edge}")
}

pub const EDGE_NAMES: [&str; 4] = ["left", "right", "above", "below"];

fn control() -> &'static Arc<Control> {
    static CONTROL: OnceLock<Arc<Control>> = OnceLock::new();
    CONTROL.get_or_init(|| Arc::new(Control::default()))
}

/// Starts watching the screen edges. Does nothing useful until the user turns
/// the sharing on in Settings.
pub fn start() {
    reload();
    edge_x11::spawn(Box::new(DaemonHost), control().clone());
}

/// Reads the settings again; call after any of them changes.
pub fn reload() {
    let c = control();
    c.enabled.store(prefs::get_bool(ENABLED, false), Ordering::Relaxed);
    let mut linked = c.linked.lock().unwrap();
    for (edge, slot) in linked.iter_mut().enumerate() {
        *slot = prefs::get_string(&link_key(edge)).is_some_and(|id| !id.is_empty());
    }
}

/// Why the last hand-off ended, once; for a toast.
pub fn take_note() -> Option<String> {
    control().last_note.lock().unwrap().take()
}

struct DaemonHost;

fn data(reply: &Value) -> &Value {
    reply
}

impl Host for DaemonHost {
    fn begin(&mut self, edge: u8, pos: u16) -> Result<(), String> {
        let target = prefs::get_string(&link_key(edge as usize)).unwrap_or_default();
        if target.is_empty() {
            return Err("no computer is linked to that edge".into());
        }
        ipc::call_blocking(json!({ "cmd": "edge_begin", "target": target, "edge": edge, "pos": pos }))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn input(&mut self, b: &Batch) -> Result<(), String> {
        ipc::call_blocking(json!({
            "cmd": "edge_input",
            "dx": b.dx,
            "dy": b.dy,
            "scroll_x": b.scroll_x,
            "scroll_y": b.scroll_y,
            "buttons": b.buttons,
            "keys": b.keys,
        }))
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    fn end(&mut self) {
        let _ = ipc::call_blocking(json!({ "cmd": "edge_end" }));
    }

    fn status(&mut self) -> Result<Status, String> {
        let reply = ipc::call_blocking(json!({ "cmd": "edge_status" })).map_err(|e| e.to_string())?;
        let reply = data(&reply);
        Ok(Status {
            returned_pos: reply["returned_pos"].as_u64().map(|p| p as u16),
            ended_reason: reply["ended_reason"].as_str().map(str::to_string),
        })
    }
}
