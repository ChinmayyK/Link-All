@file:OptIn(ExperimentalFoundationApi::class)

package app.linkall.ui

import app.linkall.HealthIssue

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.ArrowForward
import androidx.compose.material.icons.rounded.ChevronRight
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.MoreHoriz
import androidx.compose.material.icons.rounded.NorthEast
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.linkall.ActivityEntry
import app.linkall.ActivityKind
import app.linkall.PeerSnapshot
import app.linkall.SpeedTestProgress
import app.linkall.TransferProgress
import app.linkall.TransferState
import app.linkall.ui.theme.OutfitFontFamily
import app.linkall.ui.theme.crPressScale
import kotlinx.coroutines.delay
import kotlin.math.PI
import kotlin.math.sin





@Composable
fun HomeTab(
    isDark: Boolean,
    deviceName: String,
    ambientStatus: String,
    peers: List<PeerSnapshot>,
    feed: List<ActivityEntry>,
    activeTransfers: List<TransferProgress>,
    activeSpeedTests: List<SpeedTestProgress>,
    onActionStartSpeedTest: (String) -> Unit,
    onActionPushClipboard: () -> Unit,
    onActionSendQuickContext: () -> Unit,
    quickContextText: String?,
    onActionPairMagicLink: () -> Unit,
    onManualIp: () -> Unit,
    onActionSendFiles: (String?) -> Unit,
    onActionSendFolder: (String?) -> Unit,
    onActionStreamCamera: () -> Unit,
    onApplyClipboard: (ActivityEntry) -> Unit,
    onActionPauseTransfer: (String) -> Unit,
    onActionResumeTransfer: (String) -> Unit,
    onActionCancelTransfer: (String) -> Unit,
    onForgetPeer: (PeerSnapshot) -> Unit,
    onDeleteActivity: (ActivityEntry) -> Unit,
    onTogglePinActivity: (ActivityEntry) -> Unit = {},
    onResendActivity: (ActivityEntry) -> Unit,
    onReplayOnboarding: () -> Unit,
    onTabSelected: (AppTab) -> Unit,
    onRespondPairing: (PeerSnapshot, Boolean) -> Unit,
    onCancelPairing: (PeerSnapshot) -> Unit,
    healthIssue: HealthIssue? = null,
    onHealthAction: (HealthIssue) -> Unit = {}
) {
    val c = remember(isDark) { DdColors(isDark) }
    val connected = peers.filter { it.isConnected }
    // An incoming request outranks our own: when both users tapped Pair,
    // accepting theirs settles both.
    val pairingRequest = peers.firstOrNull { it.pairingRequested && !it.trusted }
    val outgoingRequest = peers.firstOrNull { it.outgoingPairingWaiting && !it.trusted }
    var showSendSheet by remember { mutableStateOf(false) }


    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = PageGutter)
    ) {
        // This phone's identity and link state. Peer names live in the
        // device list and transfer rows, not up here.
        HomeTopBar(
            c = c,
            deviceName = deviceName,
            connected = connected,
            hasPeers = peers.isNotEmpty(),
            onScanQr = onActionPairMagicLink,
            onManualIp = onManualIp
        )

        Spacer(Modifier.height(24.dp))

        if (pairingRequest != null) {
            PairingPanel(c, pairingRequest, onRespondPairing)
        } else if (outgoingRequest != null) {
            OutgoingPairingPanel(c, outgoingRequest, onCancelPairing)
        } else {
            if (healthIssue != null) {
                HealthBanner(c, healthIssue, onHealthAction)
                Spacer(Modifier.height(20.dp))
            }
            if (connected.isEmpty()) {
                StatusBlock(c = c, hasPeers = peers.isNotEmpty(), onAddDevice = onReplayOnboarding)
            }
        }

        if (peers.isNotEmpty()) {
            val enabled = connected.isNotEmpty()
            SectionHeader(c, "Send", if (enabled) null else "Connect a device first")
            Panel(c) {
                ActionRow(
                    c, Icons.Outlined.UploadFile, "Files & folders", "Send files or entire folders",
                    enabled = enabled,
                    onClick = { showSendSheet = true }
                )
                Hairline(c)
                val clip = quickContextText?.trim()?.takeIf { it.isNotBlank() }
                // What it is, not what it says: the home screen is often seen
                // by others, and copied text can be a password or a code.
                ActionRow(
                    c, Icons.Outlined.ContentPaste, "Clipboard",
                    clip?.let { "Send copied text · ${it.length} characters" } ?: "Send what you last copied",
                    enabled = enabled,
                    onClick = if (clip == null) onActionPushClipboard else onActionSendQuickContext
                )
                Hairline(c)
                ActionRow(c, Icons.Outlined.Videocam, "Camera", "Stream this camera to a device", enabled = enabled, onClick = onActionStreamCamera)
            }
        }

        if (activeTransfers.isNotEmpty()) {
            SectionHeader(c, "Transferring", "${activeTransfers.size}")
            Panel(c) {
                activeTransfers.forEachIndexed { i, t ->
                    if (i > 0) Hairline(c)
                    TransferRow(
                        c = c,
                        transfer = t,
                        onPause = { onActionPauseTransfer(t.id) },
                        onResume = { onActionResumeTransfer(t.id) },
                        onCancel = { onActionCancelTransfer(t.id) }
                    )
                }
            }
        }

        val listed = peers.filter { it.isListable }
        if (listed.isNotEmpty()) {
            SectionHeader(c, "Your devices", "${connected.size} of ${listed.size} online") {
                onTabSelected(AppTab.Devices)
            }
            Panel(c) {
                listed.forEachIndexed { i, peer ->
                    if (i > 0) Hairline(c)
                    DeviceRow(
                        c = c,
                        peer = peer,
                        speedTest = activeSpeedTests.find { it.peerId == peer.id },
                        onSendFiles = { onActionSendFiles(peer.id) },
                        onSendFolder = { onActionSendFolder(peer.id) },
                        onSpeedTest = { onActionStartSpeedTest(peer.id) },
                        onRespond = { onRespondPairing(peer, it) },
                        onForget = { onForgetPeer(peer) }
                    )
                }
            }
        }

        val recent = remember(feed) { dedupeDeviceEvents(feed).take(5) }
        Column {
            SectionHeader(c, "Recent", null, if (feed.size > 5) ({ onTabSelected(AppTab.Activity) }) else null)
            if (recent.isEmpty()) {
                EmptyRecent(c)
            } else {
                Panel(c) {
                    recent.forEachIndexed { i, entry ->
                        if (i > 0) Hairline(c)
                        ActivityRow(
                            c = c,
                            entry = entry,
                            onApply = { onApplyClipboard(entry) },
                            onResend = { onResendActivity(entry) },
                            onDelete = { onDeleteActivity(entry) },
                            onTogglePin = { onTogglePinActivity(entry) }
                        )
                    }
                }
            }
        }

        // Clears the floating dock.
        Spacer(Modifier.height(140.dp))
    }

    if (showSendSheet) {
        SendSheet(
            c = c,
            connected = connected,
            onSend = { folder, target -> if (folder) onActionSendFolder(target) else onActionSendFiles(target) },
            onDismiss = { showSendSheet = false }
        )
    }
}

