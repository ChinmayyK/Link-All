package app.linkall.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.widget.Toast
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Code
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material.icons.outlined.ContentPaste
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Edit
import androidx.compose.material.icons.outlined.FormatBold
import androidx.compose.material.icons.outlined.FormatListBulleted
import androidx.compose.material.icons.outlined.Link
import androidx.compose.material.icons.outlined.MoreVert
import androidx.compose.material.icons.outlined.PushPin
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Send
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material.icons.outlined.StickyNote2
import androidx.compose.material.icons.outlined.Title
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.rememberSwipeToDismissBoxState
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
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


/** What a note mostly is, so it can be filtered and drawn to suit. */
internal enum class NoteKind { Link, Code, Text }

private val COMMAND_START = Regex("^\\s*(\\$ |sudo |ssh |git |npm |npx |yarn |pnpm |docker |kubectl |curl |wget |cd |ls |cat |grep |brew |apt |pip |python |cargo |make |adb |chmod |scp |rsync )")

/** A note is a link when it is little more than a web address, code when it is fenced or a command. */
internal fun kindOf(text: String): NoteKind {
    val trimmed = text.trim()
    val link = firstLink(trimmed)
    if (link != null && trimmed.length <= link.length + 80) return NoteKind.Link
    if (trimmed.contains("```")) return NoteKind.Code
    val lines = trimmed.lines().filter { it.isNotBlank() }
    if (lines.isNotEmpty() && lines.all { COMMAND_START.containsMatchIn(it) }) return NoteKind.Code
    return NoteKind.Text
}

/** "example.com" for "https://www.example.com/a/b?c", or null. */
internal fun domainOf(link: String): String? =
    runCatching { Uri.parse(link).host?.removePrefix("www.") }.getOrNull()?.takeIf { it.isNotBlank() }

private enum class Filter(val label: String) { All("All"), Pinned("Pinned"), Links("Links"), Code("Code") }

private fun matches(note: ScratchNote, filter: Filter): Boolean = when (filter) {
    Filter.All -> true
    Filter.Pinned -> note.pinned
    Filter.Links -> kindOf(note.text) == NoteKind.Link
    Filter.Code -> kindOf(note.text) == NoteKind.Code
}


/**
 * Notes, links and snippets shared between your own devices: written here,
 * they appear on your computers, and the other way round. Kept on each device
 * and sent only to your paired ones.
 *
 * Add at the bottom, like a chat with yourself. Swipe a note right to pin it
 * and left to delete it (with Undo); tap it to open its actions. Links open on
 * a tap, code is shown as code, and the filters sort it all.
 */
