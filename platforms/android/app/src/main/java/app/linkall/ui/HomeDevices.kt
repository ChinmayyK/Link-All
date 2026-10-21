package app.linkall.ui

import androidx.compose.ui.graphics.Color
import app.linkall.PeerSnapshot
import app.linkall.SpeedTestProgress

/** Home's devices in two groups: the connected ones, each shown as a card, and everything else. */
internal data class HomeDevices(val connected: List<PeerSnapshot>, val other: List<PeerSnapshot>)

/**
 * Connected devices keep a fixed order (by name, then id), so a card never jumps
 * when something syncs. The rest: devices asking to pair first, then paired ones
 * (seen most recently first), then nearby devices that aren't paired. Strangers
 * not seen recently aren't listed ([PeerSnapshot.isListable]).
 */
internal fun homeDeviceGroups(peers: List<PeerSnapshot>): HomeDevices {
    val connected = peers
        .filter { it.isConnected }
        .sortedWith(compareBy({ it.name.lowercase() }, { it.id }))
    val other = peers
        .filter { it.isListable && !it.isConnected }
        .sortedWith(
            compareBy<PeerSnapshot>(
                { if (it.pairingRequested && !it.trusted) 0 else if (it.trusted) 1 else 2 },
                { -(it.lastSeenSecs ?: 0L) },
                { it.name.lowercase() },
            )
        )
    return HomeDevices(connected, other)
}

/** The one-line state of a device and its colour, shared by the device card and the device row. */
internal fun deviceStatus(c: DdColors, peer: PeerSnapshot, speedTest: SpeedTestProgress?): Pair<String, Color> = when {
    speedTest != null -> "${speedTest.phase} · ${speedTest.speedMbpsString}" to c.accent
    peer.pairingRequested && !peer.trusted -> "Wants to pair" to c.accent
    peer.isConnected -> "Connected" to c.live
    peer.outgoingPairingWaiting && !peer.trusted -> pairingWaitingLabel(peer) to c.warn
    peer.pairingOutcomeLabel != null -> peer.pairingOutcomeLabel!! to c.warn
    peer.isConnecting -> "Connecting…" to c.textMuted
    peer.trusted -> (agoLabel(peer.lastSeenSecs)?.let { "Seen $it" } ?: "Offline") to c.textMuted
    else -> "Not paired" to c.warn
}
