//! PIN-based pairing — a more explicit trust ceremony than TOFU.
//!
//! # Flow
//! 1. Both devices are on the same LAN and discover each other via mDNS.
//! 2. Device A displays: "Connecting to 'Bob's PC' — PIN: **482 917**"
//! 3. Device B simultaneously displays: "Pairing request from 'Alice's Mac' — PIN: **482 917**"
//! 4. The user visually confirms both PINs match, then taps "Trust".
//! 5. A hash of the ECDH shared secret is used to derive the PIN, so:
//!    - An active MITM attacker would produce a different PIN.
//!    - A passive eavesdropper learns nothing useful.
//!
//! # PIN derivation
//! PIN = HKDF-SHA256(shared_secret, info="deskdrop-pin") → u64 mod 10^6
//! Displayed as zero-padded 6 digits split into two groups: "482 917".
//!
//! # Security properties
//! - PIN changes with every new session (ephemeral ECDH).
//! - A brute-force attacker needs 1,000,000 guesses per connection attempt.
//! - The pairing window is short-lived (60 seconds before timeout).
//! - After pairing, the device's long-term fingerprint is stored in the
//!   trust store exactly as in TOFU mode.

use anyhow::Result;
use hkdf::Hkdf;
use sha2::Sha256;
use std::time::{Duration, Instant};

// ── PIN derivation ────────────────────────────────────────────────────────────

/// Derive a 6-digit PIN from the raw X25519 shared secret bytes.
///
/// Both devices compute the same PIN because ECDH is commutative:
/// alice.dh(bob_pub) == bob.dh(alice_pub).
pub fn derive_pin(shared_secret_bytes: &[u8], salt: &[u8]) -> PairingPin {
    let hk = Hkdf::<Sha256>::new(Some(salt), shared_secret_bytes);
    let mut okm = [0u8; 8];
    hk.expand(b"deskdrop-pin", &mut okm)
        .expect("HKDF expand never fails for 8 bytes");

    let n = u64::from_be_bytes(okm);
    let pin = (n % 1_000_000) as u32;
    PairingPin(pin)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairingPin(u32);

impl PairingPin {
    /// Zero-padded 6-digit string split into two groups: "048 291".
    pub fn display(&self) -> String {
        let s = format!("{:06}", self.0);
        format!("{} {}", &s[..3], &s[3..])
    }

    /// Raw 6-digit number for comparison.
    pub fn value(&self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for PairingPin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display())
    }
}

// ── Pairing session state machine ─────────────────────────────────────────────

/// State of a pairing attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingState {
    /// Connection initiated, waiting for handshake.
    Initiated,
    /// Handshake complete, PIN displayed to user.
    PinDisplayed,
    /// User confirmed PIN match on this device.
    Confirmed,
    /// User rejected the pairing.
    Rejected,
    /// Pairing timed out (no user action within PAIRING_TIMEOUT).
    TimedOut,
    /// Pairing completed successfully — peer is now trusted.
    Completed,
}

/// Tracks an in-progress pairing attempt for the seamless click-to-pair flow.
///
/// Created when the user clicks an untrusted device in the UI, or when
/// an untrusted peer connects to us. The session progresses through states:
///
/// ```text
/// Initiated → PinDisplayed → Confirmed → Completed
///                           → Rejected
///                           → TimedOut
/// ```
#[derive(Debug)]
pub struct PairingSession {
    pub device_id: uuid::Uuid,
    pub device_name: String,
    pub pin: PairingPin,
    pub pubkey_bytes: [u8; 32],
    pub state: PairingState,
    created_at: Instant,
}

impl PairingSession {
    pub fn new(
        device_id: uuid::Uuid,
        device_name: String,
        shared_secret: &[u8],
        salt: &[u8],
        pubkey_bytes: [u8; 32],
    ) -> Self {
        Self {
            device_id,
            device_name,
            pin: derive_pin(shared_secret, salt),
            pubkey_bytes,
            state: PairingState::Initiated,
            created_at: Instant::now(),
        }
    }

    /// Transition to PinDisplayed state.
    pub fn display_pin(&mut self) {
        if self.state == PairingState::Initiated {
            self.state = PairingState::PinDisplayed;
        }
    }

    /// User confirmed the PIN match.
    pub fn confirm(&mut self) -> bool {
        if self.state == PairingState::PinDisplayed {
            self.state = PairingState::Confirmed;
            true
        } else {
            false
        }
    }

    /// User rejected the pairing.
    pub fn reject(&mut self) -> bool {
        if self.state == PairingState::PinDisplayed || self.state == PairingState::Initiated {
            self.state = PairingState::Rejected;
            true
        } else {
            false
        }
    }

    /// Mark as completed (peer trusted).
    pub fn complete(&mut self) {
        if self.state == PairingState::Confirmed {
            self.state = PairingState::Completed;
        }
    }

