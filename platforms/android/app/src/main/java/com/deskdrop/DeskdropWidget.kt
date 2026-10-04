package com.deskdrop

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.widget.RemoteViews
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat

/**
 * Home screen widget: connection status, pause/resume, and one-tap sending
 * of the clipboard or picked files. It renders from the same prefs the
 * service writes; the service calls [updateAll] on every status change.
 */
class DeskdropWidget : AppWidgetProvider() {

    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        val views = buildViews(context)
        ids.forEach { manager.updateAppWidget(it, views) }
    }

    companion object {
        /** Called by the service whenever its status changes. */
        fun updateAll(context: Context) {
            val manager = AppWidgetManager.getInstance(context)
            val ids = manager.getAppWidgetIds(ComponentName(context, DeskdropWidget::class.java))
            if (ids.isNotEmpty()) manager.updateAppWidget(ids, buildViews(context))
        }

        private fun buildViews(context: Context): RemoteViews {
            val prefs = context.getSharedPreferences(DeskdropService.PREFS_NAME, Context.MODE_PRIVATE)
            val running = prefs.getBoolean(DeskdropService.PREF_SERVICE_RUNNING, false)
            val syncEnabled = prefs.getBoolean("sync_enabled", true)
            val names = prefs.getStringSet("connected_names", emptySet()).orEmpty().sorted()

            val (dot, status) = when {
                !running -> R.drawable.widget_dot_idle to context.getString(R.string.widget_status_off)
                !syncEnabled -> R.drawable.widget_dot_amber to context.getString(R.string.widget_status_paused)
                names.size == 1 -> R.drawable.widget_dot_green to "Connected to ${names[0]}"
                names.size > 1 -> R.drawable.widget_dot_green to "${names.size} devices connected"
                else -> R.drawable.widget_dot_idle to context.getString(R.string.widget_status_idle)
            }

            return RemoteViews(context.packageName, R.layout.widget_deskdrop).apply {
                setImageViewResource(R.id.widget_status_dot, dot)
                setTextViewText(R.id.widget_status, status)

                setImageViewResource(
                    R.id.widget_toggle,
                    if (syncEnabled) R.drawable.ic_widget_pause else R.drawable.ic_widget_play
                )
                setContentDescription(
                    R.id.widget_toggle,
                    context.getString(if (syncEnabled) R.string.notif_action_pause else R.string.notif_action_resume)
                )
                setOnClickPendingIntent(R.id.widget_toggle, actionIntent(context, 1, WidgetActionActivity.ACTION_TOGGLE_SYNC))

                setOnClickPendingIntent(
                    R.id.widget_header,
                    PendingIntent.getActivity(context, 2, Intent(context, MainActivity::class.java), PI_FLAGS)
                )
                setOnClickPendingIntent(R.id.widget_send_clipboard, actionIntent(context, 3, WidgetActionActivity.ACTION_CLIPBOARD))
                setOnClickPendingIntent(R.id.widget_send_files, actionIntent(context, 4, WidgetActionActivity.ACTION_PICK_FILES))
            }
        }

        private fun actionIntent(context: Context, requestCode: Int, action: String): PendingIntent =
            PendingIntent.getActivity(
                context,
                requestCode,
                Intent(context, WidgetActionActivity::class.java)
                    .setAction(action)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_NO_ANIMATION),
                PI_FLAGS
            )

        private const val PI_FLAGS = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
    }
}

/**
 * Invisible trampoline for the widget's buttons. Not exported, so no other
 * app can make Deskdrop push the clipboard.
 *
 * The clipboard is read here rather than in the service because Android 10+
 * only lets the focused app read it. Pause/resume also goes through here:
 * some OEM builds (ColorOS) silently drop a foreground-service start from a
 * widget PendingIntent while the app is frozen, but always allow an activity.
 */
class WidgetActionActivity : ComponentActivity() {

    private var clipboardPending = false

    private val pickFiles = registerForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        if (uris.isNotEmpty()) {
            // Hand off to the share sheet so picked files get the same
            // preview and device picker as files shared from the gallery.
            startActivity(
                Intent(this, DeskdropShareTarget::class.java)
                    .setAction(Intent.ACTION_SEND_MULTIPLE)
                    .setType("*/*")
                    .putParcelableArrayListExtra(Intent.EXTRA_STREAM, ArrayList(uris))
            )
        }
        finish()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        @Suppress("DEPRECATION")
        overridePendingTransition(0, 0)
        when (intent?.action) {
            ACTION_CLIPBOARD -> clipboardPending = true
            ACTION_PICK_FILES -> if (savedInstanceState == null) pickFiles.launch(arrayOf("*/*"))
            ACTION_TOGGLE_SYNC -> { toggleSync(); finish() }
            else -> finish()
        }
    }

    private fun toggleSync() {
        val enabled = getSharedPreferences(DeskdropService.PREFS_NAME, MODE_PRIVATE).getBoolean("sync_enabled", true)
        runCatching {
            ContextCompat.startForegroundService(
                this,
                Intent(this, DeskdropService::class.java)
                    .setAction(if (enabled) DeskdropService.ACTION_PAUSE_SYNC else DeskdropService.ACTION_RESUME_SYNC)
            )
        }.onFailure {
            android.util.Log.w("LinkAll", "Widget: could not toggle sync", it)
            Toast.makeText(this, "Couldn't reach Link All. Open the app and try again.", Toast.LENGTH_SHORT).show()
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus || !clipboardPending) return
        clipboardPending = false
        pushClipboard()
        finish()
    }

    private fun pushClipboard() {
        val prefs = getSharedPreferences(DeskdropService.PREFS_NAME, MODE_PRIVATE)
        val names = prefs.getStringSet("connected_names", emptySet()).orEmpty()
        val text = getSystemService(android.content.ClipboardManager::class.java)
            ?.primaryClip?.takeIf { it.itemCount > 0 }
            ?.getItemAt(0)?.coerceToText(this)?.toString()

        val message = when {
            names.isEmpty() -> "No devices connected"
            text.isNullOrBlank() -> "Clipboard is empty"
            else -> {
                val sent = runCatching {
                    ContextCompat.startForegroundService(
                        this,
                        Intent(this, DeskdropService::class.java)
                            .setAction(DeskdropService.ACTION_PUSH_CLIPBOARD)
                            .putExtra(DeskdropService.EXTRA_CLIPBOARD_TEXT, text)
                    )
                }.isSuccess
                when {
                    !sent -> "Couldn't send. Open Link All and try again."
                    names.size == 1 -> "Clipboard sent to ${names.first()}"
                    else -> "Clipboard sent to ${names.size} devices"
                }
            }
        }
        Toast.makeText(this, message, Toast.LENGTH_SHORT).show()
    }

    override fun finish() {
        super.finish()
        @Suppress("DEPRECATION")
        overridePendingTransition(0, 0)
    }

    companion object {
        const val ACTION_CLIPBOARD = "com.deskdrop.widget.CLIPBOARD"
        const val ACTION_PICK_FILES = "com.deskdrop.widget.PICK_FILES"
        const val ACTION_TOGGLE_SYNC = "com.deskdrop.widget.TOGGLE_SYNC"
    }
}
