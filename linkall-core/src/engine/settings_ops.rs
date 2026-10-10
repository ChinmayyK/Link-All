//! Global and per-peer settings: load, patch, persist, and push to peers.

use super::*;

impl Engine {
    /// Apply new settings to the engine at runtime (no restart needed).
    pub async fn apply_settings(&self, new_settings: Settings) {
        let mut policy = self.shared.apply_policy.lock().await;
        policy.update_from_settings(&new_settings);
        *self.shared.settings.lock().unwrap() = new_settings;
    }

    pub async fn current_settings(&self) -> Settings {
        self.shared.settings.lock().unwrap().clone()
    }

    /// Where this engine's settings.json lives. The daemon shares the default
    /// path; Android and tests set their own in `EngineConfig`.
    pub(super) fn settings_path(&self) -> PathBuf {
        self.shared.config.settings_path.clone()
    }

    /// Tell every connected peer whether we share our clipboard, so they
    /// stop broadcasting to us while it's off.
    pub(super) async fn announce_sync_state(&self, enabled: bool) {
        for (_peer_id, tx) in self.shared.peer_manager.active_senders() {
            let _ = tx
                .send(crate::protocol::AppMessage::DeviceSyncState { enabled })
                .await;
        }
    }

    pub(super) fn persist_settings_snapshot(&self, settings: Settings) -> Result<Settings> {
        let sanitized = settings.sanitize();
        let mut store = SettingsStore::load(self.settings_path())?;
        *store.get_mut() = sanitized.clone();
        store.save()?;
        Ok(sanitized)
    }

    /// Apply a JSON merge-patch to the current settings.
    pub async fn patch_settings(&self, patch: String) -> Result<()> {
        let mut current = serde_json::to_value(&*self.shared.settings.lock().unwrap())?;
        let patch_val: serde_json::Value =
            serde_json::from_str(&patch).context("patch_settings: invalid JSON patch")?;
        json_merge_patch(&mut current, &patch_val);
        let new_settings: Settings = serde_json::from_value(current)
            .context("patch_settings: patched value is invalid Settings")?;
        let was_on = self.shared.settings.lock().unwrap().sync_enabled;
        let persisted = self.persist_settings_snapshot(new_settings)?;
        let now_on = persisted.sync_enabled;
        self.apply_settings(persisted).await;
        if was_on != now_on {
            self.announce_sync_state(now_on).await;
        }
        Ok(())
    }

    /// Apply a partial settings update from the Mac preferences UI.
    pub async fn save_settings_partial(&self, p: crate::ipc::PartialSettings) -> Result<()> {
        // Clone first so we're not holding the lock while calling apply_settings.
        let mut s = self.shared.settings.lock().unwrap().clone();
        if let Some(v) = p.port {
            s.port = v;
        }
        if let Some(v) = p.device_name {
            s.device_name = v;
        }
        if let Some(v) = p.sync_enabled {
            s.sync_enabled = v;
        }
        if let Some(v) = p.sync_text {
            s.sync_text = v;
        }
        if let Some(v) = p.sync_images {
            s.sync_images = v;
        }
        if let Some(v) = p.sync_files {
            s.sync_files = v;
        }
        if let Some(v) = p.history_limit {
            s.history_limit = v;
        }
        if let Some(v) = p.max_history_text_bytes {
            s.max_history_text_bytes = v;
        }
        if let Some(v) = p.max_payload_bytes {
            s.max_payload_bytes = v;
        }
        if let Some(v) = p.clipboard_poll_ms {
            s.clipboard_poll_ms = v;
        }
        if let Some(v) = p.max_pushes_per_sec {
            s.max_pushes_per_sec = v;
        }
        if let Some(v) = p.rate_limit_burst {
            s.rate_limit_burst = v;
        }
        if let Some(v) = p.smart_sync_duplicate_window_ms {
            s.smart_sync_duplicate_window_ms = v;
        }
        if let Some(v) = p.smart_sync_debounce_ms {
            s.smart_sync_debounce_ms = v;
        }
        if let Some(v) = p.block_sensitive_text {
            s.block_sensitive_text = v;
        }
        if let Some(v) = p.require_tofu_confirmation {
            s.require_tofu_confirmation = v;
        }
        if let Some(v) = p.show_receive_notification {
            s.show_receive_notification = v;
        }
        if let Some(v) = p.ignore_patterns {
            s.ignore_patterns = v;
        }
        let persisted = self.persist_settings_snapshot(s)?;
        self.apply_settings(persisted).await;
        Ok(())
    }

