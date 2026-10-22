package app.linkall.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.NavigateBefore
import androidx.compose.material.icons.automirrored.outlined.NavigateNext
import androidx.compose.material.icons.automirrored.outlined.VolumeDown
import androidx.compose.material.icons.automirrored.outlined.VolumeOff
import androidx.compose.material.icons.automirrored.outlined.VolumeUp
import androidx.compose.material.icons.outlined.ContentPaste
import androidx.compose.material.icons.outlined.DarkMode
import androidx.compose.material.icons.outlined.DeleteOutline
import androidx.compose.material.icons.outlined.DriveFolderUpload
import androidx.compose.material.icons.outlined.FolderOpen
import androidx.compose.material.icons.outlined.Link
import androidx.compose.material.icons.outlined.LinkOff
import androidx.compose.material.icons.outlined.Lock
import androidx.compose.material.icons.outlined.Mouse
import androidx.compose.material.icons.outlined.NotificationsActive
import androidx.compose.material.icons.outlined.NotificationsOff
import androidx.compose.material.icons.outlined.PlayArrow
import androidx.compose.material.icons.outlined.PowerSettingsNew
import androidx.compose.material.icons.outlined.SkipNext
import androidx.compose.material.icons.outlined.SkipPrevious
import androidx.compose.material.icons.outlined.Speed
import androidx.compose.material.icons.outlined.SystemUpdate
import androidx.compose.material.icons.outlined.UploadFile
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.linkall.PeerSnapshot
import app.linkall.SpeedTestProgress
import app.linkall.supportsRemoteControl

/**
 * One device and everything this phone can do with it, on one page: send to
 * it, control it, play its music, make it ring, browse its files, and look
 * after the link. Each action is already aimed at this device, so nothing asks
 * "which one?". Opened from its card on Home.
 */