@Composable
private fun HomeTopBar(
    c: DdColors,
    deviceName: String,
    connected: List<PeerSnapshot>,
    hasPeers: Boolean,
    onScanQr: () -> Unit,
    onManualIp: () -> Unit
) {
    Column(Modifier.fillMaxWidth().padding(top = 12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            // No app name: the launcher already says it. This phone's name is what
            // other devices list it as when pairing, so that is what's worth showing.
            Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically) {
                if (deviceName.isNotBlank()) {
                    Text("Visible as ", style = DdType.small, color = c.textMuted, maxLines = 1)
                    Text(deviceName, style = DdType.title, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            AddDeviceButton(c, onScanQr = onScanQr, onManualIp = onManualIp)
        }
        if (hasPeers) {
            Spacer(Modifier.height(6.dp))
            LinkStatusLine(c, connected)
        }
    }
}

/** "● Connected  192.168.1.20 · synced 1h ago", or "○ Not connected". */
@Composable
private fun LinkStatusLine(c: DdColors, connected: List<PeerSnapshot>) {
    val lead = connected.firstOrNull()
    Row(verticalAlignment = Alignment.CenterVertically) {
        if (lead != null) {
            Box(Modifier.size(8.dp).background(c.live, CircleShape))
            Spacer(Modifier.width(8.dp))
            // Who, not where: the address lives on the Devices tab.
            val who = if (connected.size == 1) lead.name else "${connected.size} devices"
            Text(
                "Connected to $who",
                style = DdType.label,
                color = c.live,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = false)
            )
            agoLabel(connected.mapNotNull { it.lastSyncSecs }.maxOrNull())?.let { synced ->
                Spacer(Modifier.width(10.dp))
                Text("synced $synced", style = DdType.small, color = c.textMuted, maxLines = 1)
            }
        } else {
            Box(Modifier.size(8.dp).border(1.5.dp, c.textMuted, CircleShape))
            Spacer(Modifier.width(8.dp))
            Text("Not connected", style = DdType.label, color = c.textMuted, maxLines = 1)
        }
    }
}

// ---------------------------------------------------------------- status

/** Who this phone is linked to, said plainly. */
/** What stops sync right now, and the one thing to do about it. */
@Composable
private fun HealthBanner(c: DdColors, issue: HealthIssue, onAction: (HealthIssue) -> Unit) {
    Panel(c) {
        Column(Modifier.padding(16.dp)) {
            Text(issue.title, style = DdType.label, color = c.text)
            Spacer(Modifier.height(4.dp))
            Text(issue.detail, style = DdType.small, color = c.textMuted)
            healthActionLabel(issue.kind)?.let { label ->
                Spacer(Modifier.height(12.dp))
                PillButton(c, label, filled = true, compact = true) { onAction(issue) }
            }
        }
    }
}

internal fun healthActionLabel(kind: String): String? = when (kind) {
    "no_network" -> "Wi-Fi settings"
    "sync_paused" -> "Resume sync"
    "devices_not_found", "connection_blocked" -> "Search again"
    "listener_down" -> "Restart Link All"
    "battery_restricted" -> "Allow"
    else -> null
}

/** Nothing connected: how this phone reconnects, or how to link it. */
@Composable
private fun StatusBlock(
    c: DdColors,
    hasPeers: Boolean,
    onAddDevice: () -> Unit
) {
    Column(Modifier.fillMaxWidth()) {
        if (hasPeers) {
            Text(
                "Paired devices reconnect on their own when they're on the same Wi-Fi or hotspot.",
                style = DdType.body,
                color = c.textMuted
            )
        } else {
            Text("Link this phone to your computer", style = DdType.display, color = c.text)
            Spacer(Modifier.height(8.dp))
            Text(
                "Share your clipboard and send files over your own network. Nothing goes through a cloud.",
                style = DdType.body,
                color = c.textMuted
            )
            Spacer(Modifier.height(20.dp))
            PillButton(c, "Pair a device", filled = true, onClick = onAddDevice)
        }
    }
}

@Composable
internal fun PairingPanel(c: DdColors, peer: PeerSnapshot, onRespond: (PeerSnapshot, Boolean) -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(PanelShape)
            .background(c.surface)
            .border(1.dp, c.accent, PanelShape)
            .padding(20.dp)
    ) {
        Text("Pairing request", style = DdType.label, color = c.accent)
        Spacer(Modifier.height(6.dp))
        Text(
            if (peer.outgoingPairingWaiting) "${peer.name} also wants to link" else "${peer.name} wants to link",
            style = DdType.display.copy(fontSize = 24.sp, lineHeight = 28.sp),
            color = c.text,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis
        )
        Spacer(Modifier.height(18.dp))
        PinTiles(c, peer.pairingPin)
        Spacer(Modifier.height(12.dp))
        Text("Only accept if this code matches the one on ${peer.name}.", style = DdType.small, color = c.textMuted)
        PairingCountdown(c, peer)
        Spacer(Modifier.height(18.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            PillButton(c, "Decline", filled = false, modifier = Modifier.weight(1f)) { onRespond(peer, false) }
            PillButton(c, "Accept", filled = true, modifier = Modifier.weight(1f)) { onRespond(peer, true) }
        }
    }
}


/** Our own request, waiting on the other device: the code to compare there, and a way out. */
@Composable
internal fun OutgoingPairingPanel(c: DdColors, peer: PeerSnapshot, onCancel: (PeerSnapshot) -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(PanelShape)
            .background(c.surface)
            .border(1.dp, c.accent, PanelShape)
            .padding(20.dp)
    ) {
        Text("Waiting for approval", style = DdType.label, color = c.accent)
        Spacer(Modifier.height(6.dp))
        Text(
            "Accept on ${peer.name}",
            style = DdType.display.copy(fontSize = 24.sp, lineHeight = 28.sp),
            color = c.text,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis
        )
        Spacer(Modifier.height(18.dp))
        PinTiles(c, peer.pairingPin)
        Spacer(Modifier.height(12.dp))
        Text(
            if (peer.pairingPin == null) "Connecting to ${peer.name}…"
            else "${peer.name} shows a request with a code. Accept it there if it matches this one.",
            style = DdType.small,
            color = c.textMuted
        )
        PairingCountdown(c, peer)
        Spacer(Modifier.height(18.dp))
        PillButton(c, "Cancel request", filled = false, modifier = Modifier.fillMaxWidth()) { onCancel(peer) }
    }
}

