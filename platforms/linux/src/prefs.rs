//! Settings that only the Linux app has (the shared ones live in the daemon):
//! kept in ~/.config/linkall/linux.json.

use serde_json::{json, Value};
use std::path::PathBuf;

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("linkall").join("linux.json"))
}

fn load() -> Value {
    path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| json!({}))
}

pub fn get_bool(key: &str, default: bool) -> bool {
    load()[key].as_bool().unwrap_or(default)
}

pub fn get_u64(key: &str) -> Option<u64> {
    load()[key].as_u64()
}

pub fn get_string(key: &str) -> Option<String> {
    load()[key].as_str().map(str::to_string)
}

pub fn set(key: &str, value: Value) {
    let Some(p) = path() else { return };
    let mut v = load();
    v[key] = value;
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(p, serde_json::to_vec_pretty(&v).unwrap_or_default());
}

pub const SEND_SCREENSHOTS: &str = "send_new_screenshots";
pub const COLOR_SCHEME: &str = "color_scheme";
