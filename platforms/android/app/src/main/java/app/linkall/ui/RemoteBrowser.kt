package app.linkall.ui

import android.graphics.BitmapFactory
import android.text.format.Formatter
import android.util.LruCache
import android.widget.Toast
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material.icons.rounded.ArrowBack
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.linkall.LinkAllJni
import app.linkall.LinkAllService
import app.linkall.PeerSnapshot
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.text.DateFormat
import java.util.Date

/** Opens the file browser for a connected device; provided by MainScreen. */
internal val LocalBrowseFiles = staticCompositionLocalOf<(PeerSnapshot) -> Unit> { {} }

private const val PAGE = 60

private data class RemoteFile(
    val id: Long,
    val name: String,
    val size: Long,
    val mime: String,
    val modifiedSecs: Long,
    val category: String
) {
    val hasPreview get() = mime.startsWith("image/") || mime.startsWith("video/")
}

private enum class Category(val label: String, val key: String?, val countKey: String?) {
    All("All", null, null),
    Images("Photos", "Images", "images"),
    Videos("Videos", "Videos", "videos"),
    Audio("Audio", "Audio", "audio"),
    Documents("Documents", "Documents", "documents"),
    Archives("Archives", "Archives", "archives"),
    Apks("Apps", "Apks", "apks"),
}

private class Listing(val files: List<RemoteFile>, val total: Int, val counts: Map<String, Int>?)

/** One page of [peer]'s files, or the reason there isn't one. */
private suspend fun fetchPage(peerId: String, category: Category, query: String, offset: Int): Result<Listing> =
    withContext(Dispatchers.IO) {
        val raw = LinkAllService.withLiveEngine { h ->
            LinkAllJni.queryRemoteFiles(h, peerId, false, category.key, query.ifBlank { null }, offset, PAGE)
        } ?: return@withContext Result.failure(Exception("Link All isn't running on this phone."))
        runCatching {
            val o = JSONObject(raw)
            o.optString("error").takeIf { it.isNotBlank() && !o.isNull("error") }?.let { throw Exception(friendlyError(it)) }
            val arr = o.optJSONArray("files")
            val files = List(arr?.length() ?: 0) { i ->
                val f = arr!!.getJSONObject(i)
                RemoteFile(
                    id = f.optLong("file_id"),
                    name = f.optString("display_name"),
                    size = f.optLong("size_bytes"),
                    mime = f.optString("mime_type"),
                    modifiedSecs = f.optLong("date_modified"),
                    category = f.optString("category")
                )
            }
            val counts = o.optJSONObject("summary")?.optJSONObject("type_counts")?.let { t ->
                Category.values().mapNotNull { c -> c.countKey?.let { it to t.optInt(it) } }.toMap()
            }
            Listing(files, o.optInt("total_matching", files.size), counts)
        }
    }

private fun friendlyError(raw: String): String = when {
    raw.contains("timed out", ignoreCase = true) -> "The device didn't answer. Check it's awake and on the same network."
    raw.contains("not connected", ignoreCase = true) -> "This device isn't connected right now."
    else -> raw
}

/**
 * Previews, small and few: decoded down to the tile size and kept in a
 * bounded cache, fetched at most three at a time.
 */
private object Previews {
    private const val TARGET_PX = 192
    private val cache = object : LruCache<String, ImageBitmap>(6 * 1024 * 1024) {
        override fun sizeOf(key: String, value: ImageBitmap) = value.width * value.height * 4
    }
    @OptIn(ExperimentalCoroutinesApi::class)
    private val fetches = Dispatchers.IO.limitedParallelism(3)

    fun cached(peerId: String, fileId: Long): ImageBitmap? = cache.get("$peerId/$fileId")

