package com.deskdrop.ui

import android.content.Context
import android.graphics.Bitmap
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import android.text.format.Formatter
import android.util.Size
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.MutableTransitionState
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.deskdrop.PeerSnapshot
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

// The "Send with Deskdrop" sheet shown when files are shared into the app
// from the gallery or any other app's share menu. A real bottom sheet: a dim
// scrim over the sending app, the sheet pinned to the bottom edge, and the
// files themselves visible so it's obvious what is about to leave the phone.

private val SheetShape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp)
private val ThumbShape = RoundedCornerShape(14.dp)
private const val AnimMs = 220

private data class SharedItem(
    val uri: Uri,
    val name: String?,
    val sizeBytes: Long?,
    val mime: String?,
    val thumbnail: Bitmap?,
)

private fun loadSharedItem(context: Context, uri: Uri): SharedItem {
    val resolver = context.contentResolver
    var name: String? = null
    var size: Long? = null
    runCatching {
        resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { cursor ->
            if (cursor.moveToFirst()) {
                val n = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                val s = cursor.getColumnIndex(OpenableColumns.SIZE)
                if (n >= 0 && !cursor.isNull(n)) name = cursor.getString(n)
                if (s >= 0 && !cursor.isNull(s)) size = cursor.getLong(s)
            }
        }
    }
    val mime = runCatching { resolver.getType(uri) }.getOrNull()
    val previewable = mime?.startsWith("image/") == true || mime?.startsWith("video/") == true
    val thumb = if (previewable && Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        runCatching { resolver.loadThumbnail(uri, Size(240, 240), null) }.getOrNull()
    } else null
    return SharedItem(uri, name ?: uri.lastPathSegment, size, mime, thumb)
}

private fun iconFor(mime: String?): ImageVector = when {
    mime == null -> Icons.Outlined.InsertDriveFile
    mime.startsWith("image/") -> Icons.Outlined.Image
    mime.startsWith("video/") -> Icons.Outlined.Movie
    mime.startsWith("audio/") -> Icons.Outlined.MusicNote
    mime == "application/pdf" || mime.startsWith("text/") -> Icons.Outlined.Description
    mime.contains("zip") || mime.contains("compressed") -> Icons.Outlined.FolderZip
    else -> Icons.Outlined.InsertDriveFile
}

/** "2 photos", "1 video", "3 files" - named by kind when they all match. */
private fun describe(items: List<SharedItem>, count: Int): String {
    val kinds = items.map { it.mime?.substringBefore('/') }.distinct()
    val noun = when (kinds.singleOrNull()) {
        "image" -> if (count == 1) "photo" else "photos"
        "video" -> if (count == 1) "video" else "videos"
        else -> if (count == 1) "file" else "files"
    }
    return "$count $noun"
}

