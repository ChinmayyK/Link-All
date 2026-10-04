//! Link All runtime metrics.
//!
//! Provides a lightweight, lock-free-friendly snapshot of operational stats
//! that the UI layer can poll to show "Synced 12 items · avg 23 ms" status.
//!
//! All counters are `AtomicU64` so they can be updated from any thread
//! without locking.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

// ── Global counters ───────────────────────────────────────────────────────────

pub struct GlobalMetrics {
    /// Total clipboard pushes sent from this device.
    pub pushes_sent: AtomicU64,
    /// Total clipboard pushes received from all peers.
    pub pushes_received: AtomicU64,
    /// Total bytes sent (plaintext content).
    pub bytes_sent: AtomicU64,
    /// Total bytes received (plaintext content).
    pub bytes_received: AtomicU64,
    /// Total items suppressed by deduplication.
    pub dedup_suppressed: AtomicU64,
    /// Total items dropped by rate limiter.
    pub rate_limited: AtomicU64,
    /// Total connection errors.
    pub connection_errors: AtomicU64,
    /// Daemon start time.
    pub start_time: Instant,
}

impl GlobalMetrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            pushes_sent: AtomicU64::new(0),
            pushes_received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            dedup_suppressed: AtomicU64::new(0),
            rate_limited: AtomicU64::new(0),
            connection_errors: AtomicU64::new(0),
            start_time: Instant::now(),
        })
    }

    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Format uptime as `Xd Xh Xm Xs`.
    pub fn format_uptime(&self) -> String {
        let total = self.uptime().as_secs();
        let days = total / 86_400;
        let hours = (total % 86_400) / 3_600;
        let mins = (total % 3_600) / 60;
        let secs = total % 60;
        if days > 0 {
            format!("{}d {}h {}m", days, hours, mins)
        } else if hours > 0 {
            format!("{}h {}m {}s", hours, mins, secs)
        } else if mins > 0 {
            format!("{}m {}s", mins, secs)
        } else {
            format!("{}s", secs)
        }
    }

    /// Human-readable summary line.
    pub fn summary(&self) -> String {
        let sent_kb = self.bytes_sent.load(Relaxed) / 1024;
        let recv_kb = self.bytes_received.load(Relaxed) / 1024;
        format!(
            "↑{} pushes ({} KB)  ↓{} pushes ({} KB)  dedup={} rate_limited={} errors={}  uptime={}",
            self.pushes_sent.load(Relaxed),
            sent_kb,
            self.pushes_received.load(Relaxed),
            recv_kb,
            self.dedup_suppressed.load(Relaxed),
            self.rate_limited.load(Relaxed),
            self.connection_errors.load(Relaxed),
            self.format_uptime(),
        )
    }

    /// Produce a serializable point-in-time snapshot.
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            pushes_sent: self.pushes_sent.load(Relaxed),
            pushes_received: self.pushes_received.load(Relaxed),
            bytes_sent: self.bytes_sent.load(Relaxed),
            bytes_received: self.bytes_received.load(Relaxed),
            dedup_suppressed: self.dedup_suppressed.load(Relaxed),
            rate_limited: self.rate_limited.load(Relaxed),
            connection_errors: self.connection_errors.load(Relaxed),
            uptime_secs: self.uptime().as_secs(),
        }
    }
}

impl Default for GlobalMetrics {
    fn default() -> Self {
        Self {
            pushes_sent: AtomicU64::new(0),
            pushes_received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            dedup_suppressed: AtomicU64::new(0),
            rate_limited: AtomicU64::new(0),
            connection_errors: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }
}

/// Serializable point-in-time snapshot of global metrics.
///
/// Use this for JSON export to the CLI, IPC responses, or telemetry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MetricsSnapshot {
    pub pushes_sent: u64,
    pub pushes_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub dedup_suppressed: u64,
    pub rate_limited: u64,
    pub connection_errors: u64,
    pub uptime_secs: u64,
}

impl MetricsSnapshot {
    /// True if no traffic has been observed yet.
    pub fn is_idle(&self) -> bool {
        self.pushes_sent == 0 && self.pushes_received == 0
    }

    /// Net bytes transferred (sent + received).
    pub fn total_bytes(&self) -> u64 {
        self.bytes_sent.saturating_add(self.bytes_received)
    }
}

// ── Per-peer latency tracker ──────────────────────────────────────────────────

/// Rolling window of the last N round-trip times for a peer.
pub struct LatencyTracker {
    samples: Vec<u64>, // microseconds
    head: usize,
    full: bool,
    capacity: usize,
}

