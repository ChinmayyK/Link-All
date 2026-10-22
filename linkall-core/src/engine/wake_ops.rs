//! Waking a paired computer that has gone to sleep (see `crate::wol`).

use super::*;

impl Engine {
    /// Broadcasts a Wake-on-LAN packet for a paired computer. Returns how many
    /// datagrams went out; that says the packet was sent, not that the
    /// computer woke. The caller watches for it to connect again.
    pub async fn wake_peer(&self, target: Uuid) -> Result<usize, String> {
        let peer = self
            .shared
            .peer_manager
            .get(target)
            .ok_or_else(|| "That computer isn't known.".to_string())?;
        if !peer.trusted {
            return Err("That computer isn't paired.".to_string());
        }
        if peer.status == crate::peer_manager::PeerConnectionState::Connected {
            return Err("That computer is already awake and connected.".to_string());
        }
        let mac = peer
            .mac_address
            .as_deref()
            .and_then(crate::wol::parse_mac)
            .ok_or_else(|| {
                "Link All doesn't know that computer's network address yet. \
                 Connect to it once while it is awake."
                    .to_string()
            })?;
        tokio::task::spawn_blocking(move || crate::wol::send_magic_packet(mac))
            .await
            .map_err(|e| e.to_string())?
    }
}
