@file:OptIn(ExperimentalFoundationApi::class)

package app.linkall.ui

import androidx.compose.foundation.clickable
import app.linkall.controllableComputers
import app.linkall.supportsRemoteControl
import app.linkall.UpdateChecker
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
import androidx.compose.material.icons.automirrored.outlined.StickyNote2
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
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.linkall.ActivityEntry
import app.linkall.R
import androidx.compose.ui.res.painterResource
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
    peers: List<PeerSnapshot>,
    feed: List<ActivityEntry>,
    activeTransfers: List<TransferProgress>,
    activeSpeedTests: List<SpeedTestProgress>,
    onActionPushClipboard: () -> Unit,
    onActionSendQuickContext: () -> Unit,
    quickContextText: String?,
    onActionPairMagicLink: () -> Unit,
    onManualIp: () -> Unit,
    onActionSendFiles: (String?) -> Unit,
    onActionSendFolder: (String?) -> Unit,
    onApplyClipboard: (ActivityEntry) -> Unit,
    onActionPauseTransfer: (String) -> Unit,
    onActionResumeTransfer: (String) -> Unit,
    onActionCancelTransfer: (String) -> Unit,
    onDeleteActivity: (ActivityEntry) -> Unit,
    onTogglePinActivity: (ActivityEntry) -> Unit = {},
    onReplayOnboarding: () -> Unit,
    onOpenNotes: () -> Unit,
    onTabSelected: (AppTab) -> Unit,
    onRespondPairing: (PeerSnapshot, Boolean) -> Unit,
    onSendPairingRequest: (PeerSnapshot) -> Unit,
    onCancelPairing: (PeerSnapshot) -> Unit,
    healthIssue: HealthIssue? = null,
    moreIssues: Int = 0,
    onHealthAction: (HealthIssue) -> Unit = {}
) {
    val c = remember(isDark) { DdColors(isDark) }
    // An incoming request outranks our own: when both users tapped Pair,
    // accepting theirs settles both.
    val pairingRequest = peers.firstOrNull { it.pairingRequested && !it.trusted }
    val outgoingRequest = peers.firstOrNull { it.outgoingPairingWaiting && !it.trusted }
    val openDevice = LocalOpenDevice.current
    val browseFiles = LocalBrowseFiles.current
    val nowPlaying = rememberNowPlaying()
    var sendTarget by remember { mutableStateOf<PeerSnapshot?>(null) }
    var padTarget by remember { mutableStateOf<PeerSnapshot?>(null) }
    val groups = remember(peers) { homeDeviceGroups(peers) }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = PageGutter)
    ) {
        // This phone's identity and link state.
        HomeTopBar(c = c, isDark = isDark, peers = peers, onOpenNotes = onOpenNotes, onScanQr = onActionPairMagicLink, onManualIp = onManualIp)

        Spacer(Modifier.height(20.dp))

        if (pairingRequest != null) {
            PairingPanel(c, pairingRequest, onRespondPairing)
            Spacer(Modifier.height(8.dp))
        } else if (outgoingRequest != null) {
            OutgoingPairingPanel(c, outgoingRequest, onCancelPairing)
            Spacer(Modifier.height(8.dp))
        } else {
            val update = UpdateChecker.available.collectAsState().value
            if (update != null) {
                UpdateBanner(c, update)
                Spacer(Modifier.height(16.dp))
            }
            if (healthIssue != null) {
                HealthBanner(c, healthIssue, onHealthAction)
                if (moreIssues > 0) {
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "$moreIssues more to fix in Setup  →",
                        style = DdType.small.copy(fontWeight = FontWeight.Medium),
                        color = c.accent,
                        modifier = Modifier
                            .clip(CircleShape)
                            .clickable { onTabSelected(AppTab.Settings) }
                            .padding(horizontal = 4.dp, vertical = 6.dp),
                    )
                }
                Spacer(Modifier.height(16.dp))
            }
            if (groups.connected.isEmpty()) {
                StatusBlock(c = c, hasPeers = groups.other.isNotEmpty(), onAddDevice = onReplayOnboarding)
            }
        }

        // Each linked device, with what it's playing and its quick actions.
        // Tapping one opens its page, where everything else is.
        if (groups.connected.isNotEmpty()) {
            SectionHeader(c, "Your devices", null)
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                groups.connected.forEach { peer ->
                    DeviceHomeCard(
                        c = c,
                        peer = peer,
                        playing = nowPlaying[peer.id],
                        speedTest = activeSpeedTests.find { it.peerId == peer.id },
                        onOpen = { openDevice(peer) },
                        onSend = { sendTarget = peer },
                        onClipboard = if (quickContextText.isNullOrBlank()) onActionPushClipboard else onActionSendQuickContext,
                        onMouse = { padTarget = peer },
                        onBrowse = { browseFiles(peer) },
                    )
                }
            }
        }

        if (groups.other.isNotEmpty()) {
            SectionHeader(c, if (groups.connected.isEmpty()) "Your devices" else "Other devices", null)
            Panel(c) {
                groups.other.forEachIndexed { i, peer ->
                    if (i > 0) Hairline(c)
                    CompactDeviceRow(
                        c = c,
                        peer = peer,
                        speedTest = activeSpeedTests.find { it.peerId == peer.id },
                        onOpen = { openDevice(peer) },
                        onPair = { onSendPairingRequest(peer) },
                        onAccept = { onRespondPairing(peer, true) },
                    )
                }
            }
        }

        // What was copied here, one tap from the computers. What it is, not
        // what it says: the home screen is often seen by others, and copied
        // text can be a password or a code.
        val clip = quickContextText?.trim()?.takeIf { it.isNotBlank() }
        if (clip != null && groups.connected.isNotEmpty()) {
            Spacer(Modifier.height(16.dp))
            ClipboardCard(c, clip.length, onSend = onActionSendQuickContext)
        }

        if (activeTransfers.isNotEmpty()) {
            SectionHeader(c, "Transferring", "${activeTransfers.size}")
            Panel(c) {
                activeTransfers.forEachIndexed { i, t ->
                    if (i > 0) Hairline(c, 16.dp)
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

        val recent = remember(feed) { dedupeDeviceEvents(feed).take(3) }
        SectionHeader(c, "Recent", null, if (feed.isNotEmpty()) ({ onTabSelected(AppTab.Activity) }) else null)
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
                        onDelete = { onDeleteActivity(entry) },
                        onTogglePin = { onTogglePinActivity(entry) }
                    )
                }
            }
        }

        // Clears the floating dock.
        Spacer(Modifier.height(140.dp))
    }

    sendTarget?.let { peer ->
        ActionSheet(
            c, icon = osIcon(peer.platform, peer.name), title = "Send to ${peer.name}", subtitle = null,
            actions = listOf(
                SheetAction(Icons.Outlined.UploadFile, "Files", "Photos, videos, documents, anything") { onActionSendFiles(peer.id) },
                SheetAction(Icons.Outlined.DriveFolderUpload, "A folder", "Everything in it, subfolders included") { onActionSendFolder(peer.id) },
            ),
            onDismiss = { sendTarget = null },
        )
    }
    padTarget?.let { TrackpadDialog(c, it, onDismiss = { padTarget = null }) }
}

