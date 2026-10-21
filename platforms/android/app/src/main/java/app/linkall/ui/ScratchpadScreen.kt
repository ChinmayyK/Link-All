package app.linkall.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.PushPin
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import app.linkall.LinkAllJni
import app.linkall.LinkAllService
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray

/** One Scratchpad note as the engine reports it. */
internal data class ScratchNote(val id: String, val text: String, val pinned: Boolean, val updatedAtMs: Long = 0L)

internal fun parseScratchNotes(json: String?): List<ScratchNote> {
    if (json.isNullOrBlank()) return emptyList()
    val array = runCatching { JSONArray(json) }.getOrNull() ?: return emptyList()
    return (0 until array.length()).mapNotNull { i ->
        val o = array.optJSONObject(i) ?: return@mapNotNull null
        ScratchNote(o.optString("id"), o.optString("text"), o.optBoolean("pinned"), o.optLong("updated_at_ms"))
    }
}

private val LINK = Regex("https?://[^\\s<>\"')]+")

/** The first web link in [text], or null. */
internal fun firstLink(text: String): String? = LINK.find(text)?.value?.trimEnd('.', ',', ';', ':', '!', '?')

/** "just now", "5 min ago", "3 h ago", "2 days ago". */
internal fun ago(thenMs: Long, nowMs: Long = System.currentTimeMillis()): String {
    if (thenMs <= 0L) return ""
    val secs = ((nowMs - thenMs) / 1000).coerceAtLeast(0)
    return when {
        secs < 60 -> "just now"
        secs < 3600 -> "${secs / 60} min ago"
        secs < 86_400 -> "${secs / 3600} h ago"
        else -> "${secs / 86_400} ${if (secs / 86_400 == 1L) "day" else "days"} ago"
    }
}

/** Notes that match what was typed in the search box, every word anywhere in the note. */
internal fun filterNotes(notes: List<ScratchNote>, query: String): List<ScratchNote> {
    val words = query.trim().lowercase().split(Regex("\\s+")).filter { it.isNotEmpty() }
    if (words.isEmpty()) return notes
    return notes.filter { n -> words.all { n.text.lowercase().contains(it) } }
}

/**
 * Notes, links and snippets shared between your own devices: written here,
 * they appear on your computers, and the other way round. Kept on each
 * device and sent only to your paired ones.
 */
