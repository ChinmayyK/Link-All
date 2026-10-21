package app.linkall.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import app.linkall.ActivityEntry
import app.linkall.ActivityKind
import java.util.Calendar

private enum class ActivityFilter(val label: String, val kinds: Set<ActivityKind>?) {
    All("All", null),
    Clipboard("Clipboard", setOf(ActivityKind.CLIPBOARD_TEXT, ActivityKind.CLIPBOARD_IMAGE)),
    Files("Files", setOf(
        ActivityKind.FILE_SENT, ActivityKind.FILE_RECEIVED, ActivityKind.FILE_TRANSFER_COMPLETE,
        ActivityKind.FILE_TRANSFER_FAILED, ActivityKind.FILE_TRANSFER_INCOMING
    )),
    Devices("Devices", setOf(ActivityKind.PEER_CONNECTED, ActivityKind.PEER_DISCONNECTED, ActivityKind.WARNING)),
}

/** Everything that crossed between devices, grouped by day. */
@Composable
fun ActivityTab(
    isDark: Boolean,
    feed: List<ActivityEntry>,
    onApply: (ActivityEntry) -> Unit,
    onResend: (ActivityEntry) -> Unit,
    onDelete: (ActivityEntry) -> Unit,
    onTogglePin: (ActivityEntry) -> Unit = {},
    onClearAll: () -> Unit = {}
) {
    val c = rememberDdColors(isDark)
    var filter by rememberSaveable { mutableStateOf(ActivityFilter.All) }
    var searching by rememberSaveable { mutableStateOf(false) }
    var query by rememberSaveable { mutableStateOf("") }
    val groups = remember(feed, filter, query) {
        val q = query.trim()
        // Pinned entries get their own group on top; the rest by day.
        feed.filter { e -> filter.kinds?.contains(e.kind) ?: true }
            .filter { e -> q.isEmpty() || e.preview.contains(q, ignoreCase = true) || e.deviceName.contains(q, ignoreCase = true) }
            .groupBy { if (it.isPinned) "Pinned" else dayLabel(it.timestamp) }
            .toList()
    }

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = PageGutter, end = PageGutter, bottom = 140.dp)
    ) {
        item {
            Column {
                PageTitle(c, "Activity", if (feed.isEmpty()) null else "${feed.size} items") {
                    if (feed.isNotEmpty()) {
                        TopIconButton(c, if (searching) Icons.Rounded.Close else Icons.Outlined.Search, if (searching) "Close search" else "Search activity") {
                            searching = !searching
                            if (!searching) query = ""
                        }
                        Spacer(Modifier.width(4.dp))
                        Text(
                            "Clear",
                            style = DdType.small.copy(fontWeight = FontWeight.Medium),
                            color = c.accent,
                            modifier = Modifier
                                .clip(CircleShape)
                                .clickable(onClickLabel = "Clear all activity", onClick = onClearAll)
                                .padding(horizontal = 12.dp, vertical = 8.dp)
                        )
                    }
                }
                if (searching) {
                    Spacer(Modifier.height(12.dp))
                    SearchField(c, query, "Search clips, files and devices") { query = it }
                }
                Spacer(Modifier.height(14.dp))
                Row(
                    modifier = Modifier.horizontalScroll(rememberScrollState()),
                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    ActivityFilter.values().forEach { f ->
                        FilterChip(c, f.label, selected = f == filter) { filter = f }
                    }
                }
            }
        }

        if (groups.isEmpty()) {
            item {
                Column {
                    Spacer(Modifier.height(24.dp))
                    EmptyBox(
                        c, if (query.isNotBlank()) Icons.Outlined.Search else Icons.Outlined.History,
                        when {
                            query.isNotBlank() -> "Nothing matches \u201c${query.trim()}\u201d."
                            filter == ActivityFilter.All -> "Nothing yet. Clipboard items, files and device changes show up here."
                            else -> "No ${filter.label.lowercase()} activity yet."
                        }
                    )
                }
            }
        } else {
            groups.forEach { (day, entries) ->
                item(key = "h_$day") {
                    Text(
                        day,
                        style = DdType.title,
                        color = c.text,
                        modifier = Modifier.padding(top = 28.dp, bottom = 10.dp)
                    )
                }
                // One lazy row per entry, drawn as one panel per day, so a busy
                // day scrolls without composing all of it at once.
                itemsIndexed(entries, key = { _, e -> e.id }) { i, entry ->
                    Column(Modifier.panelSlice(c, first = i == 0, last = i == entries.lastIndex)) {
                        if (i > 0) Hairline(c)
                        ActivityRow(
                            c = c,
                            entry = entry,
                            onApply = { onApply(entry) },
                            onResend = { onResend(entry) },
                            onDelete = { onDelete(entry) },
                            onTogglePin = { onTogglePin(entry) },
                            showClockTime = true
                        )
                    }
                }
            }
        }
    }
}

/**
 * One row's part of a [Panel]: the rounded top on the first row, the rounded
 * bottom on the last, and the outline only on the outer edges.
 */
private fun Modifier.panelSlice(c: DdColors, first: Boolean, last: Boolean): Modifier {
    val radius = 24.dp
    val shape = RoundedCornerShape(
        topStart = if (first) radius else 0.dp, topEnd = if (first) radius else 0.dp,
        bottomStart = if (last) radius else 0.dp, bottomEnd = if (last) radius else 0.dp,
    )
    return fillMaxWidth()
        .clip(shape)
        .background(c.surface)
        .drawWithContent {
            drawContent()
            // The panel's outline, stretched past this row where the panel goes on,
            // so the clip leaves only the sides (and the ends on the first and last rows).
            val stroke = 1.dp.toPx()
            val r = radius.toPx()
            val top = if (first) stroke / 2 else -r - stroke
            val bottom = if (last) size.height - stroke / 2 else size.height + r + stroke
            drawRoundRect(
                color = c.line,
                topLeft = Offset(stroke / 2, top),
                size = Size(size.width - stroke, bottom - top),
                cornerRadius = CornerRadius(r - stroke / 2),
                style = Stroke(stroke),
            )
        }
}

@Composable
internal fun FilterChip(c: DdColors, text: String, selected: Boolean, onClick: () -> Unit) {
    Text(
        text,
        style = DdType.small.copy(fontWeight = androidx.compose.ui.text.font.FontWeight.Medium),
        color = if (selected) c.onAccent else c.text,
        modifier = Modifier
            .clip(CircleShape)
            .background(if (selected) c.accent else c.surface)
            .border(1.dp, if (selected) c.accent else c.line, CircleShape)
            .clickable(onClick = onClick)
            .semantics { role = Role.Tab; this.selected = selected }
            .padding(horizontal = 16.dp, vertical = 9.dp)
    )
}

private fun dayLabel(timestampMs: Long): String {
    val day = Calendar.getInstance().apply { timeInMillis = timestampMs }
    val today = Calendar.getInstance()
    fun sameDay(a: Calendar, b: Calendar) =
        a.get(Calendar.YEAR) == b.get(Calendar.YEAR) && a.get(Calendar.DAY_OF_YEAR) == b.get(Calendar.DAY_OF_YEAR)
    val yesterday = (today.clone() as Calendar).apply { add(Calendar.DAY_OF_YEAR, -1) }
    return when {
        sameDay(day, today) -> "Today"
        sameDay(day, yesterday) -> "Yesterday"
        else -> java.text.SimpleDateFormat("EEE, d MMM", java.util.Locale.getDefault()).format(day.time)
    }
}
