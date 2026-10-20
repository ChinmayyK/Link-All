package app.linkall

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap

/**
 * Folders whose new files go to a chosen computer by themselves, one way.
 *
 * Each job is a folder picked with the system folder picker and one paired
 * computer. The phone keeps a ledger of what it has sent, because the
 * computer never overwrites: a file sent twice would arrive as "name (2).jpg".
 * A file enters the ledger only once the computer has received it, so a phone
 * that leaves Wi-Fi mid-batch sends again only what didn't arrive.
 */
object FolderSync {
    private const val TAG = "FolderSync"
    private const val PREFS = "folder_sync"
    private const val KEY = "jobs"

    /** A file still being written (a video) changes size; wait until it settles. */
    const val SETTLE_MS = 15_000L
    const val MAX_FILES = 10_000
    private val SKIPPED = setOf(".DS_Store", "Thumbs.db", "desktop.ini")

    data class Job(
        val id: String,
        val treeUri: Uri,
        val label: String,
        val targetDeviceId: String,
        val targetName: String,
        val paused: Boolean,
        val lastSyncMs: Long,
    )

    /** A file in a folder tree. [key] identifies this exact version of it. */
    data class TreeFile(val docId: String, val relPath: String, val size: Long, val modifiedMs: Long) {
        val key: String get() = "$docId|$size|$modifiedMs"
    }

    // ── Jobs ──────────────────────────────────────────────────────────────

