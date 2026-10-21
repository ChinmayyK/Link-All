package app.linkall

import app.linkall.ui.homeDeviceGroups
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class HomeDevicesTest {
    private val now = System.currentTimeMillis() / 1000

    private fun peer(
        id: String,
        name: String,
        connected: Boolean = false,
        trusted: Boolean = true,
        seenAgo: Long? = 30,
        asks: Boolean = false,
    ) = PeerSnapshot(
        id = id, name = name, status = if (connected) "connected" else "disconnected",
        trusted = trusted, remembered = true, autoConnect = true, explicitDisconnect = false,
        lastSeenSecs = seenAgo?.let { now - it }, lastSyncSecs = null, lastError = null, ip = null,
        pairingRequested = asks, outgoingPairingWaiting = false, pairingPin = null,
        pairingExpiresInSecs = null, pairingOutcome = null, platform = null,
        lifecycleState = "discovered", remoteSyncEnabled = true,
    )

    @Test fun connectedDevicesKeepAFixedOrderByName() {
        val g = homeDeviceGroups(listOf(peer("2", "Zed", true), peer("1", "alpha", true)))
        assertEquals(listOf("alpha", "Zed"), g.connected.map { it.name })
        assertTrue(g.other.isEmpty())
    }

    @Test fun aConnectedDeviceIsNotAlsoListedAsOther() {
        val g = homeDeviceGroups(listOf(peer("1", "Mac", true), peer("2", "PC", false)))
        assertEquals(listOf("Mac"), g.connected.map { it.name })
        assertEquals(listOf("PC"), g.other.map { it.name })
    }

    @Test fun otherDevicesPutPairingRequestsFirstThenPairedThenNearby() {
        val g = homeDeviceGroups(
            listOf(
                peer("n", "Nearby", trusted = false, seenAgo = 10),
                peer("p", "Paired", trusted = true, seenAgo = 500),
                peer("a", "Asking", trusted = false, seenAgo = 20, asks = true),
            )
        )
        assertEquals(listOf("Asking", "Paired", "Nearby"), g.other.map { it.name })
    }

    @Test fun pairedDevicesSeenMostRecentlyComeFirst() {
        val g = homeDeviceGroups(listOf(peer("1", "Old", seenAgo = 9000), peer("2", "New", seenAgo = 60)))
        assertEquals(listOf("New", "Old"), g.other.map { it.name })
    }

    @Test fun aStrangerNotSeenRecentlyIsNotListed() {
        val g = homeDeviceGroups(listOf(peer("1", "Gone", trusted = false, seenAgo = 86_400 * 3)))
        assertTrue(g.connected.isEmpty() && g.other.isEmpty())
    }
}