/** "Expires in 42s", ticking locally between snapshots. */
@Composable
internal fun PairingCountdown(c: DdColors, peer: PeerSnapshot) {
    val expiresIn = peer.pairingExpiresInSecs ?: return
    val deadline = remember(peer.id, expiresIn) { System.currentTimeMillis() + expiresIn * 1000L }
    var left by remember(deadline) { mutableIntStateOf(expiresIn) }
    LaunchedEffect(deadline) {
        while (left > 0) {
            delay(1_000)
            left = ((deadline - System.currentTimeMillis()) / 1000L).toInt().coerceAtLeast(0)
        }
    }
    Spacer(Modifier.height(4.dp))
    Text("Expires in ${left}s", style = DdType.small, color = c.textMuted)
}

internal fun pairingWaitingLabel(peer: PeerSnapshot): String =
    peer.pairingPin?.let { "Accept on ${peer.name} · code $it" } ?: "Waiting for ${peer.name}"




@Composable
private fun ActionRow(
    c: DdColors,
    icon: ImageVector,
    title: String,
    detail: String,
    enabled: Boolean,
    onClick: () -> Unit
) {
    val haptic = LocalHapticFeedback.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .graphicsLayer { alpha = if (enabled) 1f else 0.5f }
            .combinedClickable(enabled = enabled, onClick = {
                haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                onClick()
            })
            .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Box(
            Modifier.size(40.dp).clip(WellShape).background(c.accentSoft),
            contentAlignment = Alignment.Center
        ) {
            Icon(icon, contentDescription = null, tint = c.accent, modifier = Modifier.size(20.dp))
        }
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(title, style = DdType.label, color = c.text)
            Text(
                detail,
                style = DdType.small,
                color = c.textMuted,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis
            )
        }
        Spacer(Modifier.width(8.dp))
        Icon(Icons.Rounded.ChevronRight, contentDescription = null, tint = c.textMuted, modifier = Modifier.size(20.dp))
    }
}

