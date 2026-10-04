use super::*;

impl crate::engine::Engine {
    pub async fn push_clipboard(&self, content: ClipboardContent) -> usize {
        self.push_clipboard_to(content, SyncTarget::All)
            .await
            .delivered_count()
    }

    pub async fn push_clipboard_to(
        &self,
        content: ClipboardContent,
        target: SyncTarget,
    ) -> SyncDispatchReport {
        let seq = {
            let mut guard = self.seq.lock().await;
            // Use wrapping_add so the counter rolls over safely after u64::MAX
            // instead of panicking in debug builds (LOW-01).
            *guard = guard.wrapping_add(1);
            *guard
        };

        // Hash for dedup + activity recording.
        let hash = hash_content(&content);

        // Register in mesh router so we never echo back to ourselves.
        {
            let mut router = self.shared.mesh_router.lock().await;
            router.register_local_send(hash);
        }

        // Dedup check to prevent echoing clipboard events triggered by local OS listener reflection.
        let should_send = {
            let mut dedup = self.shared.dedup.lock().await;
            dedup.should_send(hash)
        };
        if !should_send {
            tracing::debug!("suppressing local clipboard push (echo)");
            return SyncDispatchReport {
                seq,
                target: target.clone(),
                peers: Vec::new(),
            };
        }

        // Record in activity feed.
        {
            let mut feed = self.shared.activity.lock().await;
            if let ClipboardContent::Text(ref text) = content {
                feed.record_local_clipboard_text(
                    self.shared.config.device_id,
                    self.shared.config.device_name.clone(),
                    text,
                    hex::encode(hash),
                );
            }
        }

        // Record text in clipboard_store for future repush by hash.
        if let ClipboardContent::Text(ref text) = content {
            self.shared
                .clipboard_store
                .lock()
                .await
                .insert(hex::encode(hash), text.clone());
        }

        // Optionally compress images before sending.
        let compress_enabled = self.shared.settings.lock().unwrap().sync_images;
        let content = if matches!(content, ClipboardContent::Image { .. }) && compress_enabled {
            let (compressed, stats) = crate::compress::compress_image(content, true).await;
            if let Some(ref s) = stats {
                tracing::debug!(compression = %s, "image compressed for send");
            }
            compressed
        } else {
            content
        };

        // Re-hash after potential compression so the wire message is consistent.
        let hash = hash_content(&content);

        let shared_content = std::sync::Arc::new(content);

        let relay_path = vec![self.shared.config.device_name.clone()];
        let msg = AppMessage::ClipboardPush {
            seq,
            content: std::sync::Arc::clone(&shared_content),
            origin_device: self.shared.config.device_id,
            origin_device_name: self.shared.config.device_name.clone(),
            relay_path: relay_path.clone(),
        };
        let metadata = HistoryMetadata::from_content(
            &shared_content,
            self.shared.config.device_name.clone(),
            false,
        );

        let peers = self.shared.peer_manager.active_senders();
        let mut report = SyncDispatchReport {
            seq,
            target: target.clone(),
            peers: Vec::new(),
        };

        for (peer_id, tx) in peers {
            let Some(peer) = self.shared.peer_manager.get(peer_id) else {
                continue;
            };

            if !peer.trusted {
                report.peers.push(SyncDispatchPeer {
                    device_id: peer_id,
                    device_name: peer.friendly_name,
                    delivered: false,
                    metadata_only: false,
                    reason: Some("peer is not trusted".into()),
                });
                continue;
            }

            if !peer.is_sync_eligible() {
                report.peers.push(SyncDispatchPeer {
                    device_id: peer_id,
                    device_name: peer.friendly_name,
                    delivered: false,
                    metadata_only: false,
                    reason: Some("sync paused for this peer".into()),
                });
                continue;
            }

            // The peer told us (DeviceSyncState) it has clipboard sharing
            // off. A broadcast would only land in its history unapplied, so
            // skip it; a send aimed at that device on purpose still goes.
            if matches!(target, SyncTarget::All) && !peer.remote_sync_enabled {
                report.peers.push(SyncDispatchPeer {
                    device_id: peer_id,
                    device_name: peer.friendly_name,
                    delivered: false,
                    metadata_only: false,
                    reason: Some("clipboard sharing is off on this device".into()),
                });
                continue;
            }

            let is_target = match target {
                SyncTarget::All => true,
                SyncTarget::Device(target_id) => target_id == peer_id,
            };

            // Mesh router dedup check.
            let should_relay = {
                let mut router = self.shared.mesh_router.lock().await;
                router.should_relay_to(hash, self.shared.config.device_id, peer_id, &relay_path)
            };

            if !should_relay {
                report.peers.push(SyncDispatchPeer {
                    device_id: peer_id,
                    device_name: peer.friendly_name,
                    delivered: false,
                    metadata_only: false,
                    reason: Some("mesh dedup: already delivered".into()),
                });
                continue;
            }

            let app_message = if is_target {
                msg.clone()
            } else {
                AppMessage::HistoryMetadata {
                    entry: metadata.clone(),
                }
            };

            let send_result = match tx.try_send(app_message.clone()) {
                Ok(()) => Ok(()),
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => tx.send(app_message).await,
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    Err(tokio::sync::mpsc::error::SendError(app_message))
                }
            };

            match send_result {
                Ok(()) => report.peers.push(SyncDispatchPeer {
                    device_id: peer_id,
                    device_name: peer.friendly_name,
                    delivered: true,
                    metadata_only: !is_target,
                    reason: None,
                }),
                Err(_) => {
                    let reason = "peer queue unavailable".to_string();
                    let _ = self
                        .shared
                        .event_tx
                        .send(EngineEvent::ClipboardSyncFailed {
                            peer_device: peer_id,
                            peer_name: peer.friendly_name.clone(),
                            seq,
                            reason: reason.clone(),
                        })
                        .await;
                    report.peers.push(SyncDispatchPeer {
                        device_id: peer_id,
                        device_name: peer.friendly_name,
                        delivered: false,
                        metadata_only: !is_target,
                        reason: Some(reason),
                    });
                }
            }
        }