impl LatencyTracker {
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: vec![0; capacity],
            head: 0,
            full: false,
            capacity,
        }
    }

    pub fn record(&mut self, rtt_us: u64) {
        self.samples[self.head] = rtt_us;
        self.head = (self.head + 1) % self.capacity;
        if self.head == 0 {
            self.full = true;
        }
    }

    fn active_samples(&self) -> &[u64] {
        if self.full {
            &self.samples
        } else {
            &self.samples[..self.head]
        }
    }

    pub fn avg_us(&self) -> Option<u64> {
        let s = self.active_samples();
        if s.is_empty() {
            return None;
        }
        Some(s.iter().sum::<u64>() / s.len() as u64)
    }

    pub fn min_us(&self) -> Option<u64> {
        self.active_samples().iter().copied().min()
    }

    pub fn max_us(&self) -> Option<u64> {
        self.active_samples().iter().copied().max()
    }

    pub fn p95_us(&self) -> Option<u64> {
        let mut s = self.active_samples().to_vec();
        if s.is_empty() {
            return None;
        }
        s.sort_unstable();
        let idx = (s.len() as f64 * 0.95) as usize;
        Some(s[idx.min(s.len() - 1)])
    }

    pub fn sample_count(&self) -> usize {
        self.active_samples().len()
    }
}

// ── Peer stats ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PeerStats {
    pub device_id: Uuid,
    pub device_name: String,
    pub connected_at: Instant,
    pub pushes_sent: u64,
    pub pushes_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub avg_rtt_us: Option<u64>,
    pub min_rtt_us: Option<u64>,
    pub max_rtt_us: Option<u64>,
    pub p95_rtt_us: Option<u64>,
}

impl PeerStats {
    pub fn session_duration(&self) -> Duration {
        self.connected_at.elapsed()
    }

    pub fn avg_rtt_ms(&self) -> Option<f64> {
        self.avg_rtt_us.map(|us| us as f64 / 1000.0)
    }
}

// ── Metrics registry ──────────────────────────────────────────────────────────

pub struct MetricsRegistry {
    pub global: Arc<GlobalMetrics>,
    peers: RwLock<HashMap<Uuid, PeerMetricsEntry>>,
}

struct PeerMetricsEntry {
    name: String,
    connected_at: Instant,
    pushes_sent: u64,
    pushes_received: u64,
    bytes_sent: u64,
    bytes_received: u64,
    latency: LatencyTracker,
}

impl MetricsRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            global: GlobalMetrics::new(),
            peers: RwLock::new(HashMap::new()),
        })
    }

    // ── Peer lifecycle ────────────────────────────────────────────────────────

    pub fn peer_connected(&self, device_id: Uuid, name: String) {
        self.peers
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                device_id,
                PeerMetricsEntry {
                    name,
                    connected_at: Instant::now(),
                    pushes_sent: 0,
                    pushes_received: 0,
                    bytes_sent: 0,
                    bytes_received: 0,
                    latency: LatencyTracker::new(50),
                },
            );
    }

    pub fn peer_disconnected(&self, device_id: Uuid) {
        self.peers
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&device_id);
    }

    // ── Record events ─────────────────────────────────────────────────────────

    pub fn record_send(&self, device_id: Uuid, bytes: u64) {
        self.global.pushes_sent.fetch_add(1, Relaxed);
        self.global.bytes_sent.fetch_add(bytes, Relaxed);
        if let Some(p) = self
            .peers
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&device_id)
        {
            p.pushes_sent += 1;
            p.bytes_sent += bytes;
        }
    }

    pub fn record_receive(&self, device_id: Uuid, bytes: u64) {
        self.global.pushes_received.fetch_add(1, Relaxed);
        self.global.bytes_received.fetch_add(bytes, Relaxed);
        if let Some(p) = self
            .peers
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&device_id)
        {
            p.pushes_received += 1;
            p.bytes_received += bytes;
        }
    }

    pub fn record_rtt(&self, device_id: Uuid, rtt_us: u64) {
        if let Some(p) = self
            .peers
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&device_id)
        {
            p.latency.record(rtt_us);
        }
    }

    pub fn record_dedup_suppressed(&self) {
        self.global.dedup_suppressed.fetch_add(1, Relaxed);
    }

    pub fn record_rate_limited(&self) {
        self.global.rate_limited.fetch_add(1, Relaxed);
    }

    pub fn record_connection_error(&self) {
        self.global.connection_errors.fetch_add(1, Relaxed);
    }

    // ── Snapshots ─────────────────────────────────────────────────────────────

    pub fn all_peer_stats(&self) -> Vec<PeerStats> {
        self.peers
            .read()
            .unwrap()
            .iter()
            .map(|(id, p)| PeerStats {
                device_id: *id,
                device_name: p.name.clone(),
                connected_at: p.connected_at,
                pushes_sent: p.pushes_sent,
                pushes_received: p.pushes_received,
                bytes_sent: p.bytes_sent,
                bytes_received: p.bytes_received,
                avg_rtt_us: p.latency.avg_us(),
                min_rtt_us: p.latency.min_us(),
                max_rtt_us: p.latency.max_us(),
                p95_rtt_us: p.latency.p95_us(),
            })
            .collect()
    }

    pub fn peer_count(&self) -> usize {
        self.peers.read().unwrap_or_else(|e| e.into_inner()).len()
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self {
            global: GlobalMetrics::new(),
            peers: RwLock::new(HashMap::new()),
        }
    }
}