@Composable
internal fun ScratchpadDialog(c: DdColors, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var notes by remember { mutableStateOf(emptyList<ScratchNote>()) }
    var draft by remember { mutableStateOf("") }
    var editing by remember { mutableStateOf<ScratchNote?>(null) }
    var confirmClear by remember { mutableStateOf(false) }
    var query by remember { mutableStateOf("") }
    // The note just deleted, so "Undo" can put it back.
    var undo by remember { mutableStateOf<ScratchNote?>(null) }
    var now by remember { mutableStateOf(System.currentTimeMillis()) }

    suspend fun reload() {
        val json = withContext(Dispatchers.IO) {
            LinkAllService.withLiveEngine { LinkAllJni.scratchpadListJson(it) }
        }
        notes = parseScratchNotes(json)
    }

    fun act(call: (Long) -> Int) {
        scope.launch {
            val result = withContext(Dispatchers.IO) { LinkAllService.withLiveEngine(call) }
            if (result != 0) {
                Toast.makeText(context, "Couldn't save the note. It may be too long.", Toast.LENGTH_SHORT).show()
            }
            reload()
        }
    }

    // Edits made on another device show up while this is open.
    LaunchedEffect(Unit) {
        while (true) {
            reload()
            now = System.currentTimeMillis()
            delay(1500)
        }
    }
    LaunchedEffect(undo) {
        if (undo != null) {
            delay(6000)
            undo = null
        }
    }

    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Column(
            Modifier
                .fillMaxSize()
                .background(c.surface)
                .padding(horizontal = PageGutter, vertical = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Scratchpad", style = DdType.title, color = c.text)
                    Text(
                        if (notes.isEmpty()) "Shared with your paired devices, nowhere else"
                        else "${notes.size} ${if (notes.size == 1) "note" else "notes"} · shared with your paired devices only",
                        style = DdType.small, color = c.textMuted,
                    )
                }
                if (notes.any { !it.pinned }) {
                    Text(
                        "Clear all", style = DdType.label, color = c.accent,
                        modifier = Modifier.clickable { confirmClear = true }.padding(8.dp),
                    )
                }
                Icon(
                    Icons.Outlined.Close, contentDescription = "Close", tint = c.text,
                    modifier = Modifier.size(28.dp).clickable(onClick = onDismiss),
                )
            }

            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextField(
                    value = draft,
                    onValueChange = { draft = it },
                    modifier = Modifier.weight(1f),
                    placeholder = { Text("A link, a command, a thought…") },
                    maxLines = 4,
                )
                PillButton(c, "Add", filled = true, compact = true) {
                    val text = draft.trim()
                    if (text.isNotEmpty()) {
                        draft = ""
                        act { LinkAllJni.scratchpadSave(it, null, text, false) }
                    }
                }
            }

            if (notes.size >= 4) {
                TextField(
                    value = query,
                    onValueChange = { query = it },
                    modifier = Modifier.fillMaxWidth(),
                    placeholder = { Text("Search notes") },
                    singleLine = true,
                    leadingIcon = { Icon(Icons.Outlined.Search, contentDescription = null) },
                )
            }

            val shown = filterNotes(notes, query)
            if (notes.isNotEmpty() && shown.isEmpty()) {
                Text("No note matches \"${query.trim()}\".", style = DdType.body, color = c.textMuted, modifier = Modifier.padding(top = 8.dp))
            }
            if (notes.isEmpty()) {
                Text(
                    "Nothing here yet. Add something and it appears on your other devices.",
                    style = DdType.body, color = c.textMuted, modifier = Modifier.padding(top = 8.dp),
                )
            }
            LazyColumn(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                items(shown, key = { it.id }) { note ->
                    NoteCard(
                        c, note, now,
                        onEdit = { editing = note },
                        onCopy = {
                            val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                            cm.setPrimaryClip(ClipData.newPlainText("Scratchpad note", note.text))
                            Toast.makeText(context, "Copied", Toast.LENGTH_SHORT).show()
                        },
                        onPin = { act { h -> LinkAllJni.scratchpadSave(h, note.id, note.text, !note.pinned) } },
                        onDelete = {
                            undo = note
                            act { h -> LinkAllJni.scratchpadDelete(h, note.id) }
                        },
                        onOpenLink = { link ->
                            runCatching {
                                context.startActivity(
                                    android.content.Intent(android.content.Intent.ACTION_VIEW, android.net.Uri.parse(link))
                                        .addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
                                )
                            }
                        },
                    )
                }
            }

            undo?.let { gone ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(14.dp))
                        .background(c.surfaceSunk)
                        .border(1.dp, c.line, RoundedCornerShape(14.dp))
                        .padding(horizontal = 14.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Note deleted", style = DdType.body, color = c.text, modifier = Modifier.weight(1f))
                    Text(
                        "Undo", style = DdType.label, color = c.accent,
                        modifier = Modifier.clickable {
                            undo = null
                            act { h -> LinkAllJni.scratchpadSave(h, null, gone.text, gone.pinned) }
                        }.padding(8.dp),
                    )
                }
            }
        }
    }

    editing?.let { note ->
        EditNoteDialog(
            c, note,
            onSave = { text ->
                editing = null
                act { LinkAllJni.scratchpadSave(it, note.id, text, note.pinned) }
            },
            onDismiss = { editing = null },
        )
    }

    if (confirmClear) {
        Dialog(onDismissRequest = { confirmClear = false }) {
            Column(
                Modifier
                    .clip(PanelShape)
                    .background(c.surface)
                    .border(1.dp, c.line, PanelShape)
                    .padding(20.dp),
                verticalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                Text("Clear all notes?", style = DdType.title, color = c.text)
                Text(
                    "This deletes every note that isn't pinned, here and on your paired devices.",
                    style = DdType.body, color = c.textMuted,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    PillButton(c, "Cancel", filled = false, modifier = Modifier.weight(1f)) { confirmClear = false }
                    PillButton(c, "Clear all", filled = true, modifier = Modifier.weight(1f)) {
                        confirmClear = false
                        act { LinkAllJni.scratchpadClear(it) }
                    }
                }
            }
        }
    }
}

