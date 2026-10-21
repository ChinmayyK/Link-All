package app.linkall.ui

import app.linkall.supportsRemoteControl
import android.widget.Toast
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.ui.Alignment
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.SettingsRemote
import androidx.compose.material.icons.outlined.SkipNext
import androidx.compose.material.icons.outlined.SkipPrevious
import androidx.compose.material.icons.outlined.SwapHoriz
import androidx.compose.material.icons.outlined.PlayArrow
import androidx.compose.material.icons.outlined.VolumeDown
import androidx.compose.material.icons.outlined.VolumeOff
import androidx.compose.material.icons.outlined.VolumeUp
import androidx.compose.material.icons.outlined.NavigateBefore
import androidx.compose.material.icons.outlined.NavigateNext
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import app.linkall.LinkAllJni
import app.linkall.LinkAllService
import app.linkall.PeerSnapshot

private const val LAST_REMOTE = "remote_last_device"

private fun lastRemoteDevice(context: android.content.Context): String? =
    context.getSharedPreferences(LinkAllService.PREFS_NAME, android.content.Context.MODE_PRIVATE).getString(LAST_REMOTE, null)

/**
 * Which computer to control, when more than one is connected. The one used last
 * comes first and says so. A computer that is too old to understand the remote
 * is listed but can't be picked, with the reason, so it never looks like a
 * button that just does nothing.
 */
@Composable
internal fun RemoteDevicePicker(c: DdColors, computers: List<PeerSnapshot>, onPick: (PeerSnapshot) -> Unit, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val last = remember { lastRemoteDevice(context) }
    val ordered = remember(computers) { computers.sortedByDescending { it.id == last } }
    DdSheet(c, icon = Icons.Outlined.SettingsRemote, title = "Control which computer?", subtitle = "${computers.size} are connected", onDismiss = onDismiss) { close ->
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            ordered.forEach { peer ->
                val supported = supportsRemoteControl(peer.appVersion)
                val detail = when {
                    !supported -> "Needs Link All 2.1 or later"
                    peer.id == last -> "${peer.platform ?: "Computer"} · used last"
                    else -> peer.platform
                }
                SheetOption(c, osIcon(peer.platform, peer.name), peer.name, detail) {
                    if (supported) close { onPick(peer) }
                    else Toast.makeText(context, "Update Link All on ${peer.name} to control it.", Toast.LENGTH_LONG).show()
                }
            }
        }
    }
}

/** Music, volume and slide keys for one computer. The computer decides whether to allow it. */
@Composable
internal fun RemoteSheet(c: DdColors, peer: PeerSnapshot, canChange: Boolean, onChange: () -> Unit, onDismiss: () -> Unit) {
    val context = LocalContext.current
    LaunchedEffect(peer.id) {
        context.getSharedPreferences(LinkAllService.PREFS_NAME, android.content.Context.MODE_PRIVATE).edit().putString(LAST_REMOTE, peer.id).apply()
    }
    fun press(action: String) {
        Thread {
            // Null: the engine isn't running. Empty: sent. Otherwise why it could not be sent.
            val problem = LinkAllService.withLiveEngine { handle ->
                if (LinkAllJni.remoteControl(handle, peer.id, action) == 0) "" else LinkAllJni.remoteControlError(handle, peer.id) ?: "Couldn't reach ${peer.name}."
            } ?: "Link All isn't running."
            if (problem.isNotEmpty()) {
                android.os.Handler(android.os.Looper.getMainLooper()).post {
                    Toast.makeText(context, problem, Toast.LENGTH_LONG).show()
                }
            }
        }.start()
    }

    DdSheet(
        c, icon = Icons.Outlined.SettingsRemote, title = "Remote · ${peer.name}",
        subtitle = "Turn on Remote control in Link All on that computer first",
        onDismiss = onDismiss,
    ) { close ->
        Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
            if (canChange) {
                PillButton(c, "Change computer", filled = false, compact = true, icon = Icons.Outlined.SwapHoriz) { close { onChange() } }
            }
            Group(c, "Music") {
                RemoteKey(c, Icons.Outlined.SkipPrevious, "Previous") { press("previous_track") }
                RemoteKey(c, Icons.Outlined.PlayArrow, "Play") { press("play_pause") }
                RemoteKey(c, Icons.Outlined.SkipNext, "Next") { press("next_track") }
            }
            Group(c, "Volume") {
                RemoteKey(c, Icons.Outlined.VolumeDown, "Down") { press("volume_down") }
                RemoteKey(c, Icons.Outlined.VolumeOff, "Mute") { press("mute") }
                RemoteKey(c, Icons.Outlined.VolumeUp, "Up") { press("volume_up") }
            }
            Group(c, "Slides") {
                RemoteKey(c, Icons.Outlined.NavigateBefore, "Back") { press("slide_previous") }
                RemoteKey(c, Icons.Outlined.NavigateNext, "Next") { press("slide_next") }
            }
        }
    }
}

@Composable
private fun Group(c: DdColors, title: String, content: @Composable RowScope.() -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(title, style = DdType.small, color = c.textMuted)
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) { content() }
    }
}

@Composable
private fun RowScope.RemoteKey(
    c: DdColors,
    icon: ImageVector,
    label: String,
    onClick: () -> Unit,
) {
    val shape = RoundedCornerShape(18.dp)
    Column(
        Modifier
            .weight(1f)
            .clip(shape)
            .background(c.surfaceSunk)
            .border(1.dp, c.line, shape)
            .clickable(onClick = onClick)
            .padding(vertical = 12.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(icon, contentDescription = label, tint = c.accent, modifier = Modifier.size(24.dp))
        Spacer(Modifier.height(4.dp))
        Text(label, style = DdType.small, color = c.text, maxLines = 1)
    }
}
