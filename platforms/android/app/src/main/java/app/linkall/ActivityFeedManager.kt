package app.linkall

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.flow.updateAndGet

enum class ActivityKind {
    CLIPBOARD_TEXT, CLIPBOARD_IMAGE, FILE_SENT, FILE_RECEIVED,
    FILE_TRANSFER_INCOMING, FILE_TRANSFER_PROGRESS, FILE_TRANSFER_COMPLETE,
    FILE_TRANSFER_FAILED, FILE_TRANSFER_PAUSED, FILE_TRANSFER_RESUMED,
    PEER_CONNECTED, PEER_DISCONNECTED, WARNING;
}

@androidx.compose.runtime.Immutable
data class ActivityEntry(
    val id: Long = System.nanoTime(),
    val timestamp: Long = System.currentTimeMillis(),
    val deviceName: String,
    val kind: ActivityKind,
    val preview: String,
    /** For clipboard items: the full text (may be empty for images). */
    val contentHash: String = "",
    /** True if this clipboard item has been applied to local clipboard. */
    val appliedLocally: Boolean = false,
    /** For file transfers: the transfer ID hex. */
    val transferId: String = "",
    /** For file transfers: total bytes. */
    val fileTotalBytes: Long = 0L,
    /** Transfer progress 0-100. */
    val progressPercent: Int = 0,
    /** Bytes written so far for an in-flight transfer. */
    val transferBytesReceived: Long = 0L,
    /** Bytes per second, or 0 if the engine has not estimated speed yet. */
    val transferSpeedBps: Long = 0L,
    /** Seconds remaining, or -1 if unknown. */
    val transferEtaSecs: Long = -1L,
    /** Final destination path (file transfers). */
    val destPath: String = "",
    /** Kept at the top of the feed and never trimmed; survives restarts. */
    val isPinned: Boolean = false
) {
    fun formattedLine(): String = when (kind) {
        ActivityKind.CLIPBOARD_TEXT  -> "[$deviceName] copied: $preview"
        ActivityKind.CLIPBOARD_IMAGE -> "[$deviceName] copied image"
        ActivityKind.FILE_SENT       -> "[$deviceName] sent file: $preview"
        ActivityKind.FILE_RECEIVED   -> "[$deviceName] file ready: $preview"
        ActivityKind.FILE_TRANSFER_INCOMING -> "[$deviceName] sending: $preview"
        ActivityKind.FILE_TRANSFER_PROGRESS -> "[$deviceName] $progressPercent% — $preview"
        ActivityKind.FILE_TRANSFER_PAUSED   -> "[$deviceName] paused — $preview"
        ActivityKind.FILE_TRANSFER_RESUMED  -> "[$deviceName] resumed — $preview"
        ActivityKind.FILE_TRANSFER_COMPLETE -> "[$deviceName] ✓ $preview"
        ActivityKind.FILE_TRANSFER_FAILED   -> "[$deviceName] ✗ transfer failed: $preview"
        ActivityKind.PEER_CONNECTED  -> "[$deviceName] Connected"
        ActivityKind.PEER_DISCONNECTED -> "[$deviceName] Disconnected"
        ActivityKind.WARNING         -> "$preview"
    }
    /** True if the user can tap "Apply" to write this to local clipboard. */
    val isApplicable: Boolean get() = kind == ActivityKind.CLIPBOARD_TEXT && !appliedLocally
}

object ActivityFeedManager {
    var ACTIVITY_FEED_MAX = 100

    private val _feedFlow = MutableStateFlow<List<ActivityEntry>>(emptyList())
    val feedFlow: StateFlow<List<ActivityEntry>> = _feedFlow.asStateFlow()

