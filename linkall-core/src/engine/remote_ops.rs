//! Remote file browsing and cross-device link handoff: queries,
//! thumbnails, pulls, file actions, and their request/response waiters.

use super::*;

impl Engine {
    #[allow(clippy::too_many_arguments)]
    pub async fn send_remote_files_query(
        &self,
        target_device: Uuid,
        request_id: Uuid,
        summary_only: bool,
        category: Option<crate::protocol::RemoteFileCategory>,
        source: Option<crate::protocol::RemoteFileSource>,
        search_query: Option<String>,
        offset: u32,
        limit: u32,
    ) -> bool {
        let msg = AppMessage::RemoteFilesQuery {
            request_id,
            origin_device: self.shared.config.device_id,
            summary_only,
            category,
            source,
            search_query,
            offset,
            limit,
        };
        let peers = self.shared.peer_manager.all_connected_senders();
        if let Some(tx) = peers
            .into_iter()
            .find(|(id, _)| *id == target_device)
            .map(|(_, tx)| tx)
        {
            tx.send(msg).await.is_ok()
        } else {
            false
        }
    }

    pub async fn send_remote_files_response(
        &self,
        target_device: Uuid,
        request_id: Uuid,
        summary: Option<crate::protocol::RemoteFilesSummary>,
        files: Vec<crate::protocol::RemoteFileEntry>,
        total_matching: u32,
        error: Option<String>,
    ) {
        let msg = AppMessage::RemoteFilesResponse {
            request_id,
            summary,
            files,
            total_matching,
            error,
        };
        let peers = self.shared.peer_manager.all_connected_senders();
        if let Some(tx) = peers
            .into_iter()
            .find(|(id, _)| *id == target_device)
            .map(|(_, tx)| tx)
        {
            let _ = tx.send(msg).await;
        }
    }

    pub async fn send_remote_thumbnail_request(
        &self,
        target_device: Uuid,
        request_id: Uuid,
        file_id: u64,
        size_px: u32,
    ) -> bool {
        let msg = AppMessage::RemoteThumbnailRequest {
            request_id,
            origin_device: self.shared.config.device_id,
            file_id,
            size_px,
        };
        let peers = self.shared.peer_manager.all_connected_senders();
        if let Some(tx) = peers
            .into_iter()
            .find(|(id, _)| *id == target_device)
            .map(|(_, tx)| tx)
        {
            tx.send(msg).await.is_ok()
        } else {
            false
        }
    }

    pub async fn send_remote_thumbnail_response(
        &self,
        target_device: Uuid,
        request_id: Uuid,
        file_id: u64,
        data: Vec<u8>,
        error: Option<String>,
    ) {
        let msg = AppMessage::RemoteThumbnailResponse {
            request_id,
            file_id,
            data,
            error,
        };
        let peers = self.shared.peer_manager.all_connected_senders();
        if let Some(tx) = peers
            .into_iter()
            .find(|(id, _)| *id == target_device)
            .map(|(_, tx)| tx)
        {
            let _ = tx.send(msg).await;
        }
    }

    pub async fn send_remote_file_pull_request(
        &self,
        target_device: Uuid,
        request_id: Uuid,
        file_id: u64,
    ) {
        let msg = AppMessage::RemoteFilePullRequest {
            request_id,
            origin_device: self.shared.config.device_id,
            file_id,
        };
        let peers = self.shared.peer_manager.all_connected_senders();
        if let Some(tx) = peers
            .into_iter()
            .find(|(id, _)| *id == target_device)
            .map(|(_, tx)| tx)
        {
            let _ = tx.send(msg).await;
        }
    }

    pub async fn send_remote_file_action_request(
        &self,
        target_device: Uuid,
        action: String,
        file_id: u64,
        new_name: Option<String>,
    ) {
        let msg = AppMessage::RemoteFileActionRequest {
            action,
            file_id,
            new_name,
        };
        let peers = self.shared.peer_manager.all_connected_senders();
        if let Some(tx) = peers
            .into_iter()
            .find(|(id, _)| *id == target_device)
            .map(|(_, tx)| tx)
        {
            let _ = tx.send(msg).await;
        }
    }