@Composable
fun ShareSheet(
    sharedUris: List<Uri>,
    peers: List<PeerSnapshot>,
    lastUsedDeviceId: String?,
    isDark: Boolean,
    onCancel: () -> Unit,
    onSend: (targetDeviceId: String?) -> Unit,
    onOpenApp: () -> Unit,
) {
    val c = rememberDdColors(isDark)
    val context = LocalContext.current

    // null target = every connected device. Preselect when there's an
    // obvious choice: the only device, or the one used last time.
    var selected by remember {
        mutableStateOf(
            when {
                peers.size == 1 -> peers.first().id
                else -> peers.firstOrNull { it.id == lastUsedDeviceId }?.id
            }
        )
    }
    val items by produceState<List<SharedItem>>(initialValue = emptyList(), sharedUris) {
        value = withContext(Dispatchers.IO) { sharedUris.map { loadSharedItem(context, it) } }
    }

    // Animate out before finishing so dismissal doesn't just blink away.
    val shown = remember { MutableTransitionState(false).apply { targetState = true } }
    var pendingAction by remember { mutableStateOf<(() -> Unit)?>(null) }
    fun dismissThen(action: () -> Unit) {
        if (pendingAction != null) return
        pendingAction = action
        shown.targetState = false
    }
    LaunchedEffect(pendingAction) {
        val action = pendingAction ?: return@LaunchedEffect
        delay(AnimMs.toLong())
        action()
    }
    BackHandler { dismissThen(onCancel) }

    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.BottomCenter) {
        AnimatedVisibility(
            visibleState = shown,
            enter = fadeIn(tween(AnimMs)),
            exit = fadeOut(tween(AnimMs)),
        ) {
            Box(
                Modifier
                    .fillMaxSize()
                    .background(Color.Black.copy(alpha = 0.45f))
                    .clickable(
                        interactionSource = remember { MutableInteractionSource() },
                        indication = null,
                    ) { dismissThen(onCancel) }
            )
        }

        AnimatedVisibility(
            visibleState = shown,
            enter = slideInVertically(tween(AnimMs)) { it },
            exit = slideOutVertically(tween(AnimMs)) { it },
        ) {
            Column(
                Modifier
                    .fillMaxWidth()
                    .widthIn(max = 560.dp)
                    .clip(SheetShape)
                    .background(c.surface)
                    // Swallow taps so they don't fall through to the scrim.
                    .clickable(
                        interactionSource = remember { MutableInteractionSource() },
                        indication = null,
                    ) {}
                    .navigationBarsPadding()
                    .verticalScroll(rememberScrollState())
                    .padding(bottom = 16.dp)
            ) {
                Box(
                    Modifier
                        .padding(top = 10.dp, bottom = 14.dp)
                        .size(width = 36.dp, height = 4.dp)
                        .clip(CircleShape)
                        .background(c.line)
                        .align(Alignment.CenterHorizontally)
                )

                // Header: what is being sent.
                Column(Modifier.padding(horizontal = PageGutter)) {
                    Text("Send with Link All", style = DdType.title, color = c.text)
                    val total = items.mapNotNull { it.sizeBytes }.takeIf { it.size == items.size && it.isNotEmpty() }?.sum()
                    val summary = buildString {
                        append(describe(items, sharedUris.size))
                        if (total != null) append(" · ").append(Formatter.formatShortFileSize(context, total))
                    }
                    Text(summary, style = DdType.small, color = c.textMuted)
                }

                Spacer(Modifier.height(14.dp))
                SharedItemsStrip(c, items, sharedUris.size)

                Spacer(Modifier.height(22.dp))
                Text(
                    "Send to",
                    style = DdType.small,
                    color = c.textMuted,
                    modifier = Modifier.padding(horizontal = PageGutter, vertical = 6.dp)
                )

                if (peers.isEmpty()) {
                    NoDevices(c, onOpenApp = { dismissThen(onOpenApp) })
                } else {
                    Panel(c, Modifier.padding(horizontal = PageGutter)) {
                        peers.forEachIndexed { index, peer ->
                            if (index > 0) Hairline(c)
                            DeviceOption(
                                c,
                                icon = osIcon(peer.platform, peer.name),
                                title = peer.name,
                                detail = "Connected",
                                selected = selected == peer.id,
                                onClick = { selected = peer.id },
                            )
                        }
                        // Broadcasting only means something with a choice to make.
                        if (peers.size > 1) {
                            Hairline(c)
                            DeviceOption(
                                c,
                                icon = Icons.Outlined.Devices,
                                title = "All devices",
                                detail = "Send to all ${peers.size} connected",
                                selected = selected == null,
                                onClick = { selected = null },
                            )
                        }
                    }
                }

                Spacer(Modifier.height(20.dp))
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = PageGutter),
                    horizontalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    PillButton(c, "Cancel", filled = false, modifier = Modifier.weight(1f)) {
                        dismissThen(onCancel)
                    }
                    if (peers.isNotEmpty()) {
                        val target = peers.firstOrNull { it.id == selected }?.name
                        PillButton(
                            c,
                            text = if (target != null) "Send" else "Send to all",
                            filled = true,
                            icon = Icons.Outlined.Send,
                            modifier = Modifier.weight(1f),
                        ) {
                            val chosen = selected
                            dismissThen { onSend(chosen) }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun SharedItemsStrip(c: DdColors, items: List<SharedItem>, expected: Int) {
    val single = expected == 1
    val tile = if (single) 96.dp else 84.dp
    LazyRow(
        contentPadding = PaddingValues(horizontal = PageGutter),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (items.isEmpty()) {
            // Metadata is still loading: hold the space with placeholders
            // so the sheet doesn't jump when thumbnails arrive.
            items(minOf(expected, 6)) {
                Box(Modifier.size(tile).clip(ThumbShape).background(c.surfaceSunk))
            }
        } else {
            items(items, key = { it.uri.toString() }) { item ->
                if (single) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Thumb(c, item, tile)
                        Spacer(Modifier.width(14.dp))
                        Column(Modifier.widthIn(max = 220.dp)) {
                            Text(item.name ?: "File", style = DdType.label, color = c.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            item.sizeBytes?.let {
                                Text(Formatter.formatShortFileSize(LocalContext.current, it), style = DdType.small, color = c.textMuted)
                            }
                        }
                    }
                } else {
                    Thumb(c, item, tile)
                }
            }
        }
    }
}

@Composable
private fun Thumb(c: DdColors, item: SharedItem, size: androidx.compose.ui.unit.Dp) {
    Box(
        Modifier
            .size(size)
            .clip(ThumbShape)
            .background(c.surfaceSunk)
            .border(1.dp, c.line, ThumbShape),
        contentAlignment = Alignment.Center
    ) {
        val bmp = item.thumbnail
        if (bmp != null) {
            Image(
                bitmap = remember(bmp) { bmp.asImageBitmap() },
                contentDescription = item.name,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize()
            )
            if (item.mime?.startsWith("video/") == true) {
                Icon(
                    Icons.Outlined.PlayCircle,
                    contentDescription = null,
                    tint = Color.White,
                    modifier = Modifier.size(26.dp)
                )
            }
        } else {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Icon(iconFor(item.mime), contentDescription = null, tint = c.textMuted, modifier = Modifier.size(26.dp))
                val ext = item.name?.substringAfterLast('.', "")?.takeIf { it.isNotEmpty() && it.length <= 5 }
                if (ext != null) {
                    Text(ext.uppercase(), style = DdType.small, color = c.textMuted, maxLines = 1)
                }
            }
        }
    }
}

@Composable
private fun DeviceOption(
    c: DdColors,
    icon: ImageVector,
    title: String,
    detail: String,
    selected: Boolean,
    onClick: () -> Unit,
) {
    ListRow(
        c,
        icon = icon,
        title = title,
        detail = detail,
        detailColor = if (selected) c.accent else c.textMuted,
        iconTint = if (selected) c.accent else c.textMuted,
        iconBackground = if (selected) c.accentSoft else c.surfaceSunk,
        onClick = onClick,
        trailing = {
            Box(
                Modifier
                    .size(22.dp)
                    .clip(CircleShape)
                    .background(if (selected) c.accent else Color.Transparent)
                    .border(if (selected) 0.dp else 1.5.dp, c.line, CircleShape)
                    .semantics { this.selected = selected },
                contentAlignment = Alignment.Center
            ) {
                if (selected) Icon(Icons.Rounded.Check, contentDescription = null, tint = c.onAccent, modifier = Modifier.size(14.dp))
            }
        }
    )
}

@Composable
private fun NoDevices(c: DdColors, onOpenApp: () -> Unit) {
    Panel(c, Modifier.padding(horizontal = PageGutter)) {
        Column(
            Modifier.fillMaxWidth().padding(20.dp),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            IconWell(c, Icons.Outlined.LinkOff, size = 44)
            Spacer(Modifier.height(10.dp))
            Text("No device connected", style = DdType.label, color = c.text)
            Text(
                "Open Link All on your computer, then try again.",
                style = DdType.small,
                color = c.textMuted,
                textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            )
            Spacer(Modifier.height(14.dp))
            PillButton(c, "Open Link All", filled = false, compact = true, onClick = onOpenApp)
        }
    }
}
