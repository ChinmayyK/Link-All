package app.linkall

import android.content.ContentResolver
import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.MediaStore
import android.util.Log
import android.util.Size
import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.io.File

object RemoteFileManager {
    private const val TAG = "RemoteFileManager"

    @androidx.compose.runtime.Immutable
    data class FileMeta(
        val fileId: Long,
        val displayName: String,
        val sizeBytes: Long,
        val mimeType: String,
        val dateModified: Long,
        val category: String,
        val source: String,
        val contentUri: String,
        val dataPath: String
    )

    private const val DOCUMENTS_SELECTION =
        "(${MediaStore.Files.FileColumns.MIME_TYPE} = 'application/pdf' OR " +
        "${MediaStore.Files.FileColumns.MIME_TYPE} LIKE 'text/%' OR " +
        "${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE '%.pdf' OR " +
        "${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE '%.doc' OR " +
        "${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE '%.docx' OR " +
        "${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE '%.xls' OR " +
        "${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE '%.xlsx' OR " +
        "${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE '%.ppt' OR " +
        "${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE '%.pptx' OR " +
        "${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE '%.txt' OR " +
        "${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE '%.csv' OR " +
        "${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE '%.md')"

    private const val APKS_SELECTION =
        "(${MediaStore.Files.FileColumns.MIME_TYPE} = 'application/vnd.android.package-archive' OR " +
        "${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE '%.apk')"

    private const val ARCHIVES_SELECTION =
        "(${MediaStore.Files.FileColumns.MIME_TYPE} IN ('application/zip', 'application/x-zip-compressed', 'application/x-tar', 'application/gzip', 'application/x-rar-compressed', 'application/x-7z-compressed') OR " +
        "${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE '%.zip' OR " +
        "${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE '%.tar' OR " +
        "${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE '%.gz' OR " +
        "${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE '%.rar' OR " +
        "${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE '%.7z')"

    private const val OTHER_SELECTION =
        "NOT (${MediaStore.Files.FileColumns.MIME_TYPE} LIKE 'image/%' OR " +
        "${MediaStore.Files.FileColumns.MIME_TYPE} LIKE 'video/%' OR " +
        "${MediaStore.Files.FileColumns.MIME_TYPE} LIKE 'audio/%' OR " +
        DOCUMENTS_SELECTION + " OR " +
        APKS_SELECTION + " OR " +
        ARCHIVES_SELECTION + ")"

    private fun countFiles(context: Context, selection: String, selectionArgs: Array<String>? = null): Int {
        val uri = MediaStore.Files.getContentUri("external")
        val projection = arrayOf(MediaStore.Files.FileColumns._ID)
        return try {
            context.contentResolver.query(uri, projection, selection, selectionArgs, null)?.use { cursor ->
                cursor.count
            } ?: 0
        } catch (e: Exception) {
            Log.e(TAG, "Error counting files for selection: $selection", e)
            0
        }
    }

    private fun buildFilterSelection(
        categoryFilter: String?,
        sourceFilter: String?,
        searchQuery: String?
    ): Pair<String, Array<String>> {
        val selectionParts = mutableListOf<String>()
        val selectionArgs = mutableListOf<String>()

        selectionParts.add("${MediaStore.Files.FileColumns.SIZE} > 0")

        if (!categoryFilter.isNullOrEmpty() && categoryFilter != "All") {
            when (categoryFilter) {
                "Images" -> selectionParts.add("${MediaStore.Files.FileColumns.MIME_TYPE} LIKE 'image/%'")
                "Videos" -> selectionParts.add("${MediaStore.Files.FileColumns.MIME_TYPE} LIKE 'video/%'")
                "Audio" -> selectionParts.add("${MediaStore.Files.FileColumns.MIME_TYPE} LIKE 'audio/%'")
                "Documents" -> selectionParts.add(DOCUMENTS_SELECTION)
                "Apks" -> selectionParts.add(APKS_SELECTION)
                "Archives" -> selectionParts.add(ARCHIVES_SELECTION)
                "Other" -> selectionParts.add(OTHER_SELECTION)
            }
        }

        if (!sourceFilter.isNullOrEmpty() && sourceFilter != "All") {
            when (sourceFilter) {
                "WhatsApp" -> selectionParts.add("${MediaStore.Files.FileColumns.DATA} LIKE '%whatsapp%'")
                "Downloads" -> selectionParts.add("${MediaStore.Files.FileColumns.DATA} LIKE '%download%'")
                "Camera" -> selectionParts.add("(${MediaStore.Files.FileColumns.DATA} LIKE '%dcim%' OR ${MediaStore.Files.FileColumns.DATA} LIKE '%camera%')")
            }
        }

        if (!searchQuery.isNullOrEmpty()) {
            selectionParts.add("${MediaStore.Files.FileColumns.DISPLAY_NAME} LIKE ?")
            selectionArgs.add("%$searchQuery%")
        }

        val selectionString = selectionParts.joinToString(" AND ")
        return Pair(selectionString, selectionArgs.toTypedArray())
    }

