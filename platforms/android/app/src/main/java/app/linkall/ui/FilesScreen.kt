package app.linkall.ui

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.webkit.MimeTypeMap
import android.widget.Toast
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.DriveFolderUpload
import androidx.compose.material.icons.outlined.ErrorOutline
import androidx.compose.material.icons.outlined.FolderOpen
import androidx.compose.material.icons.outlined.NorthEast
import androidx.compose.material.icons.outlined.SouthWest
import androidx.compose.material.icons.outlined.SwapVert
import androidx.compose.material.icons.outlined.UploadFile
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import app.linkall.ActivityEntry
import app.linkall.ActivityKind
import app.linkall.ChoicePurpose
import app.linkall.PeerSnapshot
import app.linkall.TransferProgress
import java.io.File

/** File entries that are done: sent, received or failed. In-flight ones show under "In progress". */
private val FinishedFileKinds = setOf(
    ActivityKind.FILE_SENT, ActivityKind.FILE_RECEIVED,
    ActivityKind.FILE_TRANSFER_COMPLETE, ActivityKind.FILE_TRANSFER_FAILED,
)

/**
 * Files: send to a device, browse what is on one, follow what is moving, and
 * open what arrived. The full history stays in Activity.
 */
@Composable
fun FilesTab(
    isDark: Boolean,
    peers: kotlinx.collections.immutable.ImmutableList<PeerSnapshot>,
    feed: kotlinx.collections.immutable.ImmutableList<ActivityEntry>,
    activeTransfers: kotlinx.collections.immutable.ImmutableList<TransferProgress>,
    onActionSendFiles: (String?) -> Unit,
    onActionSendFolder: (String?) -> Unit,
    onActionPauseTransfer: (String) -> Unit,
    onActionResumeTransfer: (String) -> Unit,
    onActionCancelTransfer: (String) -> Unit,
    onSeeAll: () -> Unit,
) {
    val c = rememberDdColors(isDark)
    val context = LocalContext.current
    val browse = LocalBrowseFiles.current
    val openDevice = LocalOpenDevice.current
    // Which picker the device chooser leads to: files, or a folder.
    var choosingFor by remember { mutableStateOf<Boolean?>(null) }
    val linked = remember(peers) { peers.filter { it.trusted }.sortedByDescending { it.isConnected } }
    val recent = remember(feed) { feed.filter { it.kind in FinishedFileKinds }.take(12) }

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = PageGutter, end = PageGutter, bottom = 140.dp)
    ) {
        item { PageTitle(c, "Files") }

        item {
            Column {
                GroupLabel(c, "Send")
                TileGrid(
                    c,
                    listOf(
                        TileSpec(Icons.Outlined.UploadFile, "Send files", Tone.Blue) { choosingFor = false },
                        TileSpec(Icons.Outlined.DriveFolderUpload, "Send a folder", Tone.Blue) { choosingFor = true },
                    ),
                    columns = 2,
                )
            }
        }

        item {
            Column {
                GroupLabel(c, "Browse")
                if (linked.isEmpty()) {
                    EmptyBox(c, Icons.Outlined.FolderOpen, "Pair a computer to browse its files from here.")
                } else {
                    val (fg, bg) = c.tone(Tone.Green)
                    Panel(c) {
                        linked.forEachIndexed { i, peer ->
                            if (i > 0) Hairline(c)
                            ListRow(
                                c, osIcon(peer.platform, peer.name), peer.name,
                                detail = if (peer.isConnected) "Browse its files" else "Not connected",
                                iconTint = if (peer.isConnected) fg else c.textMuted,
                                iconBackground = if (peer.isConnected) bg else c.surfaceSunk,
                                // An offline device can't be browsed; its page says why and offers Connect.
                                onClick = { if (peer.isConnected) browse(peer) else openDevice(peer) },
                            )
                        }
                    }
                }
            }
        }

        if (activeTransfers.isNotEmpty()) {
            item {
                Column {
                    GroupLabel(c, "In progress")
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
            }
        }

        item {
            Column {
                Row(verticalAlignment = Alignment.Bottom) {
                    GroupLabel(c, "Recent", Modifier.weight(1f))
                    if (recent.isNotEmpty()) {
                        Text(
                            "See all",
                            style = DdType.small.copy(fontWeight = FontWeight.Medium),
                            color = c.accent,
                            modifier = Modifier
                                .padding(bottom = 4.dp)
                                .clip(CircleShape)
                                .clickable(onClickLabel = "See all file activity", onClick = onSeeAll)
                                .padding(horizontal = 10.dp, vertical = 6.dp)
                        )
                    }
                }
                if (recent.isEmpty()) {
                    EmptyBox(c, Icons.Outlined.SwapVert, "Files you send or receive show up here.")
                } else {
                    Panel(c) {
                        recent.forEachIndexed { i, entry ->
                            if (i > 0) Hairline(c)
                            FileRow(c, entry, onOpen = if (canOpen(entry)) ({ openReceivedFile(context, entry) }) else null)
                        }
                    }
                }
            }
        }
    }

    choosingFor?.let { folder ->
        val icon = if (folder) Icons.Outlined.DriveFolderUpload else Icons.Outlined.UploadFile
        DeviceChooserSheet(
            c, purpose = ChoicePurpose.Send, icon = icon,
            title = if (folder) "Send a folder" else "Send files",
            emptyHint = "No paired device is connected. Open Link All on your computer, on the same Wi-Fi.",
            actions = listOf(
                ChooserAction(icon, { d ->
                    val what = if (folder) "Send a folder" else "Send files"
                    if (d == null) "$what to all" else "$what to ${d.name}"
                }) { target ->
                    if (folder) onActionSendFolder(target) else onActionSendFiles(target)
                },
            ),
            onDismiss = { choosingFor = null },
        )
    }
}

