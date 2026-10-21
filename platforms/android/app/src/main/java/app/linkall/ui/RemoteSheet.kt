package app.linkall.ui

import app.linkall.ChoicePurpose
import app.linkall.DeviceChooser
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
import androidx.compose.material.icons.outlined.DarkMode
import androidx.compose.material.icons.outlined.Lock
import androidx.compose.material.icons.outlined.Mouse
import androidx.compose.material.icons.outlined.NotificationsActive
import androidx.compose.material.icons.outlined.NotificationsOff
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import app.linkall.LinkAllJni
import app.linkall.LinkAllService
import app.linkall.PeerSnapshot

/** Which computer to control, ranked by the core; old computers and phones say why they can't be. */
@Composable
internal fun RemoteDevicePicker(c: DdColors, connected: List<PeerSnapshot>, onPick: (PeerSnapshot) -> Unit, onDismiss: () -> Unit) {
    DeviceChooserSheet(
        c, purpose = ChoicePurpose.Remote, icon = Icons.Outlined.SettingsRemote,
        title = "Control which computer?",
        emptyHint = "Connect a paired computer that runs Link All 2.1 or later.",
        actions = listOf(
            ChooserAction(Icons.Outlined.SettingsRemote, { d -> "Control ${d?.name ?: "computer"}" }) { id ->
                connected.firstOrNull { it.id == id }?.let(onPick)
            },
        ),
        onDismiss = onDismiss,
    )
}

/** Music, volume and slide keys for one computer. The computer decides whether to allow it. */
@Composable
internal fun RemoteSheet(c: DdColors, peer: PeerSnapshot, canChange: Boolean, onChange: () -> Unit, onDismiss: () -> Unit) {
    val context = LocalContext.current
    LaunchedEffect(peer.id) { DeviceChooser.remember(context, ChoicePurpose.Remote, peer.id) }
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

    // Rings or stops the computer. Waits for its answer, so it runs off the main thread.
    fun ring(on: Boolean) {
        Thread {
            val problem = LinkAllService.withLiveEngine { handle -> LinkAllJni.sendFindDevice(handle, peer.id, on) }
                ?: "Link All isn't running."
            val message = when {
                problem.isNotEmpty() -> problem
                on -> "${peer.name} is ringing"
                else -> ""
            }
            if (message.isNotEmpty()) {
                android.os.Handler(android.os.Looper.getMainLooper()).post {
                    Toast.makeText(context, message, Toast.LENGTH_LONG).show()
                }
            }
        }.start()
    }
    var showPad by remember { mutableStateOf(false) }
    if (showPad) TrackpadDialog(c, peer, onDismiss = { showPad = false })

    DdSheet(
        c, icon = Icons.Outlined.SettingsRemote, title = "Remote · ${peer.name}",
        subtitle = "Turn on Remote control in Link All on that computer first",
        onDismiss = onDismiss,
    ) { close ->
        Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
            if (canChange) {
                PillButton(c, "Change computer", filled = false, compact = true, icon = Icons.Outlined.SwapHoriz) { close { onChange() } }
            }
            PillButton(c, "Trackpad and keyboard", filled = true, icon = Icons.Outlined.Mouse, modifier = Modifier.fillMaxWidth()) { showPad = true }
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
            Group(c, "Computer") {
                RemoteKey(c, Icons.Outlined.Lock, "Lock") { press("lock_screen") }
                RemoteKey(c, Icons.Outlined.DarkMode, "Screen off") { press("sleep_display") }
            }
            Group(c, "Find this computer") {
                RemoteKey(c, Icons.Outlined.NotificationsActive, "Ring") { ring(true) }
                RemoteKey(c, Icons.Outlined.NotificationsOff, "Stop") { ring(false) }
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
