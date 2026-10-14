package app.linkall

import android.content.Context
import android.content.Intent
import android.graphics.Point
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
import android.provider.DocumentsContract.Document
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject

/**
 * The folders this phone lets its computers browse, in the Google Play build.
 *
 * Play only grants all-files access to file managers, so the Play build asks
 * the user to pick folders with the system folder picker instead. Android
 * remembers each pick (a persisted tree permission), and this object answers
 * the computer's browse, thumbnail, pull and file-action requests from those
 * folders only, with the same JSON shapes [RemoteFileManager] produces from
 * MediaStore in the full build.
 */
object SharedFolders {
    private const val TAG = "SharedFolders"
    private const val PREFS = "shared_folders"
    private const val KEY = "tree_uris"

    // A listing is paged by the computer; rescanning every page would walk the
    // tree again for each scroll, so a scan is reused for a short while.
    private const val SCAN_TTL_MS = 20_000L
    private const val MAX_DEPTH = 8
    private const val MAX_FILES = 50_000

    data class Folder(val uri: Uri, val label: String)

    private data class Entry(
        val id: Long,
        val uri: Uri,
        val name: String,
        val mime: String,
        val size: Long,
        val modifiedSecs: Long,
        val path: String,
        val category: String,
        val source: String,
    )

    private class Scan(val folders: Set<String>, val at: Long, val entries: List<Entry>) {
        val byId: Map<Long, Entry> = entries.associateBy { it.id }
    }

    @Volatile private var scan: Scan? = null

    /** The folders still shared: picked by the user, and still granted by Android. */
    fun list(context: Context): List<Folder> {
        val granted = context.contentResolver.persistedUriPermissions
            .filter { it.isReadPermission }
            .map { it.uri.toString() }
            .toSet()
        return stored(context).filter { it in granted }.map { Uri.parse(it) }.map { Folder(it, label(it)) }
    }

    fun add(context: Context, uri: Uri) {
        val resolver = context.contentResolver
        // Write lets the computer rename or delete; fall back to read-only if
        // the provider didn't grant it.
        runCatching {
            resolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
        }.recoverCatching {
            resolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }.onFailure { Log.w(TAG, "Couldn't keep access to $uri", it); return }
        save(context, stored(context) + uri.toString())
        scan = null
    }

