//! Long-term device identity key.
//!
//! Unlike the ephemeral session keys (generated fresh every TCP connection),
//! the identity key is stable across restarts. Its public key is the basis
//! for the device fingerprint shown during TOFU pairing.
//!
//! # Storage strategy
//! The 32-byte raw scalar is stored at:
//!   Linux/macOS: `$XDG_DATA_HOME/deskdrop/identity.key`  (mode 0600)
//!   Windows:     `%LOCALAPPDATA%\deskdrop\identity.key`
//!
//! # ⚠ Known security gap: plain-file key storage
//! The identity private key is currently stored as a plain file on disk.
//! This is a known gap. For production hardening, keys should be stored
//! in OS-backed secret stores:
//!   macOS:   `Security.framework` — Keychain Services / Secure Enclave
//!   Windows: `crypt32.dll`        — DPAPI (`CryptProtectData`)
//!   Android: Android Keystore system
//!   Linux:   `libsecret`          — via the `secret-service` crate
//!
//! The `keychain` feature flag (not yet enabled) is reserved for this
//! migration. See SECURITY.md for the full threat model.
//!
//! # Key rotation
//! `deskdrop-cli devices rotate-key` calls `IdentityStore::rotate()` which:
//!   1. Generates a new keypair.
//!   2. Writes it to disk (atomically).
//!   3. Broadcasts a `KeyRotated` AppMessage to all connected peers so they
//!      can update their trust store entry proactively (instead of seeing a
//!      fingerprint mismatch on next connect).

use anyhow::{Context, Result};
use hmac::Mac;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

// ── IdentityKey ───────────────────────────────────────────────────────────────

/// A stable X25519 keypair representing this device's long-term identity.
pub struct IdentityKey {
    secret_bytes: [u8; 32], // kept for serialization; never exposed publicly
    _secret: StaticSecret,
    pub public: PublicKey,
    pub public_bytes: [u8; 32],
}

impl IdentityKey {
    /// Generate a cryptographically random identity key.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self::from_secret_bytes(bytes)
    }

    /// Reconstruct from 32 raw bytes (loaded from disk or keychain).
    pub fn from_secret_bytes(bytes: [u8; 32]) -> Self {
        let secret = StaticSecret::from(bytes);
        let public = PublicKey::from(&secret);
        let public_bytes = *public.as_bytes();
        Self {
            secret_bytes: bytes,
            _secret: secret,
            public,
            public_bytes,
        }
    }

    /// 32-byte SHA-256 fingerprint of the public key.
    pub fn fingerprint(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.public_bytes);
        h.finalize().into()
    }

    /// Display fingerprint: colon-separated 4-hex-char groups (first 16 bytes, 8 groups).
    ///
    /// # Format
    /// Each group is two bytes rendered as 4 uppercase hex digits. Groups are
    /// separated by colons. Example for the first 16 bytes of a SHA-256 digest:
    ///
    /// ```text
    /// "3AF2:0B9C:44E1:7D28:AABB:CCDD:EEFF:0011"
    /// ```
    ///
    /// (8 groups × 4 chars = 32 hex chars + 7 colons = 39 chars total)
    pub fn fingerprint_display(&self) -> String {
        let fp = self.fingerprint();
        fp[..16]
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .chunks(2)
            .map(|c| c.join(""))
            .collect::<Vec<_>>()
            .join(":")
    }

    /// Raw secret bytes for persistence. Zeroize the returned array after use.
    pub fn secret_bytes(&self) -> [u8; 32] {
        self.secret_bytes
    }

    /// Compute an identity proof MAC using a static-ephemeral Diffie-Hellman exchange.
    pub fn compute_proof(
        &self,
        peer_ephemeral_pubkey: &[u8; 32],
        session_salt: &[u8; 32],
    ) -> [u8; 32] {
        let peer_public = PublicKey::from(*peer_ephemeral_pubkey);
        let shared = self._secret.diffie_hellman(&peer_public);
        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(session_salt).unwrap();
        mac.update(shared.as_bytes());
        mac.update(b"deskdrop-identity-proof");
        mac.finalize().into_bytes().into()
    }
}

impl Drop for IdentityKey {
    fn drop(&mut self) {
        self.secret_bytes.zeroize();
    }
}

// ── IdentityStore ─────────────────────────────────────────────────────────────

/// Loads and persists the identity key to a file.
pub struct IdentityStore {
    path: PathBuf,
}