    /// Ask a trusted, connected peer to open a URL immediately. The caller
    /// (UI layer) is responsible for only offering already-trusted,
    /// connected devices as targets — trust is enforced on the receiving
    /// side's handler, same as `send_remote_file_action_request`.
    pub async fn open_url_on_device(&self, target_device: Uuid, url: String) {
        let msg = AppMessage::OpenUrlOnDevice {
            url,
            origin_device: self.shared.config.device_id,
            origin_device_name: self.shared.config.device_name.clone(),
        };
        let peers = self.shared.peer_manager.all_connected_senders();
        if let Some(tx) = peers
            .into_iter()
            .find(|(id, _)| *id == target_device)
            .map(|(_, tx)| tx)
        {
            let _ = tx.send(msg).await;
        }
    }

    /// The devices `purpose` ("send", "remote", "browse" or "sync_target") can
    /// go to, usable ones first, each unusable one with its reason. `last_used`
    /// is the device the user chose last for it; `offers_all` says whether the
    /// chooser should add "All devices".
    pub fn device_choices(
        &self,
        purpose: &str,
        last_used: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        use crate::device_choice::{choices, offers_all, Purpose};
        let purpose =
            Purpose::parse(purpose).ok_or_else(|| format!("unknown purpose: {purpose}"))?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let list = choices(&self.shared.peer_manager.list(), purpose, last_used, now);
        Ok(serde_json::json!({ "offers_all": offers_all(purpose, &list), "choices": list }))
    }

    /// Ask a connected, trusted computer to run a remote-control action. Only
    /// computers that report 2.1.0 or later understand the message, so an
    /// older one gets an error back instead of a message it can't decode.
    pub async fn send_remote_control(
        &self,
        target_device: Uuid,
        action: &str,
    ) -> Result<(), String> {
        let tx = self.remote_control_sender(target_device)?;
        tx.send(AppMessage::RemoteControl {
            action: action.to_string(),
        })
        .await
        .map_err(|_| "That computer isn't connected.".to_string())
    }

    /// Ask a connected, trusted phone on 2.1.0 or later to ring (`ring`) or
    /// stop. Waits for the phone's answer: `Ok` when it will, otherwise why it
    /// won't (it only rings while its own setting allows it).
    pub async fn send_find_device(&self, target_device: Uuid, ring: bool) -> Result<(), String> {
        let tx = self.remote_control_sender(target_device)?;
        let (answer, answered) = tokio::sync::oneshot::channel();
        self.shared
            .remote_waiters
            .find
            .lock()
            .await
            .insert(target_device, answer);
        let sent = tx.send(AppMessage::FindDevice { ring }).await;
        let result = match sent {
            Err(_) => Err("That device isn't connected.".to_string()),
            Ok(()) => match tokio::time::timeout(std::time::Duration::from_secs(5), answered).await
            {
                Ok(Ok(answer)) => answer,
                _ => Err("That device didn't answer.".to_string()),
            },
        };
        self.shared
            .remote_waiters
            .find
            .lock()
            .await
            .remove(&target_device);
        result
    }

