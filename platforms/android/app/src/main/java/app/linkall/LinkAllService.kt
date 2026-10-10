// Link All — Android Foreground Service
//
// Background execution strategy:
//   - Foreground service (mandatory, stays alive across screen-off + OEM killers)
//   - WakeLock (PARTIAL) held only during active event drain — released immediately after
//   - Doze/standby aware: heartbeat poll rate reduced in Battery Optimized mode
//   - Single IMPORTANCE_MIN persistent notification — silent, no heads-up, no badge
//   - Alerts channel (IMPORTANCE_DEFAULT) for trust requests + file receives only
//   - Zero per-clipboard-sync notifications — clipboard is ambient/invisible
//   - Notification actions: Pause Sync | Disconnect
//   - Activity feed (in-memory) replaces notification spam

package app.linkall

import android.app.*
import android.content.*
import android.content.res.Configuration
import android.Manifest
import android.content.ClipboardManager
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.Uri
import android.content.pm.ServiceInfo
import android.os.*
import android.provider.OpenableColumns
import android.provider.Settings
import android.util.Log
import android.webkit.MimeTypeMap
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import androidx.core.content.FileProvider
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import java.io.File
import java.io.FileOutputStream
import java.io.InputStream
import java.nio.charset.StandardCharsets
import java.security.MessageDigest
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.launch
import kotlinx.coroutines.cancel
import java.util.UUID

// ── JNI Bridge ────────────────────────────────────────────────────────────────
// The prebuilt .so exports Java_app_linkall_LinkAllJni_* symbols.
// We keep this object name to match — only user-visible strings are renamed.



// ── Activity feed model ───────────────────────────────────────────────────────



// ── Battery mode ──────────────────────────────────────────────────────────────

enum class BackgroundSyncMode {
    ALWAYS_ACTIVE,    // poll at full rate, keep WakeLock during drain
    BATTERY_OPTIMIZED // reduced poll rate, no WakeLock
}

// ── Service ───────────────────────────────────────────────────────────────────



class LinkAllService : Service() {
    private val backgroundExecutor = java.util.concurrent.Executors.newCachedThreadPool()


    companion object {
        private const val TAG = "LinkAll"
        const val PREFS_NAME = "linkall"

        // Guards engineHandle: readers hold it across JNI calls, stop() takes
        // the write lock before freeing the engine.
        val engineLock = java.util.concurrent.locks.ReentrantReadWriteLock()

        val quickSendContextFlow = kotlinx.coroutines.flow.MutableStateFlow<String?>(null)

        // Notification channels
        private const val CHAN_SERVICE = "cr_service"   // IMPORTANCE_MIN — silent persistent
        private const val CHAN_ALERTS  = "cr_alerts"    // IMPORTANCE_DEFAULT — trust/file/failure
        private const val CHAN_CALLS   = "cr_calls"     // IMPORTANCE_HIGH — incoming call banner
        private const val CHAN_PAIRING = "cr_pairing_v2"// IMPORTANCE_HIGH — dedicated pairing request

        // Notification IDs
        private const val NOTIF_ID_SERVICE           = 1001
        private const val NOTIF_ID_PAIRING_BASE      = 7000  // + (deviceId.hashCode() and 0xFFF), one per asking device
        private const val NOTIF_ID_FILE              = 1003
        private const val NOTIF_ID_FAILURE           = 1004
        private const val NOTIF_ID_CLIPBOARD_AVAILABLE = 1005
        private const val NOTIF_ID_FILE_BASE         = 2000  // + (tid.hashCode() and 0xFFF)
        private const val NOTIF_ID_CALL              = 3001  // incoming call banner

        // Intent actions
        const val ACTION_START              = "app.linkall.START"
        const val ACTION_STOP               = "app.linkall.STOP"
        /** The user swiped the service notification away. */
        private const val ACTION_SERVICE_NOTIFICATION_DISMISSED = "app.linkall.SERVICE_NOTIFICATION_DISMISSED"
        const val ACTION_PAUSE_SYNC         = "app.linkall.PAUSE_SYNC"
        const val ACTION_RESUME_SYNC        = "app.linkall.RESUME_SYNC"
        const val ACTION_DISCONNECT_ALL     = "app.linkall.DISCONNECT_ALL"
        const val ACTION_PUSH_TEXT          = "app.linkall.PUSH_TEXT"
        const val ACTION_PUSH_SHARED_URI    = "app.linkall.PUSH_SHARED_URI"
        /** Send a folder picked with ACTION_OPEN_DOCUMENT_TREE (tree URI in EXTRA_SHARED_URI). */
        const val ACTION_PUSH_FOLDER        = "app.linkall.PUSH_FOLDER"
        /** Transfer ids of a whole folder's row: "folder:" + batch id. */
        const val FOLDER_ROW_PREFIX         = "folder:"
        const val ACTION_SCAN_NOW           = "app.linkall.SCAN_NOW"
        const val ACTION_STATUS_CHANGED     = "app.linkall.STATUS_CHANGED"
        const val ACTION_SETTINGS_CHANGED   = "app.linkall.SETTINGS_CHANGED"  // re-read prefs live
        const val ACTION_PUSH_CLIPBOARD     = "app.linkall.PUSH_CLIPBOARD"    // send Android clipboard to peers
        const val ACTION_PUSH_NOTIFICATION  = "app.linkall.PUSH_NOTIFICATION"
        const val ACTION_APPLY_CLIPBOARD    = "app.linkall.APPLY_CLIPBOARD"
        const val ACTION_ACCEPT_FILE_TRANSFER = "app.linkall.ACCEPT_FILE_TRANSFER"
        const val ACTION_REJECT_FILE_TRANSFER = "app.linkall.REJECT_FILE_TRANSFER"
        const val ACTION_CANCEL_FILE_TRANSFER = "app.linkall.CANCEL_FILE_TRANSFER"
        const val ACTION_PAUSE_FILE_TRANSFER  = "app.linkall.PAUSE_FILE_TRANSFER"
        const val ACTION_RESUME_FILE_TRANSFER = "app.linkall.RESUME_FILE_TRANSFER"
        const val ACTION_START_SPEED_TEST     = "app.linkall.START_SPEED_TEST"
        const val ACTION_CONNECT_MANUAL     = "app.linkall.CONNECT_MANUAL"
        const val ACTION_TRUST_PEER         = "app.linkall.TRUST_PEER"
        const val ACTION_TRUST_PEER_FROM_QR = "app.linkall.TRUST_PEER_FROM_QR"
        const val ACTION_REJECT_PEER = "app.linkall.REJECT_PEER"
        const val ACTION_HANDLE_CALL_STATE = "app.linkall.HANDLE_CALL_STATE"
        /** How often a live call's state is repeated to peers; matches CALL_REFRESH in the core. */
        private const val CALL_REFRESH_MS = 10_000L
        const val ACTION_FORGET_PEER        = "app.linkall.FORGET_PEER"
        const val ACTION_SEND_PAIRING_REQUEST = "app.linkall.SEND_PAIRING_REQUEST"
        const val ACTION_RESPOND_TO_PAIRING = "app.linkall.RESPOND_TO_PAIRING"
        const val ACTION_CANCEL_PAIRING_REQUEST = "app.linkall.CANCEL_PAIRING_REQUEST"
        const val ACTION_DISCONNECT_PEER    = "app.linkall.DISCONNECT_PEER"
        const val ACTION_RECONNECT_PEER     = "app.linkall.RECONNECT_PEER"

        // Intent extras
        const val EXTRA_CLIPBOARD_TEXT      = "clipboard_text"
        const val EXTRA_CONTENT_HASH        = "content_hash"   // SHA-256 hex; used for full-content apply via engine
        const val EXTRA_TOKEN               = "token"          // QR Code Auth Token
        const val EXTRA_FINGERPRINT         = "fingerprint"    // QR Code: computer's key
        const val EXTRA_TRANSFER_ID         = "transfer_id"
        const val EXTRA_SHARED_URI          = "shared_uri"
        const val EXTRA_SHARED_URIS         = "shared_uris"
        const val EXTRA_SHARED_NAME         = "shared_name"
        const val EXTRA_TARGET_DEVICE_ID    = "target_device_id"
        const val EXTRA_NOTIFICATION_ID     = "notification_id"
        const val EXTRA_NOTIFICATION_PKG    = "notification_pkg"
        const val EXTRA_NOTIFICATION_TITLE  = "notification_title"
        const val EXTRA_NOTIFICATION_TEXT   = "notification_text"
        const val PREF_SERVICE_RUNNING      = "service_running"

        // Poll intervals
        private const val POLL_FULL_MS      = 20L    // 50 Hz — always-active mode
        private const val POLL_REDUCED_MS   = 100L   // 10 Hz — battery-optimized mode
        private const val CLIP_FULL_MS      = 200L   // clipboard check interval (full)
        private const val CLIP_REDUCED_MS   = 500L   // clipboard check interval (reduced)
        private const val CLIP_UNREADABLE_MS = 2_000L // while the clipboard can't be read (background, Android 10+)
        private const val ACTIVITY_FEED_MAX = 100

        // NSD (Network Service Discovery) — mirrors the mDNS service type used by the Rust engine
        private const val NSD_SERVICE_TYPE       = "_deskdrop._tcp."
        internal const val DEFAULT_LINKALL_PORT = 47823

        // Connect by IP: result broadcast for the dialog, plus recent addresses.
        const val ACTION_MANUAL_CONNECT_RESULT = "app.linkall.MANUAL_CONNECT_RESULT"
        const val EXTRA_MANUAL_HOST  = "manual_host"
        const val EXTRA_MANUAL_OK    = "manual_ok"
        const val EXTRA_MANUAL_ERROR = "manual_error"
        private const val PREF_RECENT_MANUAL = "recent_manual_addresses"
        private const val MAX_RECENT_MANUAL  = 5

        fun recentManualAddresses(context: Context): List<String> =
            context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
                .getString(PREF_RECENT_MANUAL, null)
                ?.split("\n")?.filter { it.isNotBlank() }.orEmpty()


    }

    // ── State ─────────────────────────────────────────────────────────────────

    private var engineHandle: Long = 0L
    private val handler = Handler(Looper.getMainLooper())
    private var lastClipboardSignature: String? = null
    private var serviceStartTime = 0L

    private fun executeInBackgroundWithWakeLock(tag: String, block: () -> Unit) {
        backgroundExecutor.execute {
            val pm = getSystemService(Context.POWER_SERVICE) as? android.os.PowerManager
            val wl = pm?.newWakeLock(android.os.PowerManager.PARTIAL_WAKE_LOCK, "LinkAll::$tag")
            wl?.acquire(30000L) // Maximum 30 seconds
            try {
                block()
            } finally {
                if (wl?.isHeld == true) {
                    try { wl.release() } catch (e: Exception) {}
                }
            }
        }
    }
    private var suppressNext = false
    private val connectedPeerIds = java.util.concurrent.ConcurrentHashMap<String, String>()  // deviceId → displayName
    private val engineStarted = AtomicBoolean(false)
    private val notificationManager by lazy { getSystemService(NotificationManager::class.java) }

    // NSD — peer discovery on Android (replaces stubbed Rust mDNS)
    private var nsdRegistrationListener: NsdManager.RegistrationListener? = null
    private var nsdDiscoveryListener: NsdManager.DiscoveryListener? = null
    private val isNsdRegistered = AtomicBoolean(false)
    private val pendingNsdUnregister = AtomicBoolean(false)
    private var currentNsdResolveTimeoutRunnable: Runnable? = null
    private var delayedNetworkAction: Runnable? = null

    // Self-connection filter: first 8 chars of our UUID match the NSD service name suffix.
    // Set once the engine starts; used in makeResolveListener() to skip our own advertisement.
    private var myDeviceUuidPrefix: String? = null
    private var myDeviceId: String? = null
    private var pendingManualConnectIp: String? = null
    private var pendingManualConnectPort: Int = DEFAULT_LINKALL_PORT

    // Actual NSD service name as reported by onServiceRegistered (may differ from requested
    // if Android resolved a collision by appending " (2)" etc.).
    private var myActualNsdName: String? = null

    // NSD resolution queue to prevent FAILURE_ALREADY_ACTIVE
    private val pendingNsdResolves = java.util.concurrent.ConcurrentLinkedQueue<android.net.nsd.NsdServiceInfo>()
    private val isResolvingNsd = java.util.concurrent.atomic.AtomicBoolean(false)

    // Network change callback — restarts NSD when the device switches WiFi networks
    // or reconnects after being offline (e.g. waking from sleep, roaming).
    private var networkCallback: ConnectivityManager.NetworkCallback? = null
    private var pairingReceiverRegistered = false



    // Was declared but never actually scheduled anything - repurposed below
    // for the engine health check (startEngineHealthCheck).
    private var heartbeatHandler = android.os.Handler(android.os.Looper.getMainLooper())
    private var engineHealthCheckRunnable: Runnable? = null