    /// Check and update timeout.
    pub fn check_timeout(&mut self) -> bool {
        if self.is_expired()
            && self.state != PairingState::Completed
            && self.state != PairingState::Rejected
        {
            self.state = PairingState::TimedOut;
            true
        } else {
            false
        }
    }

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() > PAIRING_TIMEOUT
    }

    pub fn time_remaining(&self) -> Duration {
        PAIRING_TIMEOUT.saturating_sub(self.created_at.elapsed())
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            PairingState::Completed | PairingState::Rejected | PairingState::TimedOut
        )
    }
}

// ── Pairing session ───────────────────────────────────────────────────────────

/// How long a pairing request is valid before it auto-expires, on both the
/// device that asked and the one being asked. Long enough to walk to the
/// other device and compare codes.
pub const PAIRING_TIMEOUT: Duration = Duration::from_secs(60);

/// While waiting on the other device, how often to ask again (see
/// `arm_pairing_expiry`).
pub const PAIRING_RESEND: Duration = Duration::from_secs(10);

/// After our user declines a device, a new request from it within this window
/// is declined automatically, so a declined device can't nag. Declining is
/// otherwise "not now", never a block: the device may ask again later.
pub const DECLINE_COOLDOWN: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum PairingDecision {
    Approved,
    Denied,
    TimedOut,
}

/// A pending pairing request from a new device.
#[derive(Debug)]
pub struct PendingPairing {
    pub device_id: uuid::Uuid,
    pub device_name: String,
    pub pin: PairingPin,
    pub pubkey_bytes: [u8; 32],
    created_at: Instant,
}

impl PendingPairing {
    pub fn new(
        device_id: uuid::Uuid,
        device_name: String,
        shared_secret: &[u8],
        salt: &[u8],
        pubkey_bytes: [u8; 32],
    ) -> Self {
        Self {
            device_id,
            device_name,
            pin: derive_pin(shared_secret, salt),
            pubkey_bytes,
            created_at: Instant::now(),
        }
    }

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() > PAIRING_TIMEOUT
    }

    pub fn time_remaining(&self) -> Duration {
        PAIRING_TIMEOUT.saturating_sub(self.created_at.elapsed())
    }
}

// ── Pairing manager ───────────────────────────────────────────────────────────

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};
use uuid::Uuid;

type PendingPairingMap = HashMap<Uuid, (PendingPairing, oneshot::Sender<bool>)>;

pub struct PairingManager {
    /// Pending pairings awaiting user decision. device_id → (pairing, decision_tx)
    pending: Arc<Mutex<PendingPairingMap>>,
    /// Channel to notify UI of new pairing requests.
    ui_tx: mpsc::Sender<PairingRequest>,
}

/// Sent to the platform UI layer when user action is needed.
#[derive(Debug)]
pub struct PairingRequest {
    pub device_id: Uuid,
    pub device_name: String,
    pub pin: PairingPin,
    pub time_remaining_secs: u64,
}

