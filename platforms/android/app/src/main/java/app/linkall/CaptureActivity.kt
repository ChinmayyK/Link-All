package app.linkall

import android.app.Activity
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.ColorMatrix
import android.graphics.ColorMatrixColorFilter
import android.graphics.Paint
import android.net.Uri
import android.os.Bundle
import android.provider.MediaStore
import android.widget.Toast
import androidx.core.content.FileProvider
import com.google.mlkit.vision.documentscanner.GmsDocumentScanning
import com.google.mlkit.vision.documentscanner.GmsDocumentScannerOptions
import com.google.mlkit.vision.documentscanner.GmsDocumentScanningResult
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlin.concurrent.thread

/**
 * "Insert from Phone": a paired computer asked for a picture. This opens the
 * camera, and the picture goes straight back to that computer. A "document"
 * is made easier to read first (grey, with its contrast stretched); a "photo"
 * is sent as taken. Nothing is captured unless its owner takes the picture.
 */
class CaptureActivity : Activity() {
    private var photo: File? = null
    private var mode = "photo"
    private var deviceId: String? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        mode = intent.getStringExtra(EXTRA_MODE) ?: "photo"
        deviceId = intent.getStringExtra(EXTRA_DEVICE_ID)
        if (deviceId == null) {
            finish()
            return
        }
        if (mode == "document" && startDocumentScanner()) return
        startCamera()
    }

    /**
     * A document is scanned with Google's scanner, which finds the page's edges,
     * straightens it and cleans it up. Returns false when this phone can't
     * (no Google Play services), and the camera is used with a simple clean-up.
     */
    private fun startDocumentScanner(): Boolean {
        return runCatching {
            val options = GmsDocumentScannerOptions.Builder()
                .setGalleryImportAllowed(false)
                .setPageLimit(1)
                .setResultFormats(GmsDocumentScannerOptions.RESULT_FORMAT_JPEG)
                .setScannerMode(GmsDocumentScannerOptions.SCANNER_MODE_FULL)
                .build()
            GmsDocumentScanning.getClient(options)
                .getStartScanIntent(this)
                .addOnSuccessListener { sender ->
                    runCatching { startIntentSenderForResult(sender, SCAN_REQUEST, null, 0, 0, 0) }
                        .onFailure { startCamera() }
                }
                .addOnFailureListener { startCamera() }
            true
        }.getOrDefault(false)
    }

    private fun startCamera() {
        val dir = File(cacheDir, "captures").apply { mkdirs() }
        // Pictures from earlier requests are no longer needed.
        dir.listFiles()?.filter { System.currentTimeMillis() - it.lastModified() > 60 * 60 * 1000 }?.forEach { it.delete() }
        val file = File(dir, "capture-${System.currentTimeMillis()}.jpg").also { photo = it }
        val uri = FileProvider.getUriForFile(this, "$packageName.fileprovider", file)
        val camera = Intent(MediaStore.ACTION_IMAGE_CAPTURE)
            .putExtra(MediaStore.EXTRA_OUTPUT, uri)
            .addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_READ_URI_PERMISSION)
        runCatching { startActivityForResult(camera, REQUEST) }.onFailure {
            Toast.makeText(this, "This phone has no camera app", Toast.LENGTH_LONG).show()
            finish()
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == SCAN_REQUEST) {
            val page = if (resultCode == RESULT_OK) {
                GmsDocumentScanningResult.fromActivityResultIntent(data)?.pages?.firstOrNull()?.imageUri
            } else null
            if (page == null) {
                finish()
                return
            }
            val dir = File(cacheDir, "captures").apply { mkdirs() }
            val scanned = File(dir, "scan-${System.currentTimeMillis()}.jpg")
            val copied = runCatching {
                contentResolver.openInputStream(page)?.use { input -> scanned.outputStream().use { input.copyTo(it) } }
                scanned.length() > 0
            }.getOrDefault(false)
            if (copied) send(scanned) else finish()
            return
        }
        val file = photo
        if (requestCode != REQUEST || resultCode != RESULT_OK || file == null || !file.exists() || file.length() == 0L) {
            finish()
            return
        }
        send(if (mode == "document") enhance(file) ?: file else file)
    }

    /** Sends the picture back to the computer that asked for it. */
    private fun send(toSend: File) {
        val target = deviceId
        val app = applicationContext
        thread {
            val name = "LinkAll Capture ${SimpleDateFormat("yyyy-MM-dd HH.mm.ss", Locale.US).format(Date())}.jpg"
            val uri: Uri = FileProvider.getUriForFile(app, "${app.packageName}.fileprovider", toSend)
            app.startService(
                Intent(app, LinkAllService::class.java).apply {
                    action = LinkAllService.ACTION_PUSH_SHARED_URI
                    putExtra(LinkAllService.EXTRA_SHARED_URI, uri.toString())
                    putExtra(LinkAllService.EXTRA_SHARED_NAME, name)
                    putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, target)
                }
            )
            runOnUiThread {
                Toast.makeText(app, "Sending the picture to your computer", Toast.LENGTH_SHORT).show()
                finish()
            }
        }
    }

    /** A document picture, grey with its contrast raised so text reads clearly. */
    private fun enhance(source: File): File? = runCatching {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(source.path, bounds)
        var sample = 1
        while (bounds.outWidth / sample > 3000 || bounds.outHeight / sample > 3000) sample *= 2
        val bitmap = BitmapFactory.decodeFile(source.path, BitmapFactory.Options().apply { inSampleSize = sample })
            ?: return@runCatching null
        val out = Bitmap.createBitmap(bitmap.width, bitmap.height, Bitmap.Config.ARGB_8888)
        val grey = ColorMatrix().apply { setSaturation(0f) }
        // Contrast 1.6 around mid-grey, then a little brighter.
        val c = 1.6f
        val shift = (-0.5f * c + 0.5f) * 255f + 18f
        grey.postConcat(ColorMatrix(floatArrayOf(c, 0f, 0f, 0f, shift, 0f, c, 0f, 0f, shift, 0f, 0f, c, 0f, shift, 0f, 0f, 0f, 1f, 0f)))
        Canvas(out).drawBitmap(bitmap, 0f, 0f, Paint().apply { colorFilter = ColorMatrixColorFilter(grey) })
        bitmap.recycle()
        File(source.parentFile, "document-${System.currentTimeMillis()}.jpg").also { f ->
            f.outputStream().use { out.compress(Bitmap.CompressFormat.JPEG, 92, it) }
            out.recycle()
        }
    }.getOrNull()

    companion object {
        const val EXTRA_MODE = "capture_mode"
        const val EXTRA_DEVICE_ID = "capture_device_id"
        private const val REQUEST = 71
        private const val SCAN_REQUEST = 72
    }
}