    suspend fun load(peerId: String, fileId: Long): ImageBitmap? {
        cached(peerId, fileId)?.let { return it }
        return withContext(fetches) {
            val bytes = LinkAllService.withLiveEngine { h ->
                LinkAllJni.requestRemoteThumbnail(h, peerId, fileId, TARGET_PX * 2)
            } ?: return@withContext null
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
            var sample = 1
            while (bounds.outWidth / (sample * 2) >= TARGET_PX && bounds.outHeight / (sample * 2) >= TARGET_PX) sample *= 2
            val bmp = BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply { inSampleSize = sample })
                ?: return@withContext null
            bmp.asImageBitmap().also { cache.put("$peerId/$fileId", it) }
        }
    }

    fun clear() = cache.evictAll()
}

@Composable
internal fun RemoteBrowserScreen(isDark: Boolean, peer: PeerSnapshot, onClose: () -> Unit) {
    val c = rememberDdColors(isDark)
    val context = LocalContext.current
    var category by remember { mutableStateOf(Category.All) }
    var query by remember { mutableStateOf("") }
    var files by remember { mutableStateOf<List<RemoteFile>>(emptyList()) }
    var total by remember { mutableStateOf(0) }
    var counts by remember { mutableStateOf<Map<String, Int>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(true) }
    var loadingMore by remember { mutableStateOf(false) }
    var attempt by remember { mutableStateOf(0) }
    val requested = remember { mutableStateListOf<Long>() }

    BackHandler(onBack = onClose)
    DisposableEffect(Unit) { onDispose { Previews.clear() } }

    // A new category, search or retry starts the list over; typing waits for a pause.
    LaunchedEffect(category, query, attempt) {
        if (query.isNotEmpty()) delay(350)
        loading = true
        error = null
        fetchPage(peer.id, category, query, 0)
            // Ids key the lazy list, so a repeat would crash it.
            .onSuccess { files = it.files.distinctBy { f -> f.id }; total = it.total; if (it.counts != null) counts = it.counts }
            .onFailure { files = emptyList(); total = 0; error = it.message }
        loading = false
    }

    val loadMore: () -> Unit = {
        if (!loadingMore && !loading && files.size < total) loadingMore = true
    }
    LaunchedEffect(loadingMore) {
        if (!loadingMore) return@LaunchedEffect
        fetchPage(peer.id, category, query, files.size).onSuccess { page ->
            val seen = files.mapTo(HashSet()) { it.id }
            files = files + page.files.filter { it.id !in seen }
            total = page.total
        }
        loadingMore = false
    }

    val download: (RemoteFile) -> Unit = { file ->
        if (file.id !in requested) {
            requested += file.id
            Toast.makeText(context, "Getting ${file.name} from ${peer.name}", Toast.LENGTH_SHORT).show()
            Thread {
                val sent = LinkAllService.withLiveEngine { h -> LinkAllJni.pullRemoteFile(h, peer.id, file.id) } == 0
                if (!sent) android.os.Handler(android.os.Looper.getMainLooper()).post {
                    requested -= file.id
                    Toast.makeText(context, "Couldn't reach ${peer.name}", Toast.LENGTH_SHORT).show()
                }
            }.start()
        }
    }

    Column(Modifier.fillMaxSize().background(c.page)) {
        Column(Modifier.padding(horizontal = PageGutter)) {
            Row(Modifier.fillMaxWidth().padding(top = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(
                    Modifier
                        .size(44.dp)
                        .clip(CircleShape)
                        .background(c.surface)
                        .border(1.dp, c.line, CircleShape)
                        .clickable(onClick = onClose)
                        .semantics { contentDescription = "Back"; role = Role.Button },
                    contentAlignment = Alignment.Center
                ) {
                    Icon(Icons.Rounded.ArrowBack, contentDescription = null, tint = c.text, modifier = Modifier.size(22.dp))
                }
                Spacer(Modifier.width(14.dp))
                Column(Modifier.weight(1f)) {
                    Text(peer.name, style = DdType.title, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text("Tap a file to get it on this phone", style = DdType.small, color = c.textMuted, maxLines = 1)
                }
                Icon(osIcon(peer.platform, peer.name), contentDescription = null, tint = c.accent, modifier = Modifier.size(22.dp))
            }
            Spacer(Modifier.height(16.dp))
            SearchField(c, query) { query = it }
        }
        Spacer(Modifier.height(12.dp))
        Row(
            Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = PageGutter),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Category.values()
                .filter { it == Category.All || counts == null || (counts!![it.countKey] ?: 0) > 0 || it == category }
                .forEach { cat ->
                    CategoryChip(c, cat.label, counts?.get(cat.countKey), selected = cat == category) { category = cat }
                }
        }
        Spacer(Modifier.height(12.dp))

        Box(Modifier.fillMaxSize()) {
            when {
                loading -> CircularProgressIndicator(color = c.accent, strokeWidth = 2.dp, modifier = Modifier.align(Alignment.Center).size(28.dp))
                error != null -> Column(Modifier.padding(PageGutter)) {
                    EmptyBox(c, Icons.Outlined.FolderOff, error!!) {
                        Spacer(Modifier.height(12.dp))
                        PillButton(c, "Try again", filled = true, compact = true) { attempt++ }
                    }
                }
                files.isEmpty() -> Column(Modifier.padding(PageGutter)) {
                    EmptyBox(c, Icons.Outlined.SearchOff, if (query.isBlank()) "No files here" else "Nothing matches \"$query\"")
                }
                category == Category.Images -> LazyVerticalGrid(
                    columns = GridCells.Fixed(3),
                    contentPadding = PaddingValues(start = PageGutter, end = PageGutter, bottom = 32.dp),
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp)
                ) {
                    itemsIndexed(files, key = { _, f -> f.id }) { i, file ->
                        if (i >= files.size - 12) LaunchedEffect(files.size) { loadMore() }
                        PhotoTile(c, peer.id, file, file.id in requested) { download(file) }
                    }
                    if (loadingMore) item(span = { GridItemSpan(3) }) { MoreSpinner(c) }
                }
                else -> LazyColumn(
                    contentPadding = PaddingValues(start = PageGutter, end = PageGutter, bottom = 32.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    itemsIndexed(files, key = { _, f -> f.id }) { i, file ->
                        if (i >= files.size - 12) LaunchedEffect(files.size) { loadMore() }
                        FileRow(c, peer.id, file, file.id in requested) { download(file) }
                    }
                    if (loadingMore) item { MoreSpinner(c) }
                }
            }
        }
    }
}

@Composable
private fun SearchField(c: DdColors, value: String, onChange: (String) -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .height(48.dp)
            .clip(RoundedCornerShape(16.dp))
            .background(c.surface)
            .border(1.dp, c.line, RoundedCornerShape(16.dp))
            .padding(horizontal = 14.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Icon(Icons.Outlined.Search, contentDescription = null, tint = c.textMuted, modifier = Modifier.size(20.dp))
        Spacer(Modifier.width(10.dp))
        Box(Modifier.weight(1f)) {
            if (value.isEmpty()) Text("Search files", style = DdType.body, color = c.textMuted)
            BasicTextField(
                value = value,
                onValueChange = onChange,
                singleLine = true,
                textStyle = DdType.body.copy(color = c.text),
                cursorBrush = SolidColor(c.accent),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                modifier = Modifier.fillMaxWidth()
            )
        }
        if (value.isNotEmpty()) {
            Icon(
                Icons.Rounded.Close, contentDescription = "Clear search", tint = c.textMuted,
                modifier = Modifier.size(20.dp).clickable { onChange("") }
            )
        }
    }
}

@Composable
private fun CategoryChip(c: DdColors, label: String, count: Int?, selected: Boolean, onClick: () -> Unit) {
    val shape = RoundedCornerShape(999.dp)
    Row(
        Modifier
            .clip(shape)
            .background(if (selected) c.accent else c.surface)
            .then(if (selected) Modifier else Modifier.border(1.dp, c.line, shape))
            .clickable(onClick = onClick)
            .padding(horizontal = 14.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(label, style = DdType.small.copy(fontWeight = FontWeight.SemiBold), color = if (selected) c.onAccent else c.text)
        if (count != null) {
            Spacer(Modifier.width(6.dp))
            Text("$count", style = DdType.small, color = if (selected) c.onAccent.copy(alpha = 0.75f) else c.textMuted)
        }
    }
}

@Composable
private fun FileRow(c: DdColors, peerId: String, file: RemoteFile, requested: Boolean, onClick: () -> Unit) {
    val context = LocalContext.current
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(18.dp))
            .background(c.surface)
            .border(1.dp, c.line, RoundedCornerShape(18.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 14.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Box(Modifier.size(44.dp).clip(WellShape).background(c.surfaceSunk), contentAlignment = Alignment.Center) {
            Icon(iconFor(file), contentDescription = null, tint = c.textMuted, modifier = Modifier.size(20.dp))
            // Covers the icon once a preview arrives.
            if (file.hasPreview) Preview(peerId, file.id, Modifier.matchParentSize())
        }
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(file.name, style = DdType.label, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                listOfNotNull(
                    Formatter.formatShortFileSize(context, file.size),
                    file.modifiedSecs.takeIf { it > 0 }?.let { DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(it * 1000)) }
                ).joinToString(" · "),
                style = DdType.small, color = c.textMuted, maxLines = 1
            )
        }
        Spacer(Modifier.width(10.dp))
        Icon(
            if (requested) Icons.Rounded.Check else Icons.Outlined.FileDownload,
            contentDescription = if (requested) "Requested" else "Get file",
            tint = if (requested) c.live else c.accent,
            modifier = Modifier.size(22.dp)
        )
    }
}