    fun queryFiles(
        context: Context,
        categoryFilter: String?,
        sourceFilter: String?,
        searchQuery: String?,
        offset: Int,
        limit: Int,
        includeSummary: Boolean,
        includeList: Boolean
    ): Triple<String?, String?, Int> {
        var summaryJson: String? = null
        if (includeSummary) {
            var images = 0
            var videos = 0
            var audio = 0
            var documents = 0
            var apks = 0
            var archives = 0
            var whatsapp = 0
            var downloads = 0
            var camera = 0

            val uri = MediaStore.Files.getContentUri("external")
            val projection = arrayOf(
                MediaStore.Files.FileColumns.MIME_TYPE,
                MediaStore.Files.FileColumns.DATA,
                MediaStore.Files.FileColumns.DISPLAY_NAME
            )
            val selection = "${MediaStore.Files.FileColumns.SIZE} > 0"
            try {
                context.contentResolver.query(uri, projection, selection, null, null)?.use { cursor ->
                    val mimeIdx = cursor.getColumnIndex(MediaStore.Files.FileColumns.MIME_TYPE)
                    val dataIdx = cursor.getColumnIndex(MediaStore.Files.FileColumns.DATA)
                    val nameIdx = cursor.getColumnIndex(MediaStore.Files.FileColumns.DISPLAY_NAME)
                    
                    while (cursor.moveToNext()) {
                        val mime = if (mimeIdx >= 0) cursor.getString(mimeIdx) ?: "" else ""
                        val name = if (nameIdx >= 0) cursor.getString(nameIdx) ?: "" else ""
                        val dataPath = if (dataIdx >= 0) cursor.getString(dataIdx) ?: "" else ""

                        val cat = getCategory(mime, name)
                        val src = getSource(dataPath)

                        when (cat) {
                            "Images" -> images++
                            "Videos" -> videos++
                            "Audio" -> audio++
                            "Documents" -> documents++
                            "Apks" -> apks++
                            "Archives" -> archives++
                        }
                        when (src) {
                            "WhatsApp" -> whatsapp++
                            "Downloads" -> downloads++
                            "Camera" -> camera++
                        }
                    }
                }
            } catch (e: Exception) {
                Log.e(TAG, "Error calculating summary in single pass", e)
            }

            val typeCounts = JSONObject().apply {
                put("images", images)
                put("videos", videos)
                put("audio", audio)
                put("documents", documents)
                put("apks", apks)
                put("archives", archives)
            }
            val sourceCounts = JSONObject().apply {
                put("whatsapp", whatsapp)
                put("downloads", downloads)
                put("camera", camera)
            }
            summaryJson = JSONObject().apply {
                put("type_counts", typeCounts)
                put("source_counts", sourceCounts)
            }.toString()
        }

        val (selectionString, selectionArgs) = buildFilterSelection(categoryFilter, sourceFilter, searchQuery)
        val totalMatching = countFiles(context, selectionString, selectionArgs)
        Log.d(TAG, "DIAG queryFiles: categoryFilter=$categoryFilter sourceFilter=$sourceFilter searchQuery=$searchQuery selection=[$selectionString] args=${selectionArgs.joinToString()} totalMatching=$totalMatching")

        var filesJson: String? = null
        if (includeList && totalMatching > 0 && offset < totalMatching && limit > 0) {
            val matchingList = mutableListOf<FileMeta>()
            val uri = MediaStore.Files.getContentUri("external")
            val projection = arrayOf(
                MediaStore.Files.FileColumns._ID,
                MediaStore.Files.FileColumns.DISPLAY_NAME,
                MediaStore.Files.FileColumns.SIZE,
                MediaStore.Files.FileColumns.MIME_TYPE,
                MediaStore.Files.FileColumns.DATE_MODIFIED,
                MediaStore.Files.FileColumns.DATA
            )

            try {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                    val queryArgs = Bundle().apply {
                        putString(ContentResolver.QUERY_ARG_SQL_SELECTION, selectionString)
                        if (selectionArgs.isNotEmpty()) {
                            putStringArray(ContentResolver.QUERY_ARG_SQL_SELECTION_ARGS, selectionArgs)
                        }
                        putStringArray(
                            ContentResolver.QUERY_ARG_SORT_COLUMNS,
                            arrayOf(MediaStore.Files.FileColumns.DATE_MODIFIED)
                        )
                        putInt(
                            ContentResolver.QUERY_ARG_SORT_DIRECTION,
                            ContentResolver.QUERY_SORT_DIRECTION_DESCENDING
                        )
                        putInt(ContentResolver.QUERY_ARG_OFFSET, offset)
                        putInt(ContentResolver.QUERY_ARG_LIMIT, limit)
                    }
                    context.contentResolver.query(uri, projection, queryArgs, null)?.use { cursor ->
                        readCursorRows(cursor, uri, matchingList)
                    }
                } else {
                    val sortOrder = "${MediaStore.Files.FileColumns.DATE_MODIFIED} DESC LIMIT $limit OFFSET $offset"
                    context.contentResolver.query(
                        uri,
                        projection,
                        selectionString,
                        if (selectionArgs.isNotEmpty()) selectionArgs else null,
                        sortOrder
                    )?.use { cursor ->
                        readCursorRows(cursor, uri, matchingList)
                    }
                }
            } catch (e: Exception) {
                Log.w(TAG, "Bundle query failed, falling back to 5-arg query: ${e.message}")
                val sortOrder = "${MediaStore.Files.FileColumns.DATE_MODIFIED} DESC LIMIT $limit OFFSET $offset"
                context.contentResolver.query(
                    uri,
                    projection,
                    selectionString,
                    if (selectionArgs.isNotEmpty()) selectionArgs else null,
                    sortOrder
                )?.use { cursor ->
                    readCursorRows(cursor, uri, matchingList)
                }
            }

            val jsonArray = JSONArray()
            for (item in matchingList) {
                val obj = JSONObject().apply {
                    put("file_id", item.fileId)
                    put("display_name", item.displayName)
                    put("size_bytes", item.sizeBytes)
                    put("mime_type", item.mimeType)
                    put("date_modified", item.dateModified)
                    put("category", item.category)
                    put("source", item.source)
                    put("content_uri", item.contentUri)
                    put("path", item.dataPath)
                }
                jsonArray.put(obj)
            }
            filesJson = jsonArray.toString()
        } else if (includeList) {
            filesJson = "[]"
        }

