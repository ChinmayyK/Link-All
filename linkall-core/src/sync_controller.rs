use crate::dedup::{hash_content, ContentHash};
use crate::filter::{FilterChain, Verdict};
use crate::protocol::ClipboardContent;
use crate::settings::Settings;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum SyncDecision {
    Allow { hash: ContentHash },
    Skip { reason: String },
}

/// Decision counters for `SyncController` — useful for metrics dashboards.
#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    /// Items allowed through to peers.
    pub allowed: u64,
    /// Items blocked by a filter rule (sensitive text, type, size).
    pub filter_skipped: u64,
    /// Items suppressed by the duplicate-hash window.
    pub dedup_skipped: u64,
    /// Items suppressed by the debounce window (rapid re-copy of same content).
    pub debounce_skipped: u64,
}

impl SyncStats {
    pub fn total_skipped(&self) -> u64 {
        self.filter_skipped + self.dedup_skipped + self.debounce_skipped
    }

    /// Fraction of evaluated items that were allowed (0.0–1.0).
    pub fn allow_rate(&self) -> f64 {
        let total = self.allowed + self.total_skipped();
        if total == 0 {
            1.0
        } else {
            self.allowed as f64 / total as f64
        }
    }
}

pub struct SyncController {
    filter: FilterChain,
    duplicate_window: Duration,
    debounce_window: Duration,
    recent_hashes: HashMap<ContentHash, Instant>,
    last_hash: Option<ContentHash>,
    last_copy_at: Option<Instant>,
    stats: SyncStats,
}

impl SyncController {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            filter: FilterChain::from_settings(settings),
            duplicate_window: Duration::from_millis(settings.smart_sync_duplicate_window_ms),
            debounce_window: Duration::from_millis(settings.smart_sync_debounce_ms),
            recent_hashes: HashMap::new(),
            last_hash: None,
            last_copy_at: None,
            stats: SyncStats::default(),
        }
    }

    pub fn evaluate_outgoing(&mut self, content: &ClipboardContent) -> SyncDecision {
        // Evict stale hashes on every call — not just on Allow — so the
        // duplicate window shrinks correctly even when we're suppressing output.
        let now = Instant::now();
        self.recent_hashes
            .retain(|_, seen_at| now.duration_since(*seen_at) <= self.duplicate_window);

        match self.filter.run(content) {
            Verdict::Allow => {}
            Verdict::Deny { reason } => {
                self.stats.filter_skipped += 1;
                return SyncDecision::Skip { reason };
            }
        }

        let hash = hash_content(content);
        if let Some(last_seen) = self.recent_hashes.get(&hash) {
            if now.duration_since(*last_seen) <= self.duplicate_window {
                self.stats.dedup_skipped += 1;
                return SyncDecision::Skip {
                    reason: format!(
                        "rapid duplicate copy suppressed ({}ms window)",
                        self.duplicate_window.as_millis()
                    ),
                };
            }
        }

        if self.last_hash == Some(hash) {
            if let Some(last_copy_at) = self.last_copy_at {
                if now.duration_since(last_copy_at) <= self.debounce_window {
                    self.stats.debounce_skipped += 1;
                    return SyncDecision::Skip {
                        reason: format!(
                            "copy event debounced ({}ms window)",
                            self.debounce_window.as_millis()
                        ),
                    };
                }
            }
        }

        self.recent_hashes.insert(hash, now);
        self.last_hash = Some(hash);
        self.last_copy_at = Some(now);
        self.stats.allowed += 1;
        SyncDecision::Allow { hash }
    }

    /// Point-in-time snapshot of sync decision counters.
    pub fn stats(&self) -> &SyncStats {
        &self.stats
    }

    /// Reset counters (filter/window state is unchanged).
    pub fn reset_stats(&mut self) {
        self.stats = SyncStats::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings {
            smart_sync_duplicate_window_ms: 1_000,
            smart_sync_debounce_ms: 100,
            ..Settings::default()
        }
    }

    #[test]
    fn sensitive_text_can_be_blocked() {
        let mut s = settings();
        s.block_sensitive_text = true;
        let mut controller = SyncController::from_settings(&s);
        let decision =
            controller.evaluate_outgoing(&ClipboardContent::Text("password: hunter2".into()));
        assert!(matches!(decision, SyncDecision::Skip { .. }));
        assert_eq!(controller.stats().filter_skipped, 1);
        assert_eq!(controller.stats().allowed, 0);
    }

    #[test]
    fn duplicate_copy_is_suppressed() {
        let mut controller = SyncController::from_settings(&settings());
        let content = ClipboardContent::Text("hello".into());
        assert!(matches!(
            controller.evaluate_outgoing(&content),
            SyncDecision::Allow { .. }
        ));
        assert!(matches!(
            controller.evaluate_outgoing(&content),
            SyncDecision::Skip { .. }
        ));
        assert_eq!(controller.stats().allowed, 1);
        assert_eq!(controller.stats().dedup_skipped, 1);
    }

    #[test]
    fn different_content_always_allowed() {
        let mut controller = SyncController::from_settings(&settings());
        for word in ["alpha", "beta", "gamma"] {
            assert!(matches!(
                controller.evaluate_outgoing(&ClipboardContent::Text(word.into())),
                SyncDecision::Allow { .. }
            ));
        }
        assert_eq!(controller.stats().allowed, 3);
    }

    #[test]
    fn stats_reset_clears_counters() {
        let mut controller = SyncController::from_settings(&settings());
        let c = ClipboardContent::Text("hi".into());
        controller.evaluate_outgoing(&c);
        controller.evaluate_outgoing(&c);
        assert_eq!(controller.stats().allowed, 1);
        controller.reset_stats();
        assert_eq!(controller.stats().allowed, 0);
        assert_eq!(controller.stats().total_skipped(), 0);
    }

    #[test]
    fn allow_rate_no_traffic() {
        let controller = SyncController::from_settings(&settings());
        assert_eq!(controller.stats().allow_rate(), 1.0);
    }

    #[test]
    fn allow_rate_with_mixed_traffic() {
        let mut controller = SyncController::from_settings(&settings());
        controller.evaluate_outgoing(&ClipboardContent::Text("a".into()));
        controller.evaluate_outgoing(&ClipboardContent::Text("b".into()));
        controller.evaluate_outgoing(&ClipboardContent::Text("a".into())); // dedup
        let rate = controller.stats().allow_rate();
        assert!((rate - 2.0 / 3.0).abs() < 0.01, "rate={}", rate);
    }
}