/** One finished transfer: which way it went, with whom, and when. A received file opens on a tap. */
@Composable
private fun FileRow(c: DdColors, entry: ActivityEntry, onOpen: (() -> Unit)?) {
    val failed = entry.kind == ActivityKind.FILE_TRANSFER_FAILED
    val received = entry.kind == ActivityKind.FILE_RECEIVED || entry.kind == ActivityKind.FILE_TRANSFER_COMPLETE
    val (icon, tone) = when {
        failed -> Icons.Outlined.ErrorOutline to Tone.Rose
        received -> Icons.Outlined.SouthWest to Tone.Green
        else -> Icons.Outlined.NorthEast to Tone.Blue
    }
    val (fg, bg) = c.tone(tone)
    val detail = listOfNotNull(
        when {
            failed -> "Didn't finish · ${entry.deviceName}"
            received -> "From ${entry.deviceName}"
            else -> "Sent to ${entry.deviceName}"
        },
        entry.fileTotalBytes.takeIf { it > 0 }?.let { sizeLabel(it) },
    ).joinToString(" · ")
    ListRow(
        c, icon, entry.preview.ifBlank { "File" },
        detail = detail,
        detailColor = if (failed) c.danger else c.textMuted,
        iconTint = fg,
        iconBackground = bg,
        onClick = onOpen,
        trailing = {
            Spacer(Modifier.width(4.dp))
            Text(relativeTime(entry.timestamp), style = DdType.mono, color = c.textMuted)
        },
    )
}

internal fun canOpen(entry: ActivityEntry) =
    entry.destPath.isNotBlank() &&
        (entry.kind == ActivityKind.FILE_RECEIVED || entry.kind == ActivityKind.FILE_TRANSFER_COMPLETE)

/**
 * Opens a received file in whatever app handles it. It was saved either to
 * Downloads (a content URI) or to a path the file provider shares.
 */
internal fun openReceivedFile(context: Context, entry: ActivityEntry) {
    val path = entry.destPath
    val uri = if (path.startsWith("content://")) Uri.parse(path) else runCatching {
        FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", File(path))
    }.getOrNull()
    if (uri == null) {
        Toast.makeText(context, "Can't find this file any more", Toast.LENGTH_SHORT).show()
        return
    }
    val type = runCatching { context.contentResolver.getType(uri) }.getOrNull()
        ?: MimeTypeMap.getSingleton().getMimeTypeFromExtension(File(entry.preview).extension.lowercase())
        ?: "*/*"
    val view = Intent(Intent.ACTION_VIEW)
        .setDataAndType(uri, type)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    try {
        context.startActivity(view)
    } catch (e: ActivityNotFoundException) {
        Toast.makeText(context, "No app on this phone opens this kind of file", Toast.LENGTH_SHORT).show()
    } catch (e: SecurityException) {
        Toast.makeText(context, "Can't open this file any more", Toast.LENGTH_SHORT).show()
    }
}

private fun sizeLabel(bytes: Long): String = when {
    bytes >= 1L shl 30 -> String.format(java.util.Locale.US, "%.1f GB", bytes / (1024.0 * 1024 * 1024))
    bytes >= 1L shl 20 -> String.format(java.util.Locale.US, "%.1f MB", bytes / (1024.0 * 1024))
    bytes >= 1L shl 10 -> "${bytes / 1024} KB"
    else -> "$bytes B"
}