    private val pairingResultReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (intent?.action != PairingActivity.ACTION_PAIRING_RESULT) return
            val deviceId = intent.getStringExtra(PairingActivity.EXTRA_DEVICE_ID) ?: return
            val approved = intent.getBooleanExtra(PairingActivity.EXTRA_APPROVED, false)
            engineLock.readLock {
                val h = engineHandle
                if (h != 0L) {
                    val result = LinkAllJni.respondToPairing(h, deviceId, approved)
                    Log.i(TAG, "Pairing result for $deviceId approved=$approved result=$result")
                    notificationManager.cancel(pairingNotifId(deviceId))
                    persistStatus()
                }
            }
        }
    }

    // NSD retry after all peers disconnect — exponential backoff, max 60 s.
    private val nsdRetryCount = AtomicLong(0L)
    private var nsdRetryRunnable: Runnable? = null

    // WakeLock — keeps the CPU awake while the foreground service is active so tokio can answer heartbeats & receive files with the screen off.
    private var wakeLock: android.os.PowerManager.WakeLock? = null

    // MulticastLock — held for the lifetime of the service.
    // Many OEM WiFi drivers (Samsung, Xiaomi, OnePlus, Realme) suppress
    // multicast/mDNS packets in hardware unless this lock is held.
    // Without it, NSD registration succeeds but packets are silently dropped,
    // so the Mac never sees the Android advertisement and vice versa.
    private var multicastLock: android.net.wifi.WifiManager.MulticastLock? = null
    private val clipboardManager: ClipboardManager by lazy {
        getSystemService(CLIPBOARD_SERVICE) as ClipboardManager
    }

    private val smsReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action != android.provider.Telephony.Sms.Intents.SMS_RECEIVED_ACTION) return

            // Check settings first
            val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            if (!prefs.getBoolean("auto_forward_sms", false)) return

            engineLock.readLock {
                val h = engineHandle
                if (h != 0L && hasConnectedPeers()) {
                    val msgs = android.provider.Telephony.Sms.Intents.getMessagesFromIntent(intent)
                    for (msg in msgs) {
                        val body = msg.messageBody
                        val codeMatch = Regex("\\b\\d{4,8}\\b").find(body ?: "")
                        if (codeMatch != null) {
                            LinkAllJni.pushText(h, codeMatch.value)
                            Log.i(TAG, "Pushed 2FA code")
                            break
                        }
                    }
                }
            }
        }
    }

    private val screenshotObserver = object : android.database.ContentObserver(android.os.Handler(android.os.Looper.getMainLooper())) {
        override fun onChange(selfChange: Boolean, uri: android.net.Uri?) {
            super.onChange(selfChange, uri)
            engineLock.readLock {
                val h = engineHandle
                
                // Check settings first
                val prefs = getSharedPreferences(PREFS_NAME, MODE_PRIVATE)
                if (!BuildConfig.FULL_PERMISSIONS || !prefs.getBoolean("auto_forward_screenshots", false)) return@readLock

                if (h == 0L || !hasConnectedPeers()) return@readLock

                try {
                    // Always query the main URI and sort by date
                    val cursor = contentResolver.query(
                        android.provider.MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
                        arrayOf(android.provider.MediaStore.Images.Media.DATA),
                        null, null,
                        android.provider.MediaStore.Images.Media.DATE_ADDED + " DESC"
                    )
                    cursor?.use {
                        if (it.moveToFirst()) {
                            val dataIndex = it.getColumnIndexOrThrow(android.provider.MediaStore.Images.Media.DATA)
                            val path = it.getString(dataIndex)
                            if (path.contains("Screenshot", ignoreCase = true)) {
                                // Check if it's new
                                val file = java.io.File(path)
                                if (file.exists() && System.currentTimeMillis() - file.lastModified() < 10000) {
                                    // It's a recent screenshot! Read the file and push it.
                                    val bytes = file.readBytes()
                                    LinkAllJni.pushImage(h, "image/png", bytes)
                                    Log.i(TAG, "Pushed new screenshot: $path")
                                }
                            }
                        }
                    }
                } catch (e: Exception) {
                    Log.w("LinkAllService", "Failed to query media store", e)
                }
            }
        }
    }

    // Cached prefs (reloaded on relevant changes)
    private fun prefs() = getSharedPreferences(PREFS_NAME, MODE_PRIVATE)
    private fun isSyncEnabled()           = prefs().getBoolean("sync_enabled", true)
    private fun isClipboardNotifyEnabled()= prefs().getBoolean("notify_on_remote_copy", false)

    // ── Engine Threading ──────────────────────────────────────────────────────
    private val serviceScope = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.SupervisorJob() + kotlinx.coroutines.Dispatchers.IO)
    @Volatile private var isRunning = true
    private var eventDrainThread: Thread? = null
    private val engineLock get() = Companion.engineLock
    private inline fun <T> java.util.concurrent.locks.ReentrantReadWriteLock.readLock(action: () -> T): T {
        val rl = readLock()
        rl.lock()
        return try {
            action()
        } finally {
            rl.unlock()
        }
    }

    /**
     * Calls [action] with the running engine's handle, or returns null when
     * the engine is stopped. The read lock keeps onDestroy from freeing the
     * engine during the call; a freed handle crashes the whole app.
     * Keep [action] to the JNI call: waiting on the main thread inside it
     * deadlocks against onDestroy's write lock.
     */
    private inline fun <T> withEngine(action: (Long) -> T): T? = engineLock.readLock {
        val h = engineHandle
        if (h != 0L) action(h) else null
    }
    private fun syncMode(): BackgroundSyncMode =
        if (prefs().getString("sync_mode", "always") == "battery") BackgroundSyncMode.BATTERY_OPTIMIZED
        else BackgroundSyncMode.ALWAYS_ACTIVE

    private val pollInterval  get() = if (syncMode() == BackgroundSyncMode.ALWAYS_ACTIVE) POLL_FULL_MS  else POLL_REDUCED_MS
    private val clipInterval  get() = if (syncMode() == BackgroundSyncMode.ALWAYS_ACTIVE) CLIP_FULL_MS  else CLIP_REDUCED_MS

    // ── Screen / Doze wake receiver ───────────────────────────────────────────
    private val screenReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            val pm = context.getSystemService(Context.POWER_SERVICE) as android.os.PowerManager
            when (intent.action) {
                Intent.ACTION_SCREEN_ON -> {
                    Log.i(TAG, "Device woke up (Screen ON) — forcing reconnect/discovery")
                    handler.post {
                        val h = engineHandle
                        if (h != 0L) {
                            backgroundExecutor.execute { withEngine { live -> LinkAllJni.notifySleepState(live, false) } }
                        }
                        // Peers that survived the screen-off need no
                        // rediscovery; restarting NSD and redialling every
                        // known peer ~100 times a day was pure overhead.
                        if (!hasConnectedPeers()) {
                            restartDiscoveryNow()
                            if (h != 0L) {
                                backgroundExecutor.execute { withEngine { live -> LinkAllJni.notifyNetworkRestored(live) } }
                            }
                        }
                    }
                }
                Intent.ACTION_SCREEN_OFF -> {
                    Log.i(TAG, "Screen OFF: Notifying Rust engine to relax heartbeats")
                    val h = engineHandle
                    if (h != 0L) {
                        backgroundExecutor.execute { withEngine { live -> LinkAllJni.notifySleepState(live, true) } }
                    }
                    // The 2-minute idle release runs on uptime, which stops
                    // while the CPU sleeps, so it could leave the multicast
                    // lock (every LAN broadcast wakes the CPU) held all night.
                    // Drop discovery locks now unless bytes are moving.
                    handler.post {
                        if (!isMovingData()) {
                            idleLockReleaseRunnable?.let { handler.removeCallbacks(it) }
                            releaseMulticastLock()
                            releaseWifiLock()
                        }
                    }
                }
                android.os.PowerManager.ACTION_DEVICE_IDLE_MODE_CHANGED -> {
                    // Doze maintenance windows end idle mode many times a
                    // night; only rediscover if we actually lost everyone.
                    if (!pm.isDeviceIdleMode && !hasConnectedPeers()) {
                        Log.i(TAG, "Device woke up (Doze ended) — forcing reconnect/discovery")
                        handler.post {
                            restartDiscoveryNow()
                            val h = engineHandle
                            if (h != 0L) {
                                backgroundExecutor.execute { withEngine { live -> LinkAllJni.notifyNetworkRestored(live) } }
                            }
                        }
                    }
                }
            }
        }
    }

    // ── Custom broadcast receiver ─────────────────────────────────────────────
    private val customReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            val action = intent.action
            Log.i(TAG, "Custom broadcast received: $action")
            if (action == "app.linkall.CUSTOM_BROADCAST") {
                val message = intent.getStringExtra("message") ?: "ping"
                Log.i(TAG, "Custom broadcast message: $message")
                
                val h = engineHandle
                if (h != 0L) {
                    // Example action: broadcast a generic notification/warning to peers or local logs
                    withEngine { live -> LinkAllJni.pushNotification(live, "custom_br", context.packageName, "Custom Broadcast", message) }
                }
            }
        }
    }

    // ── Service lifecycle ─────────────────────────────────────────────────────

    override fun onCreate() {
        super.onCreate()
        ActivityFeedManager.attach(this)
        serviceStartTime = System.currentTimeMillis()
        createNotificationChannels()
        registerPairingReceiver()
        registerCallStateCallback()
        
        // Register SMS receiver (the Play build has no RECEIVE_SMS permission)
        if (BuildConfig.FULL_PERMISSIONS) {
            val filter = IntentFilter(android.provider.Telephony.Sms.Intents.SMS_RECEIVED_ACTION)
            registerReceiver(smsReceiver, filter)
        }

        // Register screenshot observer
        contentResolver.registerContentObserver(
            android.provider.MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
            true,
            screenshotObserver
        )

        // Register screen/doze receiver
        val screenFilter = IntentFilter().apply {
            addAction(Intent.ACTION_SCREEN_ON)
            addAction(Intent.ACTION_SCREEN_OFF)
            addAction(android.os.PowerManager.ACTION_DEVICE_IDLE_MODE_CHANGED)
        }
        registerReceiver(screenReceiver, screenFilter)
        
        val customFilter = IntentFilter("app.linkall.CUSTOM_BROADCAST")
        ContextCompat.registerReceiver(this, customReceiver, customFilter, ContextCompat.RECEIVER_NOT_EXPORTED)

        setServiceRunning(true)
        startEngineHealthCheck()
    }

    // onStartCommand's `if (!engineStarted.getAndSet(true))` block only ever
    // runs once per service lifetime - if the Rust engine dies afterward
    // (engineHandle drops to 0) while engineStarted stays true, nothing was
    // watching, and every future onStartCommand call took the "already
    // running" branch instead of restarting it. The only way to recover was
    // for the user to manually kill and reopen the app (a fresh process
    // resets engineStarted), which is exactly the "phone shows offline"
    // symptom this session traced back here. This periodically checks for
    // that specific dead-handle-but-marked-started state and, if found,
    // resets the flag and re-delivers a start intent to this same service -
    // which runs through the existing, already-correct startup path in
    // onStartCommand rather than duplicating it.
    private fun startEngineHealthCheck() {
        val intervalMs = 45_000L
        val runnable = object : Runnable {
            override fun run() {
                try {
                    if (engineStarted.get() && engineHandle == 0L) {
                        Log.w(TAG, "Engine health check: handle is dead but engineStarted=true; restarting engine")
                        engineStarted.set(false)
                        startService(Intent(this@LinkAllService, LinkAllService::class.java))
                    }
                } catch (e: Exception) {
                    Log.e(TAG, "Engine health check failed", e)
                }
                heartbeatHandler.postDelayed(this, intervalMs)
            }
        }
        engineHealthCheckRunnable = runnable
        heartbeatHandler.postDelayed(runnable, intervalMs)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Once in the foreground the service stays there, and Android needs no
        // new startForeground for later starts. Calling it on every start
        // re-posted the notification each time any app posted one (the
        // notification relay starts this service), undoing the user's swipe.
        try {
            if (intent?.action != ACTION_STOP && !isInForeground) {
                startForegroundCompat(buildForegroundNotification())
            }
        } catch (e: Exception) {
            Log.e(TAG, "Early startForegroundCompat failed", e)
        }

        when (intent?.action) {
            ACTION_STOP         -> { shutdownAndStop(); return START_NOT_STICKY }
            ACTION_SERVICE_NOTIFICATION_DISMISSED -> {
                serviceNotificationDismissed = true
                return START_STICKY
            }

            // Settings changed live (e.g. sync toggle from SettingsActivity).
            // Re-read prefs and push them to the engine if possible.
            ACTION_SETTINGS_CHANGED -> {
                applySettingsToEngine()
                // Phone access may have just been granted along with Calls.
                registerCallStateCallback()
                return START_STICKY
            }

            // User tapped "Send clipboard to Mac" on the dashboard.
            ACTION_PUSH_CLIPBOARD -> {
                val h = engineHandle
                if (h != 0L) {
                    if (!hasConnectedPeers()) {
                        Log.i(TAG, "PUSH_CLIPBOARD ignored: no connected peers")
                        return START_STICKY
                    }
                    val cm = getSystemService(android.content.ClipboardManager::class.java)
                    val explicitText = intent.getStringExtra(EXTRA_CLIPBOARD_TEXT)
                    val text = explicitText ?: cm.primaryClip?.getItemAt(0)
                        ?.coerceToText(this)?.toString()
                    if (!text.isNullOrBlank()) {
                        val result = withEngine { live -> LinkAllJni.pushText(live, text) }
                        Log.i(TAG, "PUSH_CLIPBOARD: result=$result len=${text.length}")
                        if (result == 0) {
                            broadcastActivityUpdated()
                        }
                        // Hide quick context once sent
                        quickSendContextFlow.value = null
                    } else {
                        Log.w(TAG, "PUSH_CLIPBOARD: clipboard is empty")
                    }
                }
                return START_STICKY
            }
            ACTION_SCAN_NOW -> {
                restartDiscoveryNow()
                return START_STICKY
            }
            ACTION_PAUSE_SYNC   -> { setSyncEnabled(false); return START_STICKY }
            ACTION_RESUME_SYNC  -> { setSyncEnabled(true);  return START_STICKY }
            ACTION_DISCONNECT_ALL -> { disconnectAllPeers(); return START_STICKY }
            ACTION_CONNECT_MANUAL -> {
                val host = intent?.getStringExtra("ip")?.trim()
                val port = intent?.getIntExtra("port", DEFAULT_LINKALL_PORT) ?: DEFAULT_LINKALL_PORT
                if (!host.isNullOrBlank()) {
                    if (engineHandle != 0L) {
                        connectManual(host, port)
                    } else {
                        Log.i(TAG, "Engine not ready, queuing manual connect to $host:$port")
                        pendingManualConnectIp = host
                        pendingManualConnectPort = port
                    }
                }
                return START_STICKY
            }
            ACTION_RECONNECT_PEER -> {
                val targetId = intent?.getStringExtra(EXTRA_TARGET_DEVICE_ID)
                if (!targetId.isNullOrBlank() && engineHandle != 0L) {
                    val h = engineHandle
                    serviceScope.launch {
                        withEngine { live -> LinkAllJni.reconnectPeer(live, targetId) }
                        restartDiscoveryNow()
                        Log.i(TAG, "Reconnecting to peer $targetId & restarted discovery")
                    }
                } else {
                    Log.e(TAG, "Failed to reconnect: targetId=$targetId, engineHandle=$engineHandle")
                }
                return START_STICKY
            }
            ACTION_TRUST_PEER -> handleTrustPeer(intent)
            ACTION_TRUST_PEER_FROM_QR -> handleTrustPeerFromQr(intent)
            ACTION_REJECT_PEER -> handleRejectPeer(intent)
            ACTION_HANDLE_CALL_STATE -> handleCallStateIntent(intent)
            ACTION_FORGET_PEER        -> {
                val deviceId = intent?.getStringExtra(EXTRA_TARGET_DEVICE_ID) ?: return START_STICKY
                val h = engineHandle
                if (h != 0L) {
                    serviceScope.launch {
                        val result = withEngine { live -> LinkAllJni.forgetPeer(live, deviceId) }
                        Log.i(TAG, "Manual forget request for $deviceId: result=$result")
                        persistStatus()
                    }
                }
                // Also eagerly remove from shared preferences so UI updates immediately
                val prefs = prefs()
                val peersStr = prefs.getString(PREF_PEER_SNAPSHOTS_JSON, "[]")
                try {
                    val arr = org.json.JSONArray(peersStr)
                    val newArr = org.json.JSONArray()
                    for (i in 0 until arr.length()) {
                        val obj = arr.getJSONObject(i)
                        if (obj.optString("id") != deviceId) {
                            newArr.put(obj)
                        }
                    }
                    prefs.edit().putString(PREF_PEER_SNAPSHOTS_JSON, newArr.toString()).apply()
                    sendBroadcast(Intent(ACTION_STATUS_CHANGED).setPackage(packageName))
                } catch (e: Exception) {
                    Log.e(TAG, "Failed to update peers JSON on forget", e)
                }
                return START_STICKY
            }

            ACTION_SEND_PAIRING_REQUEST -> {
                val deviceId = intent?.getStringExtra(EXTRA_TARGET_DEVICE_ID) ?: return START_STICKY
                val h = engineHandle
                if (h != 0L) {
                    serviceScope.launch {
                        val result = withEngine { live -> LinkAllJni.sendPairingRequest(live, deviceId) }
                        Log.i(TAG, "Manual pairing request for $deviceId: result=$result")
                        persistStatus()
                    }
                }
                return START_STICKY
            }
            ACTION_CANCEL_PAIRING_REQUEST -> {
                val deviceId = intent?.getStringExtra(EXTRA_TARGET_DEVICE_ID) ?: return START_STICKY
                val h = engineHandle
                if (h != 0L) {
                    serviceScope.launch {
                        val result = withEngine { live -> LinkAllJni.cancelPairingRequest(live, deviceId) }
                        Log.i(TAG, "Cancelled pairing request to $deviceId: result=$result")
                        persistStatus()
                    }
                }
                return START_STICKY
            }
            ACTION_RESPOND_TO_PAIRING -> {
                val deviceId = intent?.getStringExtra(EXTRA_TARGET_DEVICE_ID) ?: return START_STICKY
                val accepted = intent?.getBooleanExtra(PairingActivity.EXTRA_APPROVED, false) ?: false
                val h = engineHandle
                if (h != 0L) {
                    serviceScope.launch {
                        val result = withEngine { live -> LinkAllJni.respondToPairing(live, deviceId, accepted) }
                        Log.i(TAG, "Pairing response for $deviceId accepted=$accepted result=$result")
                        persistStatus()
                        notificationManager.cancel(pairingNotifId(deviceId))
                        sendBroadcast(Intent("app.linkall.CLOSE_PAIRING_UI").apply {
                            setPackage(packageName)
                            putExtra(PairingActivity.EXTRA_DEVICE_ID, deviceId)
                        })
                        if (accepted) {
                            runCatching {
                                val toDashboard = Intent(this@LinkAllService, MainActivity::class.java).apply {
                                    addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
                                }
                                startActivity(toDashboard)
                            }
                        }
                    }
                }
                return START_STICKY
            }
            ACTION_DISCONNECT_PEER -> {
                val deviceId = intent?.getStringExtra(EXTRA_TARGET_DEVICE_ID) ?: return START_STICKY
                val h = engineHandle
                if (h != 0L) {
                    serviceScope.launch {
                        val result = withEngine { live -> LinkAllJni.disconnectPeer(live, deviceId) }
                        Log.i(TAG, "Manual disconnect request for $deviceId: result=$result")
                        persistStatus()
                    }
                }
                return START_STICKY
            }

            // Timeline-first: user tapped "Apply" on a notification or feed item.
            // Prefer hash-based apply (full content via engine) over truncated preview text.
            ACTION_APPLY_CLIPBOARD -> {
                val hash = intent.getStringExtra(EXTRA_CONTENT_HASH)
                val text = intent.getStringExtra(EXTRA_CLIPBOARD_TEXT)
                if (engineHandle != 0L) {
                    val cm = getSystemService(ClipboardManager::class.java)
                    suppressNext = true
                    if (!hash.isNullOrBlank()) {
                        // Engine holds the full content by hash — apply without truncation.
                        val result = withEngine { live -> LinkAllJni.applyClipboardByHash(live, hash) }
                        if (result != 1 && !text.isNullOrBlank()) {
                            // Hash not found (e.g. engine restarted) — fall back to text.
                            cm.setPrimaryClip(ClipData.newPlainText("Link All", text))
                        }
                    } else if (!text.isNullOrBlank()) {
                        cm.setPrimaryClip(ClipData.newPlainText("Link All", text))
                    } else {
                        return START_STICKY
                    }
                    notificationManager.cancel(NOTIF_ID_CLIPBOARD_AVAILABLE)
                    broadcastActivityUpdated()
                }
                return START_STICKY
            }

            // File transfer: user tapped Accept in notification.
            ACTION_ACCEPT_FILE_TRANSFER -> {
                val tid = intent.getStringExtra(EXTRA_TRANSFER_ID) ?: return START_STICKY
                if (engineHandle != 0L) {
                    withEngine { live -> LinkAllJni.acceptFileTransfer(live, tid) }
                    notificationManager.cancel(transferNotifId(tid))
                }
                return START_STICKY
            }

            // File transfer: user tapped Reject in notification.
            ACTION_REJECT_FILE_TRANSFER -> {
                val tid = intent.getStringExtra(EXTRA_TRANSFER_ID) ?: return START_STICKY
                if (engineHandle != 0L) {
                    withEngine { live -> LinkAllJni.rejectFileTransfer(live, tid) }
                    notificationManager.cancel(transferNotifId(tid))
                }
                // The core sends no local event for a reject, so the entry
                // would otherwise linger and keep the transfer Wi-Fi lock held.
                TransferManager.activeTransfers.remove(tid)
                TransferManager.publishActiveTransfers(force = true)
                syncTransferWifiLock()
                return START_STICKY
            }

            ACTION_CANCEL_FILE_TRANSFER -> {
                val tid = intent.getStringExtra(EXTRA_TRANSFER_ID) ?: return START_STICKY
                if (engineHandle != 0L) {
                    if (tid.startsWith(FOLDER_ROW_PREFIX)) {
                        val handle = engineHandle
                        backgroundExecutor.execute {
                            withEngine { live -> LinkAllJni.cancelFolder(live, tid.removePrefix(FOLDER_ROW_PREFIX)) }
                        }
                        TransferManager.activeTransfers.remove(tid)
                        TransferManager.publishActiveTransfers(force = true)
                    } else {
                        withEngine { live -> LinkAllJni.cancelFileTransfer(live, tid) }
                    }
                    notificationManager.cancel(transferNotifId(tid))
                }
                return START_STICKY
            }

            ACTION_PAUSE_FILE_TRANSFER -> {
                val tid = intent.getStringExtra(EXTRA_TRANSFER_ID) ?: return START_STICKY
                if (engineHandle != 0L) {
                    withEngine { live -> LinkAllJni.pauseFileTransfer(live, tid) }
                }
                return START_STICKY
            }

            ACTION_RESUME_FILE_TRANSFER -> {
                val tid = intent.getStringExtra(EXTRA_TRANSFER_ID) ?: return START_STICKY
                if (engineHandle != 0L) {
                    withEngine { live -> LinkAllJni.resumeFileTransfer(live, tid) }
                }
                return START_STICKY
            }
            
            ACTION_START_SPEED_TEST -> {
                val deviceId = intent.getStringExtra("device_id") ?: return START_STICKY
                if (engineHandle != 0L) {
                    withEngine { live -> LinkAllJni.startSpeedTest(live, deviceId, 10) }
                }
                return START_STICKY
            }
            // Handled after the engine start below.
            ACTION_PUSH_TEXT, ACTION_PUSH_SHARED_URI, ACTION_PUSH_NOTIFICATION -> Unit
            else -> Log.w(TAG, "Unknown action: ${intent?.action}")
        }

        // Several actions fall through to here (push text/notification/shared
        // file, trust/call handling, plain app-open starts). Foreground was
        // already (re)attached at the top of this method, and network locks
        // are only taken for real discovery or transfer work - taking them
        // here re-armed them for every notification any app posted.
        return try {
            setServiceRunning(true)

            if (!engineStarted.getAndSet(true)) {
                val deviceName = resolvedDeviceName()
                val dataDir = File(filesDir, "linkall").also { it.mkdirs() }.absolutePath
                val fileSaveDir = receiveStagingDir()
                LinkAllJni.initContext(applicationContext)
                engineHandle = LinkAllJni.start(
                    deviceName,
                    0,
                    dataDir,
                    fileSaveDir.absolutePath
                )

                if (engineHandle == 0L) {
                    Log.e(TAG, "Rust engine failed to start")
                    setServiceRunning(false)
                    stopSelf()
                    return START_NOT_STICKY
                }

                applySettingsToEngine()

                Log.i(TAG, "Engine started — $deviceName")
                startEventDrainThread()
                acquireContinuousLocks()
                // Cache our own UUID prefix so NSD can filter self-connections.
                myDeviceId = withEngine { live -> LinkAllJni.getDeviceId(live) }
                myDeviceUuidPrefix = myDeviceId?.take(8)
                startNsdDiscovery()   // advertise + browse so the Mac can find us
                registerNetworkCallback() // restart NSD on WiFi changes
                // call continuity: receiver is statically registered now
                startBatteryMonitor()     // F20: relay battery status to peers
                startStorageMonitor()     // relay storage status to peers
                persistStatus()
            } else {
                // Engine was already running — permission may have just been granted.
                // A null action is the app being opened: discovery matters now.
                if (intent?.action == null) acquireContinuousLocks()
                startBatteryMonitor()
            }

            // Process any pending manual connect that was queued before engine started
            val pIp = pendingManualConnectIp
            if (pIp != null && engineHandle != 0L) {
                pendingManualConnectIp = null
                val pPort = pendingManualConnectPort
                Log.i(TAG, "Processing pending manual connect to $pIp:$pPort")
                connectManual(pIp, pPort)
            }

            if (intent?.action == ACTION_PUSH_TEXT) {
                intent.getStringExtra("text")?.takeIf { it.isNotBlank() }?.let { text ->
                    if (engineHandle != 0L && hasConnectedPeers()) {
                        withEngine { live -> LinkAllJni.pushText(live, text) }
                    } else if (engineHandle != 0L) {
                        Log.i(TAG, "PUSH_TEXT ignored: no connected peers")
                    } else {
                        Unit
                    }
                }
            }

            // Sent by LinkAllNotificationListener for every notification the phone posts.
            if (intent?.action == ACTION_PUSH_NOTIFICATION &&
                prefs().getBoolean("notification_mirroring", false) &&
                engineHandle != 0L && hasConnectedPeers()
            ) {
                withEngine { live -> LinkAllJni.pushNotification(
                    live,
                    intent.getStringExtra(EXTRA_NOTIFICATION_ID) ?: "",
                    intent.getStringExtra(EXTRA_NOTIFICATION_PKG) ?: "",
                    intent.getStringExtra(EXTRA_NOTIFICATION_TITLE) ?: "",
                    intent.getStringExtra(EXTRA_NOTIFICATION_TEXT) ?: "",
                ) }
            }

            if (intent?.action == ACTION_PUSH_FOLDER) {
                val treeUri = intent.getStringExtra(EXTRA_SHARED_URI)?.let { runCatching { Uri.parse(it) }.getOrNull() }
                val targetDeviceId = intent.getStringExtra(EXTRA_TARGET_DEVICE_ID)
                if (treeUri != null && engineHandle != 0L && hasConnectedPeers()) {
                    backgroundExecutor.execute { sendFolderTree(treeUri, targetDeviceId) }
                }
            }

            if (intent?.action == ACTION_PUSH_SHARED_URI) {
                val rawUri = intent.getStringExtra(EXTRA_SHARED_URI)
                val rawUris = intent.getStringArrayListExtra(EXTRA_SHARED_URIS)
                val preferredName = intent.getStringExtra(EXTRA_SHARED_NAME)
                val targetDeviceId = intent.getStringExtra(EXTRA_TARGET_DEVICE_ID)
                val uriStrings = buildList {
                    if (!rawUri.isNullOrBlank()) add(rawUri)
                    rawUris?.filter { it.isNotBlank() }?.let { addAll(it) }
                }
                if (uriStrings.isNotEmpty() && engineHandle != 0L) {
                    if (!hasConnectedPeers()) {
                        Log.i(TAG, "PUSH_SHARED_URI ignored: no connected peers")
                    } else if (targetDeviceId != null && !isPeerConnected(targetDeviceId)) {
                        Log.w(TAG, "PUSH_SHARED_URI ignored: target peer is no longer connected")
                    } else {
                        backgroundExecutor.execute {
                            sendSharedUris(uriStrings, preferredName, targetDeviceId)
                        }
                    }
                }
            }

            START_STICKY
        } catch (ex: Throwable) {
            Log.e(TAG, "onStartCommand failed", ex)
            setServiceRunning(false)
            stopSelf()
            START_NOT_STICKY
        }
    }

    override fun onDestroy() {
        unregisterCallStateCallback()
        stopCallRefresh()
        serviceScope.cancel()
        stopNsdDiscovery()

        stopBatteryMonitor()
        stopStorageMonitor()
        unregisterNetworkCallback()
        cancelNsdRetry()
        releaseMulticastLock()
        releaseWifiLock()
        releaseWakeLock()
        isRunning = false
        if (engineHandle != 0L) LinkAllJni.interruptWait(engineHandle)
        eventDrainThread?.join(1000)
        handler.removeCallbacksAndMessages(null)
        heartbeatHandler.removeCallbacksAndMessages(null)
        engineHealthCheckRunnable = null

        engineLock.writeLock().lock()
        try {
            if (engineHandle != 0L) {
                LinkAllJni.stop(engineHandle)
                engineHandle = 0L
            }
        } finally {
            engineLock.writeLock().unlock()
        }
        
        engineStarted.set(false)
        connectedPeerIds.clear()
        setServiceRunning(false)
        persistStatus()
        unregisterPairingReceiver()
        
        try { unregisterReceiver(customReceiver) } catch (e: Exception) {}
        try { unregisterReceiver(smsReceiver) } catch (e: Exception) {}
        try { unregisterReceiver(screenReceiver) } catch (e: Exception) {}
        try { contentResolver.unregisterContentObserver(screenshotObserver) } catch (e: Exception) {}
        pingPlayer?.release()
        pingPlayer = null
        
        
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    // Survive task removal (user swipes app away from recents)
    override fun onTaskRemoved(rootIntent: Intent?) {
        // Re-schedule restart via AlarmManager for maximum reliability on OEM ROMs
        val pending = PendingIntent.getService(
            this, 1,
            Intent(this, LinkAllService::class.java).apply { action = ACTION_START },
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_ONE_SHOT
        )
        val am = getSystemService(ALARM_SERVICE) as AlarmManager
        am.set(AlarmManager.ELAPSED_REALTIME, SystemClock.elapsedRealtime() + 1_000L, pending)
        super.onTaskRemoved(rootIntent)
    }

    // ── WifiLock ──────────────────────────────────────────────────────────────
    //
    // No lock is held while idle: FULL_HIGH_PERF is a no-op since Android 10,
    // and on Android 8–9 it disabled Wi-Fi power saving whenever the screen
    // was on. Only a transfer that is moving bytes takes a lock (below).

    // Wi-Fi power-save wakes the radio only every beacon interval, which
    // showed up as 80-200 ms pings to the phone and capped transfers, so hold
    // a low-latency lock only while files move.
    private var transferWifiLock: android.net.wifi.WifiManager.WifiLock? = null

    // Only bytes actually moving justify a lock: a transfer waiting on the
    // user's Accept, or paused, can sit there for hours.
    private fun isMovingData(): Boolean =
        TransferManager.activeTransfers.values.any { it.state == TransferState.PROGRESS } ||
            TransferManager.activeSpeedTests.isNotEmpty()

    private fun syncTransferWifiLock() {
        val busy = isMovingData()
        val held = transferWifiLock?.isHeld == true
        if (busy == held) return
        runCatching {
            if (busy) {
                val wm = applicationContext.getSystemService(WIFI_SERVICE) as android.net.wifi.WifiManager
                val mode = if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.Q) {
                    android.net.wifi.WifiManager.WIFI_MODE_FULL_LOW_LATENCY
                } else {
                    @Suppress("DEPRECATION")
                    android.net.wifi.WifiManager.WIFI_MODE_FULL_HIGH_PERF
                }
                transferWifiLock = wm.createWifiLock(mode, "LinkAll::Transfer").apply {
                    setReferenceCounted(false)
                    acquire()
                }
            } else {
                transferWifiLock?.release()
                transferWifiLock = null
            }
        }.onFailure { Log.w(TAG, "Transfer WifiLock change failed", it) }
    }

    private fun releaseWifiLock() {
        runCatching { transferWifiLock?.let { if (it.isHeld) it.release() } }
        transferWifiLock = null
        Log.i(TAG, "WifiLock released")
    }

    private fun acquireWakeLock() {
        if (wakeLock == null) {
            val pm = runCatching {
                applicationContext.getSystemService(Context.POWER_SERVICE) as android.os.PowerManager
            }.getOrNull() ?: return
            wakeLock = pm.newWakeLock(
                android.os.PowerManager.PARTIAL_WAKE_LOCK,
                "LinkAll::ServiceWakeLock"
            ).apply {
                setReferenceCounted(true)
            }
        }
        wakeLock?.acquire(30000) // 30 seconds max per event
    }

    private fun releaseWakeLock() {
        runCatching { wakeLock?.let { if (it.isHeld) it.release() } }
    }
    // ── Multicast lock ────────────────────────────────────────────────────────
    //
    // Held for the entire service lifetime (not just during drain) because mDNS
    // needs multicast continuously.  The overhead is negligible — it only
    // prevents the WiFi driver from filtering multicast in hardware.

    private fun acquireMulticastLock() {
        if (multicastLock?.isHeld == true) return
        val wm = runCatching {
            applicationContext.getSystemService(WIFI_SERVICE) as android.net.wifi.WifiManager
        }.getOrNull() ?: return
        multicastLock = wm.createMulticastLock("LinkAll::NsdMulticast").apply {
            setReferenceCounted(false)
            acquire()
        }
        Log.i(TAG, "Multicast lock acquired")
    }

    private fun releaseMulticastLock() {
        runCatching { multicastLock?.let { if (it.isHeld) it.release() } }
        multicastLock = null
        Log.i(TAG, "Multicast lock released")
    }

    // ── Continuous Lock Management ────────────────────────────────────────────

    private var idleLockReleaseRunnable: Runnable? = null

    /**
     * Acquires the multicast lock that mDNS discovery needs.
     * Schedules it to be released after 2 minutes of idleness so it never stays held overnight.
     */
    private fun acquireContinuousLocks() {
        handler.post {
            // With the screen off nobody is waiting on discovery, and the
            // release timer below can't be trusted to fire (see SCREEN_OFF).
            val pm = getSystemService(Context.POWER_SERVICE) as android.os.PowerManager
            if (!pm.isInteractive && !isMovingData()) return@post
            acquireMulticastLock()

            idleLockReleaseRunnable?.let { handler.removeCallbacks(it) }

            val releaseTask = Runnable {
                if (isMovingData()) {
                    // Still active transfers, re-schedule
                    acquireContinuousLocks()
                } else {
                    Log.i(TAG, "Idle timeout reached (2m). Releasing continuous battery-draining locks to allow device sleep.")
                    releaseMulticastLock()
                }
            }
            idleLockReleaseRunnable = releaseTask
            handler.postDelayed(releaseTask, 120_000L) // 2 minutes
        }
    }

    // ── Sync enable / disable ─────────────────────────────────────────────────

    private fun setSyncEnabled(enabled: Boolean) {
        prefs().edit().putBoolean("sync_enabled", enabled).apply()
        val h = engineHandle
        if (h != 0L) {
            withEngine { live -> LinkAllJni.setSyncEnabled(live, enabled) }
        }
        updateForegroundNotification()
        broadcastStatus()
    }

    private fun disconnectAllPeers() {
        val h = engineHandle
        if (h != 0L) {
            // Cancel all active transfers before disconnecting
            TransferManager.activeTransfers.values.forEach { transfer ->
                withEngine { live -> LinkAllJni.cancelFileTransfer(live, transfer.id) }
            }
            TransferManager.activeTransfers.clear()
            TransferManager.activeTransfersFlow.value = emptyList()

            currentPeerSnapshots()
                .filter { it.isConnected }
                .forEach { peer -> withEngine { live -> LinkAllJni.disconnectPeer(live, peer.id) } }
        }
        connectedPeerIds.clear()
        persistStatus()
        updateForegroundNotification()
        handler.postDelayed({
            persistStatus()
            updateForegroundNotification()
        }, 750L)
    }

    private fun shutdownAndStop() {
        disconnectAllPeers()
        stopSelf()
    }

    private fun restartDiscoveryNow() {
        if (engineHandle == 0L) return
        handler.post {
            acquireContinuousLocks()
            stopNsdDiscovery()
            startNsdDiscovery()
            cancelNsdRetry()
            nsdRetryCount.set(0L)
            persistStatus()
            updateForegroundNotification()
        }
    }

    // ── Event drain (Rust → Kotlin) ───────────────────────────────────────────

    private fun startEventDrainThread() {
        isRunning = true
        eventDrainThread = Thread {
            while (isRunning) {
                // Block in the core until an event arrives instead of polling
                // every 100 ms (600 wakeups a minute, forever). onDestroy
                // calls interruptWait before taking the write lock, so
                // holding the read lock while blocked can't deadlock it.
                engineLock.readLock().lock()
                val ev = try {
                    if (engineHandle != 0L) {
                        LinkAllJni.waitEvent(engineHandle)
                    } else 0L
                } finally {
                    engineLock.readLock().unlock()
                }
                if (ev == 0L && engineHandle == 0L) break
                
                if (ev != 0L) {
                    val batch = mutableListOf(ev)
                    while (batch.size < 50) {
                        engineLock.readLock().lock()
                        val nextEv = try {
                            if (engineHandle != 0L) LinkAllJni.pollEvent(engineHandle) else 0L
                        } finally {
                            engineLock.readLock().unlock()
                        }
                        if (nextEv == 0L) break
                        batch.add(nextEv)
                    }
                    acquireWakeLock()
                    handler.post {
                        try { 
                            for (e in batch) {
                                // One event's failure must not crash the app
                                // and drop the rest of the batch.
                                try { handleEvent(e) } catch (t: Exception) {
                                    Log.e(TAG, "Event ${LinkAllJni.eventType(e)} failed", t)
                                }
                            }
                            syncTransferWifiLock()
                        } finally { 
                            for (e in batch) { LinkAllJni.freeEvent(e) }
                            releaseWakeLock() 
                        }
                    }
                } else {
                    // Only reached if the core's event channel closed; avoid
                    // spinning while the service winds down.
                    Thread.sleep(1000)
                }
            }
        }.apply { start() }
    }

    private fun handleEvent(ev: Long) {
        when (LinkAllJni.eventType(ev)) {

            // ── Clipboard text — AUTO-APPLIED (legacy or auto-apply enabled) ─
            LinkAllJni.CR_EVENT_CLIPBOARD_TEXT -> {
                val text = LinkAllJni.eventText(ev) ?: return
                if (text == "__LINKALL_PING__") {
                    pingPhone()
                    return
                }
                val from = resolvePeerDisplayName(
                    LinkAllJni.eventDeviceId(ev),
                    LinkAllJni.eventDeviceName(ev)
                )
                // Track last-sync time per peer so dashboard can show "2m ago"
                peerLastSync[from] = System.currentTimeMillis()
                addActivity(ActivityEntry(
                    deviceName = from,
                    kind = ActivityKind.CLIPBOARD_TEXT,
                    preview = text.take(400).replace('\n', ' '),
                    appliedLocally = true
                ))
                applyText(text, from)
            }

            // ── Clipboard text — TIMELINE-FIRST (available, not auto-applied) ─
            LinkAllJni.CR_EVENT_CLIPBOARD_AVAILABLE -> {
                val text = LinkAllJni.eventText(ev) ?: return
                if (text == "__LINKALL_PING__") {
                    pingPhone()
                    return
                }
                val from = resolvePeerDisplayName(
                    LinkAllJni.eventDeviceId(ev),
                    LinkAllJni.eventDeviceName(ev)
                )
                // Track last-sync time per peer
                peerLastSync[from] = System.currentTimeMillis()
                val autoApplied = LinkAllJni.eventAutoApplied(ev) == 1
                val activityId  = LinkAllJni.eventActivityId(ev)
                val preview = text.take(400).replace('\n', ' ')

                addActivity(ActivityEntry(
                    id = activityId.takeIf { it >= 0 } ?: System.nanoTime(),
                    deviceName = from,
                    kind = ActivityKind.CLIPBOARD_TEXT,
                    preview = preview,
                    contentHash = textContentHash(text),
                    appliedLocally = autoApplied && LinkAllApp.isAppInForeground
                ))

                if (autoApplied && LinkAllApp.isAppInForeground) {
                    applyText(text, from)
                } else {
                    // Show a dismissable notification with an "Apply" action.
                    showClipboardAvailableNotification(from, preview, text, textContentHash(text))
                }
            }

            // ── Clipboard image — AUTO-APPLIED ────────────────────────────────
            LinkAllJni.CR_EVENT_CLIPBOARD_IMAGE -> {
                val bytes = LinkAllJni.eventBinaryData(ev) ?: return
                val mime  = LinkAllJni.eventMimeType(ev) ?: "image/png"
                val from  = resolvePeerDisplayName(
                    LinkAllJni.eventDeviceId(ev),
                    LinkAllJni.eventDeviceName(ev)
                )
                // The core clears auto-apply when clipboard sharing is off
                // (or timeline-first mode holds it); text already honoured
                // that, images didn't and overwrote the clipboard anyway.
                val applied = LinkAllJni.eventAutoApplied(ev) == 1
                addActivity(ActivityEntry(deviceName = from, kind = ActivityKind.CLIPBOARD_IMAGE,
                    preview = "image ($mime)", appliedLocally = applied))
                if (applied) applyBinaryClipboard(bytes, imageNameForMime(mime), mime, from, isFile = false)
            }

            // ── File received (legacy clipboard file) ─────────────────────────
            LinkAllJni.CR_EVENT_CLIPBOARD_FILE -> {
                val bytes = LinkAllJni.eventBinaryData(ev) ?: return
                val name  = LinkAllJni.eventFileName(ev) ?: "LinkAll_file"
                val from  = resolvePeerDisplayName(
                    LinkAllJni.eventDeviceId(ev),
                    LinkAllJni.eventDeviceName(ev)
                )
                addActivity(ActivityEntry(deviceName = from, kind = ActivityKind.FILE_RECEIVED,
                    preview = name))
                applyBinaryClipboard(bytes, name, null, from, isFile = true)
            }

            // ── Dedicated file transfer: incoming ─────────────────────────────
            LinkAllJni.CR_EVENT_FILE_TRANSFER_INCOMING -> {
                acquireContinuousLocks()
                val tid       = LinkAllJni.eventTransferId(ev) ?: return
                val from      = resolvePeerDisplayName(
                    LinkAllJni.eventDeviceId(ev),
                    LinkAllJni.eventDeviceName(ev)
                )
                val fileName  = LinkAllJni.eventTransferFileName(ev) ?: "file"
                val totalBytes = LinkAllJni.eventTransferTotalBytes(ev)

                if (LinkAllJni.eventTransferInFolder(ev)) {
                    // One question per folder: the answer covers all of it.
                    val folder = fileName.substringBefore('/')
                    val peer = currentPeerSnapshots().firstOrNull { it.id == LinkAllJni.eventDeviceId(ev) }
                    if (peer?.trusted == true && engineHandle != 0L) {
                        withEngine { live -> LinkAllJni.acceptFileTransfer(live, tid) }
                    } else if (shouldAskAboutFolder("$from/$folder")) {
                        showFileTransferIncomingNotification(from, folder, 0L, tid, isFolder = true)
                    }
                    return
                }
                
                val isOutboundFeed = ActivityFeedManager.getFeedSnapshot().any { it.transferId == tid && it.kind == ActivityKind.FILE_SENT }
                
                if (isOutboundFeed) {
                    TransferManager.activeTransfers[tid] = TransferProgress(
                        id = tid,
                        fileName = fileName,
                        percent = 0,
                        bytesReceived = 0,
                        totalBytes = totalBytes,
                        speedBps = 0,
                        etaSecs = 0,
                        isPaused = false,
                        state = TransferState.PROGRESS,
                        peerName = from,
                        isOutbound = true
                    )
                    TransferManager.publishActiveTransfers()
                } else {
                    addActivity(ActivityEntry(deviceName = from,
                        kind = ActivityKind.FILE_TRANSFER_INCOMING, preview = fileName,
                        transferId = tid, fileTotalBytes = totalBytes))
                    
                    TransferManager.activeTransfers[tid] = TransferProgress(
                        id = tid,
                        fileName = fileName,
                        percent = 0,
                        bytesReceived = 0,
                        totalBytes = totalBytes,
                        speedBps = 0,
                        etaSecs = 0,
                        isPaused = false,
                        state = TransferState.INCOMING,
                        peerName = from,
                        isOutbound = false
                    )
                    TransferManager.publishActiveTransfers()

                    val peer = currentPeerSnapshots().firstOrNull { it.id == LinkAllJni.eventDeviceId(ev) }
                    if (peer?.trusted == true && engineHandle != 0L) {
                        withEngine { live -> LinkAllJni.acceptFileTransfer(live, tid) }
                    } else {
                        showFileTransferIncomingNotification(from, fileName, totalBytes, tid)
                    }
                }
            }

            // ── Dedicated file transfer: progress update ──────────────────────
            LinkAllJni.CR_EVENT_FILE_TRANSFER_PROGRESS -> {
                val tid           = LinkAllJni.eventTransferId(ev) ?: return
                val percent       = LinkAllJni.eventTransferProgressPercent(ev)
                val bytesReceived = LinkAllJni.eventTransferBytesReceived(ev)
                val speedBps      = LinkAllJni.eventTransferSpeedBps(ev)
                val etaSecs       = LinkAllJni.eventTransferEtaSecs(ev)
                val name          = LinkAllJni.eventTransferFileName(ev) ?: "file"
                val existing = TransferManager.activeTransfers[tid]
                // Progress arrives ~10x a second; the feed update below
                // broadcasts to the dashboard and tile, so rate-limit it
                // like the notification (250 ms), always letting 100% through.
                val nowMs = android.os.SystemClock.elapsedRealtime()
                if (percent == 100 || nowMs - (lastTransferFeedTimes[tid] ?: 0L) >= 250L) {
                    lastTransferFeedTimes[tid] = nowMs
                    updateActivityTransferProgress(
                        tid = tid,
                        percent = percent,
                        bytesReceived = bytesReceived,
                        speedBps = speedBps,
                        etaSecs = etaSecs
                    )
                }

                val isPaused = existing?.isPaused ?: false
                // Use eventTransferTotalBytes to get the real total even for outbound transfers
                val totalBytes = LinkAllJni.eventTransferTotalBytes(ev).let { if (it > 0) it else (existing?.totalBytes ?: 0L) }
                // Peer name and direction don't change mid-transfer; only
                // resolve them (a JNI peer list + JSON parse, and a feed
                // scan) the first time this transfer is seen.
                val peerName = existing?.peerName ?: resolvePeerDisplayName(
                    LinkAllJni.eventDeviceId(ev),
                    LinkAllJni.eventDeviceName(ev)
                )
                // The core reports direction on every progress event; transfers from
                // trusted peers are auto-accepted without an INCOMING event, so
                // `existing` alone cannot tell.
                val isOutbound = when (LinkAllJni.eventTransferIsOutbound(ev)) {
                    1 -> true
                    0 -> false
                    else -> existing?.isOutbound ?: TransferManager.pendingOutboundTransferIds.contains(tid)
                }
                
                if (LinkAllJni.eventTransferInFolder(ev)) {
                    onFolderItemProgress(name, isOutbound, peerName)
                    return
                }

                TransferManager.activeTransfers[tid] = TransferProgress(
                    id = tid, fileName = name, percent = percent, bytesReceived = bytesReceived, 
                    totalBytes = totalBytes, speedBps = speedBps, etaSecs = etaSecs, 
                    isPaused = isPaused, state = TransferState.PROGRESS, peerName = peerName,
                    isOutbound = isOutbound
                )
                TransferManager.publishActiveTransfers(force = (percent == 100))
                
                updateFileTransferNotificationProgress(
                    tid = tid,
                    fileName = name,
                    percent = percent,
                    bytesReceived = bytesReceived,
                    totalBytes = totalBytes,
                    speedBps = speedBps,
                    etaSecs = etaSecs,
                    isPaused = isPaused,
                    isOutbound = isOutbound
                )
            }

            // ── Dedicated file transfer: complete ─────────────────────────────
            LinkAllJni.CR_EVENT_FILE_TRANSFER_COMPLETE -> {
                val tid      = LinkAllJni.eventTransferId(ev) ?: return
                lastTransferNotifTimes.remove(tid)
                lastTransferFeedTimes.remove(tid)
                val from     = resolvePeerDisplayName(
                    LinkAllJni.eventDeviceId(ev),
                    LinkAllJni.eventDeviceName(ev)
                )
                val fileName = LinkAllJni.eventTransferFileName(ev) ?: "file"
                val destPath = LinkAllJni.eventTransferDestPath(ev) ?: ""

                if (LinkAllJni.eventTransferInFolder(ev)) {
                    // Saved in place inside its folder; the folder reports once.
                    if (destPath.isNotEmpty()) {
                        android.media.MediaScannerConnection.scanFile(this, arrayOf(destPath), null, null)
                    }
                    TransferManager.activeTransfers.remove(tid)
                    TransferManager.pendingOutboundTransferIds.remove(tid)
                    onFolderItemProgress(fileName, destPath.isEmpty(), from, force = true)
                    return
                }
                
                if (destPath.isEmpty()) {
                    // Outbound transfer completed!
                    updateActivityTransferComplete(tid, "")
                    cancelFileTransferNotification(tid)
                    TransferManager.activeTransfers.remove(tid)
                    TransferManager.publishActiveTransfers(force = true)
                    
                    val builder = NotificationCompat.Builder(this, CHAN_ALERTS).setGroup("linkall_transfers")
                        .setSmallIcon(R.mipmap.ic_launcher)
                        .setContentTitle("File sent to $from")
                        .setContentText(fileName)
                        .setAutoCancel(true)
                    val notifId = NOTIF_ID_FILE_BASE + (fileName.hashCode() and 0xFFF)
                    notificationManager.notify(notifId, builder.build())
                    
                    return
                }
                // Offload the heavy file copy to a background coroutine so we don't block the JNI thread and freeze the UI
                serviceScope.launch {
                    val srcFile = File(destPath)
                    if (!srcFile.exists()) {
                        Log.w(TAG, "Completed file does not exist at destPath: $destPath")
                        // Don't leave the progress notification stuck.
                        updateActivityTransferFailed(tid)
                        cancelFileTransferNotification(tid)
                        return@launch
                    }
                    val publicUriStr = if (srcFile.parentFile?.name == "Link All") {
                        android.media.MediaScannerConnection.scanFile(this@LinkAllService, arrayOf(destPath), null, null)
                        null
                    } else {
                        val uriStr = saveFileToPublicDownloads(srcFile)
                        if (uriStr != null) {
                            try { srcFile.delete() } catch (_: Exception) {}
                        } else {
                            Log.w(TAG, "saveFileToPublicDownloads returned null for $destPath; preserving source file to avoid data loss")
                        }
                        uriStr
                    }
                    
                    val finalPath = publicUriStr ?: destPath
                    if (ActivityFeedManager.getFeedSnapshot().none { it.transferId == tid }) {
                        // The core auto-accepts transfers from trusted peers without an
                        // INCOMING event, so no feed entry exists yet: record the file now.
                        addActivity(ActivityEntry(
                            deviceName = from,
                            kind = ActivityKind.FILE_TRANSFER_COMPLETE,
                            preview = fileName,
                            transferId = tid,
                            progressPercent = 100,
                            destPath = finalPath
                        ))
                    } else {
                        updateActivityTransferComplete(tid, finalPath)
                    }
                    cancelFileTransferNotification(tid)

                    val uriToOpen = if (publicUriStr != null) {
                        android.net.Uri.parse(publicUriStr)
                    } else {
                        androidx.core.content.FileProvider.getUriForFile(this@LinkAllService, "$packageName.fileprovider", File(destPath))
                    }

                    showFileTransferCompleteNotification(from, fileName, uriToOpen)
                }
                
                // Immediately remove from active transfers so the UI progress bar disappears without getting stuck
                TransferManager.activeTransfers.remove(tid)
                TransferManager.publishActiveTransfers(force = true)
            }

            // ── Dedicated file transfer: failed ───────────────────────────────
            LinkAllJni.CR_EVENT_FILE_TRANSFER_FAILED -> {
                val tid  = LinkAllJni.eventTransferId(ev) ?: return
                lastTransferNotifTimes.remove(tid)
                lastTransferFeedTimes.remove(tid)
                if (LinkAllJni.eventTransferInFolder(ev)) {
                    TransferManager.activeTransfers.remove(tid)
                    TransferManager.pendingOutboundTransferIds.remove(tid)
                    return
                }
                val from = resolvePeerDisplayName(
                    LinkAllJni.eventDeviceId(ev),
                    LinkAllJni.eventDeviceName(ev)
                )
                updateActivityTransferFailed(tid)
                cancelFileTransferNotification(tid)
                TransferManager.activeTransfers.remove(tid)
                TransferManager.publishActiveTransfers(force = true)
            }

            LinkAllJni.CR_EVENT_FOLDER_TRANSFER_COMPLETE -> {
                val folder = LinkAllJni.eventTransferFileName(ev) ?: "Folder"
                val destDir = LinkAllJni.eventTransferDestPath(ev).orEmpty()
                val counts = LinkAllJni.eventFolderCounts(ev)
                val total = counts?.getOrNull(0) ?: 0
                val failed = counts?.getOrNull(1) ?: 0
                val peer = LinkAllJni.eventDeviceName(ev) ?: "device"
                onFolderComplete(folder, peer, total, failed, destDir)
            }

            LinkAllJni.CR_EVENT_FILE_TRANSFER_PAUSED -> {
                val tid = LinkAllJni.eventTransferId(ev) ?: return
                val state = TransferManager.activeTransfers[tid]
                if (state != null) {
                    val newState = state.copy(isPaused = true, state = TransferState.PAUSED)
                    TransferManager.activeTransfers[tid] = newState
                    TransferManager.publishActiveTransfers(force = true)
                    updateFileTransferNotificationProgress(
                        tid = tid,
                        fileName = newState.fileName,
                        percent = newState.percent,
                        bytesReceived = newState.bytesReceived,
                        totalBytes = newState.totalBytes,
                        speedBps = newState.speedBps,
                        etaSecs = newState.etaSecs,
                        isPaused = true,
                        isOutbound = newState.isOutbound
                    )
                }
            }

            LinkAllJni.CR_EVENT_FILE_TRANSFER_RESUMED -> {
                val tid = LinkAllJni.eventTransferId(ev) ?: return
                val state = TransferManager.activeTransfers[tid]
                if (state != null) {
                    val newState = state.copy(isPaused = false, state = TransferState.PROGRESS)
                    TransferManager.activeTransfers[tid] = newState
                    TransferManager.publishActiveTransfers(force = true)
                    updateFileTransferNotificationProgress(
                        tid = tid,
                        fileName = newState.fileName,
                        percent = newState.percent,
                        bytesReceived = newState.bytesReceived,
                        totalBytes = newState.totalBytes,
                        speedBps = newState.speedBps,
                        etaSecs = newState.etaSecs,
                        isPaused = false,
                        isOutbound = newState.isOutbound
                    )
                }
            }

            // ── Speed Test ────────────────────────────────────────────────────────────
            LinkAllJni.CR_EVENT_SPEED_TEST_PROGRESS -> {
                val peerId = LinkAllJni.eventDeviceId(ev) ?: return
                val bytesTransferred = LinkAllJni.eventSpeedTestBytes(ev)
                val durationSecs = LinkAllJni.eventSpeedTestDuration(ev)
                val phase = LinkAllJni.eventSpeedTestPhase(ev) ?: "Unknown"
                val peerName = resolvePeerDisplayName(peerId, LinkAllJni.eventDeviceName(ev))
                
                TransferManager.activeSpeedTests[peerId] = SpeedTestProgress(
                    peerId = peerId,
                    peerName = peerName,
                    phase = phase,
                    bytesTransferred = bytesTransferred,
                    durationSecs = durationSecs
                )
                TransferManager.publishActiveTransfers(force = true)
            }
            LinkAllJni.CR_EVENT_SPEED_TEST_COMPLETE -> {
                val peerId = LinkAllJni.eventDeviceId(ev) ?: return
                TransferManager.activeSpeedTests.remove(peerId)
                TransferManager.publishActiveTransfers(force = true)
            }
            // ── True SAS Pairing (No auto-trust) ──────────────────────────────
            LinkAllJni.CR_EVENT_PAIRING_REQUESTED -> {
                val deviceId = LinkAllJni.eventDeviceId(ev) ?: return
                val name = resolvePeerDisplayName(deviceId, LinkAllJni.eventDeviceName(ev))
                val pin  = LinkAllJni.eventFingerprint(ev) ?: "" // JNI returns pin via eventFingerprint for now
                
                // Acquire brief WakeLock so CPU doesn't sleep mid-notification and wakes up
                runCatching {
                    val pm = getSystemService(Context.POWER_SERVICE) as? android.os.PowerManager
                    pm?.newWakeLock(
                        android.os.PowerManager.PARTIAL_WAKE_LOCK or android.os.PowerManager.ACQUIRE_CAUSES_WAKEUP,
                        "LinkAll:PairingWakeLock"
                    )?.apply {
                        acquire(5000L)
                    }
                }
                
                // Always post high-priority heads-up/full-screen notification (required on Android 10+ when app is closed/backgrounded)
                showPairingRequestNotification(deviceId, name, pin)
                
                // Also attempt direct launch in case activity is already in foreground
                runCatching {
                    val intent = Intent(this@LinkAllService, PairingActivity::class.java).apply {
                        flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
                        putExtra(PairingActivity.EXTRA_DEVICE_ID, deviceId)
                        putExtra(PairingActivity.EXTRA_DEVICE_NAME, name)
                        putExtra(PairingActivity.EXTRA_PIN, pin)
                    }
                    startActivity(intent)
                }
            }

            LinkAllJni.CR_EVENT_OUTGOING_PAIRING_WAITING -> {
                val deviceId = LinkAllJni.eventDeviceId(ev) ?: return
                val name = resolvePeerDisplayName(deviceId, LinkAllJni.eventDeviceName(ev))
                val pin  = LinkAllJni.eventFingerprint(ev) ?: ""
                Log.i(TAG, "Outgoing pairing waiting for $name ($deviceId) with pin $pin")
                // No need to launch PairingActivity here. 
                // The OnboardingScreen or Dashboard naturally reflects this state via trust_store updates.
                persistStatus()
            }

            LinkAllJni.CR_EVENT_SYSTEM_HEALTH_UPDATED -> persistStatus()

            // A request expired, was withdrawn by the other device, or lost
            // its session. Whatever is on screen for it is stale now.
            LinkAllJni.CR_EVENT_PAIRING_CHANGED -> {
                val deviceId = LinkAllJni.eventDeviceId(ev) ?: return
                Log.i(TAG, "Pairing state changed for $deviceId")
                persistStatus()
                notificationManager.cancel(pairingNotifId(deviceId))
                sendBroadcast(Intent("app.linkall.CLOSE_PAIRING_UI").apply {
                    setPackage(packageName)
                    putExtra(PairingActivity.EXTRA_DEVICE_ID, deviceId)
                })
            }


            // ── Peer discovered ───────────────────────────────────────────────
            LinkAllJni.CR_EVENT_PEER_DISCOVERED -> {
                val deviceId = LinkAllJni.eventDeviceId(ev) ?: return
                val name = resolvePeerDisplayName(
                    deviceId,
                    LinkAllJni.eventDeviceName(ev)
                )
                Log.i(TAG, "Peer discovered: $name (id=$deviceId)")
                persistStatus() // Triggers UI update via Flow
                updateForegroundNotification()
            }

            // ── Peer connected ────────────────────────────────────────────────
            LinkAllJni.CR_EVENT_PEER_CONNECTED -> {
                val deviceId = LinkAllJni.eventDeviceId(ev) ?: return
                val name = resolvePeerDisplayName(
                    deviceId,
                    LinkAllJni.eventDeviceName(ev)
                )
                Log.i(TAG, "Peer connected: $name (id=$deviceId)")
                connectedPeerIds[deviceId] = name
                persistStatus()
                updateForegroundNotification()
                // The peer may still show a call that ended while it was away.
                resyncIdleCallState(delayMs = 1_500)
                sendBroadcast(Intent("app.linkall.CLOSE_PAIRING_UI").apply {
                    setPackage(packageName)
                    putExtra(PairingActivity.EXTRA_DEVICE_ID, deviceId)
                })
                // Connection established — cancel any pending retry scans and
                // reset backoff so the next disconnect starts fresh.
                cancelNsdRetry()
                nsdRetryCount.set(0L)
                pauseNsdBrowse()
                // The storage monitor only runs with peers present, so give
                // the new peer a reading straight away.
                pushStorageStatusAsync()
            }

            // ── Peer disconnected ─────────────────────────────────────────────
            LinkAllJni.CR_EVENT_PEER_DISCONNECTED -> {
                val deviceId = LinkAllJni.eventDeviceId(ev)
                val name = resolvePeerDisplayName(
                    deviceId,
                    LinkAllJni.eventDeviceName(ev)
                )
                Log.i(TAG, "Peer disconnected: $name (id=$deviceId)")
                if (deviceId != null) {
                    connectedPeerIds.remove(deviceId)
                    // No SPEED_TEST_COMPLETE will come for a dropped peer;
                    // a stale entry would pin the transfer Wi-Fi lock.
                    if (TransferManager.activeSpeedTests.remove(deviceId) != null) {
                        TransferManager.publishActiveTransfers(force = true)
                        syncTransferWifiLock()
                    }
                }
                persistStatus()
                updateForegroundNotification()
                // If we're now peerless, schedule a retry scan so we reconnect
                // automatically when the Mac wakes up or comes back on the network.
                if (connectedPeerIds.isEmpty()) {
                    scheduleNsdRetry()
                }
            }

            // ── Engine warning ────────────────────────────────────────────────
            LinkAllJni.CR_EVENT_WARNING -> {
                val msg = LinkAllJni.eventText(ev) ?: return
                if (msg == "interrupt") return // interruptWait wake-up, not a real warning
                Log.w(TAG, "Engine warning: $msg")
                if (msg == "Pairing request was declined." || msg == "Pairing request was accepted.") {
                    sendBroadcast(Intent("app.linkall.CLOSE_PAIRING_UI").apply {
                        setPackage(packageName)
                        LinkAllJni.eventDeviceId(ev)?.let { putExtra(PairingActivity.EXTRA_DEVICE_ID, it) }
                    })
                }
                if (isCriticalFailure(msg)) showFailureNotification(msg)
                updateForegroundNotification()
            }

            // ── Call continuity ───────────────────────────────────────────────
            LinkAllJni.CR_EVENT_CALL_STATE_CHANGED -> {
                // On Android we originated this event — nothing to do.
                // Other peers (macOS) will show the incoming call banner.
                Log.d(TAG, "CallStateChanged echoed (no-op on originating device)")
            }

            LinkAllJni.CR_EVENT_CALL_ACTION -> {
                val action = LinkAllJni.eventCallAction(ev) ?: return
                Log.i(TAG, "Remote call action received: $action")
                handleRemoteCallAction(action)
                // If the call was already over, nothing else tells the peer so.
                if (action == "accept" || action == "decline") resyncIdleCallState(delayMs = 1_500)
            }

            LinkAllJni.CR_EVENT_BATTERY_STATE_CHANGED -> {
                Log.d(TAG, "BatteryStateChanged event received (no-op on Android)")
            }

            // ── Remote Explorer (Phase 2) ─────────────────────────────────────────────
            LinkAllJni.CR_EVENT_REMOTE_FILES_QUERY -> {
                val requestId = LinkAllJni.eventRequestId(ev) ?: return
                val targetDeviceId = LinkAllJni.eventDeviceId(ev) ?: return
                val summaryOnly = LinkAllJni.eventSummaryOnly(ev)
                val category = LinkAllJni.eventFileCategory(ev)
                val source = LinkAllJni.eventFileSource(ev)
                val query = LinkAllJni.eventSearchQuery(ev)
                val offset = LinkAllJni.eventOffset(ev)
                val limit = LinkAllJni.eventLimit(ev)

                executeInBackgroundWithWakeLock("RemoteFilesQuery") {
                    if (!hasFilePermissions()) {
                        Log.w(TAG, "Storage permission missing for RemoteFilesQuery")
                        try {
                            showPermissionRequiredNotification()
                        } catch (e: Exception) {
                            Log.e(TAG, "Failed to show permission notification", e)
                        }
                        withEngine { live -> LinkAllJni.sendRemoteFilesResponse(
                            live, requestId, targetDeviceId, null, null, 0,
                            if (BuildConfig.FULL_PERMISSIONS) "Permission Denied: Please grant storage permission on your Android device to browse files."
                            else "Browsing phone files isn't available in the Google Play version of Link All. Share files from the phone instead."
                        ) }
                        return@executeInBackgroundWithWakeLock
                    }

                    try {
                        val (summaryJson, filesJson, total) = RemoteFileManager.queryFiles(
                            applicationContext, category, source, query, offset, limit,
                            includeSummary = summaryOnly || offset == 0,
                            includeList = !summaryOnly
                        )
                        withEngine { live -> LinkAllJni.sendRemoteFilesResponse(
                            live, requestId, targetDeviceId, summaryJson, filesJson, total, null
                        ) }
                    } catch (e: Exception) {
                        Log.e(TAG, "Error handling RemoteFilesQuery", e)
                        withEngine { live -> LinkAllJni.sendRemoteFilesResponse(
                            live, requestId, targetDeviceId, null, null, 0, e.message ?: "Query error"
                        ) }
                    }
                }
            }

            LinkAllJni.CR_EVENT_REMOTE_THUMBNAIL_REQUEST -> {
                val requestId = LinkAllJni.eventRequestId(ev) ?: return
                val targetDeviceId = LinkAllJni.eventDeviceId(ev) ?: return
                val fileId = LinkAllJni.eventFileId(ev)
                val sizePx = LinkAllJni.eventThumbnailSizePx(ev).let { if (it <= 0) 256 else it }

                executeInBackgroundWithWakeLock("RemoteThumbnail") {
                    if (!hasFilePermissions()) {
                        try {
                            showPermissionRequiredNotification()
                        } catch (e: Exception) {
                            Log.e(TAG, "Failed to show permission notification", e)
                        }
                        withEngine { live -> LinkAllJni.sendRemoteThumbnailResponse(
                            live, requestId, targetDeviceId, fileId, null, "Permission Denied"
                        ) }
                        return@executeInBackgroundWithWakeLock
                    }
                    try {
                        val thumbnailBytes = RemoteFileManager.getThumbnail(applicationContext, fileId, sizePx)
                        if (thumbnailBytes != null) {
                            withEngine { live -> LinkAllJni.sendRemoteThumbnailResponse(
                                live, requestId, targetDeviceId, fileId, thumbnailBytes, null
                            ) }
                        } else {
                            withEngine { live -> LinkAllJni.sendRemoteThumbnailResponse(
                                live, requestId, targetDeviceId, fileId, null, "Thumbnail generation failed"
                            ) }
                        }
                    } catch (e: Exception) {
                        Log.e(TAG, "Error handling RemoteThumbnailRequest", e)
                        withEngine { live -> LinkAllJni.sendRemoteThumbnailResponse(
                            live, requestId, targetDeviceId, fileId, null, e.message ?: "Thumbnail error"
                        ) }
                    }
                }
            }

            LinkAllJni.CR_EVENT_REMOTE_FILE_PULL_REQUEST -> {
                val requestId = LinkAllJni.eventRequestId(ev) ?: return
                val targetDeviceId = LinkAllJni.eventDeviceId(ev) ?: return
                val fileId = LinkAllJni.eventFileId(ev)

                executeInBackgroundWithWakeLock("RemoteFilePull") {
                    if (!hasFilePermissions()) {
                        Log.w(TAG, "Storage permission missing for RemoteFilePullRequest")
                        try {
                            showPermissionRequiredNotification()
                        } catch (e: Exception) {
                            Log.e(TAG, "Failed to show permission notification", e)
                        }
                        return@executeInBackgroundWithWakeLock
                    }
                    try {
                        val resolved = RemoteFileManager.resolveFilePathAndMeta(applicationContext, fileId)
                        if (resolved != null) {
                            val (filePath, displayName, mimeType) = resolved
                            Log.i(TAG, "Pulling remote file: $filePath ($displayName, $mimeType) to target $targetDeviceId")
                            withEngine { live -> LinkAllJni.sendFilePath(live, filePath, displayName, mimeType, targetDeviceId, null, false, 1) }
                        } else {
                            Log.w(TAG, "Failed to resolve file path for pull request $fileId")
                        }
                    } catch (e: Exception) {
                        Log.e(TAG, "Error handling RemoteFilePullRequest", e)
                    }
                }
            }

            LinkAllJni.CR_EVENT_REMOTE_FILE_ACTION_REQUEST -> {
                val targetDeviceId = LinkAllJni.eventDeviceId(ev) ?: return
                val fileId = LinkAllJni.eventFileId(ev)
                val action = LinkAllJni.eventText(ev) ?: return
                val newName = LinkAllJni.eventSearchQuery(ev)

                executeInBackgroundWithWakeLock("RemoteFileAction") {
                    if (!hasFilePermissions()) {
                        Log.w(TAG, "Storage permission missing for RemoteFileActionRequest")
                        try {
                            showPermissionRequiredNotification()
                        } catch (e: Exception) {
                            Log.e(TAG, "Failed to show permission notification", e)
                        }
                        return@executeInBackgroundWithWakeLock
                    }
                    try {
                        Log.i(TAG, "Executing remote file action: $action on file $fileId (new name: $newName)")
                        RemoteFileManager.executeAction(applicationContext, fileId, action, newName)
                    } catch (e: Exception) {
                        Log.e(TAG, "Error executing remote file action", e)
                    }
                }
            }

            LinkAllJni.CR_EVENT_OPEN_URL_ON_DEVICE_REQUESTED -> {
                val requester = LinkAllJni.eventDeviceId(ev) ?: return
                val from = LinkAllJni.eventDeviceName(ev) ?: "A device"
                val url = LinkAllJni.eventText(ev).orEmpty()
                handleOpenUrlRequest(requester, from, url)
            }

            LinkAllJni.CR_EVENT_OPEN_URL_ON_DEVICE_ACK -> {
                if (!LinkAllJni.eventOpenUrlAckSuccess(ev)) {
                    val why = LinkAllJni.eventText(ev) ?: "The other device could not open the link"
                    Log.w(TAG, "Link was not opened: $why")
                    android.os.Handler(android.os.Looper.getMainLooper()).post {
                        android.widget.Toast.makeText(applicationContext, why, android.widget.Toast.LENGTH_LONG).show()
                    }
                }
            }
        }
    }

    // A link from another device. Android stops a background service from
    // starting the browser, so the link arrives as a notification: one tap
    // opens it. Only web links are accepted; the sender is told either way.
    private fun handleOpenUrlRequest(requesterId: String, from: String, url: String) {
        val uri = try { android.net.Uri.parse(url.trim()) } catch (e: Exception) { null }
        val scheme = uri?.scheme?.lowercase()
        if (uri == null || (scheme != "http" && scheme != "https") || uri.host.isNullOrEmpty()) {
            Log.w(TAG, "Refusing link from $from: not a web link")
            withEngine { live -> LinkAllJni.ackOpenUrlOnDevice(live, requesterId, false, "Only web links can be opened") }
            return
        }
        try {
            val openPi = PendingIntent.getActivity(
                this, uri.hashCode(), Intent(Intent.ACTION_VIEW, uri).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
            )
            val note = NotificationCompat.Builder(this, CHAN_ALERTS)
                .setSmallIcon(R.mipmap.ic_launcher)
                .setContentTitle("$from sent a link")
                .setContentText(uri.toString())
                .setAutoCancel(true)
                .setContentIntent(openPi)
                .build()
            notificationManager.notify(NOTIF_ID_FILE_BASE + (uri.hashCode() and 0xFFF), note)
            withEngine { live -> LinkAllJni.ackOpenUrlOnDevice(live, requesterId, true, null) }
        } catch (e: Exception) {
            Log.e(TAG, "Could not show the link from $from", e)
            withEngine { live -> LinkAllJni.ackOpenUrlOnDevice(live, requesterId, false, "Could not show the link") }
        }
    }

    // ── Activity feed helpers ─────────────────────────────────────────────────

    private fun addActivity(entry: ActivityEntry) {
        ActivityFeedManager.addToFeed(entry)
        broadcastActivityUpdated()
    }

    private fun updateActivityTransferProgress(
        tid: String,
        percent: Int,
        bytesReceived: Long,
        speedBps: Long,
        etaSecs: Long
    ) {
        ActivityFeedManager.updateFeedByTransferId(tid) { old ->
            old.copy(
                kind = ActivityKind.FILE_TRANSFER_PROGRESS,
                progressPercent = percent,
                transferBytesReceived = bytesReceived.coerceAtLeast(0L),
                transferSpeedBps = speedBps.coerceAtLeast(0L),
                transferEtaSecs = etaSecs
            )
        }
        broadcastActivityUpdated()
    }

    private fun updateActivityTransferComplete(tid: String, destPath: String) {
        ActivityFeedManager.updateFeedByTransferId(tid) { old ->
            old.copy(
                kind = ActivityKind.FILE_TRANSFER_COMPLETE,
                progressPercent = 100,
                destPath = destPath
            )
        }
        broadcastActivityUpdated()
    }

    private fun updateActivityTransferFailed(tid: String) {
        ActivityFeedManager.updateFeedByTransferId(tid) { old ->
            old.copy(kind = ActivityKind.FILE_TRANSFER_FAILED)
        }
        broadcastActivityUpdated()
    }

    private fun broadcastActivityUpdated() {
        sendBroadcast(Intent(ACTION_STATUS_CHANGED).setPackage(packageName))
    }

    // ── Clipboard available notification (timeline-first) ─────────────────────

    private fun showClipboardAvailableNotification(
        from: String,
        preview: String,
        fullText: String,
        contentHash: String
    ) {
        val applyIntent = Intent(ACTION_APPLY_CLIPBOARD).apply {
            `package` = packageName
            putExtra(EXTRA_CLIPBOARD_TEXT, fullText)
            putExtra(EXTRA_CONTENT_HASH, contentHash)
        }
        val applyPi = PendingIntent.getService(
            this, fullText.hashCode(),
            applyIntent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        // Tap notification itself → open MainActivity to see the activity feed.
        val openIntent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
        }
        val openPi = PendingIntent.getActivity(
            this, 0, openIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        // Show a truncated preview in collapsed state; full text (up to 400 chars)
        // in the expanded BigText style — so the user can read it before deciding.
        val bigText = if (fullText.length > 400) fullText.take(397) + "…" else fullText

        val notif = NotificationCompat.Builder(this, CHAN_ALERTS).setGroup("linkall_transfers")
            .setSmallIcon(android.R.drawable.ic_menu_edit)
            .setContentTitle("Clipboard from $from")
            .setContentText(preview)
            .setStyle(
                NotificationCompat.BigTextStyle()
                    .bigText(bigText)
                    .setSummaryText("Tap to open • Swipe to dismiss")
            )
            .setContentIntent(openPi)
            .addAction(android.R.drawable.ic_menu_set_as, "Apply to clipboard", applyPi)
            .setAutoCancel(true)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .build()

        notificationManager.notify(NOTIF_ID_CLIPBOARD_AVAILABLE, notif)
    }

    // ── File transfer notifications ───────────────────────────────────────────

    private fun showFileTransferIncomingNotification(
        from: String, fileName: String, totalBytes: Long, tid: String, isFolder: Boolean = false
    ) {
        val sizeStr = formatBytes(totalBytes)

        val acceptIntent = Intent(this, LinkAllService::class.java).apply {
            action = ACTION_ACCEPT_FILE_TRANSFER
            putExtra(EXTRA_TRANSFER_ID, tid)
        }
        val rejectIntent = Intent(this, LinkAllService::class.java).apply {
            action = ACTION_REJECT_FILE_TRANSFER
            putExtra(EXTRA_TRANSFER_ID, tid)
        }
        val acceptPi = PendingIntent.getService(this, tid.hashCode(),
            acceptIntent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val rejectPi = PendingIntent.getService(this, tid.hashCode() + 1,
            rejectIntent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)

        val notif = NotificationCompat.Builder(this, CHAN_ALERTS).setGroup("linkall_transfers")
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentTitle(if (isFolder) "Incoming folder from $from" else "Incoming file from $from")
            .setContentText(if (isFolder) fileName else "$fileName ($sizeStr)")
            .addAction(0, "Accept", acceptPi)
            .addAction(0, "Reject", rejectPi)
            .setOngoing(true)
            .build()
        notificationManager.notify(transferNotifId(tid), notif)
    }

    private fun showPairingRequestNotification(
        deviceId: String, name: String, pin: String
    ) {
        val pairingIntent = Intent(this, PairingActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
            putExtra(PairingActivity.EXTRA_DEVICE_ID, deviceId)
            putExtra(PairingActivity.EXTRA_DEVICE_NAME, name)
            putExtra(PairingActivity.EXTRA_PIN, pin)
        }
        // No ActivityOptions here: the system starts content and full-screen intents itself,
        // and Android 16 rejects a sender-side background start mode on a PendingIntent
        // being created, which crashed the service on every pairing request.
        val fullScreenPi = PendingIntent.getActivity(
            this, deviceId.hashCode(), pairingIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val acceptIntent = Intent(this, LinkAllService::class.java).apply {
            action = ACTION_RESPOND_TO_PAIRING
            putExtra(EXTRA_TARGET_DEVICE_ID, deviceId)
            putExtra(PairingActivity.EXTRA_APPROVED, true)
        }
        val rejectIntent = Intent(this, LinkAllService::class.java).apply {
            action = ACTION_RESPOND_TO_PAIRING
            putExtra(EXTRA_TARGET_DEVICE_ID, deviceId)
            putExtra(PairingActivity.EXTRA_APPROVED, false)
        }
        val acceptPi = PendingIntent.getService(this, deviceId.hashCode() + 10,
            acceptIntent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val rejectPi = PendingIntent.getService(this, deviceId.hashCode() + 11,
            rejectIntent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)

        val notif = NotificationCompat.Builder(this, CHAN_PAIRING)
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentTitle("Pairing Request from $name")
            .setContentText(if (pin.isNotEmpty()) "PIN code: $pin — Tap to review or approve" else "Tap to review pairing request")
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(NotificationCompat.CATEGORY_CALL)
            .setAutoCancel(true)
            .setContentIntent(fullScreenPi)
            .setFullScreenIntent(fullScreenPi, true)
            .addAction(0, "Accept (${if (pin.isNotEmpty()) pin else "Approve"})", acceptPi)
            .addAction(0, "Reject", rejectPi)
            .setOngoing(true)
            .build()
        notificationManager.notify(pairingNotifId(deviceId), notif)
    }

    private val lastTransferNotifTimes = java.util.concurrent.ConcurrentHashMap<String, Long>()
    private val lastTransferFeedTimes = java.util.concurrent.ConcurrentHashMap<String, Long>()

    private fun updateFileTransferNotificationProgress(
        tid: String,
        fileName: String,
        percent: Int,
        bytesReceived: Long,
        totalBytes: Long = 0L,
        speedBps: Long,
        etaSecs: Long,
        isPaused: Boolean = false,
        isOutbound: Boolean = false
    ) {
        val now = System.currentTimeMillis()
        val lastTime = lastTransferNotifTimes[tid] ?: 0L
        if (percent < 100 && !isPaused && (now - lastTime < 250L)) {
            return
        }
        lastTransferNotifTimes[tid] = now

        val cancelIntent = Intent(ACTION_CANCEL_FILE_TRANSFER).apply {
            `package` = packageName
            putExtra(EXTRA_TRANSFER_ID, tid)
        }
        val cancelPi = PendingIntent.getService(this, tid.hashCode() + 2,
            cancelIntent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)

        val pauseResumeIntent = Intent(if (isPaused) ACTION_RESUME_FILE_TRANSFER else ACTION_PAUSE_FILE_TRANSFER).apply {
            `package` = packageName
            putExtra(EXTRA_TRANSFER_ID, tid)
        }
        val pauseResumePi = PendingIntent.getService(this, tid.hashCode() + 3,
            pauseResumeIntent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)

        val builder = NotificationCompat.Builder(this, CHAN_ALERTS).setGroup("linkall_transfers")
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentTitle(if (isOutbound) "Sending $fileName" else "Receiving $fileName")
            .setContentText(buildTransferStatusLine(percent, bytesReceived, totalBytes, speedBps, etaSecs) + if (isPaused) " (Paused)" else "")
            .setProgress(100, percent, false)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
        // A folder pauses file by file in the engine; offer only Cancel.
        if (!tid.startsWith(FOLDER_ROW_PREFIX)) {
            builder.addAction(android.R.drawable.ic_media_pause, if (isPaused) "Resume" else "Pause", pauseResumePi)
        }
        builder.addAction(android.R.drawable.ic_menu_close_clear_cancel, "Cancel", cancelPi)

        notificationManager.notify(transferNotifId(tid), builder.build())
    }

    private fun showFileTransferCompleteNotification(from: String, fileName: String, uri: android.net.Uri) {
        val mimeType = contentResolver.getType(uri) 
            ?: android.webkit.MimeTypeMap.getSingleton().getMimeTypeFromExtension(java.io.File(fileName).extension.lowercase()) 
            ?: "*/*"
            
        val openIntent = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, mimeType)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }

        val openPi = PendingIntent.getActivity(this, uri.hashCode(), openIntent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)

        val builder = NotificationCompat.Builder(this, CHAN_ALERTS).setGroup("linkall_transfers")
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentTitle("File received from $from")
            .setContentText(fileName)
            .setAutoCancel(true)
            .setContentIntent(openPi)
        
        // Use a dynamic notification ID unique to the file
        val notifId = NOTIF_ID_FILE_BASE + (uri.hashCode() and 0xFFF)
        notificationManager.notify(notifId, builder.build())
    }

    private fun cancelFileTransferNotification(tid: String) {
        notificationManager.cancel(transferNotifId(tid))
    }

    private fun transferNotifId(tid: String): Int = NOTIF_ID_FILE_BASE + (tid.hashCode() and 0xFFF)
    private fun pairingNotifId(deviceId: String): Int = NOTIF_ID_PAIRING_BASE + (deviceId.hashCode() and 0xFFF)

    private fun formatBytes(bytes: Long): String = when {
        bytes >= 1_048_576L -> "%.1f MB".format(bytes / 1_048_576.0)
        bytes >= 1_024L     -> "%.0f KB".format(bytes / 1_024.0)
        else                -> "$bytes B"
    }

    private fun formatEta(seconds: Long): String = when {
        seconds < 0L -> ""
        seconds < 60L -> "${seconds}s"
        seconds < 3_600L -> "${seconds / 60}m"
        else -> "${seconds / 3_600}h"
    }

    private fun buildTransferStatusLine(
        percent: Int,
        bytesReceived: Long,
        totalBytes: Long = 0L,
        speedBps: Long,
        etaSecs: Long
    ): String {
        val exactPercentStr = if (totalBytes > 0L) {
            String.format("%.1f%%", ((bytesReceived.toDouble() / totalBytes.toDouble()).coerceIn(0.0, 1.0)) * 100.0)
        } else {
            "${percent}%"
        }
        val parts = mutableListOf(exactPercentStr)
        if (bytesReceived > 0L) {
            parts += formatBytes(bytesReceived) + if (totalBytes > 0L) " / ${formatBytes(totalBytes)}" else ""
        }
        if (speedBps > 0L) {
            parts += "${formatBytes(speedBps)}/s"
        }
        if (etaSecs >= 0L) {
            parts += "ETA ${formatEta(etaSecs)}"
        }
        return parts.joinToString("  ·  ")
    }

    private fun isCriticalFailure(msg: String): Boolean =
        msg.contains("heartbeat timeout", ignoreCase = true) ||
        msg.contains("network lost", ignoreCase = true) ||
        msg.contains("listener rebind failed", ignoreCase = true)

    // ── Clipboard watch (Kotlin → Rust) ──────────────────────────────────────

    private fun scheduleClipboardWatch() {
        val interval = clipInterval
        handler.postDelayed(object : Runnable {
            override fun run() {
                // Since Android 10 a background app reads an empty clipboard
                // unless it has focus or an enabled accessibility service, so
                // polling 2-5 times a second otherwise only kept the CPU awake.
                // The slow tick notices the app coming to the foreground.
                val canRead = LinkAllApp.isAppInForeground ||
                    android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.Q ||
                    isClipboardAccessibilityEnabled()
                if (canRead) checkClipboard()
                if (engineHandle != 0L) {
                    handler.postDelayed(this, if (canRead) clipInterval else CLIP_UNREADABLE_MS)
                }
            }
        }, interval)
    }

    private fun isClipboardAccessibilityEnabled(): Boolean {
        val enabled = android.provider.Settings.Secure.getString(
            contentResolver,
            android.provider.Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES
        ) ?: return false
        return enabled.contains("$packageName/", ignoreCase = true)
    }

    private fun checkClipboard() {
        if (engineHandle == 0L || !isSyncEnabled()) return
        if (!hasConnectedPeers()) return
        if (suppressNext) { suppressNext = false; return }

        val clip = clipboardManager.primaryClip ?: return
        if (clip.itemCount == 0) return
        val item = clip.getItemAt(0)

        val text = item.text?.toString()?.trim()
        if (!text.isNullOrEmpty()) {
            if (text.length > 5_000_000) {
                Log.w(TAG, "Clipboard text too large to sync (${text.length} chars)")
                return
            }
            val sig = "text:${text.hashCode()}"
            if (sig != lastClipboardSignature) {
                lastClipboardSignature = sig
                withEngine { live -> LinkAllJni.pushText(live, text) }
            }
            return
        }

        val uri = item.uri ?: return
        val sig = "uri:$uri"
        if (sig == lastClipboardSignature) return

        val clipboardMime = contentResolver.getType(uri).orEmpty()
        if (!clipboardMime.startsWith("image/")) {
            backgroundExecutor.execute {
                val sent = sendSharedUri(uri, preferredName = null, fallbackIndex = 1, targetDeviceId = null)
                if (sent != null) {
                    lastClipboardSignature = sig
                    TransferManager.pendingOutboundTransferIds.add(sent.transferId)
                    ActivityFeedManager.addToFeed(
                        ActivityEntry(
                            deviceName = "All devices",
                            kind = ActivityKind.FILE_SENT,
                            preview = sent.displayName,
                            transferId = sent.transferId
                        )
                    )
                    broadcastStatus()
                }
            }
            return
        }

        when (val payload = readClipboardUri(uri)) {
            null -> Unit
            is OutgoingPayload.Image -> {
                lastClipboardSignature = sig
                withEngine { live -> LinkAllJni.pushImage(live, payload.mime, payload.data) }
            }
            is OutgoingPayload.File -> {
                lastClipboardSignature = sig
                withEngine { live -> LinkAllJni.pushFile(live, payload.name, payload.data) }
            }
        }
    }

    private fun sendSharedUris(
        uriStrings: List<String>,
        preferredName: String?,
        targetDeviceId: String?
    ) {
        if (engineHandle == 0L) return
        if (!hasConnectedPeers()) {
            Log.i(TAG, "Ignoring shared URIs because no peers are connected")
            return
        }
        if (targetDeviceId != null && !isPeerConnected(targetDeviceId)) {
            Log.w(TAG, "Ignoring shared URIs because target peer is disconnected: $targetDeviceId")
            return
        }
        var sentAny = false
        uriStrings.forEachIndexed { index, rawUri ->
            val uri = runCatching { Uri.parse(rawUri) }.getOrNull() ?: return@forEachIndexed
            val staged = sendSharedUri(
                uri = uri,
                preferredName = preferredName?.takeIf { uriStrings.size == 1 },
                fallbackIndex = index + 1,
                targetDeviceId = targetDeviceId,
            )
            val tid = staged?.transferId
            if (staged != null && tid != null) {
                TransferManager.pendingOutboundTransferIds.add(tid)
                sentAny = true
                Log.i(
                    TAG,
                    "Queued shared URI ${staged.displayName} (${staged.sizeBytes} bytes, ${if (staged.direct) "direct" else "staged"}) for target=${targetDeviceId ?: "all"}"
                )
                val targetName = if (targetDeviceId != null) {
                    connectedPeerIds[targetDeviceId] ?: "Device"
                } else if (connectedPeerIds.size == 1) {
                    connectedPeerIds.values.first()
                } else {
                    "All devices"
                }
                ActivityFeedManager.addToFeed(
                    ActivityEntry(
                        deviceName = targetName,
                        kind = ActivityKind.FILE_SENT,
                        preview = staged.displayName,
                        transferId = tid
                    )
                )
            } else {
                Log.w(TAG, "Failed to queue shared URI: $rawUri")
            }
        }
        if (sentAny) {
            persistStatus()
            broadcastStatus()
        }
    }

    // ── Apply incoming clipboard ──────────────────────────────────────────────

    private fun applyText(text: String, from: String) {
        suppressNext = true
        lastClipboardSignature = "text:${text.hashCode()}"
        // Binder carries at most about 1 MB, so very long text is refused.
        try {
            clipboardManager.setPrimaryClip(
                android.content.ClipData.newPlainText("linkall", text)
            )
        } catch (e: RuntimeException) {
            suppressNext = false
            Log.w(TAG, "Clipboard from $from not applied: ${text.length} chars is too long", e)
            return
        }
        // The event handler that received this text already added its feed entry.
        broadcastStatus()

        // Respect user opt-in for clipboard copy notifications (default OFF)
        if (isClipboardNotifyEnabled()) {
            updateForegroundNotification() // update subtitle only — no new notification
        }
    }

    private fun applyBinaryClipboard(
        data: ByteArray,
        name: String,
        mime: String?,
        from: String,
        isFile: Boolean
    ) {
        serviceScope.launch(kotlinx.coroutines.Dispatchers.IO) {
            val saveDir = if (isFile) getDownloadsDir() else cacheDir
            val file = writeBinaryFile(name, data, mime, saveDir)

            // Copy file to public Downloads if it is a file
            if (isFile) {
                saveFileToPublicDownloads(file)
            }
            val finalFile = file

            val uri = FileProvider.getUriForFile(this@LinkAllService, "$packageName.fileprovider", file) // use secure private file for URI
            
            kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {
                suppressNext = true
                lastClipboardSignature = "uri:$uri"
                clipboardManager.setPrimaryClip(
                    android.content.ClipData.newUri(contentResolver, finalFile.name, uri)
                )

                val kind = if (mime?.startsWith("image/") == true) {
                    ActivityKind.CLIPBOARD_IMAGE
                } else {
                    ActivityKind.FILE_RECEIVED
                }
                ActivityFeedManager.addToFeed(ActivityEntry(deviceName = from, kind = kind, preview = finalFile.name))
                broadcastStatus()

                if (isFile) {
                    // Files always get an explicit notification — user needs to know where it landed
                    showFileReceivedNotification(from, finalFile.name, uri)
                }
                // Images and clipboard binary: silent — activity feed only
            }
        }
    }

    // ── File I/O ──────────────────────────────────────────────────────────────

    private fun getDownloadsDir(): File {
        val base = getExternalFilesDir(android.os.Environment.DIRECTORY_DOWNLOADS) ?: filesDir
        return File(base, "Link All").also { it.mkdirs() }
    }

    /**
     * Where the engine writes files as they arrive. Download/Link All when this
     * app can write there (older Android, or all-files access); otherwise the
     * app's own folder. Files outside Download/Link All are copied into
     * Downloads through MediaStore once complete, so either way the user ends
     * up with the file in Downloads.
     */
    private fun receiveStagingDir(): File {
        val publicDir = File(
            android.os.Environment.getExternalStoragePublicDirectory(
                android.os.Environment.DIRECTORY_DOWNLOADS
            ),
            "Link All"
        )
        val writable = try {
            publicDir.mkdirs()
            val probe = File(publicDir, ".write-test")
            probe.createNewFile().also { probe.delete() } || publicDir.canWrite()
        } catch (_: Exception) {
            false
        }
        if (writable) return publicDir
        Log.w(TAG, "Cannot write to $publicDir; receiving into the app's own folder")
        val base = getExternalFilesDir(null) ?: filesDir
        return File(base, "incoming").also { it.mkdirs() }
    }

    private fun saveFileToPublicDownloads(sourceFile: File): String? {
        if (!sourceFile.exists()) return null
        val mimeType = MimeTypeMap.getSingleton().getMimeTypeFromExtension(sourceFile.extension.lowercase()) ?: "*/*"
        
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val resolver = contentResolver
            val contentValues = android.content.ContentValues().apply {
                put(android.provider.MediaStore.MediaColumns.DISPLAY_NAME, sourceFile.name)
                put(android.provider.MediaStore.MediaColumns.MIME_TYPE, mimeType)
                put(android.provider.MediaStore.MediaColumns.RELATIVE_PATH, android.os.Environment.DIRECTORY_DOWNLOADS + "/Link All")
            }
            val uri = resolver.insert(android.provider.MediaStore.Downloads.EXTERNAL_CONTENT_URI, contentValues) ?: return null
            try {
                resolver.openOutputStream(uri)?.use { outStream ->
                    java.io.FileInputStream(sourceFile).use { inStream ->
                        inStream.copyTo(outStream)
                    }
                }
                return uri.toString()
            } catch (e: Exception) {
                Log.e(TAG, "Failed to copy file to public Downloads using MediaStore", e)
                return null
            }
        } else {
            // For Android 9 and below, write directly using file system
            val destDir = File(android.os.Environment.getExternalStoragePublicDirectory(android.os.Environment.DIRECTORY_DOWNLOADS), "Link All")
            destDir.mkdirs()
            val destFile = File(destDir, sourceFile.name)
            try {
                java.io.FileInputStream(sourceFile).use { inStream ->
                    java.io.FileOutputStream(destFile).use { outStream ->
                        inStream.copyTo(outStream)
                    }
                }
                // Files written straight to disk stay invisible to gallery apps until scanned.
                android.media.MediaScannerConnection.scanFile(this, arrayOf(destFile.absolutePath), arrayOf(mimeType), null)
                val uri = androidx.core.content.FileProvider.getUriForFile(
                    this, "$packageName.fileprovider",
                    destFile
                )
                return uri.toString()
            } catch (e: Exception) {
                Log.e(TAG, "Failed to copy file to public Downloads using file APIs", e)
                return null
            }
        }
    }

    private fun writeBinaryFile(
        name: String,
        data: ByteArray,
        mime: String?,
        dir: File
    ): File {
        dir.mkdirs()
        val ext = mime?.let {
            MimeTypeMap.getSingleton().getExtensionFromMimeType(it.substringBefore(';'))
        }?.takeIf { it.isNotBlank() }

        val safe = sanitize(name, ext)
        var target = File(dir, safe)
        var n = 2
        while (target.exists()) {
            val stem = target.nameWithoutExtension
            val suf  = target.extension.takeIf { it.isNotBlank() }?.let { ".$it" }.orEmpty()
            target = File(dir, "$stem-$n$suf")
            n++
        }
        FileOutputStream(target).use { it.write(data) }
        return target
    }

    private fun sanitize(raw: String, fallbackExt: String?): String {
        val clean = raw.trim().replace(Regex("[/:\\\\*?\"<>|]"), "-")
        if (clean.isNotEmpty()) return clean
        return if (fallbackExt.isNullOrBlank()) "linkall-file" else "linkall-file.$fallbackExt"
    }

    private fun readClipboardUri(uri: Uri): OutgoingPayload? = readOutgoingUri(uri, preferredName = null)

    private fun readOutgoingUri(uri: Uri, preferredName: String?): OutgoingPayload? = runCatching {
        val mime = resolveUriMimeType(uri).orEmpty()
        val name = resolveUriDisplayName(
            uri = uri,
            preferredName = preferredName,
            fallbackName = "file",
        )
        // Prevent OOM and protocol frame size errors for massive files (e.g. videos/panoramas)
        // Limit clipboard pushes to 32MB. Larger files must use standard file transfer.
        var size = 0L
        contentResolver.query(uri, null, null, null, null)?.use { cursor ->
            if (cursor.moveToFirst()) {
                val sizeIndex = cursor.getColumnIndex(android.provider.OpenableColumns.SIZE)
                if (sizeIndex != -1) {
                    size = cursor.getLong(sizeIndex)
                }
            }
        }
        if (size > 32L * 1024 * 1024) {
            Log.w(TAG, "Skipping clipboard payload > 32MB ($size bytes). Please use 'Send Files' instead.")
            return@runCatching null
        }

        val bytes = openUriInputStream(uri)?.use { it.readBytes() } ?: return null
        if (mime.startsWith("image/")) OutgoingPayload.Image(mime.ifEmpty { "image/png" }, bytes)
        else OutgoingPayload.File(name, bytes)
    }.onFailure { Log.w(TAG, "Failed to read clipboard URI $uri", it) }.getOrNull()

    private fun imageNameForMime(mime: String): String {
        val ext = MimeTypeMap.getSingleton().getExtensionFromMimeType(mime.substringBefore(';')) ?: "png"
        return "LinkAll-image.$ext"
    }

    private fun textContentHash(text: String): String {
        val digest = MessageDigest.getInstance("SHA-256")
        digest.update('T'.code.toByte())
        digest.update(text.toByteArray(Charsets.UTF_8))
        return digest.digest().joinToString("") { "%02x".format(it) }
    }

    private sealed interface OutgoingPayload {
        data class Image(val mime: String, val data: ByteArray) : OutgoingPayload
        data class File(val name: String, val data: ByteArray) : OutgoingPayload
    }

    private data class SentSharedFile(
        val transferId: String,
        val displayName: String,
        val sizeBytes: Long,
        val direct: Boolean,
    )

    /**
     * Queue a shared URI for sending. Seekable files are read straight from
     * the provider's file descriptor so the transfer starts at once; only
     * streams without a size (pipes, some cloud providers) are copied into
     * the cache first.
     */
    private fun sendSharedUri(
        uri: Uri,
        preferredName: String?,
        fallbackIndex: Int,
        targetDeviceId: String?,
    ): SentSharedFile? {
        val displayName = runCatching {
            resolveUriDisplayName(uri, preferredName, "Shared file $fallbackIndex")
        }.getOrDefault("Shared file $fallbackIndex")
        val mime = resolveUriMimeType(uri)?.takeIf { it.isNotBlank() } ?: "application/octet-stream"

        val pfd = runCatching {
            if (uri.scheme.equals("file", ignoreCase = true)) {
                uri.path?.let { ParcelFileDescriptor.open(File(it), ParcelFileDescriptor.MODE_READ_ONLY) }
            } else {
                contentResolver.openFileDescriptor(uri, "r")
            }
        }.getOrNull()
        if (pfd != null) {
            val size = pfd.statSize
            if (size >= 0) {
                // Rust owns the descriptor from here and closes it.
                val tid = withEngine { live -> LinkAllJni.sendFileFd(live, pfd.detachFd(), displayName, mime, targetDeviceId) }
                return tid?.let { SentSharedFile(it, displayName, size, direct = true) }
            }
            runCatching { pfd.close() }
        }

        val staged = stageSharedUri(uri, preferredName, fallbackIndex) ?: return null
        val tid = withEngine { live -> LinkAllJni.sendFilePath(
            live,
            staged.localFile.absolutePath,
            staged.displayName,
            staged.mimeType,
            targetDeviceId,
            null,
            false,
            1
        ) } ?: return null
        return SentSharedFile(tid, staged.displayName, staged.localFile.length(), direct = false)
    }

    private data class StagedOutgoingFile(
        val localFile: File,
        val displayName: String,
        val mimeType: String,
    )

    private fun stageSharedUri(
        uri: Uri,
        preferredName: String?,
        fallbackIndex: Int,
    ): StagedOutgoingFile? = runCatching {
        val mime = resolveUriMimeType(uri)
            ?.takeIf { it.isNotBlank() }
            ?: "application/octet-stream"
        val ext = MimeTypeMap.getSingleton()
            .getExtensionFromMimeType(mime.substringBefore(';'))
        val displayName = resolveUriDisplayName(
            uri = uri,
            preferredName = preferredName,
            fallbackName = "Shared file $fallbackIndex",
        )
        val stagedDir = File(cacheDir, "shared-outgoing").also { it.mkdirs() }
        cleanupStagedOutgoingFiles(stagedDir)
        val stagedFile = uniqueFileInDir(stagedDir, sanitize(displayName, ext))
        openUriInputStream(uri)?.use { input ->
            FileOutputStream(stagedFile).use { output ->
                input.copyTo(output, 256 * 1024)
            }
        } ?: return null
        StagedOutgoingFile(stagedFile, displayName, mime)
    }.onFailure { Log.w(TAG, "Failed to stage shared URI $uri", it) }.getOrNull()

    private fun resolveUriDisplayName(
        uri: Uri,
        preferredName: String?,
        fallbackName: String,
    ): String {
        preferredName?.trim()?.takeIf { it.isNotEmpty() }?.let { return it }

        if (uri.scheme.equals("file", ignoreCase = true)) {
            uri.path
                ?.let(::File)
                ?.name
                ?.takeIf { it.isNotBlank() }
                ?.let { return it }
        }

        val cursor = contentResolver.query(
            uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null
        )
        cursor?.use {
            val col = it.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            if (col >= 0 && it.moveToFirst()) {
                it.getString(col)?.takeIf(String::isNotBlank)?.let { displayName -> return displayName }
            }
        }

        return uri.lastPathSegment?.takeIf { it.isNotBlank() } ?: fallbackName
    }

    private fun resolveUriMimeType(uri: Uri): String? {
        contentResolver.getType(uri)
            ?.takeIf { it.isNotBlank() }
            ?.let { return it }

        if (uri.scheme.equals("file", ignoreCase = true)) {
            val ext = uri.path
                ?.let(::File)
                ?.extension
                ?.lowercase()
                ?.takeIf { it.isNotBlank() }
            if (ext != null) {
                MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext)?.let { return it }
            }
        }

        return null
    }

    private fun openUriInputStream(uri: Uri): InputStream? {
        if (uri.scheme.equals("file", ignoreCase = true)) {
            val file = uri.path?.let(::File)?.takeIf(File::exists) ?: return null
            return file.inputStream()
        }

        return contentResolver.openInputStream(uri)
    }

    // ── Folder transfers ─────────────────────────────────────────────────────
    //
    // A folder travels as a batch of file transfers; the engine paces them
    // and reports the folder once (CR_EVENT_FOLDER_TRANSFER_COMPLETE). Here a
    // folder in flight is one row and one notification, keyed by
    // FOLDER_ROW_PREFIX + batch id.

    private data class FolderInFlight(
        val batchId: String,
        val name: String,
        val fileCount: Int,
        val finished: Int,
        val outbound: Boolean,
        /** Whole folder done, 0..1, counting bytes of files in flight. */
        val progress: Double,
        val speedBps: Long,
    )

    private fun foldersInFlight(): List<FolderInFlight> {
        if (engineHandle == 0L) return emptyList()
        val json = withEngine { live -> LinkAllJni.foldersJson(live) } ?: return emptyList()
        return runCatching {
            val arr = org.json.JSONArray(json)
            (0 until arr.length()).map { i ->
                val o = arr.getJSONObject(i)
                FolderInFlight(
                    batchId = o.getString("batch_id"),
                    name = o.optString("folder_name", "Folder"),
                    fileCount = o.optInt("file_count", 1),
                    finished = o.optInt("done_count") + o.optInt("failed_count"),
                    outbound = o.optBoolean("outbound"),
                    progress = o.optDouble("progress", 0.0),
                    speedBps = o.optLong("speed_bps"),
                )
            }
        }.getOrDefault(emptyList())
    }

    private val folderAskedAt = java.util.concurrent.ConcurrentHashMap<String, Long>()

    private fun shouldAskAboutFolder(key: String): Boolean {
        val now = android.os.SystemClock.elapsedRealtime()
        val last = folderAskedAt[key]
        if (last != null && now - last < 5 * 60_000L) return false
        if (folderAskedAt.size > 64) folderAskedAt.clear()
        folderAskedAt[key] = now
        return true
    }

    private val lastFolderRefreshMs = java.util.concurrent.ConcurrentHashMap<String, Long>()

    /**
     * Refresh a folder's row and notification from the engine. Called on
     * its files' progress (rate-limited) and whenever one finishes (always),
     * so the count never lags behind the other device.
     */
    private fun onFolderItemProgress(fileName: String, isOutbound: Boolean, peerName: String, force: Boolean = false) {
        val top = fileName.substringBefore('/')
        val key = "$top/$isOutbound"
        val nowMs = android.os.SystemClock.elapsedRealtime()
        if (!force && nowMs - (lastFolderRefreshMs[key] ?: 0L) < 250L) return
        lastFolderRefreshMs[key] = nowMs
        val folder = foldersInFlight().firstOrNull { it.name == top && it.outbound == isOutbound } ?: return
        val rowId = FOLDER_ROW_PREFIX + folder.batchId
        val percent = (folder.progress * 100).toInt().coerceIn(0, 100)
        val speedBps = folder.speedBps
        val noun = if (folder.fileCount == 1) "file" else "files"
        TransferManager.activeTransfers[rowId] = TransferProgress(
            id = rowId,
            fileName = "${folder.name} · ${folder.finished} of ${folder.fileCount} $noun",
            // No byte totals: the row's bar then follows `percent`, the
            // engine's whole-folder progress.
            percent = percent,
            bytesReceived = 0,
            totalBytes = 0,
            speedBps = speedBps,
            etaSecs = 0,
            state = TransferState.PROGRESS,
            peerName = peerName,
            isOutbound = isOutbound,
        )
        TransferManager.publishActiveTransfers()
        val now = System.currentTimeMillis()
        if (!force && now - (lastTransferNotifTimes[rowId] ?: 0L) < 500L) return
        lastTransferNotifTimes[rowId] = now
        val cancelPi = PendingIntent.getService(this, rowId.hashCode() + 2,
            Intent(ACTION_CANCEL_FILE_TRANSFER).apply {
                `package` = packageName
                putExtra(EXTRA_TRANSFER_ID, rowId)
            }, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val notif = NotificationCompat.Builder(this, CHAN_ALERTS).setGroup("linkall_transfers")
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentTitle(if (isOutbound) "Sending ${folder.name}" else "Receiving ${folder.name}")
            .setContentText("${folder.finished} of ${folder.fileCount} $noun · $percent%")
            .setProgress(100, percent, false)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .addAction(android.R.drawable.ic_menu_close_clear_cancel, "Cancel", cancelPi)
            .build()
        notificationManager.notify(transferNotifId(rowId), notif)
    }

    private fun onFolderComplete(folder: String, peer: String, total: Int, failed: Int, destDir: String) {
        // Drop the rows of folders the engine no longer has in flight.
        val live = foldersInFlight().map { FOLDER_ROW_PREFIX + it.batchId }.toSet()
        TransferManager.activeTransfers.keys
            .filter { it.startsWith(FOLDER_ROW_PREFIX) && it !in live }
            .forEach { rowId ->
                TransferManager.activeTransfers.remove(rowId)
                lastTransferNotifTimes.remove(rowId)
                notificationManager.cancel(transferNotifId(rowId))
            }
        TransferManager.publishActiveTransfers(force = true)

        val noun = if (total == 1) "file" else "files"
        val files = if (failed == 0) "$total $noun" else "${total - failed} of $total $noun"
        val received = destDir.isNotEmpty()
        addActivity(ActivityEntry(
            deviceName = peer,
            kind = if (received) ActivityKind.FILE_TRANSFER_COMPLETE else ActivityKind.FILE_SENT,
            preview = "$folder ($files)",
            progressPercent = 100,
            destPath = destDir,
        ))
        val notif = NotificationCompat.Builder(this, CHAN_ALERTS).setGroup("linkall_transfers")
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentTitle(if (received) "Folder received from $peer" else "Folder sent to $peer")
            .setContentText(if (received) "$folder ($files) · Downloads/Link All" else "$folder ($files)")
            .setAutoCancel(true)
            .build()
        notificationManager.notify(NOTIF_ID_FILE_BASE + ("folder/$folder".hashCode() and 0xFFF), notif)
    }

    /**
     * Send a folder picked with the system folder picker. Walks the document
     * tree, then hands the engine one file at a time; the engine keeps a few
     * in flight and blocks the rest, so run this on a background thread.
     */
    private fun sendFolderTree(treeUri: Uri, targetDeviceId: String?) {
        val handle = engineHandle
        if (handle == 0L) return
        val rootId = runCatching { android.provider.DocumentsContract.getTreeDocumentId(treeUri) }.getOrNull() ?: return
        val folderName = runCatching {
            contentResolver.query(
                android.provider.DocumentsContract.buildDocumentUriUsingTree(treeUri, rootId),
                arrayOf(android.provider.DocumentsContract.Document.COLUMN_DISPLAY_NAME), null, null, null
            )?.use { if (it.moveToFirst()) it.getString(0) else null }
        }.getOrNull()?.takeIf { it.isNotBlank() } ?: "Folder"

        val files = listFolderTree(treeUri, rootId)
        if (files.isEmpty()) {
            Log.i(TAG, "Folder $folderName has no files to send")
            android.os.Handler(android.os.Looper.getMainLooper()).post {
                android.widget.Toast.makeText(applicationContext, "\"$folderName\" has no files to send", android.widget.Toast.LENGTH_LONG).show()
            }
            return
        }
        val batchId = java.util.UUID.randomUUID().toString()
        for ((docId, relPath) in files) {
            val uri = android.provider.DocumentsContract.buildDocumentUriUsingTree(treeUri, docId)
            val pfd = runCatching { contentResolver.openFileDescriptor(uri, "r") }.getOrNull() ?: continue
            val tid = LinkAllJni.sendFolderItemFd(
                handle, pfd.detachFd(), relPath, folderName, targetDeviceId, batchId, files.size
            ) ?: break
            TransferManager.pendingOutboundTransferIds.add(tid)
        }
        withEngine { live -> LinkAllJni.finishFolderSend(live, batchId) }
    }

    /** Every file under a document tree as (document id, path inside the folder). */
    private fun listFolderTree(treeUri: Uri, rootId: String): List<Pair<String, String>> {
        val skipped = setOf(".DS_Store", "Thumbs.db", "desktop.ini")
        val out = mutableListOf<Pair<String, String>>()
        val dirs = ArrayDeque(listOf(rootId to ""))
        val columns = arrayOf(
            android.provider.DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            android.provider.DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            android.provider.DocumentsContract.Document.COLUMN_MIME_TYPE,
        )
        while (dirs.isNotEmpty() && out.size < 10_000) {
            val (dirId, prefix) = dirs.removeFirst()
            val children = android.provider.DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, dirId)
            val rows = runCatching {
                contentResolver.query(children, columns, null, null, null)?.use { c ->
                    buildList { while (c.moveToNext()) add(Triple(c.getString(0), c.getString(1) ?: "", c.getString(2) ?: "")) }
                }
            }.getOrNull() ?: continue
            for ((id, name, mime) in rows.sortedBy { it.second }) {
                if (name.isEmpty() || name in skipped || name.startsWith("._")) continue
                if (mime == android.provider.DocumentsContract.Document.MIME_TYPE_DIR) {
                    dirs.addLast(id to "$prefix$name/")
                } else {
                    out.add(id to "$prefix$name")
                }
            }
        }
        return out
    }

    private fun cleanupStagedOutgoingFiles(dir: File) {
        val cutoff = System.currentTimeMillis() - 12 * 60 * 60 * 1000L
        dir.listFiles()?.forEach { file ->
            if (file.lastModified() < cutoff) {
                runCatching { file.delete() }
            }
        }
    }

    private fun uniqueFileInDir(dir: File, fileName: String): File {
        var candidate = File(dir, fileName)
        if (!candidate.exists()) return candidate

        val stem = candidate.nameWithoutExtension.ifBlank { "linkall-share" }
        val ext = candidate.extension.takeIf { it.isNotBlank() }?.let { ".$it" }.orEmpty()
        var index = 2
        while (candidate.exists()) {
            candidate = File(dir, "$stem-$index$ext")
            index++
        }
        return candidate
    }

    // ── Call continuity ──────────────────────────────────────────────────────
    //
    // Remote call actions (accept/decline) from the Mac are executed via TelecomManager.

    private var callStateReceiver: android.content.BroadcastReceiver? = null
    /** Last call state seen: "idle", "ringing", "offhook", or "outgoing" for a call this phone placed. */
    private var lastCallState = "idle"
    private var lastCallNumber = ""
    private var callStateCallback: android.telephony.TelephonyCallback? = null

    /**
     * Hears call state directly while the service runs. The manifest
     * receiver alone missed calls on some phones; it stays for the caller's
     * number, which this callback doesn't carry. Both feed onCallStateUpdate,
     * which sends each change once.
     */
    private fun registerCallStateCallback() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S || callStateCallback != null) return
        if (!hasCallPermissions()) return
        val callback = object : android.telephony.TelephonyCallback(),
            android.telephony.TelephonyCallback.CallStateListener {
            override fun onCallStateChanged(state: Int) {
                if (prefs().getBoolean("call_continuity_enabled", false)) onCallStateUpdate(state, null)
            }
        }
        runCatching {
            getSystemService(android.telephony.TelephonyManager::class.java)
                .registerTelephonyCallback(mainExecutor, callback)
            callStateCallback = callback
        }.onFailure { Log.w(TAG, "Call state callback unavailable", it) }
    }

    private fun unregisterCallStateCallback() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return
        callStateCallback?.let { cb ->
            runCatching { getSystemService(android.telephony.TelephonyManager::class.java).unregisterTelephonyCallback(cb) }
        }
        callStateCallback = null
    }

    private fun onCallStateUpdate(state: Int, incomingNumber: String?) {
        val stateStr = when (state) {
            android.telephony.TelephonyManager.CALL_STATE_RINGING -> "ringing"
            android.telephony.TelephonyManager.CALL_STATE_OFFHOOK -> "offhook"
            android.telephony.TelephonyManager.CALL_STATE_IDLE    -> "idle"
            else -> return
        }
        // Android reports OFFHOOK for outgoing calls too (IDLE -> OFFHOOK, no RINGING). Those are
        // this phone placing a call, not a call to take on the desktop, so they are not forwarded.
        // Forwarding them let a caller's own "offhook" overwrite the callee's "ringing" on peers.
        val previous = lastCallState
        lastCallState = when {
            stateStr == "offhook" && previous != "ringing" && previous != "offhook" -> "outgoing"
            stateStr == "offhook" && previous == "outgoing" -> "outgoing"
            else -> stateStr
        }
        if (lastCallState == "outgoing") {
            Log.i(TAG, "Call state: outgoing call, not forwarded")
            return
        }
        if (stateStr == "idle" && previous == "outgoing") return

        val number  = incomingNumber.orEmpty()
        // The callback and the receiver both report each change: send it
        // once, and again only if the receiver brings the number.
        if (stateStr == previous && (number.isEmpty() || number == lastCallNumber)) return
        if (number.isNotEmpty()) lastCallNumber = number
        if (stateStr == "idle") lastCallNumber = ""
        val known = number.ifEmpty { lastCallNumber }
        val contact = resolveContactName(known)
        Log.i(TAG, "Call state: $stateStr hasNumber=${known.isNotEmpty()} hasContact=${contact.isNotEmpty()}")
        val h = engineHandle
        if (h != 0L) {
            withEngine { live -> LinkAllJni.pushCallState(live, stateStr, known, contact) }
        }
        // Keep peers' copy of a live call alive; they drop one not heard of
        // for a while (CALL_LEASE in the core), so a lost "idle" cannot stick.
        if (stateStr == "idle") stopCallRefresh() else startCallRefresh()
        // Show/dismiss the Android-side call notification
        when (stateStr) {
            "ringing" -> showIncomingCallNotification(known, contact)
            "idle", "offhook" -> notificationManager.cancel(NOTIF_ID_CALL)
        }
    }

    /**
     * Sends "idle" when the phone has no call, bypassing the once-per-change
     * check. A peer keeps the last state it heard, so an "idle" it missed
     * (a reconnect, this service restarting mid-call) left a call showing on
     * it forever. Runs on the main thread, like the call callback.
     */
    private val callRefreshHandler = android.os.Handler(android.os.Looper.getMainLooper())
    private val callRefresh = object : Runnable {
        override fun run() {
            val h = engineHandle
            val tm = getSystemService(android.telephony.TelephonyManager::class.java)
            @Suppress("DEPRECATION")
            val state = runCatching { tm?.callState }.getOrNull()
            if (state == android.telephony.TelephonyManager.CALL_STATE_IDLE || lastCallState == "idle" || lastCallState == "outgoing") {
                // The call ended without a callback saying so.
                resyncIdleCallState(delayMs = 0)
                return
            }
            if (h != 0L) withEngine { live -> LinkAllJni.pushCallState(live, lastCallState, lastCallNumber, resolveContactName(lastCallNumber)) }
            callRefreshHandler.postDelayed(this, CALL_REFRESH_MS)
        }
    }

    private fun startCallRefresh() {
        callRefreshHandler.removeCallbacks(callRefresh)
        callRefreshHandler.postDelayed(callRefresh, CALL_REFRESH_MS)
    }

    private fun stopCallRefresh() = callRefreshHandler.removeCallbacks(callRefresh)

    private fun resyncIdleCallState(delayMs: Long) {
        android.os.Handler(android.os.Looper.getMainLooper()).postDelayed({
            if (!prefs().getBoolean("call_continuity_enabled", false) || !hasCallPermissions()) return@postDelayed
            val tm = getSystemService(android.telephony.TelephonyManager::class.java) ?: return@postDelayed
            @Suppress("DEPRECATION")
            val state = runCatching { tm.callState }.getOrNull() ?: return@postDelayed
            if (state != android.telephony.TelephonyManager.CALL_STATE_IDLE) return@postDelayed
            stopCallRefresh()
            lastCallState = "idle"
            lastCallNumber = ""
            notificationManager.cancel(NOTIF_ID_CALL)
            val h = engineHandle
            if (h != 0L) withEngine { live -> LinkAllJni.pushCallState(live, "idle", "", "") }
        }, delayMs)
    }

    private fun handleCallStateIntent(intent: Intent?) {
        if (intent == null) return
        if (!hasCallPermissions()) return
        val prefs = getSharedPreferences(PREFS_NAME, MODE_PRIVATE)
        if (!prefs.getBoolean("call_continuity_enabled", false)) return

        val stateStr = intent.getStringExtra(android.telephony.TelephonyManager.EXTRA_STATE)
        val number = intent.getStringExtra(android.telephony.TelephonyManager.EXTRA_INCOMING_NUMBER)
        val state = when (stateStr) {
            android.telephony.TelephonyManager.EXTRA_STATE_RINGING -> android.telephony.TelephonyManager.CALL_STATE_RINGING
            android.telephony.TelephonyManager.EXTRA_STATE_OFFHOOK -> android.telephony.TelephonyManager.CALL_STATE_OFFHOOK
            android.telephony.TelephonyManager.EXTRA_STATE_IDLE -> android.telephony.TelephonyManager.CALL_STATE_IDLE
            else -> -1
        }
        if (state != -1) {
            onCallStateUpdate(state, number)
        }
    }

    private fun handleTrustPeer(intent: Intent) {
        val deviceId = intent.getStringExtra(EXTRA_TARGET_DEVICE_ID) ?: return
        val h = engineHandle
        if (h != 0L) {
            serviceScope.launch {
                val result = withEngine { live -> LinkAllJni.trustPeer(live, deviceId) }
                Log.i(TAG, "Manual trust request for $deviceId: result=$result")
                persistStatus()
            }
        }
    }

    private fun handleTrustPeerFromQr(intent: Intent) {
        val deviceId = intent.getStringExtra(EXTRA_TARGET_DEVICE_ID) ?: return
        val token = intent.getStringExtra(EXTRA_TOKEN) ?: return
        val fingerprint = intent.getStringExtra(EXTRA_FINGERPRINT)
        val h = engineHandle
        if (h != 0L) {
            val ip = intent.getStringExtra("ip")
            val port = intent.getIntExtra("port", 47823)
            serviceScope.launch {
                if (ip != null && ip.isNotBlank()) {
                    withEngine { live -> LinkAllJni.connectToPeer(live, ip, port) }
                }
                val result = withEngine { live -> LinkAllJni.trustPeerFromQr(live, deviceId, token, fingerprint) }
                Log.i(TAG, "QR trust request for $deviceId: result=$result")
                persistStatus()
            }
        }
    }

    private fun handleRejectPeer(intent: Intent) {
        val deviceId = intent.getStringExtra(EXTRA_TARGET_DEVICE_ID) ?: return
        val h = engineHandle
        if (h != 0L) {
            serviceScope.launch {
                val result = withEngine { live -> LinkAllJni.rejectPeer(live, deviceId) }
                Log.i(TAG, "Manual reject request for $deviceId: result=$result")
                persistStatus()
            }
        }
    }

    private fun hasCallPermissions(): Boolean =
        checkSelfPermission(android.Manifest.permission.READ_PHONE_STATE) ==
            android.content.pm.PackageManager.PERMISSION_GRANTED

    private fun resolveContactName(number: String): String {
        if (number.isBlank()) return ""
        if (checkSelfPermission(android.Manifest.permission.READ_CONTACTS) !=
            android.content.pm.PackageManager.PERMISSION_GRANTED) return ""
        return runCatching {
            val uri = android.net.Uri.withAppendedPath(
                android.provider.ContactsContract.PhoneLookup.CONTENT_FILTER_URI,
                android.net.Uri.encode(number)
            )
            contentResolver.query(
                uri,
                arrayOf(android.provider.ContactsContract.PhoneLookup.DISPLAY_NAME),
                null, null, null
            )?.use { cursor ->
                if (cursor.moveToFirst()) cursor.getString(0) ?: "" else ""
            } ?: ""
        }.getOrDefault("")
    }

    /** Show a high-priority heads-up notification for an incoming call on Android. */
    private fun showIncomingCallNotification(number: String, contactName: String) {
        val callerLabel = when {
            contactName.isNotBlank() -> contactName
            number.isNotBlank()      -> number
            else                     -> "Unknown caller"
        }

        // Tapping the notification opens the app
        val openPi = PendingIntent.getActivity(
            this, 0,
            packageManager.getLaunchIntentForPackage(packageName),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )

        val notif = NotificationCompat.Builder(this, CHAN_CALLS)
            .setSmallIcon(android.R.drawable.stat_sys_phone_call)
            .setContentTitle("📞 Incoming call")
            .setContentText(callerLabel)
            .setSubText("Link All — showing on your computer")
            .setCategory(NotificationCompat.CATEGORY_CALL)
            .setPriority(NotificationCompat.PRIORITY_MAX)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setOngoing(true)
            .setAutoCancel(false)
            .setContentIntent(openPi)
            .setStyle(NotificationCompat.BigTextStyle()
                .bigText("$callerLabel is calling. Your computer shows it with Accept and Decline.")
                .setSummaryText("Call relay active"))
            .build()

        notificationManager.notify(NOTIF_ID_CALL, notif)
    }

    @Suppress("DEPRECATION")
    private fun handleRemoteCallAction(action: String) {
        var handled = false
        if (action == "accept" || action == "decline") {
            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.O) {
                val tm = getSystemService(TELECOM_SERVICE) as? android.telecom.TelecomManager
                if (tm != null && checkSelfPermission(android.Manifest.permission.ANSWER_PHONE_CALLS) == android.content.pm.PackageManager.PERMISSION_GRANTED) {
                    if (action == "accept") {
                        runCatching { 
                            tm.acceptRingingCall() 
                        }.onSuccess { 
                            Log.i(TAG, "Remote accept: call accepted via TelecomManager")
                            handled = true
                        }.onFailure { Log.w(TAG, "Remote accept failed via TelecomManager", it) }
                    } else if (action == "decline") {
                        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.P) {
                            runCatching { 
                                tm.endCall() 
                            }.onSuccess { 
                                Log.i(TAG, "Remote decline: call ended via TelecomManager")
                                handled = true
                            }.onFailure { Log.w(TAG, "Remote decline failed via TelecomManager", it) }
                        } else {
                            Log.i(TAG, "TelecomManager.endCall() not supported on this Android version")
                        }
                    }
                } else {
                    Log.w(TAG, "ANSWER_PHONE_CALLS permission not granted or TelecomManager unavailable")
                }
            }
            
            if (!handled) {
                Log.i(TAG, "Attempting remote call action '$action' via NotificationListener as fallback...")
                if (LinkAllNotificationListener.triggerCallAction(action)) {
                    Log.i(TAG, "Remote call action '$action' successfully triggered via NotificationListener!")
                } else {
                    Log.w(TAG, "NotificationListener could not handle call action '$action'")
                }
            }
            return
        }

        when (action) {
            "audio_earpiece" -> {
                val am = getSystemService(android.content.Context.AUDIO_SERVICE) as? android.media.AudioManager
                am?.isSpeakerphoneOn = false
                am?.stopBluetoothSco()
                am?.isBluetoothScoOn = false
                Log.i(TAG, "Remote audio route: Earpiece")
            }
            "audio_speaker" -> {
                val am = getSystemService(android.content.Context.AUDIO_SERVICE) as? android.media.AudioManager
                am?.isSpeakerphoneOn = true
                am?.stopBluetoothSco()
                am?.isBluetoothScoOn = false
                Log.i(TAG, "Remote audio route: Speaker")
            }
            "audio_bluetooth" -> {
                val am = getSystemService(android.content.Context.AUDIO_SERVICE) as? android.media.AudioManager
                am?.isSpeakerphoneOn = false
                am?.startBluetoothSco()
                am?.isBluetoothScoOn = true
                Log.i(TAG, "Remote audio route: Bluetooth")
            }
            else -> Log.w(TAG, "Unknown remote call action: $action")
        }
    }

    // ── F20: Battery status monitor ────────────────────────────────────────────────────
    private var batteryReceiver: android.content.BroadcastReceiver? = null

    private fun startBatteryMonitor() {
        if (batteryReceiver != null) return
        val receiver = object : android.content.BroadcastReceiver() {
            private var lastLevel = -1
            private var lastChargingState: Boolean? = null
            override fun onReceive(context: Context, intent: Intent) {
                if (intent.action == Intent.ACTION_BATTERY_CHANGED) {
                    val rawLevel = intent.getIntExtra(android.os.BatteryManager.EXTRA_LEVEL, -1)
                    val scale = intent.getIntExtra(android.os.BatteryManager.EXTRA_SCALE, -1)
                    val status = intent.getIntExtra(android.os.BatteryManager.EXTRA_STATUS, -1)
                    
                    val level = if (rawLevel >= 0 && scale > 0) {
                        (rawLevel * 100f / scale).toInt()
                    } else {
                        rawLevel
                    }
                    
                    val charging = status == android.os.BatteryManager.BATTERY_STATUS_CHARGING ||
                                   status == android.os.BatteryManager.BATTERY_STATUS_FULL

                    // 1% steps while the screen is on (someone may be watching the
                    // desktop's battery readout), 5% while it's off: each push
                    // wakes the radio. Charging changes always go out.
                    val interactive = (context.getSystemService(Context.POWER_SERVICE) as android.os.PowerManager).isInteractive
                    val levelChanged = Math.abs(level - lastLevel) >= (if (interactive) 1 else 5)
                    val statusChanged = charging != lastChargingState

                    if (levelChanged || statusChanged || lastLevel == -1) {
                        lastLevel = level
                        lastChargingState = charging

                        val h = engineHandle
                        if (h != 0L && level >= 0) {
                            Log.i(TAG, "Battery status update: level=$level charging=$charging")
                            withEngine { live -> LinkAllJni.pushBatteryStatus(live, level, charging) }
                        }
                    }
                }
            }
        }
        val filter = android.content.IntentFilter(Intent.ACTION_BATTERY_CHANGED)
        val stickyIntent = registerReceiver(receiver, filter)
        batteryReceiver = receiver
        stickyIntent?.let { receiver.onReceive(this, it) }
        Log.i(TAG, "Battery status monitor started")
    }

    private fun stopBatteryMonitor() {
        batteryReceiver?.let {
            runCatching { unregisterReceiver(it) }
        }
        batteryReceiver = null
        Log.i(TAG, "Battery status monitor stopped")
    }

    private var storageMonitorRunnable: Runnable? = null

    // Summing every image and video row in MediaStore is the expensive part;
    // gallery totals barely move minute to minute, so reuse them for a while.
    private val STORAGE_PUSH_INTERVAL_MS = 5 * 60_000L
    private val STORAGE_MEDIA_RESCAN_MS = 30 * 60_000L
    @Volatile private var cachedMediaSizes: Pair<Long, Long>? = null
    @Volatile private var cachedMediaAt = 0L

    private fun startStorageMonitor() {
        if (storageMonitorRunnable != null) return
        val r = object : Runnable {
            override fun run() {
                // Nobody to report to, or nobody looking at the phone's
                // numbers being refreshed: skip. A peer that connects gets
                // a fresh reading from PEER_CONNECTED.
                val pm = getSystemService(Context.POWER_SERVICE) as android.os.PowerManager
                if (hasConnectedPeers() && pm.isInteractive) pushStorageStatusAsync()
                handler.postDelayed(this, STORAGE_PUSH_INTERVAL_MS)
            }
        }
        storageMonitorRunnable = r
        handler.postDelayed(r, STORAGE_PUSH_INTERVAL_MS)
        Log.i(TAG, "Storage monitor started")
    }

    private fun pushStorageStatusAsync() {
        backgroundExecutor.execute {
            engineLock.readLock {
                val h = engineHandle
                if (h != 0L) {
                    try {
                        val storageManager = getSystemService(android.content.Context.STORAGE_STATS_SERVICE) as? android.app.usage.StorageStatsManager
                        if (storageManager != null) {
                            val rawTotalBytes = storageManager.getTotalBytes(android.os.storage.StorageManager.UUID_DEFAULT)
                            val freeBytes = storageManager.getFreeBytes(android.os.storage.StorageManager.UUID_DEFAULT)

                            val GB = 1_000_000_000L
                            val tiers = longArrayOf(16 * GB, 32 * GB, 64 * GB, 128 * GB, 256 * GB, 512 * GB, 1000 * GB, 2000 * GB)
                            var totalBytes = rawTotalBytes
                            for (tier in tiers) {
                                if (rawTotalBytes <= tier) {
                                    totalBytes = tier
                                    break
                                }
                            }

                            val now = android.os.SystemClock.elapsedRealtime()
                            val media = cachedMediaSizes?.takeIf { now - cachedMediaAt < STORAGE_MEDIA_RESCAN_MS }
                                ?: scanMediaSizes().also {
                                    cachedMediaSizes = it
                                    cachedMediaAt = now
                                }

                            LinkAllJni.pushStorageStatus(h, media.first, media.second, 0L, freeBytes, totalBytes)
                        }
                    } catch (e: Exception) {
                        Log.e(TAG, "Storage telemetry failed", e)
                    }
                }
            }
        }
    }

    /** Total bytes of (images, videos) in MediaStore. */
    private fun scanMediaSizes(): Pair<Long, Long> {
        var imgSize = 0L
        var vidSize = 0L

        val uri = android.provider.MediaStore.Files.getContentUri("external")
        val proj = arrayOf(
            android.provider.MediaStore.Files.FileColumns.MEDIA_TYPE,
            android.provider.MediaStore.Files.FileColumns.SIZE
        )
        val sel = "${android.provider.MediaStore.Files.FileColumns.MEDIA_TYPE}=? OR ${android.provider.MediaStore.Files.FileColumns.MEDIA_TYPE}=?"
        val selArgs = arrayOf(
            android.provider.MediaStore.Files.FileColumns.MEDIA_TYPE_IMAGE.toString(),
            android.provider.MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO.toString()
        )

        contentResolver.query(uri, proj, sel, selArgs, null)?.use { cursor ->
            val typeCol = cursor.getColumnIndexOrThrow(android.provider.MediaStore.Files.FileColumns.MEDIA_TYPE)
            val sizeCol = cursor.getColumnIndexOrThrow(android.provider.MediaStore.Files.FileColumns.SIZE)
            while (cursor.moveToNext()) {
                val type = cursor.getInt(typeCol)
                val size = cursor.getLong(sizeCol)
                if (type == android.provider.MediaStore.Files.FileColumns.MEDIA_TYPE_IMAGE) {
                    imgSize += size
                } else {
                    vidSize += size
                }
            }
        }
        return imgSize to vidSize
    }

    private fun stopStorageMonitor() {
        storageMonitorRunnable?.let { handler.removeCallbacks(it) }
        storageMonitorRunnable = null
        Log.i(TAG, "Storage monitor stopped")
    }

    // ── NSD (Network Service Discovery) ────────────────────────────────────────────────
    //
    // Android does not support Rust’s mdns-sd crate, so we use the
    // platform NSD API here to:
    //   1. Advertise our service (“_deskdrop._tcp”) so the Mac discovers us.
    //   2. Browse for the Mac’s _deskdrop._tcp advertisement.
    //   3. When resolved, call connectToPeer() via JNI so the Rust engine
    //      initiates a TCP handshake.

    private fun startNsdDiscovery() {
        val nm = runCatching { getSystemService(NSD_SERVICE) as NsdManager }.getOrNull()
            ?: run { Log.w(TAG, "NSD: NsdManager unavailable"); return }

        // ── 1. Register our own service so the Mac can find us ───────────────────
        //
        // Include the UUID prefix in the service name so the Mac can identify us
        // even before resolving (and so our own self-filter is reliable).
        // Format: "linkall-<uuid8>-<safename>"
        // Android may suffix " (2)" etc. on collision — we capture the actual name
        // in onServiceRegistered so our self-filter always matches correctly.
        val uuidPrefix = myDeviceUuidPrefix ?: engineHandle.toString().take(8)
        val safeName = resolvedDeviceName()
            .take(16)
            .replace(Regex("[^A-Za-z0-9\\-]"), "-")
            .trimEnd('-')
        val serviceInfo = NsdServiceInfo().apply {
            serviceName = "linkall-$uuidPrefix-$safeName"
            serviceType = NSD_SERVICE_TYPE
            port        = DEFAULT_LINKALL_PORT
            setAttribute("id", myDeviceId ?: "")
            setAttribute("v", LinkAllJni.protocolVersion().toString())
        }

        val regListener = object : NsdManager.RegistrationListener {
            override fun onServiceRegistered(info: NsdServiceInfo) {
                myActualNsdName = info.serviceName
                isNsdRegistered.set(true)
                Log.i(TAG, "NSD: registered '${info.serviceName}'")
                
                // Fix: If stopNsdDiscovery was called while in-flight, unregister now.
                if (pendingNsdUnregister.compareAndSet(true, false)) {
                    runCatching {
                        val n = getSystemService(NSD_SERVICE) as? NsdManager
                        n?.unregisterService(this)
                    }
                    isNsdRegistered.set(false)
                    if (nsdRegistrationListener === this) nsdRegistrationListener = null
                }
            }
            override fun onRegistrationFailed(info: NsdServiceInfo, code: Int) {
                Log.w(TAG, "NSD: registration failed (code=$code)")
                pendingNsdUnregister.set(false)
            }
            override fun onServiceUnregistered(info: NsdServiceInfo) {
                myActualNsdName = null
                isNsdRegistered.set(false)
                Log.i(TAG, "NSD: unregistered '${info.serviceName}'")
            }
            override fun onUnregistrationFailed(info: NsdServiceInfo, code: Int) {
                Log.w(TAG, "NSD: unregistration failed (code=$code)")
            }
        }
        isNsdRegistered.set(false)
        pendingNsdUnregister.set(false)
        nsdRegistrationListener = regListener
        runCatching { nm.registerService(serviceInfo, NsdManager.PROTOCOL_DNS_SD, regListener) }
            .onFailure { Log.w(TAG, "NSD: registerService error", it) }

        // ── 2. Browse for Link All peers (the Mac, other desktops) ──────────────
        val discListener = object : NsdManager.DiscoveryListener {
            override fun onStartDiscoveryFailed(serviceType: String, code: Int) {
                Log.w(TAG, "NSD: discovery start failed (code=$code)")
            }
            override fun onStopDiscoveryFailed(serviceType: String, code: Int) {
                Log.w(TAG, "NSD: discovery stop failed (code=$code)")
            }
            override fun onDiscoveryStarted(serviceType: String) {
                Log.i(TAG, "NSD: discovery started for $serviceType")
            }
            override fun onDiscoveryStopped(serviceType: String) {
                Log.i(TAG, "NSD: discovery stopped")
            }
            override fun onServiceFound(info: NsdServiceInfo) {
                // Quick pre-filter: skip our own service by name before resolving.
                // resolveService is a limited resource on older Android — don't waste it.
                val actual = myActualNsdName
                if (actual != null && info.serviceName == actual) {
                    Log.d(TAG, "NSD: skipping self (pre-resolve) '${info.serviceName}'")
                    return
                }
                val prefix = myDeviceUuidPrefix
                if (prefix != null && info.serviceName.contains(prefix, ignoreCase = true)) {
                    Log.d(TAG, "NSD: skipping self by UUID prefix (pre-resolve) '${info.serviceName}'")
                    return
                }
                Log.i(TAG, "NSD: found '${info.serviceName}'")
                pendingNsdResolves.offer(info)
                processNextNsdResolve()
            }
            override fun onServiceLost(info: NsdServiceInfo) {
                Log.i(TAG, "NSD: lost '${info.serviceName}'")
                // If the lost service is not ours and we're now peerless, retry.
                val actual = myActualNsdName
                if (actual == null || info.serviceName != actual) {
                    if (connectedPeerIds.isEmpty()) scheduleNsdRetry()
                }
            }
        }
        nsdDiscoveryListener = discListener
        runCatching { nm.discoverServices(NSD_SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, discListener) }
            .onFailure { Log.w(TAG, "NSD: discoverServices error", it) }
    }

    private fun getLocalIpAddresses(): Set<String> {
        val ips = mutableSetOf<String>()
        try {
            val interfaces = java.net.NetworkInterface.getNetworkInterfaces()
            if (interfaces != null) {
                for (intf in interfaces) {
                    val addrs = intf.inetAddresses
                    for (addr in addrs) {
                        if (!addr.isLoopbackAddress) {
                            val hostAddr = addr.hostAddress
                            if (hostAddr != null) {
                                ips.add(hostAddr.substringBefore('%')) // Remove IPv6 scope if present
                            }
                        }
                    }
                }
            }
        } catch (ex: Exception) {
            Log.e(TAG, "Failed to get local IPs", ex)
        }
        return ips
    }

    private fun handleResolvedNsdService(info: NsdServiceInfo) {
        try {
            // Android 14+ fix: hostAddresses vs host
            val ip = if (Build.VERSION.SDK_INT >= 34) {
                info.hostAddresses.firstOrNull()?.hostAddress
            } else {
                info.host?.hostAddress
            } ?: return

            val port = info.port
            Log.i(TAG, "NSD: resolved peer at $ip:$port (service='${info.serviceName}')")
            // Skip loopback addresses (self-discovery)
            if (ip.startsWith("127.") || ip == "::1") return
            
            // Bulletproof self-connection filter: check if IP is one of our own interfaces
            if (getLocalIpAddresses().contains(ip)) {
                Log.i(TAG, "NSD: skipping self by local IP $ip")
                return
            }
            
            // Skip IPv6 link-local — they require a scope ID the engine can't supply.
            if (ip.startsWith("fe80:") || ip.startsWith("FE80:")) {
                Log.d(TAG, "NSD: skipping link-local address $ip")
                return
            }
            // Skip our own service using the actual registered name (set in onServiceRegistered).
            val actual = myActualNsdName
            if (actual != null && info.serviceName == actual) {
                Log.d(TAG, "NSD: skipping self-resolved service '${info.serviceName}'")
                return
            }
            // Belt-and-suspenders: also skip by UUID prefix embedded in service name.
            val prefix = myDeviceUuidPrefix
            if (prefix != null && info.serviceName.contains(prefix, ignoreCase = true)) {
                Log.d(TAG, "NSD: skipping self by UUID prefix '${info.serviceName}'")
                return
            }

            val peerVersion = if (Build.VERSION.SDK_INT >= 21) info.attributes["v"]?.let { String(it) } else null
            if (peerVersion != null && peerVersion != LinkAllJni.protocolVersion().toString()) {
                Log.i(TAG, "NSD: skipping ${info.serviceName} due to protocol version $peerVersion")
                return
            }

            val peerDeviceId = if (Build.VERSION.SDK_INT >= 21) info.attributes["id"]?.let { String(it) } else null
            if (peerDeviceId.isNullOrBlank()) {
                Log.w(TAG, "NSD: peer missing device id, skipping")
                return
            }
            val myId = myDeviceId
            if (myId != null && peerDeviceId.equals(myId, ignoreCase = true)) {
                Log.d(TAG, "NSD: skipping self-resolved peer id $peerDeviceId")
                return
            }

            val h = engineHandle
            if (h != 0L) {
                val fallbackName = "Link All Device" // Name is discovered during handshake
                val result = withEngine { live -> LinkAllJni.reportDiscoveredPeer(live, peerDeviceId, fallbackName, ip, port) }
                if (result == 0) {
                    Log.i(TAG, "NSD: reportDiscoveredPeer($ip:$port, id=$peerDeviceId) pushed to DiscoveryManager")
                    nsdRetryCount.set(0L)
                } else {
                    Log.w(TAG, "NSD: reportDiscoveredPeer failed (result=$result)")
                }
            }
        } finally {
            isResolvingNsd.set(false)
            handler.post { processNextNsdResolve() }
        }
    }

    /** Creates a one-shot resolve listener for pre-API 34. */
    private fun makeResolveListener(): NsdManager.ResolveListener {
        return object : NsdManager.ResolveListener {
            override fun onResolveFailed(info: NsdServiceInfo, code: Int) {
                Log.w(TAG, "NSD: resolve failed for '${info.serviceName}' (code=$code)")
                currentNsdResolveTimeoutRunnable?.let { handler.removeCallbacks(it) }
                isResolvingNsd.set(false)
                handler.post { processNextNsdResolve() }
            }
            override fun onServiceResolved(info: NsdServiceInfo) {
                currentNsdResolveTimeoutRunnable?.let { handler.removeCallbacks(it) }
                handleResolvedNsdService(info)
            }
        }
    }

    // ── Ping Phone ──────────────────────────────────────────────────────────

    private fun pingPhone() {
        Log.i(TAG, "PING received! Ringing phone loudly...")
        try {
            val uri = android.media.RingtoneManager.getDefaultUri(android.media.RingtoneManager.TYPE_RINGTONE)
            pingPlayer?.release()
            pingPlayer = android.media.MediaPlayer().apply {
                setDataSource(applicationContext, uri)
                setAudioStreamType(android.media.AudioManager.STREAM_ALARM)
                isLooping = true
                prepare()
                start()
            }
            
            // Turn up volume to max
            val audioManager = getSystemService(android.content.Context.AUDIO_SERVICE) as android.media.AudioManager
            audioManager.setStreamVolume(
                android.media.AudioManager.STREAM_ALARM,
                audioManager.getStreamMaxVolume(android.media.AudioManager.STREAM_ALARM),
                0
            )
            
            // Stop after 5 seconds
            handler.postDelayed({
                pingPlayer?.stop()
                pingPlayer?.release()
                pingPlayer = null
            }, 5000)
            
        } catch (e: Exception) {
            Log.e(TAG, "Failed to ring phone", e)
        }
    }

    private fun processNextNsdResolve() {
        if (!isResolvingNsd.compareAndSet(false, true)) return
        val info = pendingNsdResolves.poll()
        if (info == null) {
            isResolvingNsd.set(false)
            return
        }
        val nm = runCatching { getSystemService(NSD_SERVICE) as NsdManager }.getOrNull()
        if (nm == null) {
            isResolvingNsd.set(false)
            return
        }
        
        // Add timeout to prevent resolution queue deadlocks
        currentNsdResolveTimeoutRunnable?.let { handler.removeCallbacks(it) }
        val timeoutRunnable = Runnable {
            Log.w(TAG, "NSD: resolution timed out for '${info.serviceName}', skipping")
            isResolvingNsd.set(false)
            processNextNsdResolve()
        }
        currentNsdResolveTimeoutRunnable = timeoutRunnable
        handler.postDelayed(timeoutRunnable, 3000L)

        if (Build.VERSION.SDK_INT >= 34) {
            runCatching {
                nm.registerServiceInfoCallback(info, { it.run() }, object : NsdManager.ServiceInfoCallback {
                    override fun onServiceInfoCallbackRegistrationFailed(errorCode: Int) {
                        Log.w(TAG, "NSD: registerServiceInfoCallback failed (code=$errorCode)")
                        currentNsdResolveTimeoutRunnable?.let { handler.removeCallbacks(it) }
                        isResolvingNsd.set(false)
                        handler.post { processNextNsdResolve() }
                    }
                    override fun onServiceUpdated(serviceInfo: NsdServiceInfo) {
                        currentNsdResolveTimeoutRunnable?.let { handler.removeCallbacks(it) }
                        runCatching { nm.unregisterServiceInfoCallback(this) }
                        handleResolvedNsdService(serviceInfo)
                    }
                    override fun onServiceLost() {}
                    override fun onServiceInfoCallbackUnregistered() {}
                })
            }.onFailure {
                Log.w(TAG, "NSD: registerServiceInfoCallback error", it)
                currentNsdResolveTimeoutRunnable?.let { handler.removeCallbacks(it) }
                isResolvingNsd.set(false)
                handler.post { processNextNsdResolve() }
            }
        } else {
            runCatching { nm.resolveService(info, makeResolveListener()) }
                .onFailure {
                    Log.w(TAG, "NSD: resolveService error", it)
                    currentNsdResolveTimeoutRunnable?.let { handler.removeCallbacks(it) }
                    isResolvingNsd.set(false)
                    handler.post { processNextNsdResolve() }
                }
        }
    }

    // Once a peer is connected, browsing only keeps the Wi-Fi chip sending
    // mDNS queries. Our advertisement stays up, so a new device (which has
    // no peers and is still browsing) finds us; browsing resumes when we go
    // peerless (scheduleNsdRetry) or the user scans.
    private fun pauseNsdBrowse() {
        val listener = nsdDiscoveryListener ?: return
        val nm = runCatching { getSystemService(NSD_SERVICE) as NsdManager }.getOrNull() ?: return
        runCatching { nm.stopServiceDiscovery(listener) }
        nsdDiscoveryListener = null
    }

    private fun stopNsdDiscovery() {
        val nm = runCatching { getSystemService(NSD_SERVICE) as NsdManager }.getOrNull() ?: return
        
        nsdDiscoveryListener?.let  { runCatching { nm.stopServiceDiscovery(it) } }
        nsdDiscoveryListener = null
        
        val regListener = nsdRegistrationListener
        if (regListener != null) {
            if (isNsdRegistered.get()) {
                // Safely unregister if fully registered
                runCatching { nm.unregisterService(regListener) }
                nsdRegistrationListener = null
                isNsdRegistered.set(false)
            } else {
                // In-flight registration — set flag to unregister once it completes
                pendingNsdUnregister.set(true)
            }
        }
        
        pendingNsdResolves.clear()
        currentNsdResolveTimeoutRunnable?.let { handler.removeCallbacks(it) }
        currentNsdResolveTimeoutRunnable = null
        isResolvingNsd.set(false)
    }

    // ── Network change callback ───────────────────────────────────────────────
    //
    // Restarts NSD whenever the device gains a new WiFi network (e.g. waking
    // from sleep, switching APs, reconnecting after a drop).  Without this,
    // the engine stays silently disconnected until the user kills and relaunches.

    private fun registerNetworkCallback() {
        if (networkCallback != null) return
        val cm = runCatching {
            getSystemService(CONNECTIVITY_SERVICE) as ConnectivityManager
        }.getOrNull() ?: return

        val cb = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                val cm = getSystemService(CONNECTIVITY_SERVICE) as? ConnectivityManager
                val caps = cm?.getNetworkCapabilities(network)
                val isWifiOrEth = caps?.hasTransport(android.net.NetworkCapabilities.TRANSPORT_WIFI) == true || 
                                  caps?.hasTransport(android.net.NetworkCapabilities.TRANSPORT_ETHERNET) == true

                Log.i(TAG, "Network: default network available (wifi/eth=$isWifiOrEth) — reconnecting peers")
                handler.post {
                    if (isWifiOrEth) {
                        acquireContinuousLocks()
                    }
                    
                    // Brief delay lets the IP stack settle before mDNS re-registers.
                    delayedNetworkAction?.let { handler.removeCallbacks(it) }
                    val action = Runnable {
                        delayedNetworkAction = null
                        if (isWifiOrEth) {
                            restartDiscoveryNow()
                        }
                        // Immediately tell the Rust engine to reconnect all known peers.
                        val h = engineHandle
                        if (h != 0L) {
                            backgroundExecutor.execute {
                                withEngine { live -> LinkAllJni.notifyNetworkRestored(live) }
                            }
                        }
                    }
                    delayedNetworkAction = action
                    handler.postDelayed(action, 1_500L)
                }
            }

            override fun onCapabilitiesChanged(network: Network, networkCapabilities: android.net.NetworkCapabilities) {
                super.onCapabilitiesChanged(network, networkCapabilities)
                // Network restoration and NSD restart are handled debounced in onAvailable.
            }

            override fun onLost(network: Network) {
                Log.i(TAG, "Network: default network lost — stopping discovery, scheduling retry")
                val h = engineHandle
                if (h != 0L) {
                    backgroundExecutor.execute { withEngine { live -> LinkAllJni.notifyNetworkRestored(live) } }
                }
                handler.post {
                    delayedNetworkAction?.let { handler.removeCallbacks(it) }
                    delayedNetworkAction = null
                    releaseMulticastLock() // Fix: Release multicast lock to save battery when offline
                    releaseWifiLock()
                    releaseWakeLock()
                    stopNsdDiscovery()
                    scheduleNsdRetry()
                }
            }
        }

        runCatching { cm.registerDefaultNetworkCallback(cb) }
            .onSuccess { networkCallback = cb }
            .onFailure { Log.w(TAG, "Network: failed to register callback", it) }
    }

    private fun unregisterNetworkCallback() {
        delayedNetworkAction?.let { handler.removeCallbacks(it) }
        delayedNetworkAction = null
        val cb = networkCallback ?: return
        networkCallback = null
        val cm = runCatching {
            getSystemService(CONNECTIVITY_SERVICE) as ConnectivityManager
        }.getOrNull() ?: return
        runCatching { cm.unregisterNetworkCallback(cb) }
    }

    // ── NSD retry with exponential backoff ────────────────────────────────────
    //
    // When all peers disconnect (or we lose WiFi and regain it), we schedule a
    // fresh NSD scan with exponential backoff: 5 s → 10 s → 20 s → 40 s → 60 s.
    // This covers the case where the Mac wakes up after the Android, or the
    // Android reconnects to a network before the Mac's mDNS advertisement is live.

    private fun scheduleNsdRetry() {
        cancelNsdRetry()
        val attempt = nsdRetryCount.getAndIncrement()
        val delayMs = minOf(5_000L * (1L shl attempt.coerceAtMost(4).toInt()), 60_000L)
        Log.i(TAG, "NSD retry #$attempt scheduled in ${delayMs}ms")
        val r = Runnable {
            if (engineHandle != 0L && connectedPeerIds.isEmpty()) {
                Log.i(TAG, "NSD retry: restarting discovery")
                stopNsdDiscovery()
                startNsdDiscovery()
            }
        }
        nsdRetryRunnable = r
        handler.postDelayed(r, delayMs)
    }

    private fun cancelNsdRetry() {
        nsdRetryRunnable?.let { handler.removeCallbacks(it) }
        nsdRetryRunnable = null
    }

    private fun NsdServiceInfo.attributeString(key: String): String? =
        attributes[key]
            ?.let { bytes -> String(bytes, StandardCharsets.UTF_8).trim() }
            ?.takeIf { it.isNotEmpty() }

    private fun shouldInitiateDiscoveredSession(myId: String, peerId: String): Boolean {
        return true
    }

    private fun normalizeUuidForCompare(raw: String): String? =
        runCatching { UUID.fromString(raw) }.getOrNull()
            ?.toString()
            ?.replace("-", "")
            ?.lowercase()

    private fun registerPairingReceiver() {
        if (pairingReceiverRegistered) return
        ContextCompat.registerReceiver(
            this,
            pairingResultReceiver,
            IntentFilter(PairingActivity.ACTION_PAIRING_RESULT),
            ContextCompat.RECEIVER_NOT_EXPORTED
        )
        pairingReceiverRegistered = true
    }

    private fun unregisterPairingReceiver() {
        if (!pairingReceiverRegistered) return
        runCatching { unregisterReceiver(pairingResultReceiver) }
        pairingReceiverRegistered = false
    }

    // ── Live settings application ─────────────────────────────────────────────
    //
    // Called when SettingsActivity broadcasts ACTION_SETTINGS_CHANGED.
    // Reads the current SharedPreferences and pushes them to the running
    // engine so changes take effect without a service restart.

    private fun applySettingsToEngine() {
        val h = engineHandle
        if (h == 0L) return
        val p = prefs()
        val syncEnabled = p.getBoolean("sync_enabled", true)
        val syncText    = p.getBoolean("sync_text",    true)
        val syncImages  = p.getBoolean("sync_images",  true)
        val syncFiles   = p.getBoolean("sync_files",   true)
        Log.i(TAG, "Applying settings: sync=$syncEnabled text=$syncText images=$syncImages files=$syncFiles")
        // Push to engine — JNI call updates the engine's sync filter flags atomically.
        withEngine { live -> LinkAllJni.applySyncSettings(live, syncEnabled, syncText, syncImages, syncFiles) }
        // If sync was just disabled, cancel any pending clipboard notifications.
        if (!syncEnabled) {
            notificationManager.cancel(NOTIF_ID_CLIPBOARD_AVAILABLE)
        }
    }

    // ── Device name ───────────────────────────────────────────────────────────

    private fun resolvedDeviceName(): String {
        prefs().getString("device_name", null)?.trim()?.takeIf { it.isNotEmpty() }?.let { return it }
        // Settings.Global.DEVICE_NAME is the Bluetooth/Wi-Fi Direct name the
        // OS shows the user, usually already a marketing name ("Galaxy A53
        // 5G"). Several heavily-skinned OEM ROMs (Samsung/Oppo/Vivo/Realme)
        // block third-party reads of it and return null, which is why this
        // path historically only worked on near-stock ROMs like OnePlus's.
        Settings.Global.getString(contentResolver, "device_name")?.trim()?.takeIf { it.isNotEmpty() }?.let { return it }
        val mfr   = Build.MANUFACTURER.orEmpty().trim()
        val model = Build.MODEL.orEmpty().trim()
        // Android has no public API to recover a marketing name ("Galaxy
        // A53 5G") from Build.MODEL's internal code ("SM-A536B") once the
        // Settings.Global read above is blocked, so this fallback is model
        // code, not marketing name. It's still fixed to read as an
        // intentional device label rather than a bug: Build.MANUFACTURER's
        // casing is inconsistent across OEMs (lowercase "samsung"/"vivo",
        // uppercase "OPPO"), which read like garbled text next to a clean
        // model string.
        return if (model.startsWith(mfr, ignoreCase = true)) model else "${brandDisplayName(mfr)} $model".trim()
    }

    // Canonical display casing for common Android OEM brands. Values are the
    // brand's own stylization (e.g. "vivo" and "realme" are lowercase by
    // design; "OPPO" and "ASUS" are all-caps by design), not just
    // title-casing - title-casing everything would "fix" samsung/xiaomi but
    // introduce a new wrong casing for vivo/realme/OPPO.
    private fun brandDisplayName(manufacturer: String): String {
        if (manufacturer.isEmpty()) return manufacturer
        return when (manufacturer.lowercase()) {
            "samsung" -> "Samsung"
            "xiaomi" -> "Xiaomi"
            "redmi" -> "Redmi"
            "poco" -> "POCO"
            "oppo" -> "OPPO"
            "vivo" -> "vivo"
            "realme" -> "realme"
            "oneplus" -> "OnePlus"
            "huawei" -> "Huawei"
            "honor" -> "HONOR"
            "motorola" -> "Motorola"
            "lenovo" -> "Lenovo"
            "google" -> "Google"
            "asus" -> "ASUS"
            "sony" -> "Sony"
            "lge" -> "LG"
            "nokia", "hmd global" -> "Nokia"
            "infinix" -> "Infinix"
            "tecno" -> "Tecno"
            "nothing" -> "Nothing"
            else -> manufacturer.replaceFirstChar { it.titlecase() }
        }
    }

    // ── Notification channels ─────────────────────────────────────────────────

    private fun createNotificationChannels() {
        val nm = getSystemService(NotificationManager::class.java)

        // Channel A: persistent foreground indicator — must be as quiet as possible
        nm.createNotificationChannel(NotificationChannel(
            CHAN_SERVICE,
            "Link All",
            NotificationManager.IMPORTANCE_MIN          // no sound, no vibration, no heads-up
        ).apply {
            description = "Link All background sync indicator"
            setShowBadge(false)
            enableLights(false)
            enableVibration(false)
            setSound(null, null)
        })

        // Channel B: trust requests, file receives, critical failures
        nm.createNotificationChannel(NotificationChannel(
            CHAN_ALERTS,
            "Link All Alerts",
            NotificationManager.IMPORTANCE_HIGH
        ).apply {
            description = "Trust requests, received files, connection failures"
            setShowBadge(true)
            enableLights(true)
            enableVibration(true)
        })

        // Channel C: incoming call relay banner — full heads-up priority
        nm.createNotificationChannel(NotificationChannel(
            CHAN_CALLS,
            "Link All Calls",
            NotificationManager.IMPORTANCE_HIGH
        ).apply {
            description = "Incoming call relay notifications from your phone"
            setShowBadge(true)
            enableVibration(true)
            enableLights(true)
            setBypassDnd(true)  // show even in Do Not Disturb
        })

        // Channel D: dedicated pairing requests — full heads-up priority
        nm.createNotificationChannel(NotificationChannel(
            CHAN_PAIRING,
            "Link All Pairing Requests",
            NotificationManager.IMPORTANCE_HIGH
        ).apply {
            description = "Pairing requests from your computer and devices"
            setShowBadge(true)
            enableVibration(true)
            enableLights(true)
            setSound(android.provider.Settings.System.DEFAULT_NOTIFICATION_URI, android.media.AudioAttributes.Builder()
                .setContentType(android.media.AudioAttributes.CONTENT_TYPE_SONIFICATION)
                .setUsage(android.media.AudioAttributes.USAGE_NOTIFICATION_RINGTONE)
                .build())
        })
    }

    // ── Foreground notification ───────────────────────────────────────────────
    //
    // ONE notification, ALWAYS the same ID.
    // Silent — no sound, no vibration, no heads-up banner.
    // Two action buttons: [Pause Sync] / [Resume Sync] and [Disconnect]


    private fun buildForegroundNotification(): Notification {
        val launchPi = PendingIntent.getActivity(
            this, 0,
            packageManager.getLaunchIntentForPackage(packageName),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )

        val syncEnabled = isSyncEnabled()

        // Pause/Resume Sync action
        val syncActionLabel = if (syncEnabled) "Pause Sync" else "Resume Sync"
        val syncActionIntent = Intent(this, LinkAllService::class.java).apply {
            action = if (syncEnabled) ACTION_PAUSE_SYNC else ACTION_RESUME_SYNC
        }
        val syncActionPi = PendingIntent.getService(
            this, 10,
            syncActionIntent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )

        val description = foregroundStatusText()

        val pushClipboardIntent = Intent(this, LinkAllService::class.java).apply {
            action = ACTION_PUSH_CLIPBOARD
        }
        val pushClipboardPi = PendingIntent.getService(
            this, 11,
            pushClipboardIntent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )

        val dismissedPi = PendingIntent.getService(
            this, 12,
            Intent(this, LinkAllService::class.java).apply { action = ACTION_SERVICE_NOTIFICATION_DISMISSED },
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )

        return NotificationCompat.Builder(this, CHAN_SERVICE)
            // Calm and still: no running timer, no "active" wording, so a
            // service that mostly waits does not look like busy work.
            .setContentTitle("Link All")
            .setContentText(description)
            .setShowWhen(false)
            .setSmallIcon(R.drawable.ic_cr_activity)
            .setColor(android.graphics.Color.parseColor("#3D7BFF")) // Brand blue
            // Not ongoing: Android 13+ lets the user swipe it away while the
            // service keeps running, and the delete intent keeps it away.
            .setDeleteIntent(dismissedPi)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setPriority(NotificationCompat.PRIORITY_MIN)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setContentIntent(launchPi)
            .addAction(
                if (syncEnabled) android.R.drawable.ic_media_pause else android.R.drawable.ic_media_play,
                syncActionLabel,
                syncActionPi
            )
            .apply {
                if (syncEnabled && connectedPeerIds.isNotEmpty()) {
                    addAction(
                        R.drawable.ic_baseline_content_paste_24,
                        "Push Clipboard",
                        pushClipboardPi
                    )
                }
            }
            .build()
    }

    private fun startForegroundCompat(notification: Notification) {
        postedNotificationState = notificationState()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIF_ID_SERVICE, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
        } else {
            startForeground(NOTIF_ID_SERVICE, notification)
        }
        isInForeground = true
    }

    /** Set once startForeground succeeded; this service never leaves the foreground. */
    private var isInForeground = false

    /** The user swiped the notification away; keep it away until the service restarts. */
    private var serviceNotificationDismissed = false

    /** What the service notification last showed; see [updateForegroundNotification]. */
    private var postedNotificationState: String? = null

    /**
     * Re-posts the service notification only when what it shows changed.
     * Each post briefly wakes the phone, and callers fire on every peer and
     * service event, most of which change nothing visible.
     */
    private fun updateForegroundNotification() {
        if (serviceNotificationDismissed) return
        val state = notificationState()
        if (state == postedNotificationState) return
        postedNotificationState = state
        getSystemService(NotificationManager::class.java)
            .notify(NOTIF_ID_SERVICE, buildForegroundNotification())
    }

    private fun notificationState(): String =
        "${isSyncEnabled()}|${foregroundStatusText()}"

    private fun foregroundStatusText(): String {
        if (!isSyncEnabled()) return "Paused · tap to manage"
        return when (connectedPeerIds.size) {
            0    -> "Ready · waiting for your devices"
            1    -> "Connected to ${connectedPeerIds.values.first()}"
            else -> "Connected to ${connectedPeerIds.size} devices"
        }
    }

    // ── Alert notifications ───────────────────────────────────────────────────
    //
    // These use CHAN_ALERTS — they CAN make sound/vibration.
    // Only fired for: trust request, file received, critical failure.
    // NEVER fired for: clipboard text/image sync.



    private fun showFileReceivedNotification(fromDevice: String, fileName: String, uri: Uri?) {
        val openPi = uri?.let {
            val openIntent = Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(it, contentResolver.getType(it) ?: "*/*")
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            PendingIntent.getActivity(
                this, 30,
                Intent.createChooser(openIntent, "Open $fileName"),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
            )
        }

        val notif = NotificationCompat.Builder(this, CHAN_ALERTS).setGroup("linkall_transfers")
            .setContentTitle("File received from $fromDevice")
            .setContentText(fileName)
            .setSmallIcon(android.R.drawable.stat_sys_download_done)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setAutoCancel(true)
            .apply { if (openPi != null) setContentIntent(openPi) }
            .build()

        // Use a dynamic notification ID unique to the file (fileName.hashCode() and 0xFFF)
        // so multiple files don't overwrite each other!
        val notifId = NOTIF_ID_FILE_BASE + (fileName.hashCode() and 0xFFF)
        getSystemService(NotificationManager::class.java).notify(notifId, notif)
    }

    private fun showFailureNotification(message: String) {
        val launchPi = PendingIntent.getActivity(
            this, 40,
            packageManager.getLaunchIntentForPackage(packageName),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )

        val notif = NotificationCompat.Builder(this, CHAN_ALERTS).setGroup("linkall_transfers")
            .setContentTitle("Link All Connection Error")
            .setContentText(message.take(80))
            .setSmallIcon(android.R.drawable.stat_notify_error)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setCategory(NotificationCompat.CATEGORY_ERROR)
            .setAutoCancel(true)
            .setContentIntent(launchPi)
            .build()

        getSystemService(NotificationManager::class.java).notify(NOTIF_ID_FAILURE, notif)
    }

    // ── Status persistence ────────────────────────────────────────────────────

    // Track last-sync time for connected peers
    private val peerLastSync = mutableMapOf<String, Long>()

    // For Ping Phone functionality
    private var pingPlayer: android.media.MediaPlayer? = null

    private fun currentPeerSnapshots(): List<PeerSnapshot> {
        val raw = if (engineHandle != 0L) {
            withEngine { live -> LinkAllJni.peersJson(live) }
        } else {
            prefs().getString(PREF_PEER_SNAPSHOTS_JSON, null)
        }
        return parsePeerSnapshots(raw)
    }

    private fun hasConnectedPeers(): Boolean = connectedPeerIds.isNotEmpty()

    private fun isPeerConnected(deviceId: String): Boolean =
        currentPeerSnapshots().any { peer ->
            peer.isConnected && peer.id.equals(deviceId, ignoreCase = true)
        }

    private fun resolvePeerDisplayName(deviceId: String?, fallbackName: String?): String {
        val known = deviceId?.let { id ->
            currentPeerSnapshots().firstOrNull { it.id.equals(id, ignoreCase = true) }?.name
        }
        return known?.takeIf { it.isNotBlank() }
            ?: fallbackName?.takeIf { it.isNotBlank() }
            ?: "Unknown device"
    }

    private fun persistStatus() {
        val rawPeerJson = if (engineHandle != 0L) {
            withEngine { live -> LinkAllJni.peersJson(live) }
        } else {
            prefs().getString(PREF_PEER_SNAPSHOTS_JSON, null)
        } ?: "[]"
        val peers = parsePeerSnapshots(rawPeerJson)
        DeviceShareTargets.publish(this, peers)
        connectedPeerIds.clear()
        peers.filter { it.isConnected }.forEach { connectedPeerIds[it.id] = it.name }
        peers.forEach { peer ->
            peer.lastSyncSecs?.let { peerLastSync[peer.name] = it * 1000L }
        }

        val editor = prefs().edit()
            .putString("local_device_name", resolvedDeviceName())
            .putString("device_id", if (engineHandle != 0L) withEngine { live -> LinkAllJni.getDeviceId(live) } else null)
            .putBoolean("peer_connected", connectedPeerIds.isNotEmpty())
            .putInt("connected_count", connectedPeerIds.size)
            .putStringSet("connected_names", connectedPeerIds.values.toSet())
            .putString(PREF_PEER_SNAPSHOTS_JSON, rawPeerJson)
            .putString(PREF_HEALTH_JSON, if (engineHandle != 0L) withEngine { live -> LinkAllJni.healthJson(live) } ?: "[]" else "[]")
        // Store last-sync times so the dashboard can show "Last sync: 2m ago" per peer.
        peerLastSync.forEach { (name, ts) ->
            editor.putLong("last_sync_${name.take(32)}", ts)
        }
        editor.apply()
        broadcastStatus()
    }

    /**
     * Direct connect for networks where mDNS is blocked. The engine only
     * takes IP literals, so hostnames are resolved here first. The outcome is
     * broadcast for the Connect by IP dialog, keyed by the host as typed.
     */
    private fun connectManual(host: String, port: Int) {
        val h = engineHandle
        serviceScope.launch {
            val ip = runCatching { java.net.InetAddress.getByName(host).hostAddress }.getOrNull()
            val error = when {
                ip == null -> "Couldn't find \"$host\" on this network."
                getLocalIpAddresses().contains(ip) -> "That's this phone's own address. Enter the other device's IP."
                withEngine { live -> LinkAllJni.connectToPeer(live, ip, port) } != 0 ->
                    "Couldn't reach $host:$port. Check both devices are on the same network and Link All is open on the other one."
                else -> null
            }
            Log.i(TAG, "Manual connect to $host:$port (ip=$ip): ${error ?: "ok"}")
            if (error == null) rememberManualAddress(if (port == DEFAULT_LINKALL_PORT) host else "$host:$port")
            sendBroadcast(Intent(ACTION_MANUAL_CONNECT_RESULT).setPackage(packageName).apply {
                putExtra(EXTRA_MANUAL_HOST, host)
                putExtra(EXTRA_MANUAL_OK, error == null)
                error?.let { putExtra(EXTRA_MANUAL_ERROR, it) }
            })
        }
    }

    private fun rememberManualAddress(address: String) {
        val updated = (listOf(address) + recentManualAddresses(this)).distinct().take(MAX_RECENT_MANUAL)
        prefs().edit().putString(PREF_RECENT_MANUAL, updated.joinToString("\n")).apply()
    }

    private fun broadcastStatus() {
        sendBroadcast(Intent(ACTION_STATUS_CHANGED).setPackage(packageName))
        LinkAllWidget.updateAll(this)
    }

    private fun setServiceRunning(running: Boolean) {
        prefs().edit()
            .putBoolean(PREF_SERVICE_RUNNING, running)
            .apply()
        LinkAllWidget.updateAll(this)
    }

    private fun hasFilePermissions(): Boolean {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            if (android.os.Environment.isExternalStorageManager()) {
                return true
            }
        }
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            checkSelfPermission(Manifest.permission.READ_MEDIA_IMAGES) == android.content.pm.PackageManager.PERMISSION_GRANTED &&
            checkSelfPermission(Manifest.permission.READ_MEDIA_VIDEO) == android.content.pm.PackageManager.PERMISSION_GRANTED &&
            checkSelfPermission(Manifest.permission.READ_MEDIA_AUDIO) == android.content.pm.PackageManager.PERMISSION_GRANTED
        } else {
            checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE) == android.content.pm.PackageManager.PERMISSION_GRANTED
        }
    }

    private fun showPermissionRequiredNotification() {
        // The Play build cannot ask for the access this notification asks for.
        if (!BuildConfig.FULL_PERMISSIONS) return
        val intent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK
            putExtra("request_permissions", true)
        }
        val launchPi = PendingIntent.getActivity(
            this, 42,
            intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )

        val notif = NotificationCompat.Builder(this, CHAN_ALERTS).setGroup("linkall_transfers")
            .setContentTitle("Permission Required")
            .setContentText("Link All needs storage access to browse files. Tap here to grant permission.")
            .setSmallIcon(android.R.drawable.stat_notify_error)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(NotificationCompat.CATEGORY_ERROR)
            .setAutoCancel(true)
            .setContentIntent(launchPi)
            .build()

        getSystemService(NotificationManager::class.java).notify(4242, notif)
    }
}
