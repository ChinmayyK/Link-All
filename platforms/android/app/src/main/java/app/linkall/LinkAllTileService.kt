package app.linkall

import app.linkall.ui.theme.*

import android.app.Activity
import android.content.Intent
import android.graphics.Typeface
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import androidx.annotation.ColorRes
import androidx.core.content.ContextCompat
import kotlin.math.roundToInt
import android.webkit.MimeTypeMap
import android.provider.OpenableColumns
import android.widget.ProgressBar
import android.content.res.ColorStateList
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.animation.*
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.linkall.ui.theme.AppTheme
import app.linkall.ui.theme.CRTheme

/**
 * Quick Settings tile — lets users toggle clipboard sync from the notification shade.
 *
 * Shows:
 *   - Active state: "Link All · Syncing"
 *   - Inactive state: "Link All · Paused"
 *
 * Long-press opens Link All settings.
 * Does NOT show clipboard content or peer data in the tile.
 */
class LinkAllTileService : TileService() {

    private val statusReceiver = object : android.content.BroadcastReceiver() {
        override fun onReceive(context: android.content.Context?, intent: Intent?) {
            refreshTile()
        }
    }

    override fun onStartListening() {
        super.onStartListening()
        
        // Register receiver to auto-update tile when LinkAllService broadcasts changes
        androidx.core.content.ContextCompat.registerReceiver(
            this,
            statusReceiver,
            android.content.IntentFilter(LinkAllService.ACTION_STATUS_CHANGED),
            androidx.core.content.ContextCompat.RECEIVER_NOT_EXPORTED
        )
        
        refreshTile()
    }

    override fun onStopListening() {
        super.onStopListening()
        try {
            unregisterReceiver(statusReceiver)
        } catch (e: Exception) {
            // Ignored
        }
    }

    override fun onClick() {
        super.onClick()
        toggleSync()
    }

    private fun refreshTile() {
        val tile = qsTile ?: return
        val prefs   = getSharedPreferences(LinkAllService.PREFS_NAME, MODE_PRIVATE)
        val isSyncEnabled = prefs.getBoolean("sync_enabled", true)
        val count   = prefs.getInt("connected_count", 0)

        tile.state = if (isSyncEnabled) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
        tile.label = "Link All Sync"

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            tile.subtitle = if (isSyncEnabled) {
                when {
                    count == 0   -> "Scanning"
                    count == 1   -> "1 device"
                    else         -> "$count devices"
                }
            } else {
                "Paused"
            }
        }

        tile.contentDescription = "Toggle Link All Discoverability"
        tile.updateTile()
    }

    private fun toggleSync() {
        val prefs = getSharedPreferences(LinkAllService.PREFS_NAME, MODE_PRIVATE)
        val isSyncEnabled = prefs.getBoolean("sync_enabled", true)
        
        val intent = Intent(this, LinkAllService::class.java).apply {
            action = if (isSyncEnabled) LinkAllService.ACTION_PAUSE_SYNC else LinkAllService.ACTION_RESUME_SYNC
        }
        runCatching {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                ContextCompat.startForegroundService(this, intent)
            } else {
                startService(intent)
            }
        }
        // State will update via broadcast, but we can do an optimistic UI update here
        val tile = qsTile ?: return
        tile.state = if (!isSyncEnabled) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            tile.subtitle = if (!isSyncEnabled) "Scanning" else "Paused"
        }
        tile.updateTile()
    }
}

/**
 * Share target — appears in Android's share sheet, letting users push
 * any shared text directly to Link All peers without opening the app.
 */
class LinkAllShareTarget : ComponentActivity() {

    private fun dp(v: Int): Int = (v * resources.displayMetrics.density).roundToInt()
    private fun c(@ColorRes id: Int): Int = ContextCompat.getColor(this, id)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        @Suppress("DEPRECATION")
        overridePendingTransition(0, 0)
        androidx.core.view.WindowCompat.setDecorFitsSystemWindows(window, false)
        window.setBackgroundDrawable(ColorDrawable(android.graphics.Color.TRANSPARENT))
        window.decorView.setBackgroundColor(android.graphics.Color.TRANSPARENT)
        window.statusBarColor = android.graphics.Color.TRANSPARENT
        window.navigationBarColor = android.graphics.Color.TRANSPARENT

