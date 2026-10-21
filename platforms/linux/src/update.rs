//! Looks for a newer Link All on GitHub, at most once a day, and only tells
//! the user: it never installs anything. Failures are silent.

use crate::prefs;
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const ENABLED: &str = "check_for_updates";
const LAST_CHECK: &str = "update_last_check";
const LATEST_VERSION: &str = "update_latest_version";
const LATEST_URL: &str = "update_latest_url";
const EVERY_SECS: u64 = 24 * 60 * 60;
const LATEST: &str = "https://api.github.com/repos/ChinmayyK/Link-All/releases/latest";
const REPO_PAGE: &str = "https://github.com/ChinmayyK/Link-All/";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Update {
    pub version: String,
    pub url: String,
}

pub fn enabled() -> bool {
    prefs::get_bool(ENABLED, true)
}

/// What the last check found, if it is newer than this build.
pub fn known() -> Option<Update> {
    if !enabled() {
        return None;
    }
    let version = prefs::get_string(LATEST_VERSION)?;
    let url = prefs::get_string(LATEST_URL)?;
    is_newer(&version, env!("CARGO_PKG_VERSION")).then_some(Update { version, url })
}

/// Asks GitHub if a day has passed since the last check. Blocks, so run it on
/// a thread, never on the GTK thread.
pub fn refresh_if_due() {
    if !enabled() {
        return;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if now.saturating_sub(prefs::get_u64(LAST_CHECK).unwrap_or(0)) < EVERY_SECS {
        return;
    }
    let Some(latest) = fetch() else { return };
    prefs::set(LAST_CHECK, json!(now));
    prefs::set(LATEST_VERSION, json!(latest.version));
    prefs::set(LATEST_URL, json!(latest.url));
}

fn fetch() -> Option<Update> {
    let mut response = ureq::get(LATEST)
        .config()
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "link-all")
        .call()
        .ok()?;
    parse(&response.body_mut().read_to_string().ok()?)
}

fn parse(body: &str) -> Option<Update> {
    let release: Value = serde_json::from_str(body).ok()?;
    if release["draft"].as_bool() == Some(true) || release["prerelease"].as_bool() == Some(true) {
        return None;
    }
    let tag = release["tag_name"].as_str()?;
    let url = release["html_url"].as_str()?;
    let version = tag.strip_prefix('v').unwrap_or(tag);
    // Only ever open a page on the project's own repository.
    if version.is_empty() || !url.starts_with(REPO_PAGE) {
        return None;
    }
    Some(Update {
        version: version.to_string(),
        url: url.to_string(),
    })
}

/// True when `latest` is a higher major.minor.patch than `current`.
fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> [u32; 3] {
        let mut out = [0; 3];
        for (slot, part) in out.iter_mut().zip(v.split(['.', '-', '+'])) {
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            *slot = digits.parse().unwrap_or(0);
        }
        out
    }
    parts(latest) > parts(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn higher_versions_are_newer() {
        assert!(is_newer("2.1.0", "2.0.2"));
        assert!(is_newer("2.0.10", "2.0.9"));
        assert!(is_newer("3.0.0", "2.9.9"));
    }

    #[test]
    fn same_or_older_is_not_newer() {
        assert!(!is_newer("2.0.2", "2.0.2"));
        assert!(!is_newer("2.0.1", "2.0.2"));
        assert!(!is_newer("2.0.2", "2.0.2-debug"));
    }

    #[test]
    fn a_release_gives_its_version_and_page() {
        let u = parse(
            r#"{"tag_name":"v2.1.0","html_url":"https://github.com/ChinmayyK/Link-All/releases/tag/v2.1.0"}"#,
        );
        assert_eq!(u.map(|u| u.version), Some("2.1.0".to_string()));
    }

    #[test]
    fn drafts_prereleases_and_foreign_links_are_ignored() {
        assert!(parse(r#"{"tag_name":"v9.0.0","prerelease":true,"html_url":"https://github.com/ChinmayyK/Link-All/x"}"#).is_none());
        assert!(
            parse(r#"{"tag_name":"v9.0.0","html_url":"https://evil.example/Link-All/"}"#).is_none()
        );
    }
}
