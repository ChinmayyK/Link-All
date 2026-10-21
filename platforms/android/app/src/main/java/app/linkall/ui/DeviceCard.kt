package app.linkall.ui

import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.DeleteOutline
import androidx.compose.material.icons.outlined.DriveFolderUpload
import androidx.compose.material.icons.outlined.FolderOpen
import androidx.compose.material.icons.outlined.SettingsRemote
import androidx.compose.material.icons.outlined.Speed
import androidx.compose.material.icons.outlined.UploadFile
import androidx.compose.material.icons.rounded.MoreHoriz
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
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.linkall.PeerSnapshot
import app.linkall.SpeedTestProgress
import app.linkall.supportsRemoteControl

/**
 * One connected device and what can be done with it, right on the card. Every
 * action here is already aimed at this device, so none of them asks "which
 * device?". Remote is offered only for computers; one too old for it shows the
 * button muted and says why when tapped.
 */
@Composable
internal fun DeviceCard(
    c: DdColors,
    peer: PeerSnapshot,
    speedTest: SpeedTestProgress?,
    onSendFiles: () -> Unit,
    onSendFolder: () -> Unit,
    onBrowse: () -> Unit,
    onRemote: () -> Unit,
    onSpeedTest: () -> Unit,
    onForget: () -> Unit,
) {
    val context = LocalContext.current
    var sendOpen by remember { mutableStateOf(false) }
    var moreOpen by remember { mutableStateOf(false) }
    val (status, statusColor) = deviceStatus(c, peer, speedTest)
    val isComputer = peer.platform != "Android"
    val remoteOk = supportsRemoteControl(peer.appVersion)

    Panel(c) {
        Column(Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                // The name and state read as one thing; the buttons below stay separate.
                Row(
                    Modifier
                        .weight(1f)
                        .semantics(mergeDescendants = true) { contentDescription = "${peer.name}, $status" },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    IconWell(c, osIcon(peer.platform, peer.name), tint = c.accent, background = c.accentSoft, size = 44)
                    Spacer(Modifier.width(14.dp))
                    Column(Modifier.weight(1f)) {
                        Text(peer.name, style = DdType.label, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box(Modifier.size(6.dp).background(c.live, CircleShape))
                            Spacer(Modifier.width(6.dp))
                            Text(
                                listOfNotNull(status, peer.platform).joinToString(" · "),
                                style = DdType.small, color = statusColor, maxLines = 1, overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                }
                Box(
                    Modifier
                        .size(44.dp)
                        .clip(CircleShape)
                        .clickable(onClickLabel = "More for ${peer.name}") { moreOpen = true },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(Icons.Rounded.MoreHoriz, contentDescription = "More for ${peer.name}", tint = c.textMuted, modifier = Modifier.size(22.dp))
                }
            }
            Spacer(Modifier.height(14.dp))
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                CardAction(c, Icons.Outlined.UploadFile, "Send", primary = true) { sendOpen = true }
                CardAction(c, Icons.Outlined.FolderOpen, "Browse", onClick = onBrowse)
                if (isComputer) {
                    CardAction(c, Icons.Outlined.SettingsRemote, "Remote", muted = !remoteOk) {
                        if (remoteOk) onRemote()
                        else Toast.makeText(context, "Update Link All on ${peer.name} to use Remote.", Toast.LENGTH_LONG).show()
                    }
                }
            }
        }
    }

    if (sendOpen) {
        ActionSheet(
            c, icon = osIcon(peer.platform, peer.name), title = "Send to ${peer.name}", subtitle = status,
            actions = listOf(
                SheetAction(Icons.Outlined.UploadFile, "Files", "Photos, videos, documents, anything", onClick = onSendFiles),
                SheetAction(Icons.Outlined.DriveFolderUpload, "A folder", "Everything in it, subfolders included", onClick = onSendFolder),
            ),
            onDismiss = { sendOpen = false },
        )
    }
    if (moreOpen) {
        ActionSheet(
            c, icon = osIcon(peer.platform, peer.name), title = peer.name, subtitle = status,
            actions = listOf(
                SheetAction(Icons.Outlined.Speed, "Test speed", "How fast this link is right now", onClick = onSpeedTest),
                SheetAction(Icons.Outlined.DeleteOutline, "Forget device", danger = true, onClick = onForget),
            ),
            onDismiss = { moreOpen = false },
        )
    }
}

/** An action on a device card: icon over its full label, so nothing is clipped at three across. */
@Composable
private fun RowScope.CardAction(
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
            .then(if (primary) Modifier else Modifier.border(1.dp, c.line, shape))
            .clickable(role = Role.Button, onClick = onClick)
            .padding(vertical = 11.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(icon, contentDescription = null, tint = if (primary || muted) fg else c.accent, modifier = Modifier.size(22.dp))
        Spacer(Modifier.height(4.dp))
        Text(label, style = DdType.small.copy(fontWeight = FontWeight.Medium), color = fg, maxLines = 1)
    }
}