@Composable
private fun HomeTopBar(
    c: DdColors,
    isDark: Boolean,
    peers: List<PeerSnapshot>,
    onOpenNotes: () -> Unit,
    onScanQr: () -> Unit,
    onManualIp: () -> Unit,
) {
    // Redraws "synced 2m ago" once a minute while Home is on screen.
    val now by produceState(System.currentTimeMillis()) {
        while (true) {
            delay(60_000)
            value = System.currentTimeMillis()
        }
    }
    // Paired devices only; a device still pairing shows in its own panel below.
    val linked = peers.filter { it.trusted }
    val synced = remember(linked, now) {
        agoLabel(linked.filter { it.isConnected }.mapNotNull { it.lastSyncSecs }.maxOrNull())
    }
    Row(Modifier.fillMaxWidth().padding(top = 16.dp), verticalAlignment = Alignment.CenterVertically) {
        Image(
            painter = painterResource(if (isDark) R.drawable.header_logo_dark else R.drawable.header_logo_light),
            contentDescription = null,
            modifier = Modifier.size(40.dp)
        )
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text("Link All", style = DdType.pageTitle.copy(fontSize = 24.sp, lineHeight = 28.sp), color = c.text)
            if (linked.isNotEmpty()) LinkStatusLine(c, linked, synced, Modifier.padding(top = 2.dp))
        }
        TopIconButton(c, Icons.AutoMirrored.Outlined.StickyNote2, "Notes", onClick = onOpenNotes)
        Spacer(Modifier.width(8.dp))
        AddDeviceButton(c, onScanQr = onScanQr, onManualIp = onManualIp)
    }
}