@Composable
private fun PhotoTile(c: DdColors, peerId: String, file: RemoteFile, requested: Boolean, onClick: () -> Unit) {
    Box(
        Modifier
            .aspectRatio(1f)
            .clip(RoundedCornerShape(12.dp))
            .background(c.surfaceSunk)
            .clickable(onClick = onClick)
            .semantics { contentDescription = file.name },
        contentAlignment = Alignment.Center
    ) {
        Icon(Icons.Outlined.Image, contentDescription = null, tint = c.textMuted.copy(alpha = 0.5f), modifier = Modifier.size(22.dp))
        Preview(peerId, file.id, Modifier.matchParentSize())
        if (requested) {
            Box(
                Modifier.align(Alignment.TopEnd).padding(6.dp).size(22.dp).clip(CircleShape).background(c.live),
                contentAlignment = Alignment.Center
            ) {
                Icon(Icons.Rounded.Check, contentDescription = "Requested", tint = c.onAccent, modifier = Modifier.size(14.dp))
            }
        }
    }
}

@Composable
private fun Preview(peerId: String, fileId: Long, modifier: Modifier) {
    val image by produceState(Previews.cached(peerId, fileId), peerId, fileId) {
        if (value == null) value = Previews.load(peerId, fileId)
    }
    image?.let { Image(it, contentDescription = null, contentScale = ContentScale.Crop, modifier = modifier) }
}

@Composable
private fun MoreSpinner(c: DdColors) {
    Box(Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.Center) {
        CircularProgressIndicator(color = c.accent, strokeWidth = 2.dp, modifier = Modifier.size(22.dp))
    }
}

private fun iconFor(file: RemoteFile): ImageVector = when {
    file.mime.startsWith("image/") -> Icons.Outlined.Image
    file.mime.startsWith("video/") -> Icons.Outlined.Movie
    file.mime.startsWith("audio/") -> Icons.Outlined.MusicNote
    file.mime == "application/pdf" -> Icons.Outlined.PictureAsPdf
    file.category == "Archives" -> Icons.Outlined.FolderZip
    file.category == "Apks" -> Icons.Outlined.Android
    else -> Icons.Outlined.Description
}