    pub async fn rotate_identity_key(&self) -> Result<()> {
        let new_key = {
            let mut key_lock = self
                .shared
                .identity_key
                .write()
                .unwrap_or_else(|e| e.into_inner());
            let store = crate::identity::IdentityStore::new(&self.shared.config.identity_path);
            *key_lock = store.rotate()?;
            key_lock.public_bytes
        };

        // Broadcast to all active connected peers
        let peers = self.shared.peer_manager.active_senders();
        for (_peer_id, tx) in peers {
            let _ = tx
                .send(crate::protocol::AppMessage::KeyRotated {
                    new_pubkey_bytes: new_key,
                })
                .await;
        }
        Ok(())
    }

    pub async fn set_sync_enabled(&self, enabled: bool) -> Result<()> {
        let mut settings = self.shared.settings.lock().unwrap().clone();
        settings.sync_enabled = enabled;
        let persisted = self.persist_settings_snapshot(settings)?;
        self.apply_settings(persisted).await;
        self.announce_sync_state(enabled).await;
        Ok(())
    }

    pub async fn set_timeline_first_mode(&self, enabled: bool) -> Result<()> {
        let mut settings = self.shared.settings.lock().unwrap().clone();
        settings.timeline_first_mode = enabled;
        let persisted = self.persist_settings_snapshot(settings)?;
        self.apply_settings(persisted).await;
        Ok(())
    }

    pub async fn set_auto_apply_clipboard(&self, enabled: bool) -> Result<()> {
        let mut settings = self.shared.settings.lock().unwrap().clone();
        settings.auto_apply_remote_clipboard = enabled;
        let persisted = self.persist_settings_snapshot(settings)?;
        self.apply_settings(persisted).await;
        Ok(())
    }

    // ── History ───────────────────────────────────────────────────────────────

    pub async fn get_peer_settings(
        &self,
        device_id: Uuid,
    ) -> Option<crate::settings::PeerSettings> {
        self.shared
            .settings
            .lock()
            .unwrap()
            .per_peer
            .get(&device_id.to_string())
            .cloned()
    }

    pub async fn patch_peer_settings(&self, device_id: Uuid, patch: String) -> Result<()> {
        let mut settings = self.shared.settings.lock().unwrap();
        let key = device_id.to_string();
        let existing = settings.per_peer.entry(key).or_default();
        let mut val = serde_json::to_value(&*existing)?;
        let patch_val: serde_json::Value =
            serde_json::from_str(&patch).context("patch_peer_settings: invalid JSON patch")?;
        json_merge_patch(&mut val, &patch_val);
        *existing = serde_json::from_value(val)
            .context("patch_peer_settings: patched value is invalid PeerSettings")?;
        Ok(())
    }

    /// Get detailed info for a trusted device.
    pub async fn device_details(&self, device_id: Uuid) -> Option<serde_json::Value> {
        let trust = self.shared.trust.lock().await;
        let record = trust.get(device_id)?;
        serde_json::to_value(record).ok()
    }

    // ── Speed Test ────────────────────────────────────────────────────────────
}

/// RFC 7396 JSON merge-patch: recursively overwrite `target` with non-null
/// fields from `patch`, removing null-keyed fields.
pub(super) fn json_merge_patch(target: &mut serde_json::Value, patch: &serde_json::Value) {
    if let serde_json::Value::Object(patch_obj) = patch {
        if !target.is_object() {
            *target = serde_json::Value::Object(serde_json::Map::new());
        }
        let target_obj = target
            .as_object_mut()
            .expect("target is explicitly converted to an object");
        for (key, patch_val) in patch_obj {
            if patch_val.is_null() {
                target_obj.remove(key);
            } else if let Some(existing) = target_obj.get_mut(key) {
                json_merge_patch(existing, patch_val);
            } else {
                target_obj.insert(key.clone(), patch_val.clone());
            }
        }
    } else {
        *target = patch.clone();
    }
}