@Composable
internal fun DeviceScreen(
    isDark: Boolean,
    peer: PeerSnapshot,
    speedTest: SpeedTestProgress?,
    onBack: () -> Unit,
    onSendFiles: () -> Unit,
    onSendFolder: () -> Unit,
    onSendClipboard: () -> Unit,
    onConnect: () -> Unit,
    onDisconnect: () -> Unit,
    onForget: () -> Unit,
    onPair: () -> Unit,
    onRespondPairing: (Boolean) -> Unit,
    onCancelPairing: () -> Unit,
    onSpeedTest: () -> Unit,
) {
    BackHandler(onBack = onBack)
    val c = rememberDdColors(isDark)
    val context = LocalContext.current
    val browse = LocalBrowseFiles.current
    val playing = rememberNowPlaying()[peer.id]
    var showPad by remember { mutableStateOf(false) }
    var confirmForget by remember { mutableStateOf(false) }
    val (status, statusColor) = deviceStatus(c, peer, speedTest)
    val isComputer = peer.platform != "Android"
    val controllable = isComputer && supportsRemoteControl(peer.appVersion)
    val linked = peer.trusted && peer.isConnected

    Column(
        Modifier
            .fillMaxSize()
            .background(c.page)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = PageGutter),
    ) {
        Row(Modifier.fillMaxWidth().padding(top = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            TopIconButton(c, Icons.AutoMirrored.Outlined.ArrowBack, "Back", onClick = onBack)
        }

        // Who it is and how it's linked.
        Spacer(Modifier.height(20.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            val (fg, bg) = c.tone(if (linked) Tone.Blue else Tone.Neutral)
            Box(Modifier.size(64.dp).clip(RoundedCornerShape(20.dp)).background(bg), contentAlignment = Alignment.Center) {
                Icon(osIcon(peer.platform, peer.name), contentDescription = null, tint = fg, modifier = Modifier.size(32.dp))
            }
            Spacer(Modifier.width(16.dp))
            Column(Modifier.weight(1f)) {
                Text(peer.name, style = DdType.pageTitle, color = c.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Spacer(Modifier.height(2.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (linked) {
                        Box(Modifier.size(7.dp).background(c.live, CircleShape))
                        Spacer(Modifier.width(7.dp))
                    }
                    Text(
                        listOfNotNull(status, peer.platform).joinToString(" · "),
                        style = DdType.small, color = statusColor, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }

        when {
            peer.pairingRequested && !peer.trusted -> {
                Spacer(Modifier.height(24.dp))
                PairingPanel(c, peer) { _, accept -> onRespondPairing(accept) }
            }
            peer.outgoingPairingWaiting && !peer.trusted -> {
                Spacer(Modifier.height(24.dp))
                OutgoingPairingPanel(c, peer) { onCancelPairing() }
            }
            !peer.trusted -> {
                Spacer(Modifier.height(24.dp))
                Panel(c) {
                    Column(Modifier.padding(18.dp)) {
                        Text("Not paired yet", style = DdType.label, color = c.text)
                        Spacer(Modifier.height(4.dp))
                        Text(
                            "Pair to share your clipboard and files with ${peer.name}. Both devices show a code to compare first.",
                            style = DdType.small, color = c.textMuted,
                        )
                        Spacer(Modifier.height(14.dp))
                        PillButton(c, "Pair with ${peer.name}", filled = true, modifier = Modifier.fillMaxWidth(), onClick = onPair)
                    }
                }
            }
            !peer.isConnected -> {
                Spacer(Modifier.height(24.dp))
                Panel(c) {
                    Column(Modifier.padding(18.dp)) {
                        Text("${peer.name} isn't connected", style = DdType.label, color = c.text)
                        Spacer(Modifier.height(4.dp))
                        Text(
                            "It reconnects on its own when both are on the same Wi-Fi or hotspot and Link All is open on it.",
                            style = DdType.small, color = c.textMuted,
                        )
                        Spacer(Modifier.height(14.dp))
                        if (peer.canWake) {
                            PillButton(c, "Wake ${peer.name}", filled = true, modifier = Modifier.fillMaxWidth()) {
                                RemoteActions.wake(context, peer)
                            }
                            Spacer(Modifier.height(8.dp))
                            PillButton(c, "Connect now", filled = false, modifier = Modifier.fillMaxWidth(), onClick = onConnect)
                            Spacer(Modifier.height(8.dp))
                            Text(
                                "Waking works when the computer is on a cable and Wake-on-LAN is turned on in its network settings.",
                                style = DdType.small, color = c.textMuted,
                            )
                        } else {
                            PillButton(c, "Connect now", filled = true, modifier = Modifier.fillMaxWidth(), onClick = onConnect)
                        }
                    }
                }
            }
        }

        if (linked) {
            playing?.let {
                Spacer(Modifier.height(20.dp))
                NowPlayingPanel(c, it)
            }

            GroupLabel(c, "Share")
            TileGrid(
                c,
                listOf(
                    TileSpec(Icons.Outlined.UploadFile, "Send files", Tone.Blue, onClick = onSendFiles),
                    TileSpec(Icons.Outlined.DriveFolderUpload, "Send a folder", Tone.Blue, onClick = onSendFolder),
                    TileSpec(Icons.Outlined.ContentPaste, "Send clipboard", Tone.Blue, onClick = onSendClipboard),
                ),
            )

            GroupLabel(c, "Files")
            TileGrid(c, listOf(TileSpec(Icons.Outlined.FolderOpen, "Browse files", Tone.Green) { browse(peer) }))

            if (isComputer) {
                if (controllable) {
                    GroupLabel(c, "Control")
                    TileGrid(
                        c,
                        listOf(
                            TileSpec(Icons.Outlined.Mouse, "Mouse & keyboard", Tone.Violet) { showPad = true },
                            TileSpec(Icons.Outlined.Lock, "Lock", Tone.Violet) { RemoteActions.press(context, peer, "lock_screen") },
                            TileSpec(Icons.Outlined.DarkMode, "Screen off", Tone.Violet) { RemoteActions.press(context, peer, "sleep_display") },
                            TileSpec(Icons.Outlined.PowerSettingsNew, "Sleep PC", Tone.Violet) { RemoteActions.press(context, peer, "sleep_computer") },
                        ),
                    )
                    GroupLabel(c, "Media")
                    TileGrid(
                        c,
                        listOf(
                            TileSpec(Icons.Outlined.SkipPrevious, "Previous", Tone.Teal) { RemoteActions.press(context, peer, "previous_track") },
                            TileSpec(Icons.Outlined.PlayArrow, "Play / pause", Tone.Teal) { RemoteActions.press(context, peer, "play_pause") },
                            TileSpec(Icons.Outlined.SkipNext, "Next", Tone.Teal) { RemoteActions.press(context, peer, "next_track") },
                            TileSpec(Icons.AutoMirrored.Outlined.VolumeDown, "Volume down", Tone.Teal) { RemoteActions.press(context, peer, "volume_down") },
                            TileSpec(Icons.AutoMirrored.Outlined.VolumeOff, "Mute", Tone.Teal) { RemoteActions.press(context, peer, "mute") },
                            TileSpec(Icons.AutoMirrored.Outlined.VolumeUp, "Volume up", Tone.Teal) { RemoteActions.press(context, peer, "volume_up") },
                        ),
                    )
                    GroupLabel(c, "Presenting")
                    TileGrid(
                        c,
                        listOf(
                            TileSpec(Icons.AutoMirrored.Outlined.NavigateBefore, "Previous slide", Tone.Teal) { RemoteActions.press(context, peer, "slide_previous") },
                            TileSpec(Icons.AutoMirrored.Outlined.NavigateNext, "Next slide", Tone.Teal) { RemoteActions.press(context, peer, "slide_next") },
                        ),
                        columns = 2,
                    )
                } else {
                    Spacer(Modifier.height(20.dp))
                    Panel(c) {
                        ListRow(
                            c, Icons.Outlined.SystemUpdate, "Update Link All on ${peer.name}",
                            detail = "Then you can control it from here: mouse, media, slides, lock",
                        )
                    }
                }
            }

            GroupLabel(c, "Find")
            TileGrid(
                c,
                listOf(
                    TileSpec(Icons.Outlined.NotificationsActive, "Ring", Tone.Amber, enabled = supportsRemoteControl(peer.appVersion)) {
                        RemoteActions.ring(context, peer, true)
                    },
                    TileSpec(Icons.Outlined.NotificationsOff, "Stop ringing", Tone.Amber, enabled = supportsRemoteControl(peer.appVersion)) {
                        RemoteActions.ring(context, peer, false)
                    },
                ),
                columns = 2,
            )
        }

        if (peer.trusted || peer.isListable) {
            GroupLabel(c, "About this device")
            Panel(c) {
                InfoRow(c, "Status", status)
                peer.platform?.let { Hairline(c, 16.dp); InfoRow(c, "System", it) }
                peer.appVersion?.let { Hairline(c, 16.dp); InfoRow(c, "Link All", it) }
                peer.ip?.let { Hairline(c, 16.dp); InfoRow(c, "Address", it) }
                agoLabel(peer.lastSyncSecs)?.let { Hairline(c, 16.dp); InfoRow(c, "Last synced", it) }
                if (linked) {
                    Hairline(c)
                    ListRow(
                        c, Icons.Outlined.Speed, "Test connection speed",
                        detail = speedTest?.let { "${it.phase} · ${it.speedMbpsString}" } ?: "Takes about ten seconds",
                        onClick = onSpeedTest,
                    )
                }
            }
        }

        if (peer.trusted) {
            GroupLabel(c, "Manage")
            Panel(c) {
                if (peer.isConnected) {
                    ListRow(c, Icons.Outlined.LinkOff, "Disconnect", detail = "Reconnect any time from this page", onClick = onDisconnect)
                } else {
                    ListRow(c, Icons.Outlined.Link, "Connect", onClick = onConnect)
                }
                Hairline(c)
                ListRow(
                    c, Icons.Outlined.DeleteOutline, "Forget ${peer.name}",
                    detail = "Unpairs it here. Pair again to use it",
                    iconTint = c.danger, titleColor = c.danger,
                    onClick = { confirmForget = true },
                )
            }
        }
        Spacer(Modifier.height(40.dp))
    }

    if (showPad) TrackpadDialog(c, peer, onDismiss = { showPad = false })
    if (confirmForget) {
        ConfirmSheet(
            c, icon = Icons.Outlined.DeleteOutline, title = "Forget ${peer.name}?",
            message = "This phone stops trusting it. To use it again, pair again and compare the code.",
            confirm = "Forget",
            dismiss = "Cancel",
            onConfirm = {
                onForget()
                onBack()
            },
            onDismiss = { confirmForget = false },
        )
    }
}

@Composable
private fun InfoRow(c: DdColors, label: String, value: String) {
    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, style = DdType.label, color = c.text, modifier = Modifier.weight(1f))
        Text(value, style = DdType.small, color = c.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}
