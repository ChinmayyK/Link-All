//! Link All Cryptography
//!
//! # Session key establishment
//! 1. Both peers generate an ephemeral X25519 keypair.
//! 2. They exchange public keys inside HelloFrame / HelloAckFrame (plaintext).
//! 3. ECDH shared secret → HKDF-SHA256 → two directional 32-byte session
//!    keys, one per traffic direction (initiator→responder,
//!    responder→initiator), domain-separated by distinct HKDF `info`
//!    strings. Each side uses one key to send and the other to receive.
//!    A single shared key for both directions would let each side's
//!    independent, counter-based nonce collide with the other's at the
//!    same counter value — the classic AES-GCM "forbidden attack" nonce
//!    reuse case — so the two directions must never share a key.
//! 4. Every subsequent frame is encrypted with AES-256-GCM.
//!    Nonce: 8-byte big-endian message counter || 4 zero bytes (unique per
//!    direction per session — each direction has its own key and its own
//!    independent counter starting at 0).
//!
//! # Long-term identity
//! Each device also has a stable X25519 key pair stored on disk.
//! Its public key's SHA-256 hash is the "fingerprint" shown to users
//! during TOFU (Trust On First Use) verification.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key,
};
use anyhow::{Context, Result};
type Nonce = aes_gcm::aead::Nonce<Aes256Gcm>;
use hkdf::Hkdf;
use hmac::Mac;
use rand::RngCore;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

// ── Long-term identity key ────────────────────────────────────────────────────

/// 32-byte raw scalar stored on disk (mode 0600).
pub struct IdentityKey {
    secret_bytes: [u8; 32],
    pub public: PublicKey,
}

impl IdentityKey {
    /// Generate a fresh identity key.
    pub fn generate() -> Self {
        let mut secret_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut secret_bytes);
        let secret = StaticSecret::from(secret_bytes);
        let public = PublicKey::from(&secret);
        Self {
            secret_bytes,
            public,
        }
    }

    /// Load from 32 raw secret bytes (e.g. read from disk).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        let secret = StaticSecret::from(bytes);
        let public = PublicKey::from(&secret);
        Self {
            secret_bytes: bytes,
            public,
        }
    }

    /// Export the 32-byte **private** scalar for on-disk storage.
    ///
    /// The returned bytes must be stored with mode 0600 (or equivalent).
    /// Never log or transmit these bytes — only the public key is shared.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.secret_bytes
    }

    /// SHA-256 of the public key bytes — shown to users for TOFU verification.
    pub fn fingerprint(&self) -> [u8; 32] {
        fingerprint_of(self.public.as_bytes())
    }

    /// Human-readable fingerprint: 8 groups of 4 hex chars separated by colons.
    ///
    /// Example: `"A1B2:C3D4:E5F6:0708:1920:3040:5060:7080"`
    pub fn fingerprint_display(&self) -> String {
        let fp = self.fingerprint();
        // Encode first 16 bytes as 32 hex chars, then group into 4-char chunks.
        let hex: String = fp[..16].iter().map(|b| format!("{:02X}", b)).collect();
        hex.chars()
            .collect::<Vec<_>>()
            .chunks(4)
            .map(|chunk| chunk.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(":")
    }
}

pub fn fingerprint_of(pubkey_bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(pubkey_bytes);
    hasher.finalize().into()
}

// ── Ephemeral session handshake ───────────────────────────────────────────────

pub struct EphemeralKeypair {
    secret: Option<StaticSecret>,
    pub public_bytes: [u8; 32],
}