@Composable
internal fun ScratchpadDialog(c: DdColors, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val haptic = LocalHapticFeedback.current
    var notes by remember { mutableStateOf(emptyList<ScratchNote>()) }
    var loaded by remember { mutableStateOf(false) }
    var draft by remember { mutableStateOf("") }
    var editing by remember { mutableStateOf<ScratchNote?>(null) }
    var confirmClear by remember { mutableStateOf(false) }
    var searching by remember { mutableStateOf(false) }
    var query by remember { mutableStateOf("") }
    var filter by remember { mutableStateOf(Filter.All) }
    var expanded by remember { mutableStateOf<String?>(null) }
    var menuOpen by remember { mutableStateOf(false) }
    // The note just deleted, so "Undo" can put it back.
    var undo by remember { mutableStateOf<ScratchNote?>(null) }
    var now by remember { mutableStateOf(System.currentTimeMillis()) }
    var clipboardText by remember { mutableStateOf<String?>(null) }

    suspend fun reload() {
        val json = withContext(Dispatchers.IO) {
            LinkAllService.withLiveEngine { LinkAllJni.scratchpadListJson(it) }
        }
        notes = parseScratchNotes(json)
        loaded = true
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

    fun add(text: String) {
        val clean = text.trim()
        if (clean.isEmpty()) return
        haptic.performHapticFeedback(HapticFeedbackType.TextHandleMove)
        act { LinkAllJni.scratchpadSave(it, null, clean, false) }
    }

    // Edits made on another device show up while this is open.
    LaunchedEffect(Unit) {
        while (true) {
            reload()
            now = System.currentTimeMillis()
            val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            clipboardText = runCatching { cm.primaryClip?.getItemAt(0)?.coerceToText(context)?.toString() }.getOrNull()
            delay(1500)
        }
    }
    LaunchedEffect(undo) {
        if (undo != null) {
            delay(6000)
            undo = null
        }
    }

    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false),
    ) {
        EdgeToEdgeDialogWindow(c)
        Column(
            Modifier
                .fillMaxSize()
                .background(c.page)
                .statusBarsPadding()
                .navigationBarsPadding()
                .imePadding(),
        ) {
            // ── Header ──────────────────────────────────────────────────────
            Row(
                Modifier.fillMaxWidth().padding(start = PageGutter, end = 8.dp, top = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text("Scratchpad", style = DdType.pageTitle, color = c.text)
                    Text(
                        if (notes.isEmpty()) "Shared with your paired devices only"
                        else "${notes.size} ${if (notes.size == 1) "note" else "notes"} · paired devices only",
                        style = DdType.small, color = c.textMuted,
                    )
                }
                HeaderIcon(c, Icons.Outlined.Search, "Search") {
                    searching = !searching
                    if (!searching) query = ""
                }
                Box {
                    HeaderIcon(c, Icons.Outlined.MoreVert, "More") { menuOpen = true }
                    DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                        DropdownMenuItem(
                            text = { Text("Clear all unpinned") },
                            enabled = notes.any { !it.pinned },
                            onClick = { menuOpen = false; confirmClear = true },
                        )
                    }
                }
                HeaderIcon(c, Icons.Outlined.Close, "Close", onClick = onDismiss)
            }

            AnimatedVisibility(visible = searching) {
                SearchField(c, query) { query = it }
            }

            // ── Filters ─────────────────────────────────────────────────────
            if (notes.size >= 3) {
                Row(
                    Modifier
                        .horizontalScroll(rememberScrollState())
                        .padding(horizontal = PageGutter, vertical = 10.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    for (f in Filter.values()) {
                        val count = notes.count { matches(it, f) }
                        if (f != Filter.All && count == 0) continue
                        ScratchFilterChip(c, if (f == Filter.All) f.label else "${f.label} $count", filter == f) { filter = f }
                    }
                }
            }

            // ── The notes ───────────────────────────────────────────────────
            val shown = filterNotes(notes.filter { matches(it, filter) }, query)
            Box(Modifier.weight(1f).fillMaxWidth()) {
                if (loaded && notes.isEmpty()) {
                    EmptyScratchpad(c)
                } else if (loaded && shown.isEmpty()) {
                    Text(
                        if (query.isNotBlank()) "No note matches \"${query.trim()}\"." else "No notes here.",
                        style = DdType.body, color = c.textMuted,
                        modifier = Modifier.padding(PageGutter),
                    )
                } else {
                    LazyColumn(
                        Modifier.fillMaxSize(),
                        contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = PageGutter, vertical = 6.dp),
                        verticalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        val pinned = shown.filter { it.pinned }
                        val rest = shown.filter { !it.pinned }
                        if (pinned.isNotEmpty() && filter != Filter.Pinned) {
                            item(key = "h-pinned") { SectionLabel(c, "Pinned") }
                        }
                        items(pinned, key = { it.id }) { note ->
                            SwipeableNote(c, note, now, expanded == note.id,
                                onToggle = { expanded = if (expanded == note.id) null else note.id },
                                onPin = { act { h -> LinkAllJni.scratchpadSave(h, note.id, note.text, !note.pinned) } },
                                onDelete = { undo = note; act { h -> LinkAllJni.scratchpadDelete(h, note.id) } },
                                onEdit = { editing = note },
                                onCopy = { copyNote(context, note.text) },
                                onShare = { shareNote(context, note.text) },
                                onOpenLink = { openLink(context, it) },
                            )
                        }
                        if (pinned.isNotEmpty() && rest.isNotEmpty() && filter != Filter.Pinned) {
                            item(key = "h-rest") { SectionLabel(c, "Notes") }
                        }
                        items(rest, key = { it.id }) { note ->
                            SwipeableNote(c, note, now, expanded == note.id,
                                onToggle = { expanded = if (expanded == note.id) null else note.id },
                                onPin = { act { h -> LinkAllJni.scratchpadSave(h, note.id, note.text, !note.pinned) } },
                                onDelete = { undo = note; act { h -> LinkAllJni.scratchpadDelete(h, note.id) } },
                                onEdit = { editing = note },
                                onCopy = { copyNote(context, note.text) },
                                onShare = { shareNote(context, note.text) },
                                onOpenLink = { openLink(context, it) },
                            )
                        }
                    }
                }

                // Undo, floating over the bottom of the list.
                undo?.let { gone ->
                    Row(
                        Modifier
                            .align(Alignment.BottomCenter)
                            .padding(horizontal = PageGutter, vertical = 8.dp)
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(16.dp))
                            .background(c.text)
                            .padding(horizontal = 16.dp, vertical = 10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text("Note deleted", style = DdType.body, color = c.page, modifier = Modifier.weight(1f))
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

            // ── Composer ────────────────────────────────────────────────────
            val clip = clipboardText?.trim().orEmpty()
            val clipIsNew = clip.isNotEmpty() && clip.length <= 4000 && notes.none { it.text.trim() == clip }
            if (clipIsNew && draft.isBlank()) {
                Row(
                    Modifier
                        .padding(horizontal = PageGutter)
                        .padding(bottom = 6.dp)
                        .clip(CircleShape)
                        .background(c.accentSoft)
                        .clickable { add(clip); clipboardText = null }
                        .padding(horizontal = 12.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(Icons.Outlined.ContentPaste, contentDescription = null, tint = c.accent, modifier = Modifier.size(16.dp))
                    Spacer(Modifier.width(8.dp))
                    Text(
                        "Save what you copied: " + clip.lineSequence().first().take(36),
                        style = DdType.small, color = c.accent, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            Composer(c, draft, onChange = { draft = it }) {
                add(draft)
                draft = ""
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

private fun copyNote(context: Context, text: String) {
    val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    cm.setPrimaryClip(ClipData.newPlainText("Scratchpad note", text))
    Toast.makeText(context, "Copied", Toast.LENGTH_SHORT).show()
}

private fun shareNote(context: Context, text: String) {
    val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text)
    runCatching { context.startActivity(Intent.createChooser(send, null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
}

private fun openLink(context: Context, link: String) {
    runCatching {
        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(link)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }
}

/**
 * Makes the dialog's window fill the screen edge to edge, with no dimming and
 * transparent bars, so the page colour runs up behind the status bar and the
 * icons there stay readable; the content then keeps clear of the bars itself.
 */
@Composable
private fun EdgeToEdgeDialogWindow(c: DdColors) {
    val view = androidx.compose.ui.platform.LocalView.current
    androidx.compose.runtime.SideEffect {
        val window = (view.parent as? androidx.compose.ui.window.DialogWindowProvider)?.window ?: return@SideEffect
        window.setLayout(android.view.ViewGroup.LayoutParams.MATCH_PARENT, android.view.ViewGroup.LayoutParams.MATCH_PARENT)
        window.setDimAmount(0f)
        window.addFlags(android.view.WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN)
        androidx.core.view.WindowCompat.setDecorFitsSystemWindows(window, false)
        @Suppress("DEPRECATION")
        window.statusBarColor = android.graphics.Color.TRANSPARENT
        @Suppress("DEPRECATION")
        window.navigationBarColor = android.graphics.Color.TRANSPARENT
        val light = c.page.luminance() > 0.5f
        androidx.core.view.WindowCompat.getInsetsController(window, view).apply {
            isAppearanceLightStatusBars = light
            isAppearanceLightNavigationBars = light
        }
    }
}

@Composable
private fun HeaderIcon(c: DdColors, icon: ImageVector, label: String, onClick: () -> Unit) {
    Box(
        Modifier.size(44.dp).clip(CircleShape).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, contentDescription = label, tint = c.text, modifier = Modifier.size(22.dp))
    }
}

@Composable
private fun SectionLabel(c: DdColors, text: String) {
    Text(text.uppercase(), style = DdType.small.copy(fontWeight = FontWeight.SemiBold, letterSpacing = 0.8.sp), color = c.textMuted, modifier = Modifier.padding(top = 6.dp, bottom = 2.dp))
}

@Composable
private fun ScratchFilterChip(c: DdColors, label: String, selected: Boolean, onClick: () -> Unit) {
    Box(
        Modifier
            .clip(CircleShape)
            .background(if (selected) c.accent else c.surface)
            .border(1.dp, if (selected) c.accent else c.line, CircleShape)
            .clickable(onClick = onClick)
            .padding(horizontal = 14.dp, vertical = 8.dp),
    ) {
        Text(label, style = DdType.label, color = if (selected) c.onAccent else c.text)
    }
}

@Composable
private fun SearchField(c: DdColors, value: String, onChange: (String) -> Unit) {
    Row(
        Modifier
            .padding(horizontal = PageGutter, vertical = 8.dp)
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(c.surface)
            .border(1.dp, c.line, RoundedCornerShape(14.dp))
            .padding(horizontal = 14.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Icons.Outlined.Search, contentDescription = null, tint = c.textMuted, modifier = Modifier.size(18.dp))
        Spacer(Modifier.width(10.dp))
        Box(Modifier.weight(1f)) {
            if (value.isEmpty()) Text("Search notes", style = DdType.body, color = c.textMuted)
            BasicTextField(
                value = value, onValueChange = onChange, singleLine = true,
                textStyle = DdType.body.copy(color = c.text), cursorBrush = SolidColor(c.accent),
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

@Composable
private fun EmptyScratchpad(c: DdColors) {
    Column(
        Modifier.fillMaxSize().padding(horizontal = 36.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Box(Modifier.size(72.dp).clip(CircleShape).background(c.accentSoft), contentAlignment = Alignment.Center) {
            Icon(Icons.Outlined.StickyNote2, contentDescription = null, tint = c.accent, modifier = Modifier.size(34.dp))
        }
        Spacer(Modifier.height(18.dp))
        Text("Nothing here yet", style = DdType.title, color = c.text)
        Spacer(Modifier.height(6.dp))
        Text(
            "Type below, or share text from any app and pick Scratchpad. It appears on your other devices in a moment.",
            style = DdType.body, color = c.textMuted, textAlign = androidx.compose.ui.text.style.TextAlign.Center,
        )
    }
}

/** The box at the bottom that adds a note. */
@Composable
private fun Composer(c: DdColors, value: String, onChange: (String) -> Unit, onSend: () -> Unit) {
    val ready = value.isNotBlank()
    // Some phones report the keyboard a little shorter than it is; leave room so the
    // box is never behind it.
    val keyboardOpen = WindowInsets.ime.getBottom(androidx.compose.ui.platform.LocalDensity.current) > 0
    Row(
        Modifier
            .padding(start = PageGutter, end = PageGutter, top = 8.dp, bottom = if (keyboardOpen) 34.dp else 20.dp)
            .fillMaxWidth()
            .clip(RoundedCornerShape(26.dp))
            .background(c.surface)
            .border(1.dp, c.line, RoundedCornerShape(26.dp))
            .padding(start = 18.dp, end = 6.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.Bottom,
    ) {
        // As tall as the send button, with the text centred in it, so the hint and what
        // you type sit level with the button; a longer note grows the box upwards.
        Box(
            Modifier.weight(1f).heightIn(min = 44.dp).padding(end = 8.dp),
            contentAlignment = Alignment.CenterStart,
        ) {
            if (value.isEmpty()) Text("A link, a command, a thought…", style = DdType.body, color = c.textMuted)
            BasicTextField(
                value = value, onValueChange = onChange, maxLines = 6,
                textStyle = DdType.body.copy(color = c.text), cursorBrush = SolidColor(c.accent),
                modifier = Modifier.fillMaxWidth(),
            )
        }
        Box(
            Modifier
                .size(44.dp)
                .clip(CircleShape)
                .background(if (ready) c.accent else c.surfaceSunk)
                .clickable(enabled = ready, onClick = onSend),
            contentAlignment = Alignment.Center,
        ) {
            Icon(Icons.Outlined.Send, contentDescription = "Add note", tint = if (ready) c.onAccent else c.textMuted, modifier = Modifier.size(20.dp))
        }
    }
}

@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
private fun SwipeableNote(
    c: DdColors,
    note: ScratchNote,
    now: Long,
    expanded: Boolean,
    onToggle: () -> Unit,
    onPin: () -> Unit,
    onDelete: () -> Unit,
    onEdit: () -> Unit,
    onCopy: () -> Unit,
    onShare: () -> Unit,
    onOpenLink: (String) -> Unit,
) {
    val haptic = LocalHapticFeedback.current
    val state = rememberSwipeToDismissBoxState(
        confirmValueChange = { value ->
            when (value) {
                SwipeToDismissBoxValue.StartToEnd -> { haptic.performHapticFeedback(HapticFeedbackType.LongPress); onPin(); false }
                SwipeToDismissBoxValue.EndToStart -> { haptic.performHapticFeedback(HapticFeedbackType.LongPress); onDelete(); true }
                else -> false
            }
        },
    )
    SwipeToDismissBox(
        state = state,
        backgroundContent = {
            val toPin = state.dismissDirection == SwipeToDismissBoxValue.StartToEnd
            Box(
                Modifier
                    .fillMaxSize()
                    .clip(RoundedCornerShape(18.dp))
                    .background(if (toPin) c.accent else c.danger)
                    .padding(horizontal = 22.dp),
                contentAlignment = if (toPin) Alignment.CenterStart else Alignment.CenterEnd,
            ) {
                Icon(
                    if (toPin) Icons.Outlined.PushPin else Icons.Outlined.Delete,
                    contentDescription = null, tint = if (toPin) c.onAccent else Color.White, modifier = Modifier.size(24.dp),
                )
            }
        },
    ) {
        NoteCard(c, note, now, expanded, onToggle, onPin, onDelete, onEdit, onCopy, onShare, onOpenLink)
    }
}

@Composable
private fun NoteCard(
    c: DdColors,
    note: ScratchNote,
    now: Long,
    expanded: Boolean,
    onToggle: () -> Unit,
    onPin: () -> Unit,
    onDelete: () -> Unit,
    onEdit: () -> Unit,
    onCopy: () -> Unit,
    onShare: () -> Unit,
    onOpenLink: (String) -> Unit,
) {
    val shape = RoundedCornerShape(18.dp)
    val kind = remember(note.text) { kindOf(note.text) }
    val link = remember(note.text) { firstLink(note.text) }
    val long = note.text.lines().size > 5 || note.text.length > 280
    Column(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(c.surface)
            .border(1.dp, if (note.pinned) c.accent.copy(alpha = 0.55f) else c.line, shape)
            .clickable(onClick = onToggle)
            .padding(14.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(verticalAlignment = Alignment.Top) {
            Box(
                Modifier.size(34.dp).clip(RoundedCornerShape(10.dp)).background(c.accentSoft),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    when (kind) { NoteKind.Link -> Icons.Outlined.Link; NoteKind.Code -> Icons.Outlined.Code; else -> Icons.Outlined.StickyNote2 },
                    contentDescription = null, tint = c.accent, modifier = Modifier.size(18.dp),
                )
            }
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                when (kind) {
                    NoteKind.Link -> {
                        Text(domainOf(link.orEmpty()) ?: "Link", style = DdType.label.copy(fontWeight = FontWeight.SemiBold), color = c.text, maxLines = 1)
                        Text(link.orEmpty(), style = DdType.small, color = c.accent, maxLines = if (expanded) 4 else 1, overflow = TextOverflow.Ellipsis)
                        val extra = note.text.replace(link.orEmpty(), "").trim()
                        if (extra.isNotEmpty()) Text(extra, style = DdType.small, color = c.textMuted, maxLines = if (expanded) 6 else 1, overflow = TextOverflow.Ellipsis)
                    }
                    NoteKind.Code -> {
                        Text(
                            note.text.trim().removePrefix("```").removeSuffix("```").trim(),
                            style = DdType.mono.copy(fontSize = 13.sp), color = c.text,
                            maxLines = if (expanded) Int.MAX_VALUE else 4, overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).background(c.surfaceSunk).padding(10.dp),
                        )
                    }
                    NoteKind.Text -> Text(
                        markdown(note.text, c), style = DdType.body, color = c.text,
                        maxLines = if (expanded) Int.MAX_VALUE else 5, overflow = TextOverflow.Ellipsis,
                    )
                }
                if (!expanded && long && kind == NoteKind.Text) {
                    Text("Tap to read more", style = DdType.small, color = c.accent, modifier = Modifier.padding(top = 4.dp))
                }
            }
            Spacer(Modifier.width(8.dp))
            Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Icon(
                    Icons.Outlined.ContentCopy, contentDescription = "Copy", tint = c.textMuted,
                    modifier = Modifier.size(34.dp).clip(CircleShape).clickable(onClick = onCopy).padding(7.dp),
                )
                if (note.pinned) Icon(Icons.Outlined.PushPin, contentDescription = "Pinned", tint = c.accent, modifier = Modifier.size(16.dp))
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(ago(note.updatedAtMs, now), style = DdType.small, color = c.textMuted, modifier = Modifier.weight(1f))
        }
        if (expanded) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.horizontalScroll(rememberScrollState())) {
                if (link != null) ActionPill(c, Icons.Outlined.Link, "Open") { onOpenLink(link) }
                ActionPill(c, Icons.Outlined.Edit, "Edit") { onEdit() }
                ActionPill(c, Icons.Outlined.PushPin, if (note.pinned) "Unpin" else "Pin") { onPin() }
                ActionPill(c, Icons.Outlined.Share, "Share") { onShare() }
                ActionPill(c, Icons.Outlined.Delete, "Delete", danger = true) { onDelete() }
            }
        }
    }
}

@Composable
private fun ActionPill(c: DdColors, icon: ImageVector, label: String, danger: Boolean = false, onClick: () -> Unit) {
    val tint = if (danger) c.danger else c.text
    Row(
        Modifier
            .clip(CircleShape)
            .background(c.surfaceSunk)
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size(16.dp))
        Spacer(Modifier.width(6.dp))
        Text(label, style = DdType.small.copy(fontWeight = FontWeight.Medium), color = tint)
    }
}

/** A full-screen editor with buttons that add the Markdown for you. */
@Composable
private fun EditNoteDialog(c: DdColors, note: ScratchNote, onSave: (String) -> Unit, onDismiss: () -> Unit) {
    var value by remember(note.id) { mutableStateOf(TextFieldValue(note.text, TextRange(note.text.length))) }
    fun wrap(mark: String) {
        val sel = value.selection
        val text = value.text
        val chosen = text.substring(sel.min, sel.max)
        val updated = text.substring(0, sel.min) + mark + chosen + mark + text.substring(sel.max)
        value = TextFieldValue(updated, if (chosen.isEmpty()) TextRange(sel.min + mark.length) else TextRange(sel.min + mark.length, sel.max + mark.length))
    }
    fun prefixLines(prefix: String) {
        val sel = value.selection
        val text = value.text
        val start = text.lastIndexOf('\n', (sel.min - 1).coerceAtLeast(0)).let { if (it < 0 || sel.min == 0) 0 else it + 1 }
        val end = text.indexOf('\n', sel.max).let { if (it < 0) text.length else it }
        val block = text.substring(start, end).lines().joinToString("\n") { prefix + it }
        val updated = text.substring(0, start) + block + text.substring(end)
        value = TextFieldValue(updated, TextRange(start + block.length))
    }
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false),
    ) {
        EdgeToEdgeDialogWindow(c)
        Column(
            Modifier
                .fillMaxSize()
                .background(c.page)
                .statusBarsPadding()
                .navigationBarsPadding()
                .imePadding(),
        ) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                HeaderIcon(c, Icons.Outlined.Close, "Cancel", onClick = onDismiss)
                Text("Edit note", style = DdType.title, color = c.text, modifier = Modifier.weight(1f).padding(start = 6.dp))
                PillButton(c, "Save", filled = true, compact = true) { if (value.text.isNotBlank()) onSave(value.text.trim()) }
                Spacer(Modifier.width(8.dp))
            }
            Box(
                Modifier
                    .weight(1f)
                    .padding(horizontal = PageGutter)
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(18.dp))
                    .background(c.surface)
                    .border(1.dp, c.line, RoundedCornerShape(18.dp))
                    .padding(16.dp),
            ) {
                BasicTextField(
                    value = value, onValueChange = { value = it },
                    textStyle = DdType.body.copy(color = c.text, fontSize = 16.sp), cursorBrush = SolidColor(c.accent),
                    modifier = Modifier.fillMaxSize(),
                )
            }
            Row(
                Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = PageGutter, vertical = 10.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                ActionPill(c, Icons.Outlined.Title, "Heading") { prefixLines("# ") }
                ActionPill(c, Icons.Outlined.FormatBold, "Bold") { wrap("**") }
                ActionPill(c, Icons.Outlined.Code, "Code") { wrap("`") }
                ActionPill(c, Icons.Outlined.FormatListBulleted, "List") { prefixLines("- ") }
            }
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
