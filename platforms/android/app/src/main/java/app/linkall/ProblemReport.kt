package app.linkall

import android.content.Context
import android.content.Intent
import android.os.Build

/** The text a user can send when something doesn't work. The engine adds its own state and log. */
object ProblemReport {
    private fun header(context: Context): String {
        val info = runCatching { context.packageManager.getPackageInfo(context.packageName, 0) }.getOrNull()
        val build = if (BuildConfig.FULL_PERMISSIONS) "direct download" else "Google Play"
        return buildString {
            appendLine("Android: ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})")
            appendLine("Phone: ${Build.MANUFACTURER} ${Build.MODEL}")
            append("Build: $build, ${info?.versionName ?: "unknown"}")
        }
    }

    /** Blocks on the engine, so call it off the main thread. */
    fun build(context: Context): String {
        val header = header(context)
        return LinkAllService.withLiveEngine { handle -> LinkAllJni.diagnosticReport(handle, header) }
            ?: "Link All problem report\n$header\n\nLink All was not running, so there is no device state or log.\n"
    }

    /** Opens the share sheet, which also offers Copy and Save. */
    fun share(context: Context, text: String) {
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_SUBJECT, "Link All problem report")
            putExtra(Intent.EXTRA_TEXT, text)
        }
        context.startActivity(Intent.createChooser(send, "Share problem report").addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }
}
