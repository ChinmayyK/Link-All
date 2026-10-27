//! The last few log lines, kept in memory so a problem report has something to
//! show. Lines are redacted as they are captured, so the buffer never holds an
//! address, a device ID or a file path. Nothing is written to disk or sent.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;
use uuid::Uuid;

const CAPACITY: usize = 500;

static BUFFER: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// A layer that records INFO and above into the buffer. Add it next to the
/// app's own output layer.
pub fn layer<S>() -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    Recorder.with_filter(LevelFilter::INFO)
}

/// The most recent `n` lines, oldest first.
pub fn tail(n: usize) -> Vec<String> {
    let buf = BUFFER.lock().unwrap_or_else(|e| e.into_inner());
    let skip = buf.len().saturating_sub(n);
    buf.iter().skip(skip).cloned().collect()
}

struct Recorder;

impl<S: Subscriber> Layer<S> for Recorder {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut line = String::new();
        let level = *event.metadata().level();
        let _ = write!(line, "{} {}", level_tag(level), event.metadata().target());
        event.record(&mut Fields(&mut line));
        // The time goes on after redaction, which would read it as an address.
        push(format!(
            "{} {}",
            utc_time_of_day(),
            mask_quoted(&redact(&line))
        ));
    }
}

/// HH:MM:SS in UTC. A report without times showed what happened but not
/// when, so a drop couldn't be matched to what the user was doing.
fn utc_time_of_day() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
        % 86_400;
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

fn level_tag(level: Level) -> &'static str {
    match level {
        Level::ERROR => "ERROR",
        Level::WARN => "WARN",
        Level::INFO => "INFO",
        Level::DEBUG => "DEBUG",
        Level::TRACE => "TRACE",
    }
}

fn push(line: String) {
    let mut buf = BUFFER.lock().unwrap_or_else(|e| e.into_inner());
    if buf.len() == CAPACITY {
        buf.pop_front();
    }
    buf.push_back(line);
}

struct Fields<'a>(&'a mut String);

impl Visit for Fields<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name().contains("name") {
            // peer_name, file_name, device_name: who or what, never recorded.
            let _ = write!(self.0, " {}=<hidden>", field.name());
        } else if field.name() == "message" {
            let _ = write!(self.0, ": {value:?}");
        } else {
            let _ = write!(self.0, " {}={value:?}", field.name());
        }
    }
}

/// Replaces 'single quoted' text with `'<name>'`. Log messages quote device
/// names that way; an apostrophe inside a word (don't) is left alone.
fn mask_quoted(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let opens = chars[i] == '\''
            && (i == 0 || !chars[i - 1].is_alphanumeric())
            && chars.get(i + 1).is_some_and(|c| c.is_alphanumeric());
        if opens {
            let close = (i + 1..chars.len()).find(|&j| {
                chars[j] == '\'' && chars.get(j + 1).is_none_or(|c| !c.is_alphanumeric())
            });
            if let Some(j) = close {
                out.push_str("'<name>'");
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Hides what could identify a person or a network: addresses keep only their
/// last IPv4 octet, device IDs keep six characters, paths are dropped.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    for c in text.chars() {
        if is_word_char(c) {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '_' | '/' | '\\')
}

fn flush(word: &mut String, out: &mut String) {
    if word.is_empty() {
        return;
    }
    out.push_str(&redact_word(word));
    word.clear();
}

fn redact_word(word: &str) -> String {
    if let Ok(addr) = word.parse::<SocketAddr>() {
        return redact_ip(addr.ip());
    }
    if let Ok(ip) = word.parse::<IpAddr>() {
        return redact_ip(ip);
    }
    if let Ok(id) = Uuid::parse_str(word) {
        return short_id(&id);
    }
    if is_path(word) {
        return "<path>".to_string();
    }
    word.to_string()
}

fn redact_ip(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => format!("x.x.x.{}", v4.octets()[3]),
        IpAddr::V6(_) => "<ipv6>".to_string(),
    }
}

/// The first six characters of a device ID, enough to tell devices apart in a
/// report.
pub fn short_id(id: &Uuid) -> String {
    format!("{}…", &id.simple().to_string()[..6])
}

fn is_path(word: &str) -> bool {
    let b = word.as_bytes();
    let unix = b.len() > 1 && b[0] == b'/' && word[1..].contains('/');
    let windows = b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
    unix || windows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_line_starts_with_its_utc_time() {
        use tracing_subscriber::layer::SubscriberExt;
        let subscriber = tracing_subscriber::registry().with(layer());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("a line to time");
        });
        let line = tail(CAPACITY)
            .into_iter()
            .rev()
            .find(|l| l.contains("a line to time"))
            .expect("the line was recorded");
        let (time, rest) = line.split_once(' ').unwrap();
        let parts: Vec<&str> = time.split(':').collect();
        assert_eq!(parts.len(), 3, "{line}");
        assert!(
            parts
                .iter()
                .all(|p| p.len() == 2 && p.parse::<u8>().is_ok()),
            "{line}"
        );
        assert!(rest.starts_with("INFO "), "{line}");
    }

    #[test]
    fn addresses_keep_only_the_last_octet() {
        assert_eq!(
            redact("dial 192.168.1.42:47823 failed"),
            "dial x.x.x.42 failed"
        );
        assert_eq!(redact("peer 10.0.0.7"), "peer x.x.x.7");
        assert_eq!(redact("bind [fe80::1]:5000"), "bind [<ipv6>]:5000");
    }

    #[test]
    fn device_ids_are_shortened() {
        let id = "8f14e45f-ceea-467a-9575-d1c2a0b1e4f3";
        assert_eq!(redact(&format!("device={id}")), "device=8f14e4…");
    }

    #[test]
    fn paths_are_dropped() {
        assert_eq!(
            redact("saved /Users/jo/Downloads/a.txt ok"),
            "saved <path> ok"
        );
        assert_eq!(redact(r"saved C:\Users\jo\a.txt ok"), "saved <path> ok");
        assert_eq!(redact("rate 3/4 done"), "rate 3/4 done");
    }

    #[test]
    fn quoted_names_are_masked_but_apostrophes_are_not() {
        assert_eq!(
            mask_quoted("Handshake complete with 'MacBook Pro' (ok)"),
            "Handshake complete with '<name>' (ok)"
        );
        assert_eq!(
            mask_quoted("it didn't connect, can't retry"),
            "it didn't connect, can't retry"
        );
    }

    #[test]
    fn plain_text_is_untouched() {
        let line = "INFO linkall_core::engine: connected, version 2.0.2";
        assert_eq!(redact(line), line);
    }

    #[test]
    fn the_buffer_keeps_the_newest_lines() {
        for i in 0..(CAPACITY + 20) {
            push(format!("line {i}"));
        }
        let last = tail(3);
        assert_eq!(last.len(), 3);
        assert_eq!(last[2], format!("line {}", CAPACITY + 19));
        assert!(tail(CAPACITY + 100).len() <= CAPACITY);
    }
}
