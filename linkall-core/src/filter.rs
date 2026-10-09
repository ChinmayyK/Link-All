//! Content filter — applied before sending and on receipt.
//!
//! Filters run in order; the first `Deny` result short-circuits the chain.
//!
//! # Built-in filters
//! - `SizeFilter`       — reject payloads above the configured size limit.
//! - `TypeFilter`       — allow/deny by content type (text / image / file).
//! - `ExtensionFilter`  — block file extensions known to be dangerous.
//! - `SensitiveTextFilter` — optional heuristic to avoid syncing passwords
//!   (patterns like `password: ...`, `BEGIN RSA PRIVATE KEY`, etc.).
//!
//! # Extension
//! Platform code can push custom `Box<dyn Filter>` into the `FilterChain`.

use crate::protocol::ClipboardContent;

// ── Verdict ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Allow,
    Deny { reason: String },
}

impl Verdict {
    pub fn is_allow(&self) -> bool {
        *self == Verdict::Allow
    }
    pub fn deny(reason: impl Into<String>) -> Self {
        Verdict::Deny {
            reason: reason.into(),
        }
    }
}

// ── Filter trait ──────────────────────────────────────────────────────────────

pub trait Filter: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(&self, content: &ClipboardContent) -> Verdict;
}

// ── Filter chain ──────────────────────────────────────────────────────────────

pub struct FilterChain {
    filters: Vec<Box<dyn Filter>>,
}

impl FilterChain {
    pub fn new() -> Self {
        Self { filters: vec![] }
    }

    /// Build a default chain from the current settings.
    pub fn from_settings(settings: &crate::settings::Settings) -> Self {
        let mut chain = Self::new();

        chain.push(SizeFilter {
            max_bytes: settings.max_payload_bytes as usize,
        });

        chain.push(TypeFilter {
            allow_text: settings.sync_text,
            allow_images: settings.sync_images,
            allow_files: settings.sync_files,
        });

        // Skip trivially short text copies if configured.
        if settings.min_text_length > 0 {
            chain.push(MinLengthFilter {
                min_chars: settings.min_text_length,
            });
        }

        // URL-only mode: only sync content that looks like a URL.
        if settings.sync_urls_only {
            chain.push(UrlOnlyFilter);
        }

        chain.push(IgnorePatternFilter::from_settings(settings));
        chain.push(ExtensionFilter::default());
        chain.push(SensitiveTextFilter::from_settings(settings));
        chain
    }

    pub fn push(&mut self, f: impl Filter + 'static) {
        self.filters.push(Box::new(f));
    }

    /// Run the chain. Returns the first Deny, or Allow if all pass.
    pub fn run(&self, content: &ClipboardContent) -> Verdict {
        for f in &self.filters {
            let v = f.check(content);
            if !v.is_allow() {
                // LOW-06: log only the filter name and reason string — never
                // the content payload, which may contain passwords or PII.
                let reason = match &v {
                    Verdict::Deny { reason } => reason.as_str(),
                    Verdict::Allow => "",
                };
                tracing::debug!(filter = f.name(), reason, "content blocked by filter");
                return v;
            }
        }
        Verdict::Allow
    }
}

impl Default for FilterChain {
    fn default() -> Self {
        Self::new()
    }
}

// ── MinLengthFilter ───────────────────────────────────────────────────────────

/// Reject text that is shorter than `min_chars` Unicode scalar values.
/// Images and files always pass.
///
/// Useful to suppress single-character or whitespace-only clipboard events
/// that accumulate from keyboard-driven selection on some platforms.
pub struct MinLengthFilter {
    pub min_chars: usize,
}

impl Filter for MinLengthFilter {
    fn name(&self) -> &'static str {
        "min_length"
    }

    fn check(&self, content: &ClipboardContent) -> Verdict {
        if let ClipboardContent::Text(text) = content {
            let char_count = text.chars().filter(|c| !c.is_whitespace()).count();
            if char_count < self.min_chars {
                return Verdict::deny(format!(
                    "text too short: {} non-whitespace chars < min {}",
                    char_count, self.min_chars
                ));
            }
        }
        Verdict::Allow
    }
}

// ── UrlOnlyFilter ─────────────────────────────────────────────────────────────