        report
    }

    /// Get recent activity feed entries (up to `limit`).
    pub async fn activity_recent(&self, limit: usize) -> Vec<crate::activity::ActivityEntry> {
        self.shared
            .activity
            .lock()
            .await
            .recent(limit)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get activity feed entries added after `since_id`.
    pub async fn activity_since(&self, since_id: u64) -> Vec<crate::activity::ActivityEntry> {
        self.shared
            .activity
            .lock()
            .await
            .since(since_id)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get pending remote clipboard items not yet applied locally.
    pub async fn pending_remote_clipboards(&self) -> Vec<crate::activity::ActivityEntry> {
        self.shared
            .activity
            .lock()
            .await
            .pending_remote_clipboards()
            .into_iter()
            .cloned()
            .collect()
    }

    /// Explicitly apply a remote clipboard item by its content hash.
    /// Marks it applied in the feed and emits `ClipboardReceived { auto_applied: true }`.
    pub async fn apply_clipboard_by_hash(&self, content_hash: String) -> Result<bool> {
        // Find the matching pending entry.
        let entry = {
            let feed = self.shared.activity.lock().await;
            feed.pending_remote_clipboards()
                .into_iter()
                .find(|e| e.content_hash.as_deref() == Some(&content_hash))
                .cloned()
        };
        let Some(entry) = entry else {
            return Ok(false);
        };
        let from_device = entry.device_id;
        let from_name = entry.device_name.clone();
        let text = self
            .shared
            .clipboard_store
            .lock()
            .await
            .get_text_by_hash(&content_hash)
            .or(entry.text_preview.clone())
            .unwrap_or_default();
        {
            let mut feed = self.shared.activity.lock().await;
            feed.record_clipboard_applied(from_device, from_name.clone(), content_hash);
        }
        // Emit event so the platform layer writes to local clipboard.
        let _ = self
            .shared
            .event_tx
            .send(EngineEvent::ClipboardReceived {
                from_device,
                from_name,
                content: std::sync::Arc::new(ClipboardContent::Text(text)),
                auto_applied: true,
                relay_path: entry.relay_path,
                activity_id: entry.id,
            })
            .await;
        Ok(true)
    }

    pub async fn history_recent(&self, n: usize) -> Vec<crate::history::HistoryEntry> {
        self.shared
            .history
            .lock()
            .await
            .recent(n)
            .cloned()
            .collect()
    }

    pub async fn history_search(
        &self,
        query: String,
        limit: usize,
    ) -> Vec<crate::history::HistoryEntry> {
        self.shared
            .history
            .lock()
            .await
            .search_fulltext(&query)
            .take(limit)
            .cloned()
            .collect()
    }

    pub async fn history_search_fuzzy(
        &self,
        query: String,
        limit: usize,
    ) -> Vec<serde_json::Value> {
        self.shared
            .history
            .lock()
            .await
            .search_fuzzy(&query, limit)
            .into_iter()
            .map(|scored| {
                serde_json::json!({
                    "score": scored.score,
                    "entry": scored.entry,
                })
            })
            .collect()
    }

    pub async fn history_repush(&self, id: u64, target: SyncTarget) -> Result<()> {
        let entry = self
            .shared
            .history
            .lock()
            .await
            .get(id)
            .cloned()
            .context("history entry not found")?;
        if let crate::history::HistoryPayload::Text {
            full_text, preview, ..
        } = entry.payload
        {
            let text = full_text.unwrap_or(preview);
            self.push_clipboard_to(ClipboardContent::Text(text), target)
                .await;
        }
        Ok(())
    }

    pub async fn history_set_pinned(&self, id: u64, pinned: bool) -> Result<()> {
        self.shared.history.lock().await.set_pinned(id, pinned)?;
        Ok(())
    }

    pub async fn history_delete(&self, id: u64) -> Result<bool> {
        self.shared.history.lock().await.remove(id)
    }

    pub async fn history_clear(&self) -> Result<()> {
        self.shared.history.lock().await.clear()
    }

    pub async fn history_export_csv(&self) -> String {
        self.shared.history.lock().await.export_csv()
    }

    pub async fn history_export_json(&self) -> Result<String> {
        self.shared.history.lock().await.export_json()
    }

    pub async fn history_stats(&self) -> crate::history::HistoryStats {
        self.shared.history.lock().await.stats()
    }

    pub async fn history_add_tag(&self, id: u64, tag: String) -> Result<()> {
        self.shared.history.lock().await.add_tag(id, &tag)?;
        Ok(())
    }

    pub async fn history_remove_tag(&self, id: u64, tag: String) -> Result<()> {
        self.shared.history.lock().await.remove_tag(id, &tag)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn history_filtered(
        &self,
        kind: Option<String>,
        device: Option<String>,
        from_secs: Option<u64>,
        to_secs: Option<u64>,
        tag: Option<String>,
        limit: usize,
        pinned_only: bool,
    ) -> Vec<crate::history::HistoryEntry> {
        let filter = crate::history::HistoryFilter {
            kind,
            device,
            from_secs,
            to_secs,
            tag,
            limit: Some(limit),
            pinned_only,
        };
        self.shared
            .history
            .lock()
            .await
            .filter(&filter)
            .take(limit)
            .cloned()
            .collect()
    }

    pub async fn template_list(&self) -> Vec<crate::settings::ClipboardTemplate> {
        self.shared
            .settings
            .lock()
            .unwrap()
            .clipboard_templates
            .clone()
    }

    pub async fn template_push(&self, name: String, target: SyncTarget) -> Result<()> {
        let templates = self
            .shared
            .settings
            .lock()
            .unwrap()
            .clipboard_templates
            .clone();
        let tmpl = templates
            .iter()
            .find(|t| t.name == name)
            .cloned()
            .with_context(|| format!("template '{}' not found", name))?;
        let content = crate::protocol::ClipboardContent::Text(tmpl.text);
        match target {
            SyncTarget::All => {
                self.push_clipboard(content).await;
            }
            SyncTarget::Device(id) => {
                self.push_clipboard_to(content, SyncTarget::Device(id))
                    .await;
            }
        }
        Ok(())
    }

    pub async fn template_set(
        &self,
        name: String,
        text: String,
        description: String,
    ) -> Result<()> {
        let mut settings = self.shared.settings.lock().unwrap();
        if let Some(t) = settings
            .clipboard_templates
            .iter_mut()
            .find(|t| t.name == name)
        {
            t.text = text;
            t.description = description;
        } else {
            settings
                .clipboard_templates
                .push(crate::settings::ClipboardTemplate {
                    name,
                    text,
                    description,
                });
        }
        Ok(())
    }

    pub async fn template_remove(&self, name: String) -> Result<bool> {
        let mut settings = self.shared.settings.lock().unwrap();
        let before = settings.clipboard_templates.len();
        settings.clipboard_templates.retain(|t| t.name != name);
        Ok(settings.clipboard_templates.len() < before)
    }

    /// Push the current OS clipboard content to connected peers.
    /// The daemon reads the clipboard via the platform clipboard API.
    pub async fn push_current_clipboard(&self, target: SyncTarget) -> Result<()> {
        let text = self
            .shared
            .local_clipboard
            .lock()
            .await
            .read_text()
            .context("reading local clipboard")?;
        if let Some(text) = text {
            self.push_clipboard_to(ClipboardContent::Text(text), target)
                .await;
        }
        Ok(())
    }
}

impl crate::engine::Engine {
    /// Record a local text entry in history without syncing it to peers.
    pub async fn remember_text(&self, text: String) -> Result<()> {
        let device_name = self.shared.config.device_name.clone();
        let max_bytes = self.shared.settings.lock().unwrap().max_history_text_bytes;
        let content = crate::protocol::ClipboardContent::Text(text);
        self.shared
            .history
            .lock()
            .await
            .push_with_options(&content, device_name, max_bytes)?;
        Ok(())
    }

    /// Return the raw clipboard content for a pending incoming item by ID.
    pub async fn incoming_clipboard(&self, id: u64) -> Option<serde_json::Value> {
        let entries: Vec<_> = self
            .shared
            .activity
            .lock()
            .await
            .pending_remote_clipboards()
            .into_iter()
            .cloned()
            .collect();
        entries
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| serde_json::to_value(e).ok())
    }

    // ── Templates ─────────────────────────────────────────────────────────────

    // ── Per-peer settings ─────────────────────────────────────────────────────
}