impl EphemeralKeypair {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let secret = StaticSecret::from(bytes);
        let public = PublicKey::from(&secret);
        Self {
            secret: Some(secret),
            public_bytes: *public.as_bytes(),
        }
    }

    /// Consume the ephemeral secret, perform ECDH, derive session key and PIN.
    ///
    /// `is_initiator` selects which of the two directional keys this side
    /// sends with vs. receives with — see the directional-key note in the
    /// module doc comment. Both peers must pass opposite values (the side
    /// that called `handshake_initiator` passes `true`, the side that
    /// called `handshake_responder` passes `false`).
    pub fn derive_session_key(
        &self,
        peer_pubkey_bytes: [u8; 32],
        is_initiator: bool,
    ) -> Result<(SessionKey, crate::pairing::PairingPin, [u8; 32])> {
        let secret = self.secret.as_ref().context("keypair already consumed")?;
        let peer_public = PublicKey::from(peer_pubkey_bytes);
        let shared = secret.diffie_hellman(&peer_public);

        // Copy the shared secret bytes so we can zeroize them independently of
        // the opaque `SharedSecret` wrapper (which provides no zeroize method).
        let mut shared_bytes: [u8; 32] = *shared.as_bytes();
        anyhow::ensure!(
            shared_bytes != [0u8; 32],
            "ECDH resulted in all-zero shared secret (potential low-order point attack)"
        );

        // HIGH-01 FIX: Use a deterministic salt derived from both ephemeral
        // public keys in canonical byte order. This provides defense-in-depth:
        // even if the ECDH shared secret has low entropy (weak RNG), the salt
        // ensures session keys remain unpredictable. This follows TLS 1.3 and
        // Noise protocol conventions.
        let salt: [u8; 32] = {
            let mut hasher = Sha256::new();
            if self.public_bytes <= peer_pubkey_bytes {
                hasher.update(self.public_bytes);
                hasher.update(peer_pubkey_bytes);
            } else {
                hasher.update(peer_pubkey_bytes);
                hasher.update(self.public_bytes);
            }
            hasher.finalize().into()
        };

        // HKDF-SHA256: IKM = shared secret, salt = hash(sorted ephemeral pubkeys).
        // The info strings are prefixed with the protocol version so that HKDF
        // output is domain-separated across wire-format revisions (LOW-03).
        // Changing PROTOCOL_VERSION in protocol.rs automatically invalidates
        // old session keys — peers on different protocol versions cannot
        // decrypt each other's frames even if they share an ephemeral key.
        //
        // Two directional keys are expanded from the same PRK, one per
        // traffic direction, so the initiator and responder never encrypt
        // with the same key — sharing a single key here would let each
        // side's independent, counter-based nonce collide with the other's
        // at the same counter value (AES-GCM nonce reuse — see module doc).
        let info_i2r = format!(
            "deskdrop-v{}-session-i2r",
            crate::protocol::PROTOCOL_VERSION
        );
        let info_r2i = format!(
            "deskdrop-v{}-session-r2i",
            crate::protocol::PROTOCOL_VERSION
        );
        let hk = Hkdf::<Sha256>::new(Some(&salt), &shared_bytes);

        // Derive the pairing PIN before zeroizing shared_bytes.
        let pin = crate::pairing::derive_pin(&shared_bytes, &salt);

        // Zeroize the raw DH secret immediately after feeding it into HKDF;
        // it must not linger in process memory (CRIT-02).
        shared_bytes.zeroize();

        let mut okm_i2r = [0u8; 32];
        let mut okm_r2i = [0u8; 32];
        hk.expand(info_i2r.as_bytes(), &mut okm_i2r)
            .map_err(|_| anyhow::anyhow!("HKDF expand failed"))?;
        hk.expand(info_r2i.as_bytes(), &mut okm_r2i)
            .map_err(|_| anyhow::anyhow!("HKDF expand failed"))?;

        let (send_key, recv_key) = if is_initiator {
            (&okm_i2r, &okm_r2i)
        } else {
            (&okm_r2i, &okm_i2r)
        };

        let key = SessionKey {
            send_cipher: Aes256Gcm::new(Key::<aes_gcm::aes::Aes256>::from_slice(send_key)),
            recv_cipher: Aes256Gcm::new(Key::<aes_gcm::aes::Aes256>::from_slice(recv_key)),
            send_counter: 0,
            recv_counter: 0,
        };

        okm_i2r.zeroize();
        okm_r2i.zeroize();
        Ok((key, pin, salt))
    }

    /// Verify an identity proof MAC using a static-ephemeral Diffie-Hellman exchange.
    pub fn verify_proof(
        &self,
        peer_identity_pubkey: &[u8; 32],
        session_salt: &[u8; 32],
        proof: &[u8; 32],
    ) -> bool {
        let peer_public = PublicKey::from(*peer_identity_pubkey);
        let shared = self
            .secret
            .as_ref()
            .expect("not consumed")
            .diffie_hellman(&peer_public);
        let mut mac =
            <hmac::Hmac<sha2::Sha256> as hmac::Mac>::new_from_slice(session_salt).unwrap();
        mac.update(shared.as_bytes());
        mac.update(b"deskdrop-identity-proof");

        mac.verify_slice(proof).is_ok()
    }
}