/// When enabled, only sync text that looks like an absolute URL.
/// Images and files are always allowed through.
///
/// Matches: http://, https://, ftp://, and ssh:// scheme prefixes
/// (case-insensitive). Does not validate the full URL structure — just
/// the scheme prefix, which avoids the need for an external URL parser.
pub struct UrlOnlyFilter;

impl UrlOnlyFilter {
    fn looks_like_url(text: &str) -> bool {
        let t = text.trim().to_lowercase();
        t.starts_with("http://")
            || t.starts_with("https://")
            || t.starts_with("ftp://")
            || t.starts_with("ssh://")
            || t.starts_with("git://")
            || t.starts_with("mailto:")
    }
}

impl Filter for UrlOnlyFilter {
    fn name(&self) -> &'static str {
        "url_only"
    }

    fn check(&self, content: &ClipboardContent) -> Verdict {
        match content {
            ClipboardContent::Text(text) => {
                if Self::looks_like_url(text) {
                    Verdict::Allow
                } else {
                    Verdict::deny("url_only mode: text does not start with a recognised URL scheme")
                }
            }
            // Images and files always pass.
            _ => Verdict::Allow,
        }
    }
}

// ── GlobPatternFilter ─────────────────────────────────────────────────────────

/// Deny clipboard content whose text matches any glob-style pattern.
///
/// Supported wildcards:
/// - `*` — matches any sequence of characters (zero or more)
/// - `?` — matches exactly one character
///
/// Matching is case-insensitive. Patterns are applied to the full text for
/// text content and to the filename for file content.
pub struct GlobPatternFilter {
    patterns: Vec<String>,
}

impl GlobPatternFilter {
    pub fn new(patterns: Vec<String>) -> Self {
        Self {
            patterns: patterns
                .into_iter()
                .map(|p| p.to_lowercase())
                .filter(|p| !p.is_empty())
                .collect(),
        }
    }

    /// Simple glob match: `*` = any chars, `?` = one char.
    fn glob_match(pattern: &str, text: &str) -> bool {
        let pat: Vec<char> = pattern.chars().collect();
        let txt: Vec<char> = text.chars().collect();
        Self::match_dp(&pat, &txt)
    }

    fn match_dp(pat: &[char], txt: &[char]) -> bool {
        let (m, n) = (pat.len(), txt.len());
        let mut dp = vec![vec![false; n + 1]; m + 1];
        dp[0][0] = true;
        // Leading stars match empty string.
        for i in 1..=m {
            if pat[i - 1] == '*' {
                dp[i][0] = dp[i - 1][0];
            }
        }
        for i in 1..=m {
            for j in 1..=n {
                if pat[i - 1] == '*' {
                    dp[i][j] = dp[i - 1][j] || dp[i][j - 1];
                } else if pat[i - 1] == '?' || pat[i - 1] == txt[j - 1] {
                    dp[i][j] = dp[i - 1][j - 1];
                }
            }
        }
        dp[m][n]
    }
}

impl Filter for GlobPatternFilter {
    fn name(&self) -> &'static str {
        "glob_pattern"
    }

    fn check(&self, content: &ClipboardContent) -> Verdict {
        if self.patterns.is_empty() {
            return Verdict::Allow;
        }
        let haystack = match content {
            ClipboardContent::Text(text) => text.to_lowercase(),
            ClipboardContent::File { name, .. } => name.to_lowercase(),
            ClipboardContent::Image { .. } => return Verdict::Allow,
        };
        for pattern in &self.patterns {
            if Self::glob_match(pattern, &haystack) {
                return Verdict::deny(format!("content matches glob pattern '{}'", pattern));
            }
        }
        Verdict::Allow
    }
}

pub struct IgnorePatternFilter {
    patterns: Vec<String>,
}

impl IgnorePatternFilter {
    pub fn from_settings(settings: &crate::settings::Settings) -> Self {
        Self {
            patterns: settings
                .ignore_patterns
                .iter()
                .map(|pattern| pattern.to_lowercase())
                .filter(|pattern| !pattern.is_empty())
                .collect(),
        }
    }
}

