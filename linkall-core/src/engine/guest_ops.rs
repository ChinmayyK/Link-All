//! Guest Drop calls from the apps: start a session that takes files from
//! someone who isn't paired, and stop it.

use super::*;
use crate::guest_drop::{self, GuestSession};

impl Engine {
    /// Starts a guest session (replacing one already running) and returns what
    /// to show the guest: a link and a PIN. Files go to `Guest` inside the
    /// folder received files are saved to, and each one shows up in Activity.
    pub async fn guest_drop_start(&self) -> Result<GuestSession, String> {
        let ip = {
            let state = self.shared.network_state.lock().await;
            state
                .active_interface
                .as_ref()
                .map(|i| i.ip)
                .unwrap_or(state.bind_addr.ip())
        };
        if ip.is_unspecified() {
            return Err("Connect to a network first.".to_string());
        }
        let dir = self
            .shared
            .config
            .file_save_dir
            .clone()
            .unwrap_or_else(default_save_dir)
            .join("Guest");
        let shared = self.shared.clone();
        let on_file: guest_drop::OnFile = Arc::new(move |path, bytes, name| {
            let shared = shared.clone();
            tokio::spawn(async move {
                let transfer_id = Uuid::new_v4().to_string();
                shared.activity.lock().await.record_file_transfer_complete(
                    Uuid::nil(),
                    "a guest".to_string(),
                    name,
                    bytes,
                    transfer_id,
                    Some(path.to_string_lossy().to_string()),
                );
            });
        });
        let handle = guest_drop::start(ip, dir, on_file)
            .await
            .map_err(|e| e.to_string())?;
        let session = handle.session.clone();
        // The old session, if any, ends when its handle is dropped here.
        *self.shared.guest.lock().unwrap() = Some(handle);
        Ok(session)
    }

    pub fn guest_drop_stop(&self) {
        self.shared.guest.lock().unwrap().take();
    }
}