    fun list(context: Context): List<Job> {
        val raw = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY, null) ?: return emptyList()
        return runCatching {
            val array = JSONArray(raw)
            (0 until array.length()).map { i ->
                val o = array.getJSONObject(i)
                Job(
                    id = o.getString("id"),
                    treeUri = Uri.parse(o.getString("tree")),
                    label = o.getString("label"),
                    targetDeviceId = o.getString("target"),
                    targetName = o.optString("target_name"),
                    paused = o.optBoolean("paused"),
                    lastSyncMs = o.optLong("last_sync"),
                )
            }
        }.getOrDefault(emptyList())
    }

    /**
     * Starts syncing [treeUri] to [targetDeviceId]. With [includeExisting]
     * false, what is in the folder now counts as already sent.
     */
    fun add(context: Context, treeUri: Uri, label: String, targetDeviceId: String, targetName: String, includeExisting: Boolean): Job? {
        runCatching {
            context.contentResolver.takePersistableUriPermission(treeUri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }.onFailure {
            Log.w(TAG, "Couldn't keep access to $treeUri", it)
            return null
        }
        val job = Job(UUID.randomUUID().toString(), treeUri, label, targetDeviceId, targetName, paused = false, lastSyncMs = 0L)
        if (!includeExisting) {
            val now = System.currentTimeMillis()
            // Files too new to settle will be sent: they were still being written.
            walk(context, treeUri).filter { now - it.modifiedMs >= SETTLE_MS }.forEach { recordSent(context, job.id, it.key) }
        }
        save(context, list(context) + job)
        return job
    }

    fun remove(context: Context, id: String) {
        val job = list(context).firstOrNull { it.id == id } ?: return
        save(context, list(context).filter { it.id != id })
        ledgerFile(context, id).delete()
        // Keep the permission if the folder is also shared for browsing.
        if (SharedFolders.list(context).none { it.uri == job.treeUri }) {
            runCatching {
                context.contentResolver.releasePersistableUriPermission(job.treeUri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
        }
    }

    fun setPaused(context: Context, id: String, paused: Boolean) =
        update(context, id) { it.copy(paused = paused) }

    fun markRan(context: Context, id: String, nowMs: Long = System.currentTimeMillis()) =
        update(context, id) { it.copy(lastSyncMs = nowMs) }

    @Synchronized
    private fun update(context: Context, id: String, change: (Job) -> Job) =
        save(context, list(context).map { if (it.id == id) change(it) else it })

    @Synchronized
    private fun save(context: Context, jobs: List<Job>) {
        val array = JSONArray()
        jobs.forEach { j ->
            array.put(
                JSONObject()
                    .put("id", j.id).put("tree", j.treeUri.toString()).put("label", j.label)
                    .put("target", j.targetDeviceId).put("target_name", j.targetName)
                    .put("paused", j.paused).put("last_sync", j.lastSyncMs)
            )
        }
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(KEY, array.toString()).apply()
    }

    // ── Ledger ────────────────────────────────────────────────────────────

    private fun ledgerFile(context: Context, id: String) = File(File(context.filesDir, "foldersync"), "$id.ledger")

    @Synchronized
    fun sentKeys(context: Context, id: String): Set<String> =
        runCatching { ledgerFile(context, id).readLines().toHashSet() }.getOrDefault(emptySet())

    @Synchronized
    fun recordSent(context: Context, id: String, key: String) {
        val file = ledgerFile(context, id)
        runCatching {
            file.parentFile?.mkdirs()
            file.appendText(key + "\n")
        }.onFailure { Log.w(TAG, "Couldn't record $key", it) }
    }

    // ── In flight ─────────────────────────────────────────────────────────

    private class Queued(val jobId: String, val key: String)

    private val inFlight = ConcurrentHashMap<String, Queued>()
    // A transfer can finish before its sender registers it; remember it briefly.
    private val deliveredEarly = ConcurrentHashMap.newKeySet<String>()

    fun inFlightKeys(jobId: String): Set<String> =
        inFlight.values.filter { it.jobId == jobId }.map { it.key }.toHashSet()

    /** A file of [jobId] was handed to the engine as transfer [tid]. */
    fun queued(context: Context, tid: String, jobId: String, key: String) {
        inFlight[tid] = Queued(jobId, key)
        if (deliveredEarly.remove(tid)) delivered(context, tid)
    }

    /** Transfer [tid] reached the computer. */
    fun delivered(context: Context, tid: String) {
        val q = inFlight.remove(tid)
        if (q == null) {
            if (deliveredEarly.size > 500) deliveredEarly.clear()
            deliveredEarly.add(tid)
            return
        }
        recordSent(context, q.jobId, q.key)
    }

    /** Transfer [tid] didn't arrive; the next run sends it again. */
    fun failed(tid: String) {
        inFlight.remove(tid)
        deliveredEarly.remove(tid)
    }

    // ── What to send ──────────────────────────────────────────────────────

    /** Files that are new or changed, settled, and not already on their way. */
    internal fun pending(files: List<TreeFile>, sent: Set<String>, inFlightKeys: Set<String>, nowMs: Long): List<TreeFile> =
        files.filter { it.key !in sent && it.key !in inFlightKeys && nowMs - it.modifiedMs >= SETTLE_MS }

    fun pending(context: Context, job: Job): List<TreeFile> =
        pending(walk(context, job.treeUri), sentKeys(context, job.id), inFlightKeys(job.id), System.currentTimeMillis())

    /** Every file under a folder picked with the system folder picker. */
    fun walk(context: Context, treeUri: Uri): List<TreeFile> {
        val rootId = runCatching { DocumentsContract.getTreeDocumentId(treeUri) }.getOrNull() ?: return emptyList()
        val columns = arrayOf(
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_MIME_TYPE,
            DocumentsContract.Document.COLUMN_SIZE,
            DocumentsContract.Document.COLUMN_LAST_MODIFIED,
        )
        val out = mutableListOf<TreeFile>()
        val dirs = ArrayDeque(listOf(rootId to ""))
        while (dirs.isNotEmpty() && out.size < MAX_FILES) {
            val (dirId, prefix) = dirs.removeFirst()
            val children = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, dirId)
            val rows = runCatching {
                context.contentResolver.query(children, columns, null, null, null)?.use { c ->
                    buildList {
                        while (c.moveToNext()) {
                            add(
                                arrayOf(
                                    c.getString(0), c.getString(1) ?: "", c.getString(2) ?: "",
                                    c.getLong(3).toString(), c.getLong(4).toString(),
                                )
                            )
                        }
                    }
                }
            }.getOrNull() ?: continue
            for (r in rows.sortedBy { it[1] }) {
                val name = r[1]
                if (name.isEmpty() || name in SKIPPED || name.startsWith("._")) continue
                if (r[2] == DocumentsContract.Document.MIME_TYPE_DIR) {
                    dirs.addLast(r[0] to "$prefix$name/")
                } else {
                    out.add(TreeFile(r[0], "$prefix$name", r[3].toLong(), r[4].toLong()))
                }
            }
        }
        return out
    }
}
