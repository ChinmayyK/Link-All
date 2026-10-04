@file:OptIn(ExperimentalFoundationApi::class)

package app.linkall.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material.icons.rounded.MoreHoriz
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.linkall.PeerSnapshot
import app.linkall.SpeedTestProgress

/**
 * Every device this phone knows about, in three groups: requests to answer,
 * paired devices, and unpaired ones seen on the network.
 */
@Composable
fun DevicesTab(
    isDark: Boolean,
    peers: List<PeerSnapshot>,
    activeSpeedTests: List<SpeedTestProgress>,
    onConnectPeer: (PeerSnapshot) -> Unit,
    onDisconnectPeer: (PeerSnapshot) -> Unit,
    onSendPairingRequest: (PeerSnapshot) -> Unit,
    onRespondPairing: (PeerSnapshot, Boolean) -> Unit,
    onCancelPairing: (PeerSnapshot) -> Unit,
    onForgetPeer: (PeerSnapshot) -> Unit,
    onSendFiles: (String?) -> Unit,
    onSendFolder: (String?) -> Unit,
    onSpeedTest: (String) -> Unit,
    onScanQr: () -> Unit,
    onManualIp: () -> Unit
) {
    val c = rememberDdColors(isDark)
    val requests = peers.filter { it.pairingRequested && !it.trusted }
    // A device that also asked us is answered through its request instead.
    val outgoing = peers.filter { it.outgoingPairingWaiting && !it.pairingRequested && !it.trusted }
    val paired = peers.filter { it.trusted }
    val nearby = peers.filter { !it.trusted && !it.pairingRequested && !it.outgoingPairingWaiting && it.isListable }
    val online = paired.count { it.isConnected }

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = PageGutter, end = PageGutter, bottom = 140.dp)
    ) {
        item {
            Column {
                PageTitle(
                    c, "Devices",
                    if (paired.isEmpty()) null else "${paired.size} paired · $online online"
                ) {
                    AddDeviceButton(c, onScanQr = onScanQr, onManualIp = onManualIp)
                }
            }
        }

        items(requests, key = { "req_${it.id}" }) { peer ->
            Column {
                Spacer(Modifier.height(20.dp))
                PairingPanel(c, peer, onRespondPairing)
            }
        }

        items(outgoing, key = { "out_${it.id}" }) { peer ->
            Column {
                Spacer(Modifier.height(20.dp))
                OutgoingPairingPanel(c, peer, onCancelPairing)
            }
        }

        if (paired.isNotEmpty()) {
            item {
                Column {
                    SectionHeader(c, "Paired", null)
                    Panel(c) {
                        paired.forEachIndexed { i, peer ->
                            if (i > 0) Hairline(c)
                            PeerRow(
                                c = c,
                                peer = peer,
                                speedTest = activeSpeedTests.find { it.peerId == peer.id },
                                onPrimary = { if (peer.isConnected) onDisconnectPeer(peer) else onConnectPeer(peer) },
                                onSendFiles = { onSendFiles(peer.id) },
                                onSendFolder = { onSendFolder(peer.id) },
                                onSpeedTest = { onSpeedTest(peer.id) },
                                onForget = { onForgetPeer(peer) }
                            )
                        }
                    }
                }
            }
        }

        if (nearby.isNotEmpty()) {
            item {
                Column {
                    SectionHeader(c, "Nearby", "Not paired yet")
                    Panel(c) {
                        nearby.forEachIndexed { i, peer ->
                            if (i > 0) Hairline(c)
                            PeerRow(
                                c = c,
                                peer = peer,
                                speedTest = null,
                                onPrimary = { onSendPairingRequest(peer) },
                                onSendFiles = {},
                                onSendFolder = {},
                                onSpeedTest = {},
                                onForget = { onForgetPeer(peer) }
                            )
                        }
                    }
                }
            }
        }

        if (peers.isEmpty()) {
            item {
                Column {
                    Spacer(Modifier.height(24.dp))
                    EmptyBox(
                        c, Icons.Outlined.Devices,
                        "No devices yet. Open Link All on your computer on the same Wi-Fi, or use + to scan its code."
                    ) {
                        Spacer(Modifier.height(12.dp))
                        PillButton(c, "Can't find it? Connect by IP", filled = false, compact = true, icon = Icons.Outlined.Lan, onClick = onManualIp)
                    }
                }
            }
        }

        item {
            Column {
                Text(
                    "Devices find each other on the same Wi-Fi. Away from home, turn on your phone's hotspot and join it from your computer.",
                    style = DdType.small,
                    color = c.textMuted,
                    modifier = Modifier.padding(top = 24.dp, start = 4.dp, end = 4.dp)
                )
            }
        }
    }
}