        val sharedUris = when (intent?.action) {
            Intent.ACTION_SEND -> {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)?.let { arrayListOf(it) }
                } else {
                    @Suppress("DEPRECATION")
                    intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)?.let { arrayListOf(it) }
                }
            }
            Intent.ACTION_SEND_MULTIPLE -> {
                val items = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
                } else {
                    @Suppress("DEPRECATION")
                    intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)
                }
                items
            }
            else -> null
        }
        val sharedText = when {
            sharedUris.isNullOrEmpty() &&
                intent?.action == Intent.ACTION_SEND &&
                intent.hasExtra(Intent.EXTRA_TEXT) ->
                intent.getStringExtra(Intent.EXTRA_TEXT)
            else -> null
        }
        val sharedName = intent?.getStringExtra(Intent.EXTRA_TITLE)?.takeIf { it.isNotBlank() }

        if (!sharedText.isNullOrBlank()) {
            runCatching {
                ContextCompat.startForegroundService(this, Intent(this, LinkAllService::class.java).apply {
                    action = LinkAllService.ACTION_PUSH_TEXT
                    putExtra("text", sharedText)
                })
            }
            Toast.makeText(this, "Sent to your devices", Toast.LENGTH_SHORT).show()
            finish()
        } else if (!sharedUris.isNullOrEmpty()) {
            val peers = getSharedPreferences(LinkAllService.PREFS_NAME, MODE_PRIVATE)
                .peerSnapshots()
                .filter { it.isConnected }
            val prefs = getSharedPreferences(LinkAllService.PREFS_NAME, MODE_PRIVATE)
            // Picked by name in Android's share sheet: send straight to it.
            val chosenId = DeviceShareTargets.peerIdOf(
                intent?.getStringExtra(androidx.core.content.pm.ShortcutManagerCompat.EXTRA_SHORTCUT_ID)
            )
            val chosen = peers.find { it.id == chosenId }
            if (chosen != null) {
                sendFiles(sharedUris, sharedName, chosen.id) // finishes the activity
                return
            }
            val themeMode = app.linkall.ui.theme.themeModeOf(prefs)
            // A chosen device that has since gone offline is preselected instead.
            val lastUsedId = chosenId ?: prefs.getString("last_used_device_id", null)

            setContent {
                val isDark = app.linkall.ui.theme.isDarkFor(themeMode)
                AppTheme(useDarkTheme = isDark) {
                    app.linkall.ui.ShareSheet(
                        sharedUris = sharedUris,
                        peers = peers,
                        lastUsedDeviceId = lastUsedId,
                        isDark = isDark,
                        onCancel = { finish() },
                        onSend = { targetId -> sendFiles(sharedUris, sharedName, targetId) },
                        onOpenApp = {
                            startActivity(Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                            finish()
                        },
                    )
                }
            }
        } else {
            Toast.makeText(this, "Nothing to push", Toast.LENGTH_SHORT).show()
            finish()
        }
    }

    // The sheet animates itself in and out over a dimmed scrim; the default
    // activity slide on top of that looked like two things opening at once.
    override fun finish() {
        super.finish()
        @Suppress("DEPRECATION")
        overridePendingTransition(0, 0)
    }

    private fun sendFiles(sharedUris: List<Uri>, sharedName: String?, targetId: String?) {
        for (uri in sharedUris) {
            if (uri.scheme == "content") {
                runCatching {
                    contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
            }
        }
        val stringUris = sharedUris.map { it.toString() }
        val svc = Intent(this@LinkAllShareTarget, LinkAllService::class.java).apply {
            action = LinkAllService.ACTION_PUSH_SHARED_URI
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            
            if (sharedUris.isNotEmpty()) {
                val cd = android.content.ClipData.newRawUri("shared_uris", sharedUris[0])
                for (i in 1 until sharedUris.size) {
                    cd.addItem(android.content.ClipData.Item(sharedUris[i]))
                }
                clipData = cd
            }

            putStringArrayListExtra(
                LinkAllService.EXTRA_SHARED_URIS,
                ArrayList(stringUris)
            )
            sharedName?.let { putExtra(LinkAllService.EXTRA_SHARED_NAME, it) }
            targetId?.let { 
                putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, it)
                getSharedPreferences(LinkAllService.PREFS_NAME, MODE_PRIVATE).edit().putString("last_used_device_id", it).apply()
            }
        }
        val started = runCatching { ContextCompat.startForegroundService(this@LinkAllShareTarget, svc) }
            .onFailure { android.util.Log.w("LinkAll", "Share: could not hand files to the service", it) }
            .isSuccess
        Toast.makeText(
            this@LinkAllShareTarget,
            if (started) "Sending to Link All" else "Couldn't send. Open Link All and try again.",
            Toast.LENGTH_SHORT
        ).show()
        finish()
    }
}