    /// Send one mouse or keyboard message from the phone's trackpad to a
    /// connected, trusted computer on 2.1.0 or later. Doesn't wait for an
    /// answer, and drops the message when the link is backed up: a pointer
    /// move that arrives late is worse than one that never does. Anything
    /// that isn't a trackpad message is refused.
    pub fn send_remote_input(
        &self,
        target_device: Uuid,
        message: AppMessage,
    ) -> Result<(), String> {
        if !matches!(
            message,
            AppMessage::RemotePointerMove { .. }
                | AppMessage::RemotePointerButton { .. }
                | AppMessage::RemotePointerScroll { .. }
                | AppMessage::RemoteTypeText { .. }
                | AppMessage::RemoteKeyPress { .. }
                | AppMessage::RemoteShortcut { .. }
                | AppMessage::RemoteModifier { .. }
        ) {
            return Err("That isn't a trackpad message.".to_string());
        }
        let tx = self.remote_control_sender(target_device)?;
        match tx.try_send(message) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                Err("That computer isn't connected.".to_string())
            }
        }
    }

    /// Offer a connected, trusted phone on 2.1.0 or later a call to `number`.
    /// The phone only opens its dialer when its owner taps the notification.
    pub async fn send_dial_number(&self, target_device: Uuid, number: &str) -> Result<(), String> {
        let number = crate::remote_input::clean_phone_number(number)
            .ok_or_else(|| "That doesn't look like a phone number.".to_string())?;
        let tx = self.remote_control_sender(target_device)?;
        tx.send(AppMessage::DialNumber { number })
            .await
            .map_err(|_| "That phone isn't connected.".to_string())
    }

    /// Ask a connected, trusted phone on 2.1.0 or later to take a picture and
    /// send it back. `mode` is "photo" or "document".
    pub async fn send_capture_request(
        &self,
        target_device: Uuid,
        mode: &str,
    ) -> Result<(), String> {
        let tx = self.remote_control_sender(target_device)?;
        let mode = if mode == "document" {
            "document"
        } else {
            "photo"
        };
        tx.send(AppMessage::CaptureRequest {
            mode: mode.to_string(),
        })
        .await
        .map_err(|_| "That phone isn't connected.".to_string())
    }

    /// Why a remote-control action could not be sent to `target_device`, or
    /// `Ok` when it could.
    pub async fn send_remote_control_check(&self, target_device: Uuid) -> Result<(), String> {
        self.remote_control_sender(target_device).map(|_| ())
    }

    pub(super) fn remote_control_sender(
        &self,
        target_device: Uuid,
    ) -> Result<mpsc::Sender<AppMessage>, String> {
        let peer = self
            .shared
            .peer_manager
            .get(target_device)
            .ok_or_else(|| "That computer isn't known.".to_string())?;
        if !crate::device_choice::understands_remote_control(peer.app_version.as_deref()) {
            return Err("Update Link All on that computer to use the remote.".to_string());
        }
        self.shared
            .peer_manager
            .all_connected_senders()
            .into_iter()
            .find(|(id, _)| *id == target_device)
            .map(|(_, tx)| tx)
            .ok_or_else(|| "That computer isn't connected.".to_string())
    }

    /// Report back whether we actually managed to open a URL a peer asked
    /// us to open (`requester_device` is whoever sent the original
    /// `OpenUrlOnDevice` — only the platform layer knows if the OS-level
    /// open call actually succeeded, e.g. no default browser configured,
    /// so this is a separate call from the receive handler rather than an
    /// automatic ack).
    pub async fn ack_open_url_on_device(
        &self,
        requester_device: Uuid,
        success: bool,
        error: Option<String>,
    ) {
        let msg = AppMessage::OpenUrlOnDeviceAck { success, error };
        let peers = self.shared.peer_manager.all_connected_senders();
        if let Some(tx) = peers
            .into_iter()
            .find(|(id, _)| *id == requester_device)
            .map(|(_, tx)| tx)
        {
            let _ = tx.send(msg).await;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn query_remote_files_sync(
        &self,
        target_device: Uuid,
        summary_only: bool,
        category: Option<crate::protocol::RemoteFileCategory>,
        source: Option<crate::protocol::RemoteFileSource>,
        search_query: Option<String>,
        offset: u32,
        limit: u32,
        timeout_secs: u64,
    ) -> Result<RemoteFilesResult> {
        let effective_timeout = if timeout_secs == 0 { 10 } else { timeout_secs };
        let request_id = Uuid::new_v4();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.shared
            .remote_waiters
            .files
            .lock()
            .await
            .insert(request_id, (target_device, tx));
        let sent = self
            .send_remote_files_query(
                target_device,
                request_id,
                summary_only,
                category,
                source,
                search_query,
                offset,
                limit,
            )
            .await;
        if !sent {
            self.shared
                .remote_waiters
                .files
                .lock()
                .await
                .remove(&request_id);
            anyhow::bail!("Target device {} is not connected", target_device);
        }
        match tokio::time::timeout(std::time::Duration::from_secs(effective_timeout), rx).await {
            Ok(Ok(res)) => {
                if let Some(err) = res.error {
                    anyhow::bail!("{err}");
                }
                Ok(res)
            }
            Ok(Err(_)) => {
                self.shared
                    .remote_waiters
                    .files
                    .lock()
                    .await
                    .remove(&request_id);
                anyhow::bail!("Remote files query channel closed unexpectedly")
            }
            Err(_) => {
                self.shared
                    .remote_waiters
                    .files
                    .lock()
                    .await
                    .remove(&request_id);
                anyhow::bail!("Remote files query timed out after {}s", effective_timeout)
            }
        }
    }

    pub async fn request_remote_thumbnail_sync(
        &self,
        target_device: Uuid,
        file_id: u64,
        size_px: u32,
        timeout_secs: u64,
    ) -> Result<RemoteThumbnailResult> {
        let request_id = Uuid::new_v4();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.shared
            .remote_waiters
            .thumbnails
            .lock()
            .await
            .insert(request_id, (target_device, tx));
        let sent = self
            .send_remote_thumbnail_request(target_device, request_id, file_id, size_px)
            .await;
        if !sent {
            self.shared
                .remote_waiters
                .thumbnails
                .lock()
                .await
                .remove(&request_id);
            anyhow::bail!("Target device {} is not connected", target_device);
        }
        match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx).await {
            Ok(Ok(res)) => Ok(res),
            Ok(Err(_)) => {
                self.shared
                    .remote_waiters
                    .thumbnails
                    .lock()
                    .await
                    .remove(&request_id);
                anyhow::bail!("Remote thumbnail request channel closed unexpectedly")
            }
            Err(_) => {
                self.shared
                    .remote_waiters
                    .thumbnails
                    .lock()
                    .await
                    .remove(&request_id);
                anyhow::bail!("Remote thumbnail request timed out after {}s", timeout_secs)
            }
        }
    }
}

