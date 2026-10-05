use crate::crypto::fingerprint_of;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrustState {
    #[default]
    Untrusted,
    Trusted,
    Rejected,
    Revoked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CapabilityProfile {
    /// Allow auto-syncing plain text clipboard items from this device
    pub allow_text: bool,
    /// Allow auto-syncing images from this device
    pub allow_images: bool,
    /// Allow receiving file transfers from this device
    pub allow_files: bool,
    /// Optional per-device maximum payload size in bytes (overrides global setting if smaller)
    pub max_payload_bytes: Option<usize>,
}

impl Default for CapabilityProfile {
    fn default() -> Self {
        Self {
            allow_text: true,
            allow_images: true,
            allow_files: true,
            max_payload_bytes: None,
        }
    }
}

impl CapabilityProfile {
    pub fn text_only() -> Self {
        Self {
            allow_text: true,
            allow_images: false,
            allow_files: false,
            max_payload_bytes: Some(1024 * 1024),
        }
    }

    pub fn read_only() -> Self {
        Self {
            allow_text: false,
            allow_images: false,
            allow_files: false,
            max_payload_bytes: Some(0),
        }
    }

    /// Check whether a payload of a given content type and size is allowed by this profile.
    pub fn allows_payload(
        &self,
        is_text: bool,
        is_image: bool,
        is_file: bool,
        payload_size: usize,
    ) -> Result<(), &'static str> {
        if let Some(limit) = self.max_payload_bytes {
            if payload_size > limit {
                return Err("payload exceeds peer capability size limit");
            }
        }
        if is_text && !self.allow_text {
            return Err("text sync disabled for this peer");
        }
        if is_image && !self.allow_images {
            return Err("image sync disabled for this peer");
        }
        if is_file && !self.allow_files {
            return Err("file transfer disabled for this peer");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TrustRecord {
    pub device_id: Uuid,
    pub device_name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub public_key: [u8; 32],
    pub key_fingerprint: [u8; 32],
    pub state: TrustState,
    pub first_seen: u64,
    pub trusted_since: Option<u64>,
    pub last_seen: u64,
    pub capability_profile: CapabilityProfile,
}

impl Default for TrustRecord {
    fn default() -> Self {
        Self {
            device_id: Uuid::nil(),
            device_name: String::new(),
            display_name: None,
            public_key: [0; 32],
            key_fingerprint: [0; 32],
            state: TrustState::Untrusted,
            first_seen: 0,
            trusted_since: None,
            last_seen: 0,
            capability_profile: CapabilityProfile::default(),
        }
    }
}

impl TrustRecord {
    pub fn effective_name(&self) -> &str {
        self.display_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.device_name)
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct StoreData {
    devices: HashMap<Uuid, TrustRecord>,
    /// Set once the blocks left by older builds are lifted (see `load`).
    #[serde(default)]
    legacy_blocks_cleared: bool,
    /// Set once the duplicates left by reinstalls are cleared (see
    /// `clear_old_installs_once`).
    #[serde(default)]
    old_installs_cleared: bool,
}

pub struct TrustStore {
    data: StoreData,
    path: PathBuf,
}

impl TrustStore {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut data = if path.exists() {
            let bytes = std::fs::read(&path).context("reading trust store")?;
            if bytes.is_empty() {
                StoreData::default()
            } else {
                serde_json::from_slice(&bytes)
                    .unwrap_or_else(|err| crate::settings::set_aside_corrupt(&path, &err))
            }
        } else {
            StoreData::default()
        };
        for record in data.devices.values_mut() {
            if record.state == TrustState::Untrusted
                && record.trusted_since.is_some()
                && record.key_fingerprint != [0; 32]
            {
                record.state = TrustState::Trusted;
            }
        }
        // Older builds turned every declined pairing request into a block,
        // and Android declined on its own after 30 s. Those devices could
        // never reach this one again and their users never learned why.
        // Declining is "not now" today, so lift those blocks once; a reject
        // made after this (linkall-cli devices reject) stays.
        if !data.legacy_blocks_cleared {
            for record in data.devices.values_mut() {
                if record.state == TrustState::Rejected {
                    record.state = TrustState::Untrusted;
                }
            }
            data.legacy_blocks_cleared = true;
        }
        Ok(Self { data, path })
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(parent)
                    .context("creating trust dir")?;
            }
            #[cfg(not(unix))]
            {
                std::fs::create_dir_all(parent).context("creating trust dir")?;
            }
        }
        use std::io::Write;
        let tmp = self.path.with_extension("tmp");
        // Compact, not pretty — internal cache, not hand-edited.
        // `observe_peer()` calls this unconditionally on every handshake, so
        // a flaky connection reconnecting repeatedly rewrites this file
        // every time; same reasoning as the history.rs/peer_manager.rs fixes.
        let bytes = serde_json::to_vec(&self.data)?;
        {
            let mut file = std::fs::File::create(&tmp).context("creating trust temp file")?;
            file.write_all(&bytes).context("writing trust store")?;
            let _ = file.sync_all();
        }
        std::fs::rename(&tmp, &self.path).context("renaming trust store")?;
        Ok(())
    }

    pub fn observe_peer(
        &mut self,
        device_id: Uuid,
        device_name: String,
        public_key: &[u8; 32],
    ) -> Result<TrustRecord> {
        let fingerprint = fingerprint_of(public_key);
        let now = now_secs();
        self.migrate_matching_identity(
            device_id,
            device_name.clone(),
            public_key,
            fingerprint,
            now,
        )?;
        let record = self
            .data
            .devices
            .entry(device_id)
            .or_insert_with(|| TrustRecord {
                device_id,
                device_name: device_name.clone(),
                display_name: None,
                public_key: *public_key,
                key_fingerprint: fingerprint,
                state: TrustState::Untrusted,
                first_seen: now,
                trusted_since: None,
                last_seen: now,
                capability_profile: CapabilityProfile::default(),
            });

        if record.key_fingerprint != [0; 32] && record.key_fingerprint != fingerprint {
            anyhow::bail!(
                "fingerprint mismatch for device {}: expected {}, got {}",
                device_id,
                format_fingerprint(&record.key_fingerprint),
                format_fingerprint(&fingerprint)
            );
        }

        record.device_name = device_name;
        record.public_key = *public_key;
        record.key_fingerprint = fingerprint;
        record.last_seen = now;
        let snapshot = record.clone();
        self.save()?;
        Ok(snapshot)
    }
    pub fn rotate_peer_key(&mut self, device_id: Uuid, new_public_key: &[u8; 32]) -> Result<()> {
        let fingerprint = fingerprint_of(new_public_key);
        if let Some(record) = self.data.devices.get_mut(&device_id) {
            record.public_key = *new_public_key;
            record.key_fingerprint = fingerprint;
            record.last_seen = now_secs();
            self.save()?;
            Ok(())
        } else {
            anyhow::bail!("Cannot rotate key for unknown peer {}", device_id)
        }
    }

    fn migrate_matching_identity(
        &mut self,
        device_id: Uuid,
        device_name: String,
        public_key: &[u8; 32],
        fingerprint: [u8; 32],
        now: u64,
    ) -> Result<()> {
        if self.data.devices.contains_key(&device_id) {
            return Ok(());
        }

        let previous_id = self.data.devices.iter().find_map(|(existing_id, record)| {
            if record.public_key == *public_key || record.key_fingerprint == fingerprint {
                Some(*existing_id)
            } else {
                None
            }
        });

        let Some(previous_id) = previous_id else {
            return Ok(());
        };

        if let Some(mut record) = self.data.devices.remove(&previous_id) {
            record.device_id = device_id;
            record.device_name = device_name;
            record.public_key = *public_key;
            record.key_fingerprint = fingerprint;
            record.last_seen = now;
            self.data.devices.insert(device_id, record);
        }

        Ok(())
    }

    pub fn is_trusted(&self, device_id: Uuid) -> bool {
        self.data
            .devices
            .get(&device_id)
            .map(|record| record.state == TrustState::Trusted)
            .unwrap_or(false)
    }

    pub fn trust_peer(&mut self, device_id: Uuid) -> Result<Option<TrustRecord>> {
        let now = now_secs();
        if let Some(record) = self.data.devices.get(&device_id) {
            if record.public_key == [0; 32] || record.key_fingerprint == [0; 32] {
                anyhow::bail!("cannot trust peer without valid public key");
            }
        }
        let snapshot = self.data.devices.get_mut(&device_id).map(|record| {
            record.state = TrustState::Trusted;
            if record.trusted_since.is_none() {
                record.trusted_since = Some(now);
            }
            record.last_seen = now;
            record.clone()
        });
        if snapshot.is_some() {
            self.save()?;
        }
        Ok(snapshot)
    }

    pub fn reject_peer(&mut self, device_id: Uuid) -> Result<Option<TrustRecord>> {
        let snapshot = self.data.devices.get_mut(&device_id).map(|record| {
            record.state = TrustState::Rejected;
            record.clone()
        });
        if snapshot.is_some() {
            self.save()?;
        }
        Ok(snapshot)
    }

    pub fn unreject_peer(&mut self, device_id: Uuid) -> Result<bool> {
        let changed = if let Some(record) = self.data.devices.get_mut(&device_id) {
            if record.state == TrustState::Rejected || record.state == TrustState::Revoked {
                record.state = TrustState::Untrusted;
                true
            } else {
                false
            }
        } else {
            false
        };
        if changed {
            self.save()?;
        }
        Ok(changed)
    }

    pub fn revoke_peer(&mut self, device_id: Uuid) -> Result<bool> {
        let changed = if let Some(record) = self.data.devices.get_mut(&device_id) {
            record.state = TrustState::Revoked;
            true
        } else {
            false
        };
        if changed {
            self.save()?;
        }
        Ok(changed)
    }

    /// Reinstalling Link All or wiping its data gives a device a new identity,
    /// so the same phone piles up as one trusted entry per install. Once
    /// `kept` is trusted, other trusted entries with its name that are not
    /// connected right now are old installs of that device: drop them.
    /// Returns the dropped ids. A second device that really shares the name
    /// is asked to pair again on its next connection.
    pub fn retire_old_installs(
        &mut self,
        kept: Uuid,
        is_connected: impl Fn(Uuid) -> bool,
    ) -> Result<Vec<Uuid>> {
        let Some(name) = self
            .data
            .devices
            .get(&kept)
            .filter(|r| r.state == TrustState::Trusted)
            .map(|r| r.device_name.trim().to_lowercase())
        else {
            return Ok(Vec::new());
        };
        if name.is_empty() {
            return Ok(Vec::new());
        }
        let retired: Vec<Uuid> = self
            .data
            .devices
            .values()
            .filter(|r| {
                r.device_id != kept
                    && r.state == TrustState::Trusted
                    && r.device_name.trim().to_lowercase() == name
                    && !is_connected(r.device_id)
            })
            .map(|r| r.device_id)
            .collect();
        if !retired.is_empty() {
            for id in &retired {
                self.data.devices.remove(id);
            }
            self.save()?;
        }
        Ok(retired)
    }

    /// Clears the duplicates that reinstalls left before
    /// `retire_old_installs` existed: per name, the most recently seen
    /// trusted entry stays. Runs once per store. Returns the dropped ids.
    pub fn clear_old_installs_once(&mut self) -> Result<Vec<Uuid>> {
        if self.data.old_installs_cleared {
            return Ok(Vec::new());
        }
        let mut newest: HashMap<String, (u64, Uuid)> = HashMap::new();
        for r in self.all_trusted() {
            let name = r.device_name.trim().to_lowercase();
            if name.is_empty() {
                continue;
            }
            let entry = newest.entry(name).or_insert((r.last_seen, r.device_id));
            if r.last_seen > entry.0 {
                *entry = (r.last_seen, r.device_id);
            }
        }
        let mut retired = Vec::new();
        for (_, kept) in newest.into_values() {
            retired.extend(self.retire_old_installs(kept, |_| false)?);
        }
        self.data.old_installs_cleared = true;
        self.save()?;
        Ok(retired)
    }

    pub fn set_capability_profile(
        &mut self,
        device_id: Uuid,
        profile: CapabilityProfile,
    ) -> Result<Option<TrustRecord>> {
        let snapshot = self.data.devices.get_mut(&device_id).map(|record| {
            record.capability_profile = profile;
            record.clone()
        });
        if snapshot.is_some() {
            self.save()?;
        }
        Ok(snapshot)
    }

    pub fn capability_profile(&self, device_id: Uuid) -> Option<CapabilityProfile> {
        self.data
            .devices
            .get(&device_id)
            .map(|record| record.capability_profile)
    }

    pub fn get(&self, device_id: Uuid) -> Option<&TrustRecord> {
        self.data.devices.get(&device_id)
    }

    pub fn rename_peer(
        &mut self,
        device_id: Uuid,
        display_name: String,
    ) -> Result<Option<TrustRecord>> {
        let snapshot = self.data.devices.get_mut(&device_id).map(|record| {
            record.display_name = Some(display_name);
            record.clone()
        });
        if snapshot.is_some() {
            self.save()?;
        }
        Ok(snapshot)
    }

    pub fn all_devices(&self) -> impl Iterator<Item = &TrustRecord> {
        self.data.devices.values()
    }

    pub fn device_count(&self) -> usize {
        self.data.devices.len()
    }

    /// Count of devices in `Trusted` state.
    pub fn trusted_count(&self) -> usize {
        self.data
            .devices
            .values()
            .filter(|r| r.state == TrustState::Trusted)
            .count()
    }

    /// Remove stale `Untrusted` and `Rejected` records not seen in `max_age_secs`.
    /// Trusted and Revoked records are always kept.
    /// Returns the number of records pruned.
    pub fn prune_stale(&mut self, max_age_secs: u64) -> Result<usize> {
        let now = now_secs();
        let before = self.data.devices.len();
        self.data.devices.retain(|_, record| match record.state {
            TrustState::Trusted | TrustState::Revoked => true,
            TrustState::Untrusted | TrustState::Rejected => {
                now.saturating_sub(record.last_seen) <= max_age_secs
            }
        });
        let pruned = before - self.data.devices.len();
        if pruned > 0 {
            self.save()?;
        }
        Ok(pruned)
    }

    /// Iterator over all `Trusted` records.
    pub fn all_trusted(&self) -> impl Iterator<Item = &TrustRecord> {
        self.data
            .devices
            .values()
            .filter(|r| r.state == TrustState::Trusted)
    }

    /// True if the device has been explicitly rejected.
    pub fn is_rejected(&self, device_id: Uuid) -> bool {
        self.data
            .devices
            .get(&device_id)
            .map(|r| r.state == TrustState::Rejected)
            .unwrap_or(false)
    }

    /// True if the device had trust but it was revoked.
    pub fn is_revoked(&self, device_id: Uuid) -> bool {
        self.data
            .devices
            .get(&device_id)
            .map(|r| r.state == TrustState::Revoked)
            .unwrap_or(false)
    }

    /// Check whether a device should be permitted to sync (trusted and not revoked).
    pub fn is_sync_allowed(&self, device_id: Uuid) -> bool {
        self.data
            .devices
            .get(&device_id)
            .map(|r| r.state == TrustState::Trusted)
            .unwrap_or(false)
    }

    /// Export a human-readable summary of all known devices for diagnostics.
    pub fn export_summary(&self) -> String {
        let mut lines = vec![format!(
            "{} device(s) known ({} trusted):",
            self.device_count(),
            self.trusted_count()
        )];
        let mut records: Vec<_> = self.data.devices.values().collect();
        records.sort_by_key(|r| r.last_seen);
        for r in records.iter().rev() {
            lines.push(format!(
                "  [{:?}] {}  fp={}  last_seen={}",
                r.state,
                r.effective_name(),
                format_fingerprint(&r.key_fingerprint),
                r.last_seen,
            ));
        }
        lines.join("\n")
    }

    pub fn check(
        &mut self,
        device_id: Uuid,
        public_key: &[u8; 32],
        device_name: String,
    ) -> Result<Option<TrustRecord>> {
        let record = self.observe_peer(device_id, device_name, public_key)?;
        if record.state == TrustState::Trusted {
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    pub fn trust(
        &mut self,
        device_id: Uuid,
        device_name: String,
        pubkey_bytes: &[u8],
    ) -> Result<TrustRecord> {
        let public_key: [u8; 32] = pubkey_bytes
            .try_into()
            .context("trust requires a 32-byte public key")?;
        self.observe_peer(device_id, device_name, &public_key)?;
        self.trust_peer(device_id)?
            .context("peer disappeared while trusting")
    }

    pub fn touch(&mut self, device_id: Uuid) -> Result<()> {
        if let Some(record) = self.data.devices.get_mut(&device_id) {
            record.last_seen = now_secs();
            self.save()?;
        }
        Ok(())
    }

    pub fn revoke(&mut self, device_id: Uuid) -> Result<bool> {
        self.revoke_peer(device_id)
    }
}

/// Format a 32-byte fingerprint as 8 groups of 4 hex chars, colon-separated.
///
/// Example: `"A1B2:C3D4:E5F6:0708:1920:3040:5060:7080"`
///
/// Consistent with `IdentityKey::fingerprint_display()` in `crypto.rs`.
pub fn format_fingerprint(fp: &[u8; 32]) -> String {
    let hex: String = fp[..16].iter().map(|b| format!("{:02X}", b)).collect();
    hex.chars()
        .collect::<Vec<_>>()
        .chunks(4)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn unreadable_store_is_set_aside_and_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trust.json");
        std::fs::write(&path, b"{\"devices\":{\"half-writ").unwrap();
        let store = TrustStore::load(&path).expect("load must not fail");
        assert!(store.data.devices.is_empty());
        assert!(dir.path().join("trust.json.corrupt").exists());
    }

    #[test]
    fn observe_then_trust_roundtrip() {
        let file = NamedTempFile::new().unwrap();
        let mut store = TrustStore::load(file.path()).unwrap();
        let id = Uuid::new_v4();
        let pubkey = [42u8; 32];

        let observed = store.observe_peer(id, "Desk".into(), &pubkey).unwrap();
        assert_eq!(observed.state, TrustState::Untrusted);
        assert!(!store.is_trusted(id));

        store.trust_peer(id).unwrap();
        assert!(store.is_trusted(id));
        assert_eq!(store.trusted_count(), 1);
    }

    #[test]
    fn legacy_blocks_are_lifted_once() {
        let file = NamedTempFile::new().unwrap();
        let id = Uuid::new_v4();
        {
            let mut store = TrustStore::load(file.path()).unwrap();
            store.observe_peer(id, "Desk".into(), &[42u8; 32]).unwrap();
            store.reject_peer(id).unwrap();
        }
        // What an older build left behind: a block and no migration marker.
        let mut raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(file.path()).unwrap()).unwrap();
        raw.as_object_mut().unwrap().remove("legacy_blocks_cleared");
        std::fs::write(file.path(), serde_json::to_vec(&raw).unwrap()).unwrap();

        let mut store = TrustStore::load(file.path()).unwrap();
        assert_eq!(store.get(id).unwrap().state, TrustState::Untrusted);

        // A reject made after the migration is kept.
        store.reject_peer(id).unwrap();
        let store = TrustStore::load(file.path()).unwrap();
        assert_eq!(store.get(id).unwrap().state, TrustState::Rejected);
    }

    #[test]
    fn pairing_a_reinstall_retires_its_old_installs() {
        let file = NamedTempFile::new().unwrap();
        let mut store = TrustStore::load(file.path()).unwrap();
        let (old, online_twin, other, new) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        for (id, name, key) in [
            (old, "Pixel 8", 1u8),
            (online_twin, "Pixel 8", 2),
            (other, "Laptop", 3),
            (new, " pixel 8 ", 4),
        ] {
            store.observe_peer(id, name.into(), &[key; 32]).unwrap();
            store.trust_peer(id).unwrap();
        }

        let retired = store
            .retire_old_installs(new, |id| id == online_twin)
            .unwrap();
        assert_eq!(retired, vec![old]);
        assert!(store.get(old).is_none());
        assert!(store.is_trusted(online_twin));
        assert!(store.is_trusted(other));
        assert!(store.is_trusted(new));
    }

    #[test]
    fn old_installs_are_cleared_once_keeping_the_newest() {
        let file = NamedTempFile::new().unwrap();
        let (old, newest, other) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        {
            let mut store = TrustStore::load(file.path()).unwrap();
            for (id, name, key) in [
                (old, "Phone", 1u8),
                (newest, "Phone", 2),
                (other, "Desk", 3),
            ] {
                store.observe_peer(id, name.into(), &[key; 32]).unwrap();
                store.trust_peer(id).unwrap();
            }
            store.data.devices.get_mut(&old).unwrap().last_seen = 100;
            store.data.devices.get_mut(&newest).unwrap().last_seen = 200;
            store.save().unwrap();
        }

        let mut store = TrustStore::load(file.path()).unwrap();
        assert_eq!(store.clear_old_installs_once().unwrap(), vec![old]);
        assert!(store.is_trusted(newest) && store.is_trusted(other));

        // A same-named device paired later is left alone by the startup pass.
        let twin = Uuid::new_v4();
        store
            .observe_peer(twin, "Phone".into(), &[4u8; 32])
            .unwrap();
        store.trust_peer(twin).unwrap();
        let mut store = TrustStore::load(file.path()).unwrap();
        assert!(store.clear_old_installs_once().unwrap().is_empty());
        assert!(store.is_trusted(twin) && store.is_trusted(newest));
    }

    #[test]
    fn fingerprint_mismatch_is_err() {
        let file = NamedTempFile::new().unwrap();
        let mut store = TrustStore::load(file.path()).unwrap();
        let id = Uuid::new_v4();
        store.observe_peer(id, "Device".into(), &[1u8; 32]).unwrap();

        assert!(store.observe_peer(id, "Device".into(), &[2u8; 32]).is_err());
    }

    #[test]
    fn reject_and_revoke_states() {
        let file = NamedTempFile::new().unwrap();
        let mut store = TrustStore::load(file.path()).unwrap();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();

        store.observe_peer(a, "A".into(), &[1u8; 32]).unwrap();
        store.observe_peer(b, "B".into(), &[2u8; 32]).unwrap();

        store.reject_peer(a).unwrap();
        assert!(store.is_rejected(a));
        assert!(!store.is_sync_allowed(a));

        store.trust_peer(b).unwrap();
        store.revoke_peer(b).unwrap();
        assert!(store.is_revoked(b));
        assert!(!store.is_trusted(b));
        assert!(!store.is_sync_allowed(b));
    }

    #[test]
    fn all_trusted_filters_correctly() {
        let file = NamedTempFile::new().unwrap();
        let mut store = TrustStore::load(file.path()).unwrap();

        for (i, key) in [[1u8; 32], [2u8; 32], [3u8; 32]].iter().enumerate() {
            let id = Uuid::new_v4();
            store.observe_peer(id, format!("Dev{}", i), key).unwrap();
            if i < 2 {
                store.trust_peer(id).unwrap();
            }
        }
        let trusted: Vec<_> = store.all_trusted().collect();
        assert_eq!(trusted.len(), 2);
    }

    #[test]
    fn format_fingerprint_produces_8_groups() {
        let fp = [
            0xA1, 0xB2, 0xC3, 0xD4, 0xE5, 0xF6, 0x07, 0x08, 0x19, 0x20, 0x30, 0x40, 0x50, 0x60,
            0x70, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00u8,
        ];
        let s = format_fingerprint(&fp);
        let parts: Vec<_> = s.split(':').collect();
        assert_eq!(parts.len(), 8, "fingerprint: {}", s);
        for p in &parts {
            assert_eq!(p.len(), 4, "part '{}' in {}", p, s);
        }
    }

    #[test]
    fn export_summary_contains_device_names() {
        let file = NamedTempFile::new().unwrap();
        let mut store = TrustStore::load(file.path()).unwrap();
        let id = Uuid::new_v4();
        store
            .observe_peer(id, "MyPhone".into(), &[7u8; 32])
            .unwrap();
        store.trust_peer(id).unwrap();

        let summary = store.export_summary();
        assert!(summary.contains("MyPhone"), "summary: {}", summary);
        assert!(summary.contains("Trusted"), "summary: {}", summary);
    }

    #[test]
    fn capability_profile_check() {
        let text_only = CapabilityProfile::text_only();
        assert!(text_only.allows_payload(true, false, false, 100).is_ok());
        assert!(text_only.allows_payload(false, true, false, 100).is_err());
        assert!(text_only.allows_payload(false, false, true, 100).is_err());
        assert!(text_only
            .allows_payload(true, false, false, 2 * 1024 * 1024)
            .is_err());
    }

    #[test]
    fn store_capability_profile_persistence() {
        let file = NamedTempFile::new().unwrap();
        let mut store = TrustStore::load(file.path()).unwrap();
        let id = Uuid::new_v4();
        store.observe_peer(id, "iPad".into(), &[8u8; 32]).unwrap();

        let updated = store
            .set_capability_profile(id, CapabilityProfile::text_only())
            .unwrap();
        assert!(updated.is_some());
        assert_eq!(
            store.capability_profile(id),
            Some(CapabilityProfile::text_only())
        );

        // Verify persistence across reload
        let store2 = TrustStore::load(file.path()).unwrap();
        assert_eq!(
            store2.capability_profile(id),
            Some(CapabilityProfile::text_only())
        );
    }
}
