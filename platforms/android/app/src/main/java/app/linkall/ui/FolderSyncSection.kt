package app.linkall.ui

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.text.format.DateUtils
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CreateNewFolder
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.Switch
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.layout.Column
import androidx.core.content.ContextCompat
import app.linkall.FolderSync
import app.linkall.LinkAllService
import app.linkall.PeerSnapshot
import app.linkall.SharedFolders
import app.linkall.peerSnapshots

/**
 * Folders whose new files go to a paired computer by themselves. Adding one is
 * three steps: pick the folder, pick the computer, say whether what is already
 * in the folder goes too.
 */
@Composable
internal fun FolderSyncSection(c: DdColors) {
    val context = LocalContext.current
    var jobs by remember { mutableStateOf(FolderSync.list(context)) }
    var pickedFolder by remember { mutableStateOf<Uri?>(null) }
    var pickedComputer by remember { mutableStateOf<Pair<Uri, PeerSnapshot>?>(null) }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
        if (uri != null) pickedFolder = uri
    }

    Column {
        SectionHeader(c, "Folder sync", null)
        Panel(c) {
            jobs.forEach { job ->
                ListRow(
                    c, Icons.Outlined.Folder, SharedFolders.label(job.treeUri),
                    detail = when {
                        job.paused -> "Paused · to ${job.targetName}"
                        job.lastSyncMs == 0L -> "To ${job.targetName} · nothing sent yet"
                        else -> "To ${job.targetName} · ${DateUtils.getRelativeTimeSpanString(job.lastSyncMs)}"
                    },
                    trailing = {
                        Row {
                            Switch(
                                checked = !job.paused,
                                onCheckedChange = {
                                    FolderSync.setPaused(context, job.id, !it)
                                    jobs = FolderSync.list(context)
                                },
                            )
                            Icon(
                                Icons.Rounded.Close,
                                contentDescription = "Stop syncing ${job.label}",
                                tint = c.textMuted,
                                modifier = Modifier.size(20.dp).clickable {
                                    FolderSync.remove(context, job.id)
                                    jobs = FolderSync.list(context)
                                },
                            )
                        }
                    },
                )
                Hairline(c)
            }
            if (jobs.isNotEmpty()) {
                ListRow(c, Icons.Outlined.Sync, "Sync now", "Send new files to computers that are connected", onClick = { syncNow(context) })
                Hairline(c)
            }
            ListRow(
                c, Icons.Outlined.CreateNewFolder, "Add a folder to sync",
                if (jobs.isEmpty()) "New photos and files go to your computer by themselves when it is connected"
                else "New files in these folders go to the computer you chose",
                onClick = { pick.launch(null) },
            )
        }
    }

    pickedFolder?.let { folder ->
        val computers = remember(folder) {
            context.getSharedPreferences(LinkAllService.PREFS_NAME, Context.MODE_PRIVATE).peerSnapshots().filter { it.trusted }
        }
        DdSheet(
            c, icon = Icons.Outlined.Folder, title = "Send to which computer?",
            subtitle = if (computers.isEmpty()) "Pair a computer first" else SharedFolders.label(folder),
            onDismiss = { pickedFolder = null },
        ) { close ->
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                computers.forEach { peer ->
                    SheetOption(c, osIcon(peer.platform, peer.name), peer.name, null) {
                        close { pickedComputer = folder to peer }
                    }
                }
            }
        }
    }

    pickedComputer?.let { (folder, peer) ->
        fun add(includeExisting: Boolean) {
            FolderSync.add(context, folder, SharedFolders.label(folder), peer.id, peer.name, includeExisting)
            jobs = FolderSync.list(context)
            syncNow(context)
        }
        DdSheet(
            c, icon = Icons.Outlined.Sync, title = "What goes to ${peer.name}?",
            subtitle = SharedFolders.label(folder),
            onDismiss = { pickedComputer = null; pickedFolder = null },
        ) { close ->
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                SheetOption(c, Icons.Outlined.Sync, "Only new files from now", "What is in the folder already stays on the phone") {
                    close { add(false) }
                }
                SheetOption(c, Icons.Outlined.Folder, "Everything in the folder", "Sends the files that are there now too") {
                    close { add(true) }
                }
            }
        }
    }
}

private fun syncNow(context: Context) {
    ContextCompat.startForegroundService(
        context,
        Intent(context, LinkAllService::class.java).apply { action = LinkAllService.ACTION_FOLDER_SYNC_NOW },
    )
}