impl IdentityStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Default path for the current platform.
    pub fn default_path() -> PathBuf {
        #[cfg(target_os = "android")]
        let base = PathBuf::from("/data/user/0/com.deskdrop.debug/files");
        #[cfg(not(target_os = "android"))]
        let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));

        crate::settings::app_dir(&base).join("identity.key")
    }

    /// Load an existing key, or generate and persist a new one.
    pub fn load_or_create(&self) -> Result<IdentityKey> {
        if self.path.exists() {
            self.load()
        } else {
            let key = IdentityKey::generate();
            self.save(&key)?;
            Ok(key)
        }
    }

    /// Load key from disk.
    pub fn load(&self) -> Result<IdentityKey> {
        let bytes = std::fs::read(&self.path)
            .with_context(|| format!("reading identity key from {:?}", self.path))?;

        anyhow::ensure!(
            bytes.len() == 32,
            "identity key file corrupt: expected 32 bytes, got {}",
            bytes.len()
        );

        let mut raw = [0u8; 32];
        raw.copy_from_slice(&bytes);
        Ok(IdentityKey::from_secret_bytes(raw))
    }

    /// Save key to disk.
    pub fn save(&self, key: &IdentityKey) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(parent)
                    .context("creating key directory")?;
            }
            #[cfg(not(unix))]
            {
                std::fs::create_dir_all(parent).context("creating key directory")?;
            }
        }

        let tmp = self.path.with_extension("tmp");

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true).mode(0o600);
            let mut file = options
                .open(&tmp)
                .context("creating temporary identity key file")?;
            std::io::Write::write_all(&mut file, &key.secret_bytes())
                .context("writing identity key")?;
        }

        #[cfg(not(unix))]
        {
            std::fs::write(&tmp, key.secret_bytes()).context("writing identity key")?;
        }

        std::fs::rename(&tmp, &self.path).context("renaming identity key")?;
        Ok(())
    }

    /// Generate a new identity key, replacing the existing one.
    ///
    /// The caller is responsible for broadcasting `AppMessage::KeyRotated`
    /// to connected peers so they can update their trust records.
    pub fn rotate(&self) -> Result<IdentityKey> {
        let new_key = IdentityKey::generate();
        self.save(&new_key)?;
        tracing::info!(
            "Identity key rotated. New fingerprint: {}",
            new_key.fingerprint_display()
        );
        Ok(new_key)
    }

    /// Delete the identity key file (full reset).
    pub fn delete(&self) -> Result<()> {
        if self.path.exists() {
            std::fs::remove_file(&self.path).context("deleting identity key")?;
        }
        Ok(())
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn fingerprint_display_format_is_correct() {
        // Use a deterministic key so we can verify the exact output format.
        let k = IdentityKey::from_secret_bytes([42u8; 32]);
        let display = k.fingerprint_display();
        // Format: 8 groups of 4 hex chars, separated by colons.
        // Total length: 8*4 + 7 = 39 characters.
        assert_eq!(
            display.len(),
            39,
            "fingerprint display has wrong length: {}",
            display
        );
        let parts: Vec<&str> = display.split(':').collect();
        assert_eq!(
            parts.len(),
            8,
            "expected 8 colon-separated groups: {}",
            display
        );
        for part in &parts {
            assert_eq!(
                part.len(),
                4,
                "each group must be 4 chars, got '{}' in '{}'",
                part,
                display
            );
            assert!(
                part.chars().all(|c| c.is_ascii_hexdigit()),
                "non-hex char in '{}'",
                display
            );
        }
    }

    #[test]
    fn generate_and_fingerprint() {
        let k = IdentityKey::generate();
        let fp = k.fingerprint();
        assert_eq!(fp.len(), 32);
        let display = k.fingerprint_display();
        // Should look like "AA:BB:CC:..."
        assert!(display.contains(':'), "fingerprint display: {}", display);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let store = IdentityStore::new(dir.path().join("identity.key"));

        let key1 = store.load_or_create().unwrap();
        let key2 = store.load().unwrap();

        assert_eq!(key1.public_bytes, key2.public_bytes);
        assert_eq!(key1.fingerprint(), key2.fingerprint());
    }

    #[test]
    fn load_or_create_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let store = IdentityStore::new(dir.path().join("identity.key"));

        let k1 = store.load_or_create().unwrap();
        let k2 = store.load_or_create().unwrap(); // second call must return same key
        assert_eq!(k1.public_bytes, k2.public_bytes);
    }

    #[test]
    fn rotate_changes_key() {
        let dir = TempDir::new().unwrap();
        let store = IdentityStore::new(dir.path().join("identity.key"));

        let original = store.load_or_create().unwrap();
        let rotated = store.rotate().unwrap();
        let reloaded = store.load().unwrap();

        // The rotated key must differ from the original.
        assert_ne!(original.public_bytes, rotated.public_bytes);
        // Disk must reflect the rotated key.
        assert_eq!(rotated.public_bytes, reloaded.public_bytes);
    }

    #[test]
    fn corrupt_file_returns_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("identity.key");
        std::fs::write(&path, b"too short").unwrap();
        let store = IdentityStore::new(&path);
        assert!(store.load().is_err());
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let bytes = [42u8; 32];
        let k1 = IdentityKey::from_secret_bytes(bytes);
        let k2 = IdentityKey::from_secret_bytes(bytes);
        assert_eq!(k1.fingerprint(), k2.fingerprint());
    }

    #[test]
    fn different_keys_different_fingerprints() {
        let k1 = IdentityKey::generate();
        let k2 = IdentityKey::generate();
        // Collision probability: 2^-256 — safe to assert
        assert_ne!(k1.fingerprint(), k2.fingerprint());
    }
}