// ---------------------------------------------------------------- lists

@Composable
private fun TransferRow(
    c: DdColors,
    transfer: TransferProgress,
    onPause: () -> Unit,
    onResume: () -> Unit,
    onCancel: () -> Unit
) {
    val ratio = if (transfer.totalBytes > 0L) {
        (transfer.bytesReceived.toDouble() / transfer.totalBytes).coerceIn(0.0, 1.0).toFloat()
    } else transfer.percent / 100f
    // Data lands in 4 MB chunks, so the raw ratio moves in steps. Glide to
    // each new value over roughly the time the last step took, which keeps
    // the bar moving steadily instead of jumping.
    val shown = remember(transfer.id) { Animatable(ratio) }
    var lastStepAt by remember(transfer.id) { mutableLongStateOf(0L) }
    LaunchedEffect(ratio) {
        val now = android.os.SystemClock.uptimeMillis()
        val gap = if (lastStepAt == 0L) 300L else (now - lastStepAt).coerceIn(120L, 1200L)
        lastStepAt = now
        if (ratio < shown.value) shown.snapTo(ratio)
        else shown.animateTo(ratio, tween(gap.toInt(), easing = LinearEasing))
    }
    val animated = shown.value
    val speed = when {
        transfer.isPaused -> "Paused"
        transfer.speedBps >= 1024 * 1024 -> String.format(java.util.Locale.US, "%.1f MB/s", transfer.speedBps / (1024.0 * 1024))
        transfer.speedBps >= 1024 -> "${transfer.speedBps / 1024} KB/s"
        else -> "Starting…"
    }
    val direction = when {
        transfer.peerName.isBlank() -> null
        transfer.isOutbound -> "to ${transfer.peerName}"
        else -> "from ${transfer.peerName}"
    }

    Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 12.dp, bottom = 14.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(transfer.fileName, style = DdType.label, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    listOfNotNull(direction, speed).joinToString(" · "),
                    style = DdType.small,
                    color = if (transfer.isPaused) c.warn else c.textMuted,
                    maxLines = 1
                )
            }
            Text("${(animated * 100).toInt()}%", style = DdType.mono, color = c.text)
            // A folder pauses file by file in the engine; offer only Cancel.
            if (!transfer.id.startsWith(app.linkall.LinkAllService.FOLDER_ROW_PREFIX)) {
                IconButton(onClick = if (transfer.isPaused) onResume else onPause) {
                    Icon(
                        if (transfer.isPaused) Icons.Rounded.PlayArrow else Icons.Rounded.Pause,
                        contentDescription = if (transfer.isPaused) "Resume" else "Pause",
                        tint = c.text,
                        modifier = Modifier.size(20.dp)
                    )
                }
            }
            IconButton(onClick = onCancel) {
                Icon(Icons.Rounded.Close, contentDescription = "Cancel", tint = c.textMuted, modifier = Modifier.size(20.dp))
            }
        }
        Spacer(Modifier.height(8.dp))
        Box(
            Modifier
                .padding(end = 12.dp)
                .fillMaxWidth()
                .height(3.dp)
                .clip(CircleShape)
                .background(c.surfaceSunk)
        ) {
            Box(
                Modifier
                    .fillMaxWidth(animated)
                    .fillMaxHeight()
                    .background(if (transfer.isPaused) c.warn else c.accent)
            )
        }
    }
}