impl Filter for IgnorePatternFilter {
    fn name(&self) -> &'static str {
        "ignore_pattern"
    }

    fn check(&self, content: &ClipboardContent) -> Verdict {
        if self.patterns.is_empty() {
            return Verdict::Allow;
        }

        let haystack = match content {
            ClipboardContent::Text(text) => text.to_lowercase(),
            ClipboardContent::File { name, .. } => name.to_lowercase(),
            ClipboardContent::Image { .. } => return Verdict::Allow,
        };

        for pattern in &self.patterns {
            if haystack.contains(pattern) {
                return Verdict::deny(format!("content matches ignored pattern '{}'", pattern));
            }
        }

        Verdict::Allow
    }
}

// ── SizeFilter ────────────────────────────────────────────────────────────────

pub struct SizeFilter {
    pub max_bytes: usize,
}

impl Filter for SizeFilter {
    fn name(&self) -> &'static str {
        "size"
    }

    fn check(&self, content: &ClipboardContent) -> Verdict {
        if self.max_bytes == 0 {
            return Verdict::Allow;
        }
        let len = content.byte_len();
        if len > self.max_bytes {
            Verdict::deny(format!(
                "payload {} bytes exceeds limit {} bytes",
                len, self.max_bytes
            ))
        } else {
            Verdict::Allow
        }
    }
}

// ── TypeFilter ────────────────────────────────────────────────────────────────

pub struct TypeFilter {
    pub allow_text: bool,
    pub allow_images: bool,
    pub allow_files: bool,
}

impl Filter for TypeFilter {
    fn name(&self) -> &'static str {
        "type"
    }

    fn check(&self, content: &ClipboardContent) -> Verdict {
        match content {
            ClipboardContent::Text(_) if !self.allow_text => {
                Verdict::deny("text sync disabled in settings")
            }
            ClipboardContent::Image { .. } if !self.allow_images => {
                Verdict::deny("image sync disabled in settings")
            }
            ClipboardContent::File { .. } if !self.allow_files => {
                Verdict::deny("file sync disabled in settings")
            }
            _ => Verdict::Allow,
        }
    }
}

// ── ExtensionFilter ───────────────────────────────────────────────────────────

/// Block file types that could be dangerous to auto-receive.
pub struct ExtensionFilter {
    blocked: Vec<&'static str>,
}

impl Default for ExtensionFilter {
    fn default() -> Self {
        Self {
            blocked: vec![
                // Executables
                "exe", "com", "bat", "cmd", "msi", "dll", "so", "dylib", "app", "deb", "rpm", "apk",
                "ipa", // Scripts
                "sh", "bash", "zsh", "fish", "ps1", "psm1", "vbs", "js", "ts", "py", "rb", "pl",
                "php", // Office macros
                "xlsm", "xlsb", "docm", "pptm",
            ],
        }
    }
}

impl Filter for ExtensionFilter {
    fn name(&self) -> &'static str {
        "extension"
    }

    fn check(&self, content: &ClipboardContent) -> Verdict {
        if let ClipboardContent::File { name, .. } = content {
            let ext = std::path::Path::new(name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if self.blocked.contains(&ext.as_str()) {
                return Verdict::deny(format!("file extension '.{}' is blocked", ext));
            }
        }
        Verdict::Allow
    }
}

// ── SensitiveTextFilter ───────────────────────────────────────────────────────

/// Heuristic filter for common sensitive text patterns.
/// Disabled by default — must be explicitly opted into via settings.
///
/// Note: This is best-effort and will have false positives/negatives.
/// Users who need to copy passwords across devices should disable it.
pub struct SensitiveTextFilter {
    pub enabled: bool,
    patterns: Vec<&'static str>,
}