    fun isUserFacingActivity(kind: ActivityKind): Boolean = when (kind) {
        ActivityKind.FILE_RECEIVED,
        ActivityKind.FILE_SENT,
        ActivityKind.FILE_TRANSFER_INCOMING,
        ActivityKind.FILE_TRANSFER_PROGRESS,
        ActivityKind.FILE_TRANSFER_COMPLETE,
        ActivityKind.FILE_TRANSFER_FAILED,
        ActivityKind.FILE_TRANSFER_PAUSED,
        ActivityKind.FILE_TRANSFER_RESUMED,
        ActivityKind.CLIPBOARD_TEXT,
        ActivityKind.CLIPBOARD_IMAGE -> true
        else -> false
    }

    // Pinned entries are written to preferences, so they outlive the process
    // the rest of the feed lives in.
    private const val PREFS = "linkall_pinned_feed"
    private const val KEY = "entries"
    private var prefs: android.content.SharedPreferences? = null

    /** Loads pinned entries. Safe to call from every component's onCreate. */
    fun attach(context: android.content.Context) {
        if (prefs != null) return
        val p = context.applicationContext.getSharedPreferences(PREFS, android.content.Context.MODE_PRIVATE)
        prefs = p
        val pinned = runCatching {
            val arr = org.json.JSONArray(p.getString(KEY, "[]"))
            (0 until arr.length()).map { i ->
                val o = arr.getJSONObject(i)
                ActivityEntry(
                    id = o.getLong("id"),
                    timestamp = o.getLong("timestamp"),
                    deviceName = o.getString("deviceName"),
                    kind = ActivityKind.valueOf(o.getString("kind")),
                    preview = o.getString("preview"),
                    contentHash = o.optString("contentHash"),
                    transferId = o.optString("transferId"),
                    fileTotalBytes = o.optLong("fileTotalBytes"),
                    destPath = o.optString("destPath"),
                    isPinned = true,
                )
            }
        }.getOrDefault(emptyList())
        _feedFlow.update { current -> arrange(pinned + current.filterNot { c -> pinned.any { it.id == c.id } }) }
    }

    /** Pinned first, then everything else; both newest first as added. */
    private fun arrange(list: List<ActivityEntry>): List<ActivityEntry> {
        val (pinned, rest) = list.partition { it.isPinned }
        return pinned + rest.take(ACTIVITY_FEED_MAX)
    }

    private fun savePinned(list: List<ActivityEntry>) {
        val p = prefs ?: return
        val arr = org.json.JSONArray()
        list.filter { it.isPinned }.forEach { e ->
            arr.put(org.json.JSONObject().apply {
                put("id", e.id)
                put("timestamp", e.timestamp)
                put("deviceName", e.deviceName)
                put("kind", e.kind.name)
                put("preview", e.preview)
                put("contentHash", e.contentHash)
                put("transferId", e.transferId)
                put("fileTotalBytes", e.fileTotalBytes)
                put("destPath", e.destPath)
            })
        }
        p.edit().putString(KEY, arr.toString()).apply()
    }

    fun addToFeed(entry: ActivityEntry) {
        if (!isUserFacingActivity(entry.kind)) return
        _feedFlow.update { current -> arrange(listOf(entry) + current) }
    }

    fun togglePin(id: Long) {
        val updated = _feedFlow.updateAndGet { current ->
            arrange(current.map { if (it.id == id) it.copy(isPinned = !it.isPinned) else it })
        }
        savePinned(updated)
    }

    fun removeFromFeed(id: Long) {
        val updated = _feedFlow.updateAndGet { current -> current.filterNot { it.id == id } }
        savePinned(updated)
    }

    /** Clears everything except pinned entries. */
    fun clearFeed() {
        _feedFlow.update { current -> current.filter { it.isPinned } }
    }

    fun updateFeedByTransferId(tid: String, transform: (ActivityEntry) -> ActivityEntry) {
        _feedFlow.update { current ->
            val idx = current.indexOfFirst { it.transferId == tid }
            if (idx != -1) {
                val mut = current.toMutableList()
                mut[idx] = transform(mut[idx])
                mut
            } else {
                current
            }
        }
    }

    fun getFeedSnapshot(): List<ActivityEntry> = _feedFlow.value
}