    fun remove(context: Context, uri: Uri) {
        runCatching {
            context.contentResolver.releasePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
        }.recoverCatching {
            context.contentResolver.releasePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        save(context, stored(context) - uri.toString())
        scan = null
    }

    /** "DCIM/Camera", or "Music (SD card)" for a folder on removable storage. */
    fun label(uri: Uri): String {
        val docId = runCatching { DocumentsContract.getTreeDocumentId(uri) }.getOrNull() ?: return uri.lastPathSegment ?: "Folder"
        val volume = docId.substringBefore(':')
        val path = docId.substringAfter(':', "").trimEnd('/')
        val name = path.ifEmpty { "Storage" }
        return if (volume == "primary" || volume == "home") name else "$name (SD card)"
    }

    /** Same result as [RemoteFileManager.queryFiles]: summary JSON, files JSON and the matching count. */
    fun query(
        context: Context,
        categoryFilter: String?,
        sourceFilter: String?,
        searchQuery: String?,
        offset: Int,
        limit: Int,
        includeSummary: Boolean,
        includeList: Boolean,
    ): Triple<String?, String?, Int> {
        val all = current(context, forceRescan = offset == 0).entries

        val summaryJson = if (includeSummary) {
            fun count(cat: String) = all.count { it.category == cat }
            fun from(src: String) = all.count { it.source == src }
            JSONObject().apply {
                put("type_counts", JSONObject().apply {
                    put("images", count("Images")); put("videos", count("Videos")); put("audio", count("Audio"))
                    put("documents", count("Documents")); put("apks", count("Apks")); put("archives", count("Archives"))
                })
                put("source_counts", JSONObject().apply {
                    put("whatsapp", from("WhatsApp")); put("downloads", from("Downloads")); put("camera", from("Camera"))
                })
            }.toString()
        } else null

        val matching = all.filter { e ->
            (categoryFilter.isNullOrEmpty() || categoryFilter == "All" || e.category == categoryFilter) &&
                (sourceFilter.isNullOrEmpty() || sourceFilter == "All" || e.source == sourceFilter) &&
                (searchQuery.isNullOrEmpty() || e.name.contains(searchQuery, ignoreCase = true))
        }

        val filesJson = if (includeList) {
            JSONArray().apply {
                matching.drop(offset.coerceAtLeast(0)).take(limit.coerceAtLeast(0)).forEach { e ->
                    put(JSONObject().apply {
                        put("file_id", e.id)
                        put("display_name", e.name)
                        put("size_bytes", e.size)
                        put("mime_type", e.mime)
                        put("date_modified", e.modifiedSecs)
                        put("category", e.category)
                        put("source", e.source)
                        put("content_uri", e.uri.toString())
                        put("path", e.path)
                    })
                }
            }.toString()
        } else null

        return Triple(summaryJson, filesJson, matching.size)
    }

    fun thumbnail(context: Context, fileId: Long, sizePx: Int): ByteArray? {
        val e = find(context, fileId) ?: return null
        val bitmap = runCatching {
            DocumentsContract.getDocumentThumbnail(context.contentResolver, e.uri, Point(sizePx, sizePx), null)
        }.getOrNull()
        return bitmap?.let { RemoteFileManager.toJpeg(it) } ?: RemoteFileManager.decodeThumbnail(context, e.uri, sizePx)
    }

    /** An open file for the engine to send, with its name and type, or null if it's gone. */
    fun open(context: Context, fileId: Long): Triple<ParcelFileDescriptor, String, String>? {
        val e = find(context, fileId) ?: return null
        val pfd = runCatching { context.contentResolver.openFileDescriptor(e.uri, "r") }.getOrNull() ?: return null
        return Triple(pfd, e.name, e.mime)
    }

    fun executeAction(context: Context, fileId: Long, action: String, newName: String?) {
        val e = find(context, fileId) ?: return
        runCatching {
            when (action) {
                "delete" -> { DocumentsContract.deleteDocument(context.contentResolver, e.uri) }
                "rename" -> { if (!newName.isNullOrBlank()) DocumentsContract.renameDocument(context.contentResolver, e.uri, newName) else null }
                else -> { Log.w(TAG, "Unknown file action: $action") }
            }
        }.onFailure { Log.e(TAG, "File action $action failed for ${e.name}", it) }
        scan = null
    }

    private fun find(context: Context, fileId: Long): Entry? =
        current(context, forceRescan = false).byId[fileId] ?: current(context, forceRescan = true).byId[fileId]

    private fun current(context: Context, forceRescan: Boolean): Scan {
        val folders = list(context)
        val keys = folders.map { it.uri.toString() }.toSet()
        val cached = scan
        if (cached != null && cached.folders == keys && !forceRescan &&
            System.currentTimeMillis() - cached.at < SCAN_TTL_MS
        ) return cached
        val entries = mutableListOf<Entry>()
        for (f in folders) {
            if (entries.size >= MAX_FILES) break
            val root = runCatching { DocumentsContract.getTreeDocumentId(f.uri) }.getOrNull() ?: continue
            walk(context, f.uri, root, f.label, 0, entries)
        }
        entries.sortByDescending { it.modifiedSecs }
        return Scan(keys, System.currentTimeMillis(), entries).also { scan = it }
    }

    private fun walk(context: Context, tree: Uri, docId: String, path: String, depth: Int, out: MutableList<Entry>) {
        if (depth > MAX_DEPTH || out.size >= MAX_FILES) return
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, docId)
        val columns = arrayOf(
            Document.COLUMN_DOCUMENT_ID, Document.COLUMN_DISPLAY_NAME, Document.COLUMN_MIME_TYPE,
            Document.COLUMN_SIZE, Document.COLUMN_LAST_MODIFIED,
        )
        val dirs = mutableListOf<Pair<String, String>>()
        try {
            context.contentResolver.query(children, columns, null, null, null)?.use { c ->
                while (c.moveToNext() && out.size < MAX_FILES) {
                    val id = c.getString(0) ?: continue
                    val name = c.getString(1) ?: continue
                    // Hidden folders hold caches and trash (.thumbnails, .trashed-…), not the user's files.
                    if (name.startsWith(".")) continue
                    val mime = c.getString(2) ?: "application/octet-stream"
                    val childPath = "$path/$name"
                    if (mime == Document.MIME_TYPE_DIR) {
                        dirs += id to childPath
                        continue
                    }
                    val size = if (c.isNull(3)) 0L else c.getLong(3)
                    if (size <= 0L) continue
                    val uri = DocumentsContract.buildDocumentUriUsingTree(tree, id)
                    out += Entry(
                        id = stableId(uri),
                        uri = uri,
                        name = name,
                        mime = mime,
                        size = size,
                        modifiedSecs = if (c.isNull(4)) 0L else c.getLong(4) / 1000,
                        path = childPath,
                        category = RemoteFileManager.categoryOf(mime, name),
                        source = RemoteFileManager.sourceOf(childPath),
                    )
                }
            }
        } catch (e: Exception) {
            Log.w(TAG, "Couldn't list $path", e)
        }
        for ((id, p) in dirs) walk(context, tree, id, p, depth + 1, out)
    }

    // File ids travel as numbers; a document URI is stable for as long as the
    // file stays where it is, so its hash is too.
    private fun stableId(uri: Uri): Long {
        var h = 1125899906842597L
        for (ch in uri.toString()) h = 31 * h + ch.code
        return h and Long.MAX_VALUE
    }

    private fun stored(context: Context): Set<String> =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getStringSet(KEY, emptySet())?.toSet() ?: emptySet()

    private fun save(context: Context, uris: Set<String>) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putStringSet(KEY, uris).apply()
    }
}