// ── Symmetric session ─────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SessionKey {
    send_cipher: Aes256Gcm,
    recv_cipher: Aes256Gcm,
    send_counter: u64,
    recv_counter: u64,
}

impl Drop for SessionKey {
    fn drop(&mut self) {
        // Zeroize the cipher state from memory when the session is dropped.
        unsafe {
            std::ptr::write_bytes(self as *mut Self, 0, 1);
        }
    }
}

impl SessionKey {
    /// Encrypt `plaintext`. Returns `nonce || ciphertext`.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let nonce = counter_nonce(self.send_counter);
        self.send_counter = self
            .send_counter
            .checked_add(1)
            .context("send counter overflow")?;

        let ct = self
            .send_cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| anyhow::anyhow!("encrypt: {:?}", e))?;

        let mut out = Vec::with_capacity(12 + ct.len());
        out.extend_from_slice(nonce.as_slice());
        out.extend_from_slice(&ct);
        Ok(out)
    }

    /// Encrypt `buffer` in-place (avoids memory allocation).
    /// The resulting ciphertext replaces the contents of `buffer` and the 16-byte
    /// auth tag is appended to it.
    /// Returns the 12-byte nonce used, which must be sent alongside the ciphertext.
    pub fn encrypt_in_place(&mut self, buffer: &mut Vec<u8>) -> Result<Nonce> {
        use aes_gcm::aead::AeadInPlace;
        let nonce = counter_nonce(self.send_counter);
        self.send_counter = self
            .send_counter
            .checked_add(1)
            .context("send counter overflow")?;

        self.send_cipher
            .encrypt_in_place(&nonce, &[], buffer)
            .map_err(|e| anyhow::anyhow!("encrypt: {:?}", e))?;

        Ok(nonce)
    }

    /// Encrypts the provided slice in-place, and returns the generated nonce and the 16-byte authentication tag.
    pub fn encrypt_slice_in_place(&mut self, buffer: &mut [u8]) -> Result<(Nonce, aes_gcm::Tag)> {
        use aes_gcm::aead::AeadInPlace;
        let nonce = counter_nonce(self.send_counter);
        self.send_counter = self
            .send_counter
            .checked_add(1)
            .context("send counter overflow")?;

        let tag = self
            .send_cipher
            .encrypt_in_place_detached(&nonce, &[], buffer)
            .map_err(|e| anyhow::anyhow!("encrypt: {:?}", e))?;

        Ok((nonce, tag))
    }

    /// Decrypt a frame produced by [`encrypt`]. Enforces monotonic counter.
    pub fn decrypt(&mut self, frame: &[u8]) -> Result<Vec<u8>> {
        anyhow::ensure!(frame.len() >= 12, "frame too short");
        let (nonce_bytes, ct) = frame.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        // Replay protection: nonce counter must be exactly the next expected
        // value. Using strict equality (== recv_counter) instead of >=
        // prevents replay of any previously seen or skipped frame — a captured
        // frame can never satisfy counter == recv_counter once it has been
        // incremented past it.
        let counter = u64::from_be_bytes(
            nonce_bytes[..8]
                .try_into()
                .expect("nonce slice is exactly 8 bytes"),
        );
        anyhow::ensure!(
            counter == self.recv_counter,
            "replayed or out-of-order frame: got counter {}, expected {}",
            counter,
            self.recv_counter
        );
        let next_counter = self
            .recv_counter
            .checked_add(1)
            .context("recv counter overflow")?;

        let res = self
            .recv_cipher
            .decrypt(nonce, ct)
            .map_err(|e| anyhow::anyhow!("decrypt: {:?}", e));

        // FIX: High-stakes Denial-of-Service vulnerability.
        // Only increment the replay counter if the ciphertext is successfully
        // authenticated (MAC validation passes). Incrementing before authentication
        // allows an attacker to inject garbage packets to permanently desync the session.
        if res.is_ok() {
            self.recv_counter = next_counter;
        }
        res
    }

    /// Decrypt a frame in-place. The first 12 bytes of the buffer must be the nonce,
    /// followed by the ciphertext. Upon success, the buffer is shrunk to just the plaintext.
    pub fn decrypt_in_place(&mut self, buffer: &mut Vec<u8>) -> Result<()> {
        use aes_gcm::aead::AeadInPlace;
        anyhow::ensure!(
            buffer.len() >= 12 + 16,
            "frame too short (must have nonce and tag)"
        );

        let mut nonce_bytes = [0u8; 12];
        nonce_bytes.copy_from_slice(&buffer[..12]);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let counter = u64::from_be_bytes(
            nonce_bytes[..8]
                .try_into()
                .expect("nonce slice is exactly 8 bytes"),
        );
        anyhow::ensure!(
            counter == self.recv_counter,
            "replayed or out-of-order frame: got counter {}, expected {}",
            counter,
            self.recv_counter
        );
        let next_counter = self
            .recv_counter
            .checked_add(1)
            .context("recv counter overflow")?;

        // Remove the 12-byte nonce from the beginning of the buffer.
        buffer.drain(..12);

        self.recv_cipher
            .decrypt_in_place(nonce, &[], buffer)
            .map_err(|e| anyhow::anyhow!("decrypt: {:?}", e))?;

        // FIX: Update counter only on successful decryption.
        self.recv_counter = next_counter;
        Ok(())
    }
}