@Composable
private fun NoteCard(
    c: DdColors,
    note: ScratchNote,
    now: Long,
    onEdit: () -> Unit,
    onCopy: () -> Unit,
    onPin: () -> Unit,
    onDelete: () -> Unit,
    onOpenLink: (String) -> Unit,
) {
    val shape = RoundedCornerShape(16.dp)
    val long = note.text.lines().size > 6 || note.text.length > 360
    var expanded by remember(note.id) { mutableStateOf(false) }
    val link = remember(note.text) { firstLink(note.text) }
    Column(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(c.surfaceSunk)
            .border(1.dp, if (note.pinned) c.accent else c.line, shape)
            .clickable(onClick = onEdit)
            .padding(14.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(
            markdown(note.text, c), style = DdType.body, color = c.text,
            maxLines = if (expanded) Int.MAX_VALUE else 6,
            overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
        )
        if (long) {
            Text(
                if (expanded) "Show less" else "Show more", style = DdType.small, color = c.accent,
                modifier = Modifier.clickable { expanded = !expanded },
            )
        }
        if (link != null) {
            Text(
                "Open link", style = DdType.label, color = c.accent,
                modifier = Modifier.clickable { onOpenLink(link) },
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(18.dp), verticalAlignment = Alignment.CenterVertically) {
            NoteAction(Icons.Outlined.ContentCopy, "Copy", c.textMuted, onCopy)
            NoteAction(Icons.Outlined.PushPin, if (note.pinned) "Unpin" else "Pin", if (note.pinned) c.accent else c.textMuted, onPin)
            NoteAction(Icons.Outlined.Delete, "Delete", c.textMuted, onDelete)
            Spacer(Modifier.weight(1f))
            Text(ago(note.updatedAtMs, now), style = DdType.small, color = c.textMuted)
        }
    }
}

@Composable
private fun NoteAction(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, tint: androidx.compose.ui.graphics.Color, onClick: () -> Unit) {
    Icon(icon, contentDescription = label, tint = tint, modifier = Modifier.size(22.dp).clickable(onClick = onClick))
}

@Composable
private fun EditNoteDialog(c: DdColors, note: ScratchNote, onSave: (String) -> Unit, onDismiss: () -> Unit) {
    var text by remember(note.id) { mutableStateOf(note.text) }
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Column(
            Modifier
                .fillMaxSize()
                .background(c.surface)
                .padding(horizontal = PageGutter, vertical = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("Edit note", style = DdType.title, color = c.text)
            Text("Markdown works: # heading, **bold**, `code`, - list", style = DdType.small, color = c.textMuted)
            TextField(
                value = text,
                onValueChange = { text = it },
                modifier = Modifier.fillMaxWidth().weight(1f),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                PillButton(c, "Cancel", filled = false, modifier = Modifier.weight(1f), onClick = onDismiss)
                PillButton(c, "Save", filled = true, modifier = Modifier.weight(1f)) { onSave(text.trim()) }
            }
            Spacer(Modifier.height(4.dp))
        }
    }
}

/**
 * A small subset of Markdown, enough for notes: headings, bullets, **bold**,
 * `code` and fenced blocks. Anything else shows as written.
 */
internal fun markdown(text: String, c: DdColors): AnnotatedString = buildAnnotatedString {
    val code = SpanStyle(fontFamily = FontFamily.Monospace, background = c.line.copy(alpha = 0.5f))
    var inFence = false
    text.lines().forEachIndexed { i, rawLine ->
        if (i > 0) append("\n")
        if (rawLine.trim().startsWith("```")) {
            inFence = !inFence
            return@forEachIndexed
        }
        if (inFence) {
            withStyle(code) { append(rawLine) }
            return@forEachIndexed
        }
        var line = rawLine
        var heading = false
        when {
            line.startsWith("### ") -> { line = line.drop(4); heading = true }
            line.startsWith("## ") -> { line = line.drop(3); heading = true }
            line.startsWith("# ") -> { line = line.drop(2); heading = true }
            line.startsWith("- ") || line.startsWith("* ") -> line = "• " + line.drop(2)
        }
        val base = if (heading) SpanStyle(fontWeight = FontWeight.Bold, fontSize = 18.sp) else SpanStyle()
        withStyle(base) { appendInline(line, code) }
    }
}

private fun AnnotatedString.Builder.appendInline(line: String, code: SpanStyle) {
    var i = 0
    while (i < line.length) {
        when {
            line.startsWith("**", i) -> {
                val end = line.indexOf("**", i + 2)
                if (end > i + 2) {
                    withStyle(SpanStyle(fontWeight = FontWeight.Bold)) { append(line.substring(i + 2, end)) }
                    i = end + 2
                } else { append(line[i]); i++ }
            }
            line[i] == '`' -> {
                val end = line.indexOf('`', i + 1)
                if (end > i + 1) {
                    withStyle(code) { append(line.substring(i + 1, end)) }
                    i = end + 1
                } else { append(line[i]); i++ }
            }
            else -> { append(line[i]); i++ }
        }
    }
}