@Composable
internal fun DeviceRow(
    c: DdColors,
    peer: PeerSnapshot,
    speedTest: SpeedTestProgress?,
    onSendFiles: () -> Unit,
    onSendFolder: () -> Unit,
    onSpeedTest: () -> Unit,
    onRespond: (Boolean) -> Unit,
    onForget: () -> Unit
) {
    val haptic = LocalHapticFeedback.current
    var menuOpen by remember { mutableStateOf(false) }
    val (status, statusColor) = when {
        speedTest != null -> "${speedTest.phase} · ${speedTest.speedMbpsString}" to c.accent
        peer.pairingRequested && !peer.trusted -> "Wants to pair" to c.accent
        peer.isConnected -> "Connected" to c.live
        peer.outgoingPairingWaiting && !peer.trusted -> pairingWaitingLabel(peer) to c.warn
        peer.pairingOutcomeLabel != null -> peer.pairingOutcomeLabel!! to c.warn
        peer.isConnecting -> "Connecting…" to c.textMuted
        peer.trusted -> (agoLabel(peer.lastSeenSecs)?.let { "Seen $it" } ?: "Offline") to c.textMuted
        else -> "Not paired" to c.warn
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
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Box(
                modifier = Modifier
                    .size(40.dp)
                    .clip(WellShape)
                    .background(if (peer.isConnected) c.accentSoft else c.surfaceSunk),
                contentAlignment = Alignment.Center
            ) {
                Icon(
                    osIcon(peer.platform, peer.name),
                    contentDescription = null,
                    tint = if (peer.isConnected) c.accent else c.textMuted,
                    modifier = Modifier.size(20.dp)
                )
            }
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(peer.name, style = DdType.label, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (peer.isConnected) {
                        Box(Modifier.size(6.dp).background(c.live, CircleShape))
                        Spacer(Modifier.width(6.dp))
                    }
                    Text(status, style = DdType.small, color = statusColor, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            Icon(Icons.Rounded.MoreHoriz, contentDescription = "Options for ${peer.name}", tint = c.textMuted, modifier = Modifier.size(20.dp))
        }

        if (menuOpen) {
            ActionSheet(
                c,
                icon = osIcon(peer.platform, peer.name),
                title = peer.name,
                subtitle = status,
                actions = buildList {
                    if (peer.pairingRequested && !peer.trusted) {
                        add(SheetAction(Icons.Outlined.Check, "Accept pairing") { onRespond(true) })
                        add(SheetAction(Icons.Outlined.Close, "Decline") { onRespond(false) })
                    } else if (peer.isConnected) {
                        add(SheetAction(Icons.Outlined.UploadFile, "Send files", "Photos, videos, documents, anything", onClick = onSendFiles))
                        add(SheetAction(Icons.Outlined.DriveFolderUpload, "Send a folder", "Everything in it, subfolders included", onClick = onSendFolder))
                        add(SheetAction(Icons.Outlined.Speed, "Test speed", "How fast this link is right now", onClick = onSpeedTest))
                    }
                    add(SheetAction(Icons.Outlined.DeleteOutline, "Forget device", danger = true, onClick = onForget))
                },
                onDismiss = { menuOpen = false }
            )
        }
    }
}


@Composable
private fun EmptyRecent(c: DdColors) = EmptyBox(c, Icons.Outlined.History, "Copy something on either device, or send a file. It shows up here.")


// ---------------------------------------------------------------- primitives






// ---------------------------------------------------------------- dialogs


// ---------------------------------------------------------------- helpers

/** Keeps only the latest connect/disconnect per device so they don't flood the list. */
private fun dedupeDeviceEvents(feed: List<ActivityEntry>): List<ActivityEntry> {
    val seen = mutableSetOf<String>()
    return feed.filter { e ->
        val isDeviceEvent = e.kind == ActivityKind.PEER_CONNECTED || e.kind == ActivityKind.PEER_DISCONNECTED
        !isDeviceEvent || seen.add(e.deviceName)
    }
}