        return Triple(summaryJson, filesJson, totalMatching)
    }

    private fun readCursorRows(
        cursor: android.database.Cursor,
        uri: Uri,
        matchingList: MutableList<FileMeta>
    ) {
        val idIdx = cursor.getColumnIndex(MediaStore.Files.FileColumns._ID)
        val nameIdx = cursor.getColumnIndex(MediaStore.Files.FileColumns.DISPLAY_NAME)
        val sizeIdx = cursor.getColumnIndex(MediaStore.Files.FileColumns.SIZE)
        val mimeIdx = cursor.getColumnIndex(MediaStore.Files.FileColumns.MIME_TYPE)
        val dateIdx = cursor.getColumnIndex(MediaStore.Files.FileColumns.DATE_MODIFIED)
        val dataIdx = cursor.getColumnIndex(MediaStore.Files.FileColumns.DATA)

        while (cursor.moveToNext()) {
            val size = if (sizeIdx >= 0) cursor.getLong(sizeIdx) else 0L
            if (size <= 0L) continue

            val id = if (idIdx >= 0) cursor.getLong(idIdx) else 0L
            val name = if (nameIdx >= 0) cursor.getString(nameIdx) ?: "" else ""
            val mime = if (mimeIdx >= 0) cursor.getString(mimeIdx) ?: "" else ""
            val dateMod = if (dateIdx >= 0) cursor.getLong(dateIdx) else 0L
            val dataPath = if (dataIdx >= 0) cursor.getString(dataIdx) ?: "" else ""

            if (dataPath.isNotEmpty() && !java.io.File(dataPath).exists()) continue

            val cat = getCategory(mime, name)
            val src = getSource(dataPath)
            val contentUri = ContentUris.withAppendedId(uri, id).toString()

            matchingList.add(
                FileMeta(
                    fileId = id,
                    displayName = name,
                    sizeBytes = size,
                    mimeType = mime.ifEmpty { "application/octet-stream" },
                    dateModified = dateMod,
                    category = cat,
                    source = src,
                    contentUri = contentUri,
                    dataPath = dataPath
                )
            )
        }
    }

    fun getThumbnail(context: Context, fileId: Long, sizePx: Int): ByteArray? {
        val uri = ContentUris.withAppendedId(MediaStore.Files.getContentUri("external"), fileId)
        var bitmap: Bitmap? = null

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            try {
                bitmap = context.contentResolver.loadThumbnail(uri, Size(sizePx, sizePx), null)
            } catch (e: Exception) {
                Log.w(TAG, "loadThumbnail Q+ failed for uri $uri: ${e.message}")
            }
        }

        if (bitmap == null) {
            try {
                context.contentResolver.openInputStream(uri)?.use { stream ->
                    val options = BitmapFactory.Options().apply {
                        inJustDecodeBounds = true
                    }
                    BitmapFactory.decodeStream(stream, null, options)
                    options.inSampleSize = calculateInSampleSize(options, sizePx, sizePx)
                    options.inJustDecodeBounds = false
                    context.contentResolver.openInputStream(uri)?.use { stream2 ->
                        val rawBitmap = BitmapFactory.decodeStream(stream2, null, options)
                        if (rawBitmap != null) {
                            // Fit the longest side to sizePx, keeping aspect -
                            // a plain sizePx x sizePx scale squashed every
                            // non-square photo in the desktop preview grid.
                            val scale = sizePx.toFloat() / maxOf(rawBitmap.width, rawBitmap.height)
                            val w = (rawBitmap.width * scale).toInt().coerceAtLeast(1)
                            val h = (rawBitmap.height * scale).toInt().coerceAtLeast(1)
                            bitmap = Bitmap.createScaledBitmap(rawBitmap, w, h, true)
                            if (bitmap != rawBitmap) rawBitmap.recycle()
                        }
                    }
                }
            } catch (e: Exception) {
                Log.w(TAG, "BitmapFactory fallback failed for uri $uri: ${e.message}")
            }
        }

        return bitmap?.let { bmp ->
            val bos = ByteArrayOutputStream()
            bmp.compress(Bitmap.CompressFormat.JPEG, 80, bos)
            bmp.recycle()
            bos.toByteArray()
        }
    }

    fun resolveFilePathAndMeta(context: Context, fileId: Long): Triple<String, String, String>? {
        val projection = arrayOf(
            MediaStore.Files.FileColumns.DISPLAY_NAME,
            MediaStore.Files.FileColumns.MIME_TYPE,
            MediaStore.Files.FileColumns.DATA
        )
        val uri = ContentUris.withAppendedId(MediaStore.Files.getContentUri("external"), fileId)
        try {
            context.contentResolver.query(uri, projection, null, null, null)?.use { cursor ->
                if (cursor.moveToFirst()) {
                    val nameIdx = cursor.getColumnIndex(MediaStore.Files.FileColumns.DISPLAY_NAME)
                    val mimeIdx = cursor.getColumnIndex(MediaStore.Files.FileColumns.MIME_TYPE)
                    val dataIdx = cursor.getColumnIndex(MediaStore.Files.FileColumns.DATA)

                    val name = if (nameIdx >= 0) cursor.getString(nameIdx) ?: "" else ""
                    val mime = if (mimeIdx >= 0) cursor.getString(mimeIdx) ?: "" else ""
                    val path = if (dataIdx >= 0) cursor.getString(dataIdx) ?: "" else ""

                    if (path.isNotEmpty() && File(path).exists()) {
                        return Triple(
                            path,
                            name.ifEmpty { File(path).name },
                            mime.ifEmpty { "application/octet-stream" }
                        )
                    }
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "Error resolving file path for id $fileId", e)
        }
        return null
    }

    private fun getCategory(mime: String, name: String): String {
        if (mime.startsWith("image/", ignoreCase = true)) return "Images"
        if (mime.startsWith("video/", ignoreCase = true)) return "Videos"
        if (mime.startsWith("audio/", ignoreCase = true)) return "Audio"

        if (mime.equals("application/vnd.android.package-archive", ignoreCase = true)) return "Apks"
        if (mime.equals("application/pdf", ignoreCase = true) || mime.startsWith("text/", ignoreCase = true)) return "Documents"
        
        if (mime.equals("application/zip", ignoreCase = true) || 
            mime.equals("application/x-zip-compressed", ignoreCase = true) || 
            mime.equals("application/x-tar", ignoreCase = true) || 
            mime.equals("application/gzip", ignoreCase = true) || 
            mime.equals("application/x-rar-compressed", ignoreCase = true) || 
            mime.equals("application/x-7z-compressed", ignoreCase = true)) return "Archives"

        val lowerName = name.lowercase()
        return when {
            lowerName.endsWith(".apk") -> "Apks"
            lowerName.endsWith(".zip") || lowerName.endsWith(".tar") || lowerName.endsWith(".gz") ||
                    lowerName.endsWith(".rar") || lowerName.endsWith(".7z") -> "Archives"
            lowerName.endsWith(".pdf") || lowerName.endsWith(".doc") || lowerName.endsWith(".docx") ||
                    lowerName.endsWith(".xls") || lowerName.endsWith(".xlsx") || lowerName.endsWith(".ppt") ||
                    lowerName.endsWith(".pptx") || lowerName.endsWith(".txt") || lowerName.endsWith(".csv") ||
                    lowerName.endsWith(".md") -> "Documents"
            else -> "Other"
        }
    }

    private fun getSource(dataPath: String): String {
        if (dataPath.contains("whatsapp", ignoreCase = true)) return "WhatsApp"
        if (dataPath.contains("download", ignoreCase = true)) return "Downloads"
        if (dataPath.contains("dcim", ignoreCase = true) || dataPath.contains("camera", ignoreCase = true)) return "Camera"
        return "All"
    }

    private fun matchesFilters(
        name: String,
        category: String,
        source: String,
        categoryFilter: String?,
        sourceFilter: String?,
        searchQuery: String?
    ): Boolean {
        if (!categoryFilter.isNullOrEmpty() && categoryFilter != "All" && category != categoryFilter) {
            return false
        }
        if (!sourceFilter.isNullOrEmpty() && sourceFilter != "All" && source != sourceFilter) {
            return false
        }
        if (!searchQuery.isNullOrEmpty() && !name.contains(searchQuery, ignoreCase = true)) {
            return false
        }
        return true
    }

    private fun calculateInSampleSize(options: BitmapFactory.Options, reqWidth: Int, reqHeight: Int): Int {
        val (height: Int, width: Int) = options.outHeight to options.outWidth
        var inSampleSize = 1
        if (height > reqHeight || width > reqWidth) {
            val halfHeight: Int = height / 2
            val halfWidth: Int = width / 2
            while (halfHeight / inSampleSize >= reqHeight && halfWidth / inSampleSize >= reqWidth) {
                inSampleSize *= 2
            }
        }
        return inSampleSize
    }

    fun executeAction(context: Context, fileId: Long, action: String, newName: String?) {
        val uri = ContentUris.withAppendedId(MediaStore.Files.getContentUri("external"), fileId)
        val resolver = context.contentResolver
        when (action) {
            "delete" -> {
                try {
                    val count = resolver.delete(uri, null, null)
                    Log.i(TAG, "Deleted file $fileId, count: $count")
                } catch (e: Exception) {
                    Log.e(TAG, "Failed to delete file $fileId", e)
                }
            }
            "rename" -> {
                if (newName.isNullOrEmpty()) return
                try {
                    val values = ContentValues().apply {
                        put(MediaStore.MediaColumns.DISPLAY_NAME, newName)
                    }
                    val count = resolver.update(uri, values, null, null)
                    Log.i(TAG, "Renamed file $fileId to $newName, count: $count")
                } catch (e: Exception) {
                    Log.e(TAG, "Failed to rename file $fileId", e)
                }
            }
            else -> {
                Log.w(TAG, "Unknown file action: $action")
            }
        }
    }
}