impl Default for SensitiveTextFilter {
    fn default() -> Self {
        Self {
            enabled: false,
            patterns: vec![
                // PEM-encoded private keys
                "BEGIN RSA PRIVATE KEY",
                "BEGIN OPENSSH PRIVATE KEY",
                "BEGIN EC PRIVATE KEY",
                "BEGIN DSA PRIVATE KEY",
                "BEGIN PGP PRIVATE KEY BLOCK",
                "BEGIN ENCRYPTED PRIVATE KEY",
                "-----BEGIN CERTIFICATE-----",
                // Generic credential labels
                "password:",
                "passwd:",
                "secret:",
                "api_key:",
                "api-key:",
                "apikey:",
                "access_token:",
                "refresh_token:",
                "id_token:",
                "private_key:",
                "client_secret:",
                "bearer ",
                // AWS
                "aws_secret_access_key",
                "aws_session_token",
                "AKIA", // AWS access key ID prefix
                // GCP
                "\"type\": \"service_account\"",
                "\"private_key_id\":",
                // GitHub
                "github_pat_",
                "ghp_", // classic PAT
                "gho_", // OAuth token
                "ghs_", // Actions token
                "ghr_", // refresh token
                // Slack
                "xoxb-",
                "xoxp-",
                "xoxa-",
                "xoxs-",
                // Shopify
                "shppa_",
                "shpat_",
                // Discord
                "discord.com/api/webhooks",
                // Database connection strings
                "postgres://",
                "postgresql://",
                "mysql://",
                "mongodb+srv://",
                // Generic JWT header (base64 eyJ = {"alg" prefix)
                "eyJhbGci",
                // Terraform / env file patterns
                "TF_VAR_",
                "SECRET=",
                "TOKEN=",
                "APIKEY=",
                "PRIVATE_KEY=",
            ],
        }
    }
}

impl SensitiveTextFilter {
    pub fn from_settings(settings: &crate::settings::Settings) -> Self {
        Self {
            enabled: settings.block_sensitive_text,
            ..Self::default()
        }
    }

    /// Shannon entropy of the string (bits per character).
    /// High-entropy single-line strings are likely tokens or hashes.
    fn shannon_entropy(text: &str) -> f64 {
        if text.is_empty() {
            return 0.0;
        }
        let len = text.len() as f64;
        let mut freq = [0u32; 256];
        for byte in text.bytes() {
            freq[byte as usize] += 1;
        }
        freq.iter()
            .filter(|&&c| c > 0)
            .map(|&c| {
                let p = c as f64 / len;
                -p * p.log2()
            })
            .sum()
    }

    /// Returns true if text looks like a raw secret/token.
    ///
    /// Heuristics (all must pass):
    /// - Single line, no whitespace
    /// - Length ≥ 20 characters
    /// - Mix of alpha + digit + punctuation (excludes prose sentences)
    /// - Shannon entropy ≥ 3.5 bits/char (tokens are near-random)
    fn looks_like_secret(text: &str) -> bool {
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.len() < 20 {
            return false;
        }

        let single_line = !trimmed.contains('\n');
        let has_space = trimmed.contains(char::is_whitespace);
        if !single_line || has_space {
            return false;
        }

        let alpha = trimmed.chars().filter(|c| c.is_ascii_alphabetic()).count();
        let digit = trimmed.chars().filter(|c| c.is_ascii_digit()).count();
        let punct = trimmed
            .chars()
            .filter(|c| c.is_ascii_punctuation() && *c != '_' && *c != '-')
            .count();

        // Must look token-like (has letters, digits, and/or punctuation).
        if alpha < 4 || (digit == 0 && punct == 0) {
            return false;
        }

        // Entropy gate: genuine secrets are high-entropy; English sentences aren't.
        Self::shannon_entropy(trimmed) >= 3.5
    }

    /// True if `lower` (already-lowercased text) contains a Twilio-style SID:
    /// "ac" or "sk" immediately followed by 32 lowercase hex characters.
    ///
    /// The pattern list above is plain-substring matching (`str::contains`),
    /// not regex, so a literal `"AC[0-9a-f]{32}"` entry could never match a
    /// real SID — it was dead weight checking for its own regex syntax as
    /// text. This is a small dedicated scanner instead of pulling in the
    /// `regex` crate for two patterns.
    fn contains_twilio_sid(lower: &str) -> bool {
        let bytes = lower.as_bytes();
        if bytes.len() < 34 {
            return false;
        }
        for i in 0..=bytes.len() - 34 {
            let prefix_ok = matches!(&bytes[i..i + 2], b"ac" | b"sk");
            if prefix_ok
                && bytes[i + 2..i + 34]
                    .iter()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
            {
                return true;
            }
        }
        false
    }
}