// ── Throughput estimator ──────────────────────────────────────────────────────

/// Measures actual bytes-per-second over a sliding 5-second window.
pub struct ThroughputEstimator {
    samples: std::collections::VecDeque<(Instant, u64)>, // (timestamp, bytes)
    window: Duration,
}

impl ThroughputEstimator {
    pub fn new() -> Self {
        Self {
            samples: std::collections::VecDeque::new(),
            window: Duration::from_secs(5),
        }
    }

    /// Record `bytes` transferred right now.
    pub fn record(&mut self, bytes: u64) {
        let now = Instant::now();
        self.samples.push_back((now, bytes));
        // Evict samples older than the window.
        while self
            .samples
            .front()
            .map(|(t, _)| now - *t > self.window)
            .unwrap_or(false)
        {
            self.samples.pop_front();
        }
    }

    /// Estimated bytes/second over the last window.
    pub fn bps(&self) -> f64 {
        if self.samples.len() < 2 {
            return 0.0;
        }
        let total: u64 = self.samples.iter().map(|(_, b)| b).sum();
        let span = self
            .samples
            .back()
            .unwrap()
            .0
            .duration_since(self.samples.front().unwrap().0)
            .as_secs_f64();
        if span < 0.001 {
            return 0.0;
        }
        total as f64 / span
    }

    /// Human-readable throughput string.
    pub fn display(&self) -> String {
        let bps = self.bps();
        if bps < 1_024.0 {
            format!("{:.0} B/s", bps)
        } else if bps < 1_048_576.0 {
            format!("{:.1} KB/s", bps / 1_024.0)
        } else {
            format!("{:.2} MB/s", bps / 1_048_576.0)
        }
    }
}

impl Default for ThroughputEstimator {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throughput_estimator_basic() {
        let mut est = ThroughputEstimator::new();
        est.record(1_000_000);
        std::thread::sleep(Duration::from_millis(100));
        est.record(1_000_000);
        let bps = est.bps();
        assert!(bps > 0.0, "bps = {}", bps);
    }

    #[test]
    fn latency_tracker_statistics() {
        let mut t = LatencyTracker::new(10);
        for &v in &[1000u64, 2000, 3000, 4000, 5000] {
            t.record(v);
        }
        assert_eq!(t.avg_us(), Some(3000));
        assert_eq!(t.min_us(), Some(1000));
        assert_eq!(t.max_us(), Some(5000));
        assert_eq!(t.sample_count(), 5);
    }

    #[test]
    fn latency_tracker_ring_wraps() {
        let mut t = LatencyTracker::new(3);
        for v in [10, 20, 30, 40] {
            t.record(v);
        }
        // After wrap: contains [40, 20, 30] in some order — oldest evicted.
        assert_eq!(t.sample_count(), 3);
        assert!(t.min_us().unwrap() >= 20); // 10 was evicted
    }

    #[test]
    fn latency_tracker_empty() {
        let t = LatencyTracker::new(10);
        assert_eq!(t.avg_us(), None);
        assert_eq!(t.p95_us(), None);
    }

    #[test]
    fn registry_peer_lifecycle() {
        let reg = MetricsRegistry::new();
        let id = Uuid::new_v4();
        reg.peer_connected(id, "TestPeer".into());
        assert_eq!(reg.peer_count(), 1);
        reg.record_send(id, 1024);
        reg.record_rtt(id, 5000);
        let stats = reg.all_peer_stats();
        assert_eq!(stats[0].bytes_sent, 1024);
        assert_eq!(stats[0].avg_rtt_us, Some(5000));
        reg.peer_disconnected(id);
        assert_eq!(reg.peer_count(), 0);
    }

    #[test]
    fn global_metrics_snapshot() {
        let m = GlobalMetrics::new();
        m.pushes_sent
            .fetch_add(5, std::sync::atomic::Ordering::Relaxed);
        m.bytes_sent
            .fetch_add(2048, std::sync::atomic::Ordering::Relaxed);
        let snap = m.snapshot();
        assert_eq!(snap.pushes_sent, 5);
        assert_eq!(snap.bytes_sent, 2048);
        assert!(!snap.is_idle());
        assert_eq!(snap.total_bytes(), 2048);
    }

    #[test]
    fn format_uptime_displays_correctly() {
        // We can't control Instant::now(), so just confirm no panic and non-empty.
        let m = GlobalMetrics::new();
        let s = m.format_uptime();
        assert!(!s.is_empty());
        assert!(s.ends_with('s') || s.ends_with('m') || s.ends_with('h'));
    }

    #[test]
    fn summary_includes_key_fields() {
        let m = GlobalMetrics::new();
        m.dedup_suppressed
            .fetch_add(3, std::sync::atomic::Ordering::Relaxed);
        let s = m.summary();
        assert!(s.contains("dedup=3"));
        assert!(s.contains("uptime="));
    }
}