fn counter_nonce(counter: u64) -> Nonce {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(&counter.to_be_bytes());
    *Nonce::from_slice(&n)
}

// ── Random helpers ────────────────────────────────────────────────────────────

pub fn random_nonce16() -> [u8; 16] {
    let mut n = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut n);
    n
}

/// Generate a random 6-digit numeric PIN for legacy/test pairing displays.
///
/// Note: The production pairing PIN is derived via HKDF in `pairing::derive_pin`.
/// This function has negligible modulo bias (u32::MAX is not evenly divisible
/// by 1_000_000, so PINs 0–295967 are ~0.007% more likely). This is acceptable
/// for a 6-digit PIN but callers requiring cryptographic uniformity should use
/// rejection sampling instead.
pub fn generate_pairing_pin() -> String {
    let mut bytes = [0u8; 4];
    rand::thread_rng().fill_bytes(&mut bytes);
    let n = u32::from_le_bytes(bytes) % 1_000_000;
    format!("{:06}", n)
}

/// Constant-time comparison of two byte slices (prevents timing attacks on MACs).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_session_key() {
        let alice = EphemeralKeypair::generate();
        let bob = EphemeralKeypair::generate();
        let alice_pub = alice.public_bytes;
        let bob_pub = bob.public_bytes;

        let (mut alice_sess, _, _) = alice.derive_session_key(bob_pub, true).unwrap();
        let (mut bob_sess, _, _) = bob.derive_session_key(alice_pub, false).unwrap();

        let msg = b"hello linkall!";
        let ct = alice_sess.encrypt(msg).unwrap();
        let pt = bob_sess.decrypt(&ct).unwrap();
        assert_eq!(pt, msg);

        // And the reverse direction (bob sends, alice receives).
        let ct2 = bob_sess.encrypt(b"hi back").unwrap();
        let pt2 = alice_sess.decrypt(&ct2).unwrap();
        assert_eq!(pt2, b"hi back");
    }

    #[test]
    fn reject_replay() {
        let alice = EphemeralKeypair::generate();
        let bob = EphemeralKeypair::generate();
        let alice_pub = alice.public_bytes;
        let bob_pub = bob.public_bytes;
        let (mut alice_sess, _, _) = alice.derive_session_key(bob_pub, true).unwrap();
        let (mut bob_sess, _, _) = bob.derive_session_key(alice_pub, false).unwrap();

        let ct = alice_sess.encrypt(b"first").unwrap();
        bob_sess.decrypt(&ct).unwrap();
        assert!(bob_sess.decrypt(&ct).is_err(), "replay should fail");
    }

    /// Regression test for the AES-GCM nonce-reuse bug: initiator and
    /// responder must never encrypt with the same key. Before the fix,
    /// both sides derived one shared key and both counters started at 0,
    /// so encrypting the same plaintext on both sides at counter 0 (same
    /// key, same nonce, same plaintext) produced byte-identical ciphertext.
    #[test]
    fn directional_keys_differ() {
        let alice = EphemeralKeypair::generate();
        let bob = EphemeralKeypair::generate();
        let alice_pub = alice.public_bytes;
        let bob_pub = bob.public_bytes;

        let (mut alice_sess, _, _) = alice.derive_session_key(bob_pub, true).unwrap();
        let (mut bob_sess, _, _) = bob.derive_session_key(alice_pub, false).unwrap();

        let msg = b"same plaintext on both sides";
        let alice_ct = alice_sess.encrypt(msg).unwrap();
        let bob_ct = bob_sess.encrypt(msg).unwrap();

        assert_ne!(
            alice_ct, bob_ct,
            "initiator and responder must never encrypt under the same key"
        );
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let key = IdentityKey::generate();
        assert_eq!(key.fingerprint(), key.fingerprint());
    }

    #[test]
    fn identity_key_roundtrips_through_bytes() {
        let key = IdentityKey::generate();
        let bytes = key.to_bytes();
        // Must NOT be all-zeros (placeholder bug).
        assert_ne!(bytes, [0u8; 32], "to_bytes() must return private scalar");
        // Must NOT equal the public key bytes.
        assert_ne!(
            &bytes,
            key.public.as_bytes(),
            "to_bytes() must not return public key"
        );
        // Reloading from private bytes must reproduce the same public key.
        let reloaded = IdentityKey::from_bytes(bytes);
        assert_eq!(reloaded.public.as_bytes(), key.public.as_bytes());
        // Fingerprints must also match.
        assert_eq!(reloaded.fingerprint(), key.fingerprint());
    }

    #[test]
    fn fingerprint_display_format() {
        let key = IdentityKey::generate();
        let disp = key.fingerprint_display();
        // Expect 8 groups of 4 hex chars separated by colons.
        let parts: Vec<&str> = disp.split(':').collect();
        assert_eq!(parts.len(), 8, "fingerprint should have 8 groups: {}", disp);
        for part in parts {
            assert_eq!(part.len(), 4, "each group should be 4 chars: {}", part);
            assert!(
                part.chars().all(|c| c.is_ascii_hexdigit()),
                "non-hex char in: {}",
                part
            );
        }
    }

    #[test]
    fn pairing_pin_is_six_digits() {
        for _ in 0..20 {
            let pin = generate_pairing_pin();
            assert_eq!(pin.len(), 6, "PIN must be 6 digits: {}", pin);
            assert!(
                pin.chars().all(|c| c.is_ascii_digit()),
                "non-digit in PIN: {}",
                pin
            );
        }
    }

    #[test]
    fn ct_eq_works() {
        assert!(ct_eq(b"hello", b"hello"));
        assert!(!ct_eq(b"hello", b"world"));
        assert!(!ct_eq(b"hello", b"hell"));
    }
}
