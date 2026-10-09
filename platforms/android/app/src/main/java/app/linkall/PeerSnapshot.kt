package app.linkall

import android.content.SharedPreferences
import org.json.JSONArray

const val PREF_PEER_SNAPSHOTS_JSON = "peer_snapshots_json"

/** An unpaired peer not seen for this long is treated as gone. */
const val NEARBY_WINDOW_SECS = 300L

@androidx.compose.runtime.Immutable
data class PeerSnapshot(
    val id: String,
    val name: String,
    val status: String,
    val trusted: Boolean,
    val remembered: Boolean,
    val autoConnect: Boolean,
    val explicitDisconnect: Boolean,
    val lastSeenSecs: Long?,
    val lastSyncSecs: Long?,
    val lastError: String?,
    val ip: String?,
    /** This peer asked to pair and is waiting for our answer. */
    val pairingRequested: Boolean,
    /** We asked this peer to pair and are waiting for its answer. */
    val outgoingPairingWaiting: Boolean,
    val pairingPin: String?,
    /** Seconds left on the pending request, either direction, as of the snapshot. */
    val pairingExpiresInSecs: Int?,
    /** How the last request ended: "accepted", "declined", "cancelled", "expired" or "update_needed". */
    val pairingOutcome: String?,
    /** "Android", "macOS", "Windows" or "Linux"; null until the device has connected with 1.3.4+. */
    val platform: String? = null,
    val lifecycleState: String,
    val remoteSyncEnabled: Boolean,
) {
    val isConnected: Boolean get() = status == "connected" && trusted
    val isConnecting: Boolean get() = status == "connecting"
    val isReconnectable: Boolean get() = trusted && remembered && autoConnect && !isConnected && !explicitDisconnect
    val needsAttention: Boolean get() = status == "failed"
    /**
     * Whether the peer belongs in device lists. The core keeps every untrusted peer it has
     * ever discovered, so an unpaired one only counts while it has been seen recently.
     */
    val isListable: Boolean get() = trusted || pairingRequested || outgoingPairingWaiting || isConnecting ||
        (lastSeenSecs ?: 0L) >= System.currentTimeMillis() / 1000 - NEARBY_WINDOW_SECS
    /** Why the last request with an unpaired peer ended, for its status line. */
    val pairingOutcomeLabel: String? get() = when {
        trusted || pairingRequested || outgoingPairingWaiting -> null
        pairingOutcome == "declined" -> "Declined · try again"
        pairingOutcome == "expired" -> "No answer · try again"
        pairingOutcome == "cancelled" -> "Request withdrawn"
        pairingOutcome == "update_needed" -> "Update Link All on it, then try again"
        else -> null
    }
    val needsTrust: Boolean get() = !trusted && (needsAttention || status == "disconnected")
    val isRejected: Boolean get() = lastError?.contains("rejected", ignoreCase = true) == true ||
        lastError?.contains("not trusted", ignoreCase = true) == true
}

fun parsePeerSnapshots(raw: String?): List<PeerSnapshot> {
    if (raw.isNullOrBlank()) return emptyList()
    val array = runCatching { JSONArray(raw) }.getOrNull() ?: return emptyList()
    val uniquePeers = mutableMapOf<String, PeerSnapshot>()
    for (i in 0 until array.length()) {
        val obj = array.optJSONObject(i) ?: continue
        val id = obj.optString("id")
        if (id.isBlank()) continue
        val displayName = obj.optString("display_name")
        val friendlyName = obj.optString("friendly_name")
        val name = displayName.ifBlank { friendlyName }.ifBlank { "Unknown device" }
        
        val peer = PeerSnapshot(
            id = id,
            name = name,
            status = obj.optString("status", "disconnected"),
            trusted = obj.optBoolean("trusted", false),
            remembered = obj.optBoolean("remembered", true),
            autoConnect = obj.optBoolean("auto_connect", true),
            explicitDisconnect = obj.optBoolean("explicit_disconnect", false),
            lastSeenSecs = obj.takeIf { !it.isNull("last_seen") }?.optLong("last_seen"),
            lastSyncSecs = obj.takeIf { !it.isNull("last_sync") }?.optLong("last_sync"),
            lastError = obj.takeIf { !it.isNull("last_error") }?.optString("last_error"),
            ip = (obj.takeIf { !it.isNull("ips") }?.optJSONArray("ips")?.let { if (it.length() > 0) it.optString(0) else null } ?: if (!obj.isNull("ip")) obj.optString("ip") else null)
                // IPv4 peers arrive IPv6-mapped ("::ffff:192.168.1.5"); show the plain address.
                ?.removePrefix("::ffff:"),
            pairingRequested = obj.optBoolean("pairing_requested", false),
            outgoingPairingWaiting = obj.optBoolean("outgoing_pairing_waiting", false),
            pairingPin = if (obj.isNull("pairing_pin")) null else obj.optString("pairing_pin"),
            pairingExpiresInSecs = if (obj.isNull("pairing_expires_in_secs")) null else obj.optInt("pairing_expires_in_secs"),
            pairingOutcome = if (obj.isNull("pairing_outcome")) null else obj.optString("pairing_outcome"),
            platform = if (obj.isNull("platform")) null else obj.optString("platform").ifBlank { null },
            lifecycleState = obj.optString("lifecycle_state", "discovered"),
            remoteSyncEnabled = obj.optBoolean("remote_sync_enabled", true),
        )
        
        val existing = uniquePeers[peer.id]
        if (existing == null) {
            uniquePeers[peer.id] = peer
        } else {
            val peerPriority = if (peer.isConnected) 2 else if (peer.isConnecting) 1 else 0
            val existingPriority = if (existing.isConnected) 2 else if (existing.isConnecting) 1 else 0
            
            if (peerPriority > existingPriority) {
                uniquePeers[peer.id] = peer
            } else if (peerPriority == existingPriority) {
                if ((peer.lastSeenSecs ?: 0) > (existing.lastSeenSecs ?: 0)) {
                    uniquePeers[peer.id] = peer
                }
            }
        }
    }
    return uniquePeers.values.sortedWith(
        compareBy<PeerSnapshot>(
            { if (it.isConnected) 0 else if (it.isConnecting) 1 else 2 },
            { it.name.lowercase() }
        )
    )
}

fun SharedPreferences.peerSnapshots(): List<PeerSnapshot> =
    parsePeerSnapshots(getString(PREF_PEER_SNAPSHOTS_JSON, null))