// ---------------------------------------------------------------- status

/** Who this phone is linked to, said plainly. */
/** A newer version exists. Opens its release page; never installs. */
@Composable
private fun UpdateBanner(c: DdColors, update: UpdateChecker.Update) {
    val context = androidx.compose.ui.platform.LocalContext.current
    Panel(c) {
        Column(Modifier.padding(16.dp)) {
            Text("Link All ${update.version} is out", style = DdType.label, color = c.text)
            Spacer(Modifier.height(4.dp))
            Text("Download it from the release page to update.", style = DdType.small, color = c.textMuted)
            Spacer(Modifier.height(12.dp))
            PillButton(c, "See what's new", filled = true, compact = true) {
                context.startActivity(
                    android.content.Intent(android.content.Intent.ACTION_VIEW, android.net.Uri.parse(update.url))
                        .addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
                )
            }
        }
    }
}

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
    "notification_access" -> "Turn on"
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




// ---------------------------------------------------------------- lists

@Composable
internal fun TransferRow(
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

/**
 * A linked device on Home: who it is, what it's playing, and the three things
 * done most with it. Tapping the card opens the device's page.
 */
@Composable
private fun DeviceHomeCard(
    c: DdColors,
    peer: PeerSnapshot,
    playing: NowPlayingItem?,
    speedTest: SpeedTestProgress?,
    onOpen: () -> Unit,
    onSend: () -> Unit,
    onClipboard: () -> Unit,
    onMouse: () -> Unit,
    onBrowse: () -> Unit,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val (status, statusColor) = deviceStatus(c, peer, speedTest)
    val isComputer = peer.platform != "Android"
    val controllable = isComputer && supportsRemoteControl(peer.appVersion)
    Column(
        Modifier
            .fillMaxWidth()
            .clip(PanelShape)
            .background(c.surface)
            .border(1.dp, c.line, PanelShape)
            .clickable(onClickLabel = "Open ${peer.name}", onClick = onOpen)
            .padding(16.dp)
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.semantics(mergeDescendants = true) { contentDescription = "${peer.name}, $status" },
        ) {
            val (fg, bg) = c.tone(Tone.Blue)
            Box(Modifier.size(48.dp).clip(RoundedCornerShape(16.dp)).background(bg), contentAlignment = Alignment.Center) {
                Icon(osIcon(peer.platform, peer.name), contentDescription = null, tint = fg, modifier = Modifier.size(24.dp))
            }
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(peer.name, style = DdType.title, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.size(6.dp).background(c.live, CircleShape))
                    Spacer(Modifier.width(6.dp))
                    Text(
                        listOfNotNull(status, peer.platform).joinToString(" · "),
                        style = DdType.small, color = statusColor, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            Icon(Icons.Rounded.ChevronRight, contentDescription = null, tint = c.textMuted, modifier = Modifier.size(22.dp))
        }
        if (playing != null) {
            Spacer(Modifier.height(12.dp))
            NowPlayingMini(c, playing)
        }
        Spacer(Modifier.height(14.dp))
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            QuickAction(c, Icons.Outlined.UploadFile, "Send", primary = true, onClick = onSend)
            QuickAction(c, Icons.Outlined.ContentPaste, "Clipboard", onClick = onClipboard)
            when {
                controllable -> QuickAction(c, Icons.Outlined.Mouse, "Mouse", onClick = onMouse)
                isComputer -> QuickAction(c, Icons.Outlined.Mouse, "Mouse", muted = true) {
                    android.widget.Toast.makeText(context, "Update Link All on ${peer.name} to control it.", android.widget.Toast.LENGTH_LONG).show()
                }
                else -> QuickAction(c, Icons.Outlined.FolderOpen, "Browse", onClick = onBrowse)
            }
        }
    }
}