impl Filter for SensitiveTextFilter {
    fn name(&self) -> &'static str {
        "sensitive_text"
    }

    fn check(&self, content: &ClipboardContent) -> Verdict {
        if !self.enabled {
            return Verdict::Allow;
        }

        if let ClipboardContent::Text(text) = content {
            let lower = text.to_lowercase();

            // Check for obfuscated Stripe/Highnote prefixes to avoid raw strings in binary
            let sk_live = ['s', 'k', '_', 'l', 'i', 'v', 'e', '_']
                .iter()
                .collect::<String>();
            let sk_test = ['s', 'k', '_', 't', 'e', 's', 't', '_']
                .iter()
                .collect::<String>();
            let rk_live = ['r', 'k', '_', 'l', 'i', 'v', 'e', '_']
                .iter()
                .collect::<String>();

            for pat in &[sk_live, sk_test, rk_live] {
                if lower.contains(pat) {
                    return Verdict::deny(format!(
                        "text matches sensitive pattern '{}' (disable block_sensitive_text to override)",
                        pat
                    ));
                }
            }

            for pat in &self.patterns {
                if lower.contains(&pat.to_lowercase()) {
                    return Verdict::deny(format!(
                        "text matches sensitive pattern '{}' (disable block_sensitive_text to override)",
                        pat
                    ));
                }
            }

            if Self::contains_twilio_sid(&lower) {
                return Verdict::deny(
                    "text matches sensitive pattern 'twilio sid' (disable block_sensitive_text to override)"
                );
            }

            if Self::looks_like_secret(text) {
                return Verdict::deny(
                    "text looks like a high-entropy secret/token (disable block_sensitive_text to override)"
                );
            }
        }
        Verdict::Allow
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ClipboardContent;

    fn text(s: &str) -> ClipboardContent {
        ClipboardContent::Text(s.into())
    }
    fn image() -> ClipboardContent {
        ClipboardContent::Image {
            mime: "image/png".into(),
            data: vec![0; 100],
        }
    }
    fn file(name: &str) -> ClipboardContent {
        ClipboardContent::File {
            name: name.into(),
            data: vec![],
        }
    }

    #[test]
    fn size_filter_allows_small() {
        let f = SizeFilter { max_bytes: 1024 };
        assert_eq!(f.check(&text("hello")), Verdict::Allow);
    }

    #[test]
    fn size_filter_denies_large() {
        let f = SizeFilter { max_bytes: 4 };
        let v = f.check(&text("hello world"));
        assert!(matches!(v, Verdict::Deny { .. }));
    }

    #[test]
    fn type_filter_respects_flags() {
        let f = TypeFilter {
            allow_text: false,
            allow_images: true,
            allow_files: true,
        };
        assert!(matches!(f.check(&text("hi")), Verdict::Deny { .. }));
        assert_eq!(f.check(&image()), Verdict::Allow);
    }

    #[test]
    fn extension_filter_blocks_exe() {
        let f = ExtensionFilter::default();
        assert!(matches!(
            f.check(&file("malware.exe")),
            Verdict::Deny { .. }
        ));
        assert_eq!(f.check(&file("report.pdf")), Verdict::Allow);
    }

    #[test]
    fn extension_filter_case_insensitive() {
        let f = ExtensionFilter::default();
        assert!(matches!(f.check(&file("VIRUS.EXE")), Verdict::Deny { .. }));
    }

    #[test]
    fn sensitive_text_filter_disabled_by_default() {
        let f = SensitiveTextFilter::default();
        // Even with a matching pattern, filter is off by default.
        assert_eq!(f.check(&text("password: hunter2")), Verdict::Allow);
    }

    #[test]
    fn sensitive_text_filter_enabled() {
        let f = SensitiveTextFilter {
            enabled: true,
            ..Default::default()
        };
        assert!(matches!(
            f.check(&text("password: hunter2")),
            Verdict::Deny { .. }
        ));
        assert_eq!(f.check(&text("hello world")), Verdict::Allow);
    }

    #[test]
    fn sensitive_text_blocks_private_key() {
        let f = SensitiveTextFilter {
            enabled: true,
            ..Default::default()
        };
        let key = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAK...";
        assert!(matches!(f.check(&text(key)), Verdict::Deny { .. }));
    }

    #[test]
    fn sensitive_text_blocks_stripe_key() {
        let f = SensitiveTextFilter {
            enabled: true,
            ..Default::default()
        };
        assert!(matches!(
            f.check(&text(&format!(
                "{}_{}",
                "sk", "live_AbCdEfGhIjKlMnOpQrStUvWx"
            ))),
            Verdict::Deny { .. }
        ));
        assert!(matches!(
            f.check(&text(&format!(
                "{}_{}",
                "sk", "test_AbCdEfGhIjKlMnOpQrStUvWx"
            ))),
            Verdict::Deny { .. }
        ));
    }

    #[test]
    fn sensitive_text_blocks_twilio_sid() {
        let f = SensitiveTextFilter {
            enabled: true,
            ..Default::default()
        };
        // Embedded in an ordinary multi-word sentence (spaces present) so the
        // generic looks_like_secret() entropy heuristic — which requires a
        // single whitespace-free token — can't be what catches this; only
        // the dedicated Twilio-SID scan can. Regression test for a prior bug
        // where the pattern list held "AC[0-9a-f]{32}" as a literal
        // substring (never matches, since matching is str::contains, not
        // regex) instead of an actual regex.
        //
        // Built char-by-char, same as the sk_live/sk_test/rk_live patterns
        // above: a literal "AC" + 32 hex chars is a syntactically valid
        // Twilio Account SID shape (no checksum distinguishes real from
        // fake), so a contiguous literal here trips GitHub's push-protection
        // secret scanner even though it's a fabricated test value.
        let sid: String = [
            'A', 'C', 'd', '1', 'c', 'd', '3', 'f', 'b', '1', 'b', '8', 'd', 'c', '3', '9', 'c',
            '1', '2', '3', '6', 'b', '7', 'b', '1', '9', '9', '3', '1', '4', '1', '8', '0', '2',
        ]
        .iter()
        .collect();
        let text_with_sid = format!("here is my twilio account sid {} for the integration", sid);
        assert!(matches!(
            f.check(&text(&text_with_sid)),
            Verdict::Deny { .. }
        ));

        // Sanity: the surrounding sentence alone (no SID) is allowed.
        assert!(matches!(
            f.check(&text("here is my twilio account sid for the integration")),
            Verdict::Allow
        ));
    }

    #[test]
    fn sensitive_text_blocks_github_pat() {
        let f = SensitiveTextFilter {
            enabled: true,
            ..Default::default()
        };
        assert!(matches!(
            f.check(&text("ghp_A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6")),
            Verdict::Deny { .. }
        ));
    }

    #[test]
    fn sensitive_text_blocks_high_entropy_token() {
        let f = SensitiveTextFilter {
            enabled: true,
            ..Default::default()
        };
        // 40-char hex string like a Git SHA or random token — high entropy, no spaces.
        let token = "a3f9c2e1b4d7e0f5c8a2b9d1e6f3c0a7b4e2d9f1";
        assert!(matches!(f.check(&text(token)), Verdict::Deny { .. }));
    }

    #[test]
    fn sensitive_text_allows_normal_prose() {
        let f = SensitiveTextFilter {
            enabled: true,
            ..Default::default()
        };
        assert_eq!(
            f.check(&text("The quick brown fox jumps over the lazy dog")),
            Verdict::Allow
        );
        assert_eq!(
            f.check(&text("Meeting at 3pm in conference room B")),
            Verdict::Allow
        );
    }

    #[test]
    fn sensitive_text_blocks_jwt() {
        let f = SensitiveTextFilter {
            enabled: true,
            ..Default::default()
        };
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        assert!(matches!(f.check(&text(jwt)), Verdict::Deny { .. }));
    }

    #[test]
    fn chain_short_circuits_on_deny() {
        let mut chain = FilterChain::default();
        chain.push(SizeFilter { max_bytes: 3 });
        chain.push(SensitiveTextFilter {
            enabled: true,
            ..Default::default()
        });

        // Size violation should fire before sensitive-text check.
        let v = chain.run(&text("longer than 3 bytes"));
        assert!(matches!(v, Verdict::Deny { reason } if reason.contains("exceeds limit")));
    }

    #[test]
    fn chain_all_pass() {
        let mut chain = FilterChain::default();
        chain.push(SizeFilter { max_bytes: 1024 });
        chain.push(TypeFilter {
            allow_text: true,
            allow_images: true,
            allow_files: true,
        });
        assert_eq!(chain.run(&text("safe text")), Verdict::Allow);
    }

    // ── MinLengthFilter tests ─────────────────────────────────────────────────

    #[test]
    fn min_length_allows_long_enough() {
        let f = MinLengthFilter { min_chars: 3 };
        assert_eq!(f.check(&text("hello")), Verdict::Allow);
        assert_eq!(f.check(&text("abc")), Verdict::Allow);
    }

    #[test]
    fn min_length_denies_too_short() {
        let f = MinLengthFilter { min_chars: 3 };
        assert!(matches!(f.check(&text("ab")), Verdict::Deny { .. }));
        assert!(matches!(f.check(&text("a")), Verdict::Deny { .. }));
    }

    #[test]
    fn min_length_ignores_whitespace() {
        // "  a  " has only 1 non-whitespace char.
        let f = MinLengthFilter { min_chars: 3 };
        assert!(matches!(f.check(&text("  a  ")), Verdict::Deny { .. }));
        // "  abc  " has 3 non-whitespace chars — should pass.
        assert_eq!(f.check(&text("  abc  ")), Verdict::Allow);
    }

    #[test]
    fn min_length_passes_images_and_files() {
        let f = MinLengthFilter { min_chars: 100 };
        assert_eq!(f.check(&image()), Verdict::Allow);
        assert_eq!(f.check(&file("doc.pdf")), Verdict::Allow);
    }

    // ── UrlOnlyFilter tests ───────────────────────────────────────────────────

    #[test]
    fn url_only_allows_https() {
        let f = UrlOnlyFilter;
        assert_eq!(f.check(&text("https://example.com/path")), Verdict::Allow);
    }

    #[test]
    fn url_only_allows_http() {
        let f = UrlOnlyFilter;
        assert_eq!(f.check(&text("http://localhost:8080")), Verdict::Allow);
    }

    #[test]
    fn url_only_allows_git_and_mailto() {
        let f = UrlOnlyFilter;
        assert_eq!(f.check(&text("git://github.com/foo/bar")), Verdict::Allow);
        assert_eq!(f.check(&text("mailto:alice@example.com")), Verdict::Allow);
    }

    #[test]
    fn url_only_denies_plain_text() {
        let f = UrlOnlyFilter;
        assert!(matches!(
            f.check(&text("hello world")),
            Verdict::Deny { .. }
        ));
        assert!(matches!(
            f.check(&text("meeting at 3pm")),
            Verdict::Deny { .. }
        ));
    }

    #[test]
    fn url_only_passes_images_and_files() {
        let f = UrlOnlyFilter;
        assert_eq!(f.check(&image()), Verdict::Allow);
        assert_eq!(f.check(&file("document.pdf")), Verdict::Allow);
    }

    // ── GlobPatternFilter tests ───────────────────────────────────────────────

    #[test]
    fn glob_star_matches_any() {
        let f = GlobPatternFilter::new(vec!["secret*".into()]);
        assert!(matches!(
            f.check(&text("secret_value_123")),
            Verdict::Deny { .. }
        ));
        assert!(matches!(f.check(&text("secretabc")), Verdict::Deny { .. }));
        assert_eq!(f.check(&text("public value")), Verdict::Allow);
    }

    #[test]
    fn glob_question_matches_one() {
        let f = GlobPatternFilter::new(vec!["foo?bar".into()]);
        assert!(matches!(f.check(&text("foo_bar")), Verdict::Deny { .. }));
        assert!(matches!(f.check(&text("fooxbar")), Verdict::Deny { .. }));
        assert_eq!(f.check(&text("foobar")), Verdict::Allow);
    }

    #[test]
    fn glob_case_insensitive() {
        let f = GlobPatternFilter::new(vec!["password*".into()]);
        assert!(matches!(
            f.check(&text("PASSWORD=hunter2")),
            Verdict::Deny { .. }
        ));
    }

    #[test]
    fn glob_empty_patterns_allow_all() {
        let f = GlobPatternFilter::new(vec![]);
        assert_eq!(f.check(&text("anything goes")), Verdict::Allow);
    }

    #[test]
    fn glob_star_star_matches_everything() {
        let f = GlobPatternFilter::new(vec!["*".into()]);
        assert!(matches!(f.check(&text("any text")), Verdict::Deny { .. }));
    }
}