impl PairingManager {
    pub fn new(ui_tx: mpsc::Sender<PairingRequest>) -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
            ui_tx,
        }
    }

    /// Register a new pairing request. Returns a future that resolves to the user's decision.
    pub async fn request_pairing(&self, pairing: PendingPairing) -> Result<bool> {
        let (decision_tx, decision_rx) = oneshot::channel();

        // Capture device_id before moving `pairing` into the map.
        let device_id = pairing.device_id;

        let pr = PairingRequest {
            device_id: pairing.device_id,
            device_name: pairing.device_name.clone(),
            pin: pairing.pin,
            time_remaining_secs: pairing.time_remaining().as_secs(),
        };

        {
            let mut pending = self.pending.lock().await;

            // SECURITY FIX: Bounded pairing queue to prevent DoS via UI spam and OOM.
            // If an attacker initiates thousands of handshakes, we cap the pending
            // UI prompts at 5. Any further requests are automatically denied.
            if pending.len() >= 5 && !pending.contains_key(&device_id) {
                tracing::warn!(
                    "Pairing request dropped: too many pending requests (DoS protection)"
                );
                return Ok(false);
            }

            pending.insert(pairing.device_id, (pairing, decision_tx));
        }

        self.ui_tx.send(pr).await.ok();

        // Wait for user decision with timeout.
        match tokio::time::timeout(PAIRING_TIMEOUT, decision_rx).await {
            Ok(Ok(approved)) => Ok(approved),
            Ok(Err(_)) => Ok(false), // sender dropped
            Err(_) => {
                // Timed out — clean up using the captured device_id, not Uuid::nil().
                self.pending.lock().await.remove(&device_id);
                Ok(false)
            }
        }
    }

    /// Called by the platform UI when the user approves or denies.
    pub async fn resolve(&self, device_id: Uuid, approved: bool) {
        if let Some((_, tx)) = self.pending.lock().await.remove(&device_id) {
            tx.send(approved).ok();
        }
    }

    /// Get all pending pairing requests (for UI display).
    pub async fn pending_requests(&self) -> Vec<PairingRequest> {
        self.pending
            .lock()
            .await
            .values()
            .filter(|(p, _)| !p.is_expired())
            .map(|(p, _)| PairingRequest {
                device_id: p.device_id,
                device_name: p.device_name.clone(),
                pin: p.pin,
                time_remaining_secs: p.time_remaining().as_secs(),
            })
            .collect()
    }

    /// Remove any pairing requests that have exceeded `PAIRING_TIMEOUT`.
    ///
    /// Call periodically (e.g. every 5 s) to prevent stale entries from
    /// accumulating if the UI layer forgets to call `resolve()`.
    ///
    /// v3 fix: expired entries now explicitly send `false` on the oneshot
    /// channel before dropping it, so any task awaiting `request_pairing()`
    /// receives `Ok(false)` (denied) instead of `Ok(Err(_))` (channel broken).
    pub async fn expire_stale(&self) -> usize {
        let mut pending = self.pending.lock().await;
        let before = pending.len();
        let expired_ids: Vec<Uuid> = pending
            .iter()
            .filter(|(_, (p, _))| p.is_expired())
            .map(|(id, _)| *id)
            .collect();
        for id in &expired_ids {
            if let Some((_, tx)) = pending.remove(id) {
                // Explicitly deny — the waiter sees Ok(false) not a channel error.
                let _ = tx.send(false);
            }
        }
        before - pending.len()
    }

    /// Total pending pairing count (including expired, not yet cleaned up).
    pub async fn pending_count(&self) -> usize {
        self.pending.lock().await.len()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_pin() {
        let mut secret = [0u8; 32];
        secret[0] = 42;
        let p1 = derive_pin(&secret, &[0u8; 32]);
        let p2 = derive_pin(&secret, &[0u8; 32]);
        assert_eq!(p1, p2);
    }

    #[test]
    fn pin_display_format() {
        let p = PairingPin(48291);
        assert_eq!(p.display(), "048 291");
    }

    #[test]
    fn pin_display_max() {
        let p = PairingPin(999999);
        assert_eq!(p.display(), "999 999");
    }

    #[test]
    fn different_secrets_different_pins() {
        let p1 = derive_pin(&[0xAA; 32], &[0u8; 32]);
        let p2 = derive_pin(&[0xBB; 32], &[0u8; 32]);
        // Extremely unlikely to collide.
        assert_ne!(p1.value(), p2.value());
    }

    #[test]
    fn pin_in_valid_range() {
        for seed in 0u8..=255 {
            let p = derive_pin(&[seed; 32], &[0u8; 32]);
            assert!(p.value() < 1_000_000);
        }
    }

    #[test]
    fn pin_distribution_rough_uniformity() {
        // Sample 1000 random secrets and check that pins span multiple thousands.
        // This catches degenerate distributions (e.g. all pins < 1000).
        use std::collections::HashSet;
        let thousands: HashSet<u32> = (0u32..1000)
            .map(|i| {
                let mut secret = [0u8; 32];
                secret[0] = (i & 0xFF) as u8;
                secret[1] = ((i >> 8) & 0xFF) as u8;
                derive_pin(&secret, &[0u8; 32]).value() / 1000
            })
            .collect();
        // Should hit at least 5 different thousand-buckets.
        assert!(
            thousands.len() >= 5,
            "poor PIN distribution: {:?}",
            thousands
        );
    }

    #[test]
    fn pending_pairing_is_not_immediately_expired() {
        let id = Uuid::new_v4();
        let pairing =
            PendingPairing::new(id, "TestDevice".into(), &[0xAB; 32], &[0u8; 32], [0u8; 32]);
        assert!(!pairing.is_expired());
        assert!(pairing.time_remaining() > Duration::from_secs(25));
    }

    #[tokio::test]
    async fn pairing_manager_deny_and_remove() {
        let (ui_tx, mut ui_rx) = tokio::sync::mpsc::channel(4);
        let mgr = PairingManager::new(ui_tx);
        let id = Uuid::new_v4();
        let pairing = PendingPairing::new(id, "Phone".into(), &[0xCC; 32], &[0u8; 32], [0u8; 32]);

        let mgr_ref = std::sync::Arc::new(mgr);
        let mgr2 = mgr_ref.clone();

        // Spawn the blocking pairing request.
        let handle = tokio::spawn(async move { mgr2.request_pairing(pairing).await });

        // Receive the UI notification.
        let req = ui_rx.recv().await.unwrap();
        assert_eq!(req.device_id, id);

        // Deny it.
        mgr_ref.resolve(id, false).await;
        let approved = handle.await.unwrap().unwrap();
        assert!(!approved);

        // Map should be empty after resolution.
        assert_eq!(mgr_ref.pending_count().await, 0);
    }
}
