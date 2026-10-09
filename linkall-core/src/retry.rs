//! Connection retry with exponential back-off and jitter.
//!
//! Link All peers are expected to come and go (sleep/wake, Wi-Fi handoffs).
//! This module provides a reusable `Backoff` type that coordinates reconnect
//! attempts without hammering the network.
//!
//! # Policy
//! - First retry: 200 ms
//! - Each subsequent retry doubles the interval, up to `MAX_DELAY`.
//! - ±25 % random jitter prevents thundering herd when many peers restart
//!   simultaneously (e.g. whole office reboots after a power cut).
//! - After `MAX_ATTEMPTS` failures the peer is marked "unreachable" and
//!   the engine waits for a fresh mDNS announcement before trying again.

use std::time::Duration;
use tracing::debug;

// ── Configuration ─────────────────────────────────────────────────────────────

/// Default initial retry interval.
pub const INITIAL_DELAY: Duration = Duration::from_millis(200);
/// Maximum back-off interval before we give up / wait for mDNS.
pub const MAX_DELAY: Duration = Duration::from_secs(5);
/// Give up after this many consecutive failures.
pub const MAX_ATTEMPTS: u32 = 5;
/// Jitter fraction — delay ± this fraction of the current interval.
const JITTER: f64 = 0.25;

/// Configurable retry policy.
///
/// Use `RetryConfig::default()` for the standard Link All policy, or build
/// a custom one for e.g. unit tests that need faster timeouts.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Delay before the first retry attempt.
    pub initial_delay: Duration,
    /// Maximum delay after many failures (exponential back-off cap).
    pub max_delay: Duration,
    /// Maximum number of consecutive attempts before giving up.
    pub max_attempts: u32,
    /// Jitter fraction (0.0 = no jitter, 0.25 = ±25 %).
    pub jitter: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            initial_delay: INITIAL_DELAY,
            max_delay: MAX_DELAY,
            max_attempts: MAX_ATTEMPTS,
            jitter: JITTER,
        }
    }
}

impl RetryConfig {
    /// Fast retry config suitable for unit tests.
    pub fn fast() -> Self {
        Self {
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(16),
            max_attempts: 4,
            jitter: 0.0,
        }
    }
}

// ── Backoff ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Backoff {
    config: RetryConfig,
    current: Duration,
    attempts: u32,
    peer_label: String,
}

impl Backoff {
    pub fn new(peer_label: impl Into<String>) -> Self {
        let config = RetryConfig::default();
        let initial = config.initial_delay;
        Self {
            config,
            current: initial,
            attempts: 0,
            peer_label: peer_label.into(),
        }
    }

    /// Create a `Backoff` with a custom `RetryConfig`.
    pub fn with_config(peer_label: impl Into<String>, config: RetryConfig) -> Self {
        let initial = config.initial_delay;
        Self {
            config,
            current: initial,
            attempts: 0,
            peer_label: peer_label.into(),
        }
    }

    /// Returns `Some(delay_to_sleep)` if another attempt should be made,
    /// `None` if the peer is considered unreachable for now.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Duration> {
        if self.attempts >= self.config.max_attempts {
            debug!(
                "[retry] {} — gave up after {} attempts",
                self.peer_label, self.attempts
            );
            return None;
        }

        self.attempts += 1;
        let delay = self.jittered(self.current);
        debug!(
            "[retry] {} — attempt {} in {:.0}ms",
            self.peer_label,
            self.attempts,
            delay.as_millis()
        );

        // Advance for next call.
        self.current = (self.current * 2).min(self.config.max_delay);
        Some(delay)
    }

    /// Reset after a successful connection.
    pub fn reset(&mut self) {
        self.current = self.config.initial_delay;
        self.attempts = 0;
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    pub fn exhausted(&self) -> bool {
        self.attempts >= self.config.max_attempts
    }

    fn jittered(&self, base: Duration) -> Duration {
        let base_ns = base.as_nanos() as f64;
        let range = base_ns * self.config.jitter;
        // Uniform random in [-range, +range].
        let offset = (rand::random::<f64>() * 2.0 - 1.0) * range;
        let ns = (base_ns + offset).max(1.0) as u64;
        Duration::from_nanos(ns)
    }
}

// ── retry_async helper ────────────────────────────────────────────────────────

/// Run `op` with automatic retry using the given `Backoff`.
/// Returns the first `Ok` result, or the last error if all attempts fail.
pub async fn retry_async<F, Fut, T, E>(backoff: &mut Backoff, mut op: F) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Debug,
{
    loop {
        match op().await {
            Ok(v) => {
                backoff.reset();
                return Ok(v);
            }
            Err(e) => match backoff.next() {
                Some(delay) => {
                    debug!("[retry] error {:?} — sleeping {:?}", e, delay);
                    tokio::time::sleep(delay).await;
                }
                None => return Err(e),
            },
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles() {
        let mut b = Backoff::with_config(
            "test",
            RetryConfig {
                jitter: 0.0,
                ..RetryConfig::default()
            },
        );
        let d0 = b.next().unwrap();
        let d1 = b.next().unwrap();
        // Second delay must be exactly 2× the first (no jitter).
        let ratio = d1.as_nanos() as f64 / d0.as_nanos() as f64;
        assert!((1.9..=2.1).contains(&ratio), "ratio {:.3}", ratio);
    }

    #[test]
    fn backoff_exhausts() {
        let mut b = Backoff::new("test");
        let mut count = 0;
        while b.next().is_some() {
            count += 1;
        }
        assert_eq!(count, MAX_ATTEMPTS as usize);
        assert!(b.exhausted());
    }

    #[test]
    fn backoff_resets() {
        let mut b = Backoff::new("test");
        b.next();
        b.next();
        b.next();
        assert_eq!(b.attempts(), 3);
        b.reset();
        assert_eq!(b.attempts(), 0);
        // After reset, next() returns the initial delay again.
        let d = b.next().unwrap();
        // Should be close to INITIAL_DELAY ±25 %.
        let lo = (INITIAL_DELAY.as_millis() as f64 * 0.75) as u128;
        let hi = (INITIAL_DELAY.as_millis() as f64 * 1.25) as u128;
        assert!((lo..=hi).contains(&d.as_millis()), "{:?}", d);
    }

    #[test]
    fn backoff_caps_at_max() {
        let mut b = Backoff::new("test");
        // Burn through many attempts; delay must never exceed MAX_DELAY + jitter.
        for _ in 0..MAX_ATTEMPTS {
            if let Some(d) = b.next() {
                let cap = (MAX_DELAY.as_millis() as f64 * 1.25) as u128;
                assert!(d.as_millis() <= cap, "{:?}", d);
            }
        }
    }

    #[test]
    fn custom_config_fast() {
        let cfg = RetryConfig::fast();
        let mut b = Backoff::with_config("fast-test", cfg.clone());
        let mut count = 0;
        while b.next().is_some() {
            count += 1;
        }
        assert_eq!(count, cfg.max_attempts as usize);
    }

    #[tokio::test]
    async fn retry_async_succeeds_on_second_try() {
        use std::sync::{
            atomic::{AtomicU32, Ordering},
            Arc,
        };

        let mut b = Backoff::with_config("test", RetryConfig::fast());
        let calls = Arc::new(AtomicU32::new(0));
        let calls_for_retry = calls.clone();
        let result: Result<u32, &str> = retry_async(&mut b, || async {
            let call = calls_for_retry.fetch_add(1, Ordering::Relaxed) + 1;
            if call < 2 {
                Err("not yet")
            } else {
                Ok(42)
            }
        })
        .await;
        assert_eq!(result.unwrap(), 42);
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }
}