/** A quick action on a device card: icon over its full label, so nothing is clipped at three across. */
@Composable
private fun RowScope.QuickAction(
    c: DdColors,
    icon: ImageVector,
    label: String,
    primary: Boolean = false,
    muted: Boolean = false,
    onClick: () -> Unit,
) {
    val shape = RoundedCornerShape(16.dp)
    val fg = when {
        primary -> c.onAccent
        muted -> c.textMuted
        else -> c.text
    }
    Column(
        Modifier
            .weight(1f)
            .clip(shape)
            .background(if (primary) c.accent else c.surfaceSunk)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(vertical = 11.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(icon, contentDescription = null, tint = if (primary || muted) fg else c.accent, modifier = Modifier.size(22.dp))
        Spacer(Modifier.height(4.dp))
        Text(label, style = DdType.small.copy(fontWeight = FontWeight.Medium), color = fg, maxLines = 1)
    }
}

/** A device that isn't linked right now, or one nearby to pair: one line, and its page on a tap. */
@Composable
private fun CompactDeviceRow(
    c: DdColors,
    peer: PeerSnapshot,
    speedTest: SpeedTestProgress?,
    onOpen: () -> Unit,
    onPair: () -> Unit,
    onAccept: () -> Unit,
) {
    val asksUs = peer.pairingRequested && !peer.trusted
    val waiting = peer.outgoingPairingWaiting && !peer.trusted
    val canPair = !peer.trusted && !asksUs && !waiting && !peer.isConnecting
    val (status, statusColor) = deviceStatus(c, peer, speedTest)
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClickLabel = "Open ${peer.name}", onClick = onOpen)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        IconWell(c, osIcon(peer.platform, peer.name))
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(peer.name, style = DdType.label, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(status, style = DdType.small, color = statusColor, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Spacer(Modifier.width(8.dp))
        when {
            asksUs -> PillButton(c, "Accept", filled = true, compact = true, onClick = onAccept)
            canPair -> PillButton(c, "Pair", filled = true, compact = true, onClick = onPair)
            waiting -> {}
            else -> Icon(Icons.Rounded.ChevronRight, contentDescription = null, tint = c.textMuted, modifier = Modifier.size(20.dp))
        }
    }
}

/** Something was copied here: send just that, without showing it. */
@Composable
private fun ClipboardCard(c: DdColors, length: Int, onSend: () -> Unit) {
    Panel(c) {
        Row(Modifier.padding(horizontal = 16.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
            val (fg, bg) = c.tone(Tone.Blue)
            IconWell(c, Icons.Outlined.ContentPaste, tint = fg, background = bg)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text("Copied text", style = DdType.label, color = c.text)
                Text("$length characters", style = DdType.small, color = c.textMuted, maxLines = 1)
            }
            PillButton(c, "Send", filled = true, compact = true, onClick = onSend)
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