/// Drain pending remote file waiters and remote thumbnail waiters for a given peer
/// and notify oneshot receivers with an immediate fast-path error ("Peer disconnected").
pub(crate) async fn drain_remote_waiters(shared: &EngineShared, peer_id: Uuid) {
    let waiters_to_notify: Vec<tokio::sync::oneshot::Sender<RemoteFilesResult>> = {
        let mut waiters = shared.remote_waiters.files.lock().await;
        let matching_keys: Vec<Uuid> = waiters
            .iter()
            .filter_map(|(req_id, (target, _))| {
                if *target == peer_id {
                    Some(*req_id)
                } else {
                    None
                }
            })
            .collect();
        matching_keys
            .into_iter()
            .filter_map(|req_id| waiters.remove(&req_id).map(|(_, tx)| tx))
            .collect()
    };

    for tx in waiters_to_notify {
        let _ = tx.send(RemoteFilesResult {
            summary: None,
            files: Vec::new(),
            total_matching: 0,
            error: Some("Peer disconnected".to_string()),
        });
    }

    let thumb_waiters_to_notify: Vec<tokio::sync::oneshot::Sender<RemoteThumbnailResult>> = {
        let mut thumb_waiters = shared.remote_waiters.thumbnails.lock().await;
        let matching_keys: Vec<Uuid> = thumb_waiters
            .iter()
            .filter_map(|(req_id, (target, _))| {
                if *target == peer_id {
                    Some(*req_id)
                } else {
                    None
                }
            })
            .collect();
        matching_keys
            .into_iter()
            .filter_map(|req_id| thumb_waiters.remove(&req_id).map(|(_, tx)| tx))
            .collect()
    };

    for tx in thumb_waiters_to_notify {
        let _ = tx.send(RemoteThumbnailResult {
            file_id: 0,
            data: Vec::new(),
            error: Some("Peer disconnected".to_string()),
        });
    }
}