@Composable
private fun PeerRow(
    c: DdColors,
    peer: PeerSnapshot,
    speedTest: SpeedTestProgress?,
    onPrimary: () -> Unit,
    onSendFiles: () -> Unit,
    onSendFolder: () -> Unit,
    onSpeedTest: () -> Unit,
    onForget: () -> Unit
) {
    val haptic = LocalHapticFeedback.current
    var menuOpen by remember { mutableStateOf(false) }
    // Only our own request: an unpaired peer that merely holds a session
    // open is not waiting on anyone and keeps its Pair button.
    val waiting = peer.outgoingPairingWaiting && !peer.trusted
    val (status, statusColor) = when {
        speedTest != null -> "${speedTest.phase} · ${speedTest.speedMbpsString}" to c.accent
        peer.isConnected -> (listOfNotNull("Connected", peer.ip).joinToString(" · ")) to c.live
        peer.isConnecting -> "Connecting…" to c.textMuted
        waiting -> pairingWaitingLabel(peer) to c.warn
        peer.pairingOutcomeLabel != null -> peer.pairingOutcomeLabel!! to c.warn
        peer.isRejected -> "Declined · try again" to c.danger
        peer.trusted -> (agoLabel(peer.lastSeenSecs)?.let { "Last seen $it" } ?: "Offline") to c.textMuted
        else -> (peer.ip ?: "On your network") to c.textMuted
    }

    Box {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .combinedClickable(
                    onClick = { menuOpen = true },
                    onLongClick = {
                        haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                        menuOpen = true
                    }
                )
                .padding(start = 16.dp, end = 12.dp, top = 12.dp, bottom = 12.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconWell(
                c,
                osIcon(peer.platform, peer.name),
                tint = if (peer.isConnected) c.accent else c.textMuted,
                background = if (peer.isConnected) c.accentSoft else c.surfaceSunk
            )
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(peer.name, style = DdType.label, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(status, style = DdType.small, color = statusColor, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.width(8.dp))
            when {
                peer.isConnecting || waiting -> {}
                peer.trusted && peer.isConnected -> androidx.compose.material3.Icon(
                    androidx.compose.material.icons.Icons.Rounded.MoreHoriz,
                    contentDescription = "Options for ${peer.name}",
                    tint = c.textMuted,
                    modifier = Modifier.padding(end = 4.dp).size(20.dp)
                )
                peer.trusted -> PillButton(c, "Connect", filled = true, compact = true, onClick = onPrimary)
                else -> PillButton(c, "Pair", filled = true, compact = true, onClick = onPrimary)
            }
        }

        if (menuOpen) {
            ActionSheet(
                c,
                icon = osIcon(peer.platform, peer.name),
                title = peer.name,
                subtitle = status,
                actions = buildList {
                    if (peer.isConnected) {
                        add(SheetAction(Icons.Outlined.UploadFile, "Send files", "Photos, videos, documents, anything", onClick = onSendFiles))
                        add(SheetAction(Icons.Outlined.DriveFolderUpload, "Send a folder", "Everything in it, subfolders included", onClick = onSendFolder))
                        add(SheetAction(Icons.Outlined.Speed, "Test speed", "How fast this link is right now", onClick = onSpeedTest))
                        add(SheetAction(Icons.Outlined.LinkOff, "Disconnect", "Reconnect any time from here", onClick = onPrimary))
                    }
                    add(SheetAction(Icons.Outlined.DeleteOutline, "Forget device", danger = true, onClick = onForget))
                },
                onDismiss = { menuOpen = false }
            )
        }
    }
}
