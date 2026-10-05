package app.linkall

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Lock
import androidx.compose.material.icons.outlined.Security
import app.linkall.ui.theme.*

import android.Manifest
import android.content.*
import android.os.Build
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.SystemBarStyle
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import androidx.core.content.ContextCompat
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import androidx.compose.animation.*
import androidx.compose.animation.core.*
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.collections.immutable.toImmutableList
import app.linkall.ui.MainScreen
import app.linkall.ui.OnboardingScreen
import app.linkall.ui.theme.AppTheme
import app.linkall.ui.theme.CRTheme
import app.linkall.ui.theme.glassmorphism

class MainActivity : ComponentActivity() {

    companion object {
        private const val FEED_REFRESH_MS = 5_000L
    }

    private val isServiceRunning = mutableStateOf(false)
    private val isSyncEnabled = mutableStateOf(true)
    private val syncText = mutableStateOf(true)
    private val syncImages = mutableStateOf(true)
    private val syncFiles = mutableStateOf(true)
    private val callContinuityEnabled = mutableStateOf(false)
    private val notificationMirroringEnabled = mutableStateOf(false)
    private val autoForwardSms = mutableStateOf(false)
    private val autoForwardScreenshots = mutableStateOf(false)
    private val deviceName = mutableStateOf("")
    private val deviceId = mutableStateOf("")
    private val peers = mutableStateOf<List<PeerSnapshot>>(emptyList())
    private val ambientStatus = mutableStateOf("Looking for network...")
    private val healthIssues = mutableStateOf<List<HealthIssue>>(emptyList())
    private val isDarkMode = mutableStateOf(false)
    private val hasCompletedOnboarding = mutableStateOf(false)
    private val toastMessage = mutableStateOf("")

    private var targetDeviceIdForNextSend: String? = null

    /** A question shown in the app's own sheet (see ConfirmSheet). */
    private data class Prompt(
        val icon: androidx.compose.ui.graphics.vector.ImageVector,
        val title: String,
        val message: String,
        val confirm: String,
        val dismiss: String?,
        val onConfirm: () -> Unit,
    )
    private val prompt = mutableStateOf<Prompt?>(null)

    private val filePickerLauncher = registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.GetMultipleContents()) { uris ->
        if (uris.isNotEmpty()) {
            val intent = Intent(this, LinkAllService::class.java).apply {
                action = LinkAllService.ACTION_PUSH_SHARED_URI
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                if (uris.isNotEmpty()) {
                    val cd = android.content.ClipData.newRawUri("shared_uris", uris[0])
                    for (i in 1 until uris.size) {
                        cd.addItem(android.content.ClipData.Item(uris[i]))
                    }
                    clipData = cd
                }
                putStringArrayListExtra(LinkAllService.EXTRA_SHARED_URIS, java.util.ArrayList(uris.map { it.toString() }))
                if (targetDeviceIdForNextSend != null) {
                    putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, targetDeviceIdForNextSend)
                }
            }
            ContextCompat.startForegroundService(this, intent)
            showSnack("Sending ${uris.size} file(s)...")
        }
        targetDeviceIdForNextSend = null
    }

    private val folderPickerLauncher = registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.OpenDocumentTree()) { uri ->
        if (uri != null) {
            val intent = Intent(this, LinkAllService::class.java).apply {
                action = LinkAllService.ACTION_PUSH_FOLDER
                putExtra(LinkAllService.EXTRA_SHARED_URI, uri.toString())
                targetDeviceIdForNextSend?.let { putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, it) }
            }
            ContextCompat.startForegroundService(this, intent)
            showSnack("Sending folder...")
        }
        targetDeviceIdForNextSend = null
    }

    private val feedRefreshHandler = android.os.Handler(android.os.Looper.getMainLooper())

    private val statusReceiver = object : BroadcastReceiver() {
        override fun onReceive(ctx: Context?, intent: Intent?) {
            runOnUiThread {
                refreshDashboardState()
            }
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) {
            val cm = getSystemService(android.content.ClipboardManager::class.java)
            val clip = cm.primaryClip?.getItemAt(0)?.coerceToText(this)?.toString()
            if (!clip.isNullOrBlank()) {
                LinkAllService.quickSendContextFlow.value = clip
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        installSplashScreen()
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.auto(
                android.graphics.Color.TRANSPARENT,
                android.graphics.Color.TRANSPARENT
            ),
            navigationBarStyle = SystemBarStyle.auto(
                android.graphics.Color.TRANSPARENT,
                android.graphics.Color.TRANSPARENT
            )
        )
        super.onCreate(savedInstanceState)
        ActivityFeedManager.attach(this)
        if (intent?.getBooleanExtra("benchmark", false) == true) {
            ActivityFeedManager.ACTIVITY_FEED_MAX = 20000
            for (i in 1..10000) {
                ActivityFeedManager.addToFeed(ActivityEntry(id = i.toLong(), deviceName = "TestDevice", kind = ActivityKind.CLIPBOARD_TEXT, preview = "Test item $i", contentHash = "hash$i"))
            }
        }
        
        // Initialize persistent preferences
        val prefs = getSharedPreferences(LinkAllService.PREFS_NAME, MODE_PRIVATE)
        isDarkMode.value = prefs.getBoolean("dark_mode", false)
        hasCompletedOnboarding.value = prefs.getBoolean("has_completed_onboarding", false)

        // Request Notification Permission immediately on launch for Android 13+
        if (Build.VERSION.SDK_INT >= 33) {
            if (ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1005)
            }
        }
        
        // UX FIX: Auto-complete onboarding if we have a trusted peer (user closed app during onboarding previously)
        // We do this ONLY on create so we don't abruptly close the OnboardingScreen while the user is actively using it!
        val allPeers = prefs.peerSnapshots()
        if (!hasCompletedOnboarding.value && allPeers.any { it.trusted }) {
            prefs.edit().putBoolean("has_completed_onboarding", true).apply()
            hasCompletedOnboarding.value = true
        }
        // No preferred display mode: pinning the panel's top rate kept a
        // 120 Hz screen at 120 Hz while the dashboard sat still. The system
        // already raises the rate for scrolling and animation.

        if (intent?.getBooleanExtra("request_permissions", false) == true) {
            requestRuntimePermissions()
        }

        if (hasCompletedOnboarding.value) {
            requestBatteryOptimizationExemption()
        }

        val imageLoader = coil.ImageLoader.Builder(this)
            .components {
                add(coil.decode.VideoFrameDecoder.Factory())
            }
            .build()
        coil.Coil.setImageLoader(imageLoader)

        setContent {
            val activeTransfers by TransferManager.activeTransfersFlow.collectAsStateWithLifecycle()
            val activeSpeedTests by TransferManager.activeSpeedTestsFlow.collectAsStateWithLifecycle()
            val feedState by ActivityFeedManager.feedFlow.collectAsStateWithLifecycle()

            AppTheme(useDarkTheme = isDarkMode.value) {
                prompt.value?.let { p ->
                    app.linkall.ui.ConfirmSheet(
                        c = app.linkall.ui.rememberDdColors(isDarkMode.value),
                        icon = p.icon,
                        title = p.title,
                        message = p.message,
                        confirm = p.confirm,
                        dismiss = p.dismiss,
                        onConfirm = p.onConfirm,
                        onDismiss = { prompt.value = null },
                    )
                }
                var showManualIpDialog by remember { mutableStateOf(false) }
                if (showManualIpDialog) {
                    app.linkall.ui.ConnectByIpDialog(
                        isDark = isDarkMode.value,
                        onDismiss = { showManualIpDialog = false },
                        onConnected = { address ->
                            showManualIpDialog = false
                            showSnack("Connected to $address")
                        },
                        onPairingLink = { link -> handlePairingInput(link) },
                    )
                }

                Box(modifier = Modifier.fillMaxSize()) {
                    if (!hasCompletedOnboarding.value) {
                        OnboardingScreen(
                            isDark = isDarkMode.value,
                            peers = peers.value,
                            onConnectPeer = { peer ->
                                ContextCompat.startForegroundService(this@MainActivity,
                                    Intent(this@MainActivity, LinkAllService::class.java).apply {
                                        action = LinkAllService.ACTION_SEND_PAIRING_REQUEST
                                        putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, peer.id)
                                    }
                                )
                            },
                            onCancelPairing = { peer ->
                                ContextCompat.startForegroundService(this@MainActivity,
                                    Intent(this@MainActivity, LinkAllService::class.java).apply {
                                        action = LinkAllService.ACTION_CANCEL_PAIRING_REQUEST
                                        putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, peer.id)
                                    }
                                )
                            },
                            onSendSampleText = { peer ->
                                val svc = Intent(this@MainActivity, LinkAllService::class.java).apply {
                                    action = LinkAllService.ACTION_PUSH_CLIPBOARD
                                    putExtra(LinkAllService.EXTRA_CLIPBOARD_TEXT, "Hello from Android")
                                    putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, peer.id)
                                }
                                ContextCompat.startForegroundService(this@MainActivity, svc)
                                showSnack("Sending sample to ${peer.name}…")
                            },
                            onScanQr = {
                                startQrScanner()
                            },
                            onManualIp = {
                                showManualIpDialog = true
                            },
                            onComplete = {
                                getSharedPreferences(LinkAllService.PREFS_NAME, MODE_PRIVATE).edit().putBoolean("has_completed_onboarding", true).apply()
                                hasCompletedOnboarding.value = true
                                requestBatteryOptimizationExemption()
                            }
                        )
                    } else {
                        MainScreen(
                        isDark = isDarkMode.value,
                        isServiceRunning = isServiceRunning.value,
                        isSyncEnabled = isSyncEnabled.value,
                        syncText = syncText.value,
                        syncImages = syncImages.value,
                        syncFiles = syncFiles.value,
                        callContinuityEnabled = callContinuityEnabled.value,
                        notificationMirroringEnabled = notificationMirroringEnabled.value,
                        autoForwardSms = autoForwardSms.value,
                        autoForwardScreenshots = autoForwardScreenshots.value,
                        deviceName = deviceName.value,
                        deviceId = deviceId.value,
                        peers = peers.value.toImmutableList(),
                        feed = feedState.toImmutableList(),
                        ambientStatus = ambientStatus.value,
                        healthIssue = healthIssues.value.firstOrNull(),
                        onHealthAction = ::onHealthAction,
                        activeTransfers = activeTransfers.toImmutableList(),
                        activeSpeedTests = activeSpeedTests.toImmutableList(),
                        onSyncEnabledChange = {
                            isSyncEnabled.value = it
                            saveBooleanPref("sync_enabled", it)
                        },
                        onSyncTextChange = {
                            syncText.value = it
                            saveBooleanPref("sync_text", it)
                        },
                        onSyncImagesChange = {
                            syncImages.value = it
                            saveBooleanPref("sync_images", it)
                        },
                        onSyncFilesChange = {
                            syncFiles.value = it
                            saveBooleanPref("sync_files", it)
                        },
                        onCallContinuityChange = { enabled ->
                            val apply = {
                                callContinuityEnabled.value = enabled
                                saveBooleanPref("call_continuity_enabled", enabled)
                                if (enabled) requestCallContinuityPermissions()
                            }
                            if (enabled) confirmDataSharing(
                                "Show calls on your computer?",
                                "When your phone rings, Link All sends the caller's number and name, when Android provides them, to your paired devices. It goes directly over your network, never through a server.",
                                apply
                            ) else apply()
                        },
                        onNotificationMirroringChange = { enabled ->
                            val apply = {
                                notificationMirroringEnabled.value = enabled
                                saveBooleanPref("notification_mirroring", enabled)
                                if (enabled) requestNotificationListenerPermission()
                            }
                            if (enabled) confirmDataSharing(
                                "Mirror notifications?",
                                "Link All sends the notifications this phone receives, including their text, to your paired devices. It goes directly over your network, never through a server.",
                                apply
                            ) else apply()
                        },
                        onAutoForwardSmsChange = {
                            autoForwardSms.value = it
                            saveBooleanPref("auto_forward_sms", it)
                            if (it) requestSmsPermission()
                        },
                        onAutoForwardScreenshotsChange = {
                            autoForwardScreenshots.value = it
                            saveBooleanPref("auto_forward_screenshots", it)
                            if (it) requestMediaPermissions()
                        },
                        onDarkModeChange = {
                            isDarkMode.value = it
                            saveBooleanPref("dark_mode", it)
                        },
                        onForgetDevice = { targetId ->
                            ContextCompat.startForegroundService(this@MainActivity,
                                Intent(this@MainActivity, LinkAllService::class.java).apply {
                                    action = LinkAllService.ACTION_FORGET_PEER
                                    putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, targetId)
                                }
                            )
                            peers.value = peers.value.filter { it.id != targetId }
                            Toast.makeText(this@MainActivity, "Device forgotten", Toast.LENGTH_SHORT).show()
                        },
                        onActionStartSpeedTest = { deviceId ->
                            val intent = android.content.Intent(this@MainActivity, LinkAllService::class.java).apply {
                                action = LinkAllService.ACTION_START_SPEED_TEST
                                putExtra("device_id", deviceId)
                            }
                            startService(intent)
                        },
                        onStartSync = { launchService() },
                    onResumeSync = { sendAction(LinkAllService.ACTION_RESUME_SYNC) },
                    onScanNow = {
                        sendAction(LinkAllService.ACTION_SCAN_NOW)
                        showSnack("Scanning for devices...")
                    },
                    onActionPushClipboard = {
                        val cm = getSystemService(ClipboardManager::class.java)
                        val clip = cm.primaryClip?.getItemAt(0)?.coerceToText(this@MainActivity)
                        if (clip.isNullOrBlank()) {
                            showSnack("Clipboard is empty")
                        } else {
                            sendAction(LinkAllService.ACTION_PUSH_CLIPBOARD)
                            showSnack("Sending clipboard...")
                        }
                    },
                    onActionPairMagicLink = { showMagicLinkPairingDialog() },
                    onManualIp = { showManualIpDialog = true },
                    onActionPauseSync = {
                        sendAction(LinkAllService.ACTION_PAUSE_SYNC)
                        refreshDashboardState()
                    },
                    onActionDisconnectAll = {
                        sendAction(LinkAllService.ACTION_DISCONNECT_ALL)
                        refreshDashboardState()
                    },
                    onActionStopService = {
                        stopService(Intent(this@MainActivity, LinkAllService::class.java))
                        refreshDashboardState()
                    },
                    onApplyClipboard = { entry ->
                        val svc = Intent(this@MainActivity, LinkAllService::class.java).apply {
                            action = LinkAllService.ACTION_APPLY_CLIPBOARD
                            if (entry.contentHash.isNotBlank()) {
                                putExtra(LinkAllService.EXTRA_CONTENT_HASH, entry.contentHash)
                            }
                            putExtra(LinkAllService.EXTRA_CLIPBOARD_TEXT, entry.preview)
                        }
                        ContextCompat.startForegroundService(this@MainActivity, svc)
                        showSnack("Applied to clipboard")
                        
                    },
                    onDeleteActivity = { entry ->
                        ActivityFeedManager.removeFromFeed(entry.id)
                    },
                    onTogglePinActivity = { entry ->
                        ActivityFeedManager.togglePin(entry.id)
                        showSnack(if (entry.isPinned) "Unpinned" else "Pinned to the top")
                    },
                    onClearActivity = {
                        ActivityFeedManager.clearFeed()
                        showSnack("Activity cleared")
                    },
                    onTrustPeer = { peer ->
                        ContextCompat.startForegroundService(this@MainActivity,
                            Intent(this@MainActivity, LinkAllService::class.java).apply {
                                action = LinkAllService.ACTION_TRUST_PEER
                                putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, peer.id)
                            }
                        )
                        showSnack("Trusted ${peer.name}")
                        window.decorView.postDelayed({ refreshDashboardState() }, 200)
                    },
                    onRejectPeer = { peer ->
                        ContextCompat.startForegroundService(this@MainActivity,
                            Intent(this@MainActivity, LinkAllService::class.java).apply {
                                action = LinkAllService.ACTION_REJECT_PEER
                                putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, peer.id)
                            }
                        )
                        showSnack("Rejected ${peer.name}")
                        window.decorView.postDelayed({ refreshDashboardState() }, 200)
                    },
                    onConnectPeer = { peer ->
                        ContextCompat.startForegroundService(this@MainActivity,
                            Intent(this@MainActivity, LinkAllService::class.java).apply {
                                action = LinkAllService.ACTION_RECONNECT_PEER
                                putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, peer.id)
                            }
                        )
                        showSnack("Connecting to ${peer.name}...")
                    },
                    onDisconnectPeer = { peer ->
                        ContextCompat.startForegroundService(this@MainActivity,
                            Intent(this@MainActivity, LinkAllService::class.java).apply {
                                action = LinkAllService.ACTION_DISCONNECT_PEER
                                putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, peer.id)
                            }
                        )
                        showSnack("Disconnected from ${peer.name}")
                    },
                    onSendPairingRequest = { peer ->
                        ContextCompat.startForegroundService(this@MainActivity,
                            Intent(this@MainActivity, LinkAllService::class.java).apply {
                                action = LinkAllService.ACTION_SEND_PAIRING_REQUEST
                                putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, peer.id)
                            }
                        )
                        showSnack("Pairing request sent to ${peer.name}")
                    },
                    onCancelPairing = { peer ->
                        ContextCompat.startForegroundService(this@MainActivity,
                            Intent(this@MainActivity, LinkAllService::class.java).apply {
                                action = LinkAllService.ACTION_CANCEL_PAIRING_REQUEST
                                putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, peer.id)
                            }
                        )
                    },
                    onRespondPairing = { peer, accepted ->
                        ContextCompat.startForegroundService(this@MainActivity,
                            Intent(this@MainActivity, LinkAllService::class.java).apply {
                                action = LinkAllService.ACTION_RESPOND_TO_PAIRING
                                putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, peer.id)
                                putExtra(PairingActivity.EXTRA_APPROVED, accepted)
                            }
                        )
                        if (accepted) {
                            showSnack("Accepted pairing from ${peer.name}")
                        } else {
                            showSnack("Declined pairing from ${peer.name}")
                        }
                        window.decorView.postDelayed({ refreshDashboardState() }, 200)
                    },
                    onActionStreamCamera = {
                        startActivity(Intent(this@MainActivity, CameraStreamActivity::class.java))
                    },
                    onActionPauseTransfer = { tid ->
                        ContextCompat.startForegroundService(this@MainActivity, Intent(this@MainActivity, LinkAllService::class.java).apply {
                            action = LinkAllService.ACTION_PAUSE_FILE_TRANSFER
                            putExtra(LinkAllService.EXTRA_TRANSFER_ID, tid)
                        })
                    },
                    onActionResumeTransfer = { tid ->
                        ContextCompat.startForegroundService(this@MainActivity, Intent(this@MainActivity, LinkAllService::class.java).apply {
                            action = LinkAllService.ACTION_RESUME_FILE_TRANSFER
                            putExtra(LinkAllService.EXTRA_TRANSFER_ID, tid)
                        })
                    },
                    onActionCancelTransfer = { tid ->
                        ContextCompat.startForegroundService(this@MainActivity, Intent(this@MainActivity, LinkAllService::class.java).apply {
                            action = LinkAllService.ACTION_CANCEL_FILE_TRANSFER
                            putExtra(LinkAllService.EXTRA_TRANSFER_ID, tid)
                        })
                    },
                    onActionAcceptTransfer = { tid ->
                        ContextCompat.startForegroundService(this@MainActivity, Intent(this@MainActivity, LinkAllService::class.java).apply {
                            action = LinkAllService.ACTION_ACCEPT_FILE_TRANSFER
                            putExtra(LinkAllService.EXTRA_TRANSFER_ID, tid)
                        })
                    },
                    onActionRejectTransfer = { tid ->
                        ContextCompat.startForegroundService(this@MainActivity, Intent(this@MainActivity, LinkAllService::class.java).apply {
                            action = LinkAllService.ACTION_REJECT_FILE_TRANSFER
                            putExtra(LinkAllService.EXTRA_TRANSFER_ID, tid)
                        })
                    },
                    onActionSendFiles = { targetId ->
                        targetDeviceIdForNextSend = targetId
                        filePickerLauncher.launch("*/*")
                    },
                    onActionSendFolder = { targetId ->
                        targetDeviceIdForNextSend = targetId
                        folderPickerLauncher.launch(null)
                    },
                    onDropFiles = { targetId, uris ->
                        if (uris.isNotEmpty()) {
                            val intent = Intent(this@MainActivity, LinkAllService::class.java).apply {
                                action = LinkAllService.ACTION_PUSH_SHARED_URI
                                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                                if (uris.isNotEmpty()) {
                                    val cd = android.content.ClipData.newRawUri("shared_uris", uris[0])
                                    for (i in 1 until uris.size) {
                                        cd.addItem(android.content.ClipData.Item(uris[i]))
                                    }
                                    clipData = cd
                                }
                                putStringArrayListExtra(LinkAllService.EXTRA_SHARED_URIS, java.util.ArrayList(uris.map { it.toString() }))
                                putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, targetId)
                            }
                            ContextCompat.startForegroundService(this@MainActivity, intent)
                            showSnack("Sending ${uris.size} file(s)...")
                        }
                    },
                    onForgetPeer = { peer ->
                        ContextCompat.startForegroundService(this@MainActivity,
                            Intent(this@MainActivity, LinkAllService::class.java).apply {
                                action = LinkAllService.ACTION_FORGET_PEER
                                putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, peer.id)
                            }
                        )
                        showSnack("Forgot ${peer.name}")
                        window.decorView.postDelayed({ refreshDashboardState() }, 200)
                    },
                    onOpenDiagnostics = {
                        startActivity(Intent(this@MainActivity, DiagnosticsActivity::class.java))
                    },
                    onBatterySettingsClicked = { openBatterySettings() },
                    onStorageSettingsClicked = { openStorageSettings() },
                    onNotificationSettingsClicked = { openNotificationSettings() },
                    onReplayOnboarding = {
                        getSharedPreferences(LinkAllService.PREFS_NAME, MODE_PRIVATE).edit().putBoolean("has_completed_onboarding", false).apply()
                        hasCompletedOnboarding.value = false
                    }
                )
                }
                
                // Custom Toast Overlay
                AnimatedVisibility(
                    visible = toastMessage.value.isNotEmpty(),
                    enter = slideInVertically(initialOffsetY = { -it }) + fadeIn(),
                    exit = slideOutVertically(targetOffsetY = { -it }) + fadeOut(),
                    modifier = Modifier.align(Alignment.TopCenter).padding(top = 48.dp)
                ) {
                    CRToast(message = toastMessage.value, isDark = isDarkMode.value)
                }
                }
            }
        }
        
        launchService()
        refreshDashboardState()
    }

    override fun onResume() {
        super.onResume()
        refreshDashboardState()
        
        try {
            startService(Intent(this, LinkAllService::class.java))
        } catch (e: Exception) {
            android.util.Log.e("MainActivity", "Failed to refresh service onResume", e)
        }
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        if (targetDeviceIdForNextSend != null) {
            outState.putString("targetDeviceIdForNextSend", targetDeviceIdForNextSend)
        }
    }

    override fun onStart() {
        super.onStart()
        ContextCompat.registerReceiver(
            this, statusReceiver,
            IntentFilter(LinkAllService.ACTION_STATUS_CHANGED),
            ContextCompat.RECEIVER_NOT_EXPORTED
        )
    }

    override fun onStop() {
        unregisterReceiver(statusReceiver)
        super.onStop()
    }

    private fun saveBooleanPref(key: String, value: Boolean) {
        getSharedPreferences(LinkAllService.PREFS_NAME, MODE_PRIVATE).edit().putBoolean(key, value).apply()
        sendBroadcast(Intent(LinkAllService.ACTION_SETTINGS_CHANGED).setPackage(packageName))
    }

    private fun refreshDashboardState() {
        val prefs = getSharedPreferences(LinkAllService.PREFS_NAME, MODE_PRIVATE)
        isServiceRunning.value = prefs.getBoolean(LinkAllService.PREF_SERVICE_RUNNING, false)
        isSyncEnabled.value = prefs.getBoolean("sync_enabled", true)
        syncText.value = prefs.getBoolean("sync_text", true)
        syncImages.value = prefs.getBoolean("sync_images", true)
        syncFiles.value = prefs.getBoolean("sync_files", true)
        callContinuityEnabled.value = prefs.getBoolean("call_continuity_enabled", false)
        notificationMirroringEnabled.value = prefs.getBoolean("notification_mirroring", false)
        autoForwardSms.value = prefs.getBoolean("auto_forward_sms", false)
        autoForwardScreenshots.value = prefs.getBoolean("auto_forward_screenshots", false)
        deviceName.value = prefs.getString("device_name", null)?.trim()?.takeIf { it.isNotBlank() }
            ?: prefs.getString("local_device_name", null)?.trim()?.takeIf { it.isNotBlank() }
            ?: Build.MODEL
        deviceId.value = prefs.getString("device_id", "—") ?: "—"
        isDarkMode.value = prefs.getBoolean("dark_mode", false)
        hasCompletedOnboarding.value = prefs.getBoolean("has_completed_onboarding", false)
        
        val allPeers = prefs.peerSnapshots()
        peers.value = allPeers

        val isConnected = allPeers.any { it.isConnected }
        healthIssues.value = withBatteryCheck(prefs.healthIssues(), allPeers)
        ambientStatus.value = if (isConnected) "Secure Connection  •  LAN Active" else "Looking for network..."
    }

    /**
     * The engine can't see Android's battery limits. When paired devices
     * can't be found and the phone may be pausing Link All, say that first:
     * it's the likelier cause.
     */
    private fun withBatteryCheck(engineIssues: List<HealthIssue>, allPeers: List<PeerSnapshot>): List<HealthIssue> {
        val pm = getSystemService(POWER_SERVICE) as android.os.PowerManager
        val notFound = engineIssues.indexOfFirst { it.kind == "devices_not_found" }
        if (notFound < 0 || pm.isIgnoringBatteryOptimizations(packageName)) return engineIssues
        val battery = HealthIssue(
            "battery_restricted",
            "Android may be pausing Link All",
            "To save battery your phone can stop Link All in the background, and your devices can't reach it. Let it run in the background.",
            null
        )
        return engineIssues.toMutableList().apply { add(notFound, battery) }
    }

    private fun onHealthAction(issue: HealthIssue) {
        when (issue.kind) {
            "no_network" -> runCatching { startActivity(Intent(android.provider.Settings.ACTION_WIFI_SETTINGS)) }
            "sync_paused" -> {
                isSyncEnabled.value = true
                saveBooleanPref("sync_enabled", true)
            }
            "devices_not_found", "connection_blocked" -> sendAction(LinkAllService.ACTION_SCAN_NOW)
            "listener_down" -> {
                stopService(Intent(this, LinkAllService::class.java))
                launchService()
            }
            "battery_restricted" -> openBatterySettings()
        }
    }

    private fun showSnack(message: String) {
        toastMessage.value = message
        // Auto-dismiss after 3 seconds
        feedRefreshHandler.postDelayed({
            if (toastMessage.value == message) {
                toastMessage.value = ""
            }
        }, 3000)
    }

    private fun launchService() = runCatching {
        ContextCompat.startForegroundService(this,
            Intent(this, LinkAllService::class.java).apply {
                action = LinkAllService.ACTION_START 
            }
        )
    }

    private fun sendAction(action: String) = runCatching {
        ContextCompat.startForegroundService(this,
            Intent(this, LinkAllService::class.java).apply { this.action = action })
    }

    private fun requestRuntimePermissions() {
        val needed = mutableListOf<String>()

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
            needed += Manifest.permission.POST_NOTIFICATIONS
        }
        // The Play build asks for notifications only; phone access is asked
        // for when the user turns on Calls.
        if (!BuildConfig.FULL_PERMISSIONS) {
            if (needed.isNotEmpty()) requestPermissions(needed.toTypedArray(), 1001)
            return
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            if (checkSelfPermission(Manifest.permission.READ_MEDIA_IMAGES) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                needed += Manifest.permission.READ_MEDIA_IMAGES
            }
            if (checkSelfPermission(Manifest.permission.READ_MEDIA_VIDEO) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                needed += Manifest.permission.READ_MEDIA_VIDEO
            }
            if (checkSelfPermission(Manifest.permission.READ_MEDIA_AUDIO) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                needed += Manifest.permission.READ_MEDIA_AUDIO
            }
        } else if (checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
            needed += Manifest.permission.READ_EXTERNAL_STORAGE
        }

        if (checkSelfPermission(Manifest.permission.READ_PHONE_STATE) !=
            android.content.pm.PackageManager.PERMISSION_GRANTED) {
            needed += Manifest.permission.READ_PHONE_STATE
        }

        if (needed.isNotEmpty()) {
            prompt.value = Prompt(
                icon = Icons.Outlined.Security,
                title = "A few permissions",
                message = "Link All needs storage to save and send files, notifications to show transfer progress, and phone state for calls on your computer. Android asks for each one next.",
                confirm = "Continue",
                dismiss = null,
                onConfirm = { requestPermissions(needed.toTypedArray(), 1001) },
            )
        }

        if (BuildConfig.FULL_PERMISSIONS && Build.VERSION.SDK_INT >= Build.VERSION_CODES.R && !android.os.Environment.isExternalStorageManager()) {
            runCatching {
                val intent = Intent(
                    android.provider.Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                    android.net.Uri.parse("package:$packageName")
                )
                startActivity(intent)
            }
        }
    }

    private fun requestBatteryOptimizationExemption() {
        // Play restricts the direct request; that build offers the system
        // list from Settings instead of opening it on every launch.
        if (!BuildConfig.FULL_PERMISSIONS) return
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            val pm = getSystemService(POWER_SERVICE) as android.os.PowerManager
            if (!pm.isIgnoringBatteryOptimizations(packageName)) {
                runCatching {
                    startActivity(getBatteryOptimizationExemptionIntent())
                }
            }
        }
    }

    private fun getBatteryOptimizationExemptionIntent(): android.content.Intent {
        return android.content.Intent(
            android.provider.Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
            android.net.Uri.parse("package:$packageName")
        )
    }

    override fun onNewIntent(intent: Intent?) {
        super.onNewIntent(intent)
        if (intent?.getBooleanExtra("request_permissions", false) == true) {
            requestRuntimePermissions()
        }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == 1001) {
            val readPhone = permissions.indexOf(Manifest.permission.READ_PHONE_STATE)
            if (readPhone >= 0 &&
                grantResults[readPhone] == android.content.pm.PackageManager.PERMISSION_GRANTED) {
                startService(Intent(this, LinkAllService::class.java))
            }
        } else if (requestCode == 1002) {
            ContextCompat.startForegroundService(this, Intent(this, LinkAllService::class.java).apply {
                action = LinkAllService.ACTION_SETTINGS_CHANGED
            })
        }
    }

    /** Says what leaves the phone before asking for the access that allows it. */
    private fun confirmDataSharing(title: String, message: String, onAllow: () -> Unit) {
        prompt.value = Prompt(
            icon = Icons.Outlined.Lock,
            title = title,
            message = message,
            confirm = "Allow",
            dismiss = "Not now",
            onConfirm = onAllow,
        )
    }

    private fun requestCallContinuityPermissions() {
        val needed = mutableListOf<String>()
        if (ContextCompat.checkSelfPermission(this, android.Manifest.permission.READ_PHONE_STATE) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
            needed += android.Manifest.permission.READ_PHONE_STATE
        }
        // The Play build mirrors calls without these: no caller number on Android 9+
        // (so no contact to look up) and no answering from the computer.
        if (BuildConfig.FULL_PERMISSIONS) {
            if (ContextCompat.checkSelfPermission(this, android.Manifest.permission.READ_CONTACTS) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                needed += android.Manifest.permission.READ_CONTACTS
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && ContextCompat.checkSelfPermission(this, android.Manifest.permission.ANSWER_PHONE_CALLS) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                needed += android.Manifest.permission.ANSWER_PHONE_CALLS
            }
            if (ContextCompat.checkSelfPermission(this, android.Manifest.permission.READ_CALL_LOG) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                needed += android.Manifest.permission.READ_CALL_LOG
            }
        }
        if (needed.isNotEmpty()) {
            requestPermissions(needed.toTypedArray(), 1002)
        }
    }

    private fun requestNotificationListenerPermission() {
        val enabledListeners = android.provider.Settings.Secure.getString(contentResolver, "enabled_notification_listeners")
        val hasPermission = enabledListeners?.contains(packageName) == true
        if (!hasPermission) {
            Toast.makeText(this, "Please allow Link All to read notifications", Toast.LENGTH_LONG).show()
            startActivity(Intent("android.settings.ACTION_NOTIFICATION_LISTENER_SETTINGS"))
        }
    }

    private fun requestSmsPermission() {
        if (ContextCompat.checkSelfPermission(this, android.Manifest.permission.RECEIVE_SMS) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(android.Manifest.permission.RECEIVE_SMS), 1003)
        }
    }

    private fun requestMediaPermissions() {
        val needed = mutableListOf<String>()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            if (ContextCompat.checkSelfPermission(this, android.Manifest.permission.READ_MEDIA_IMAGES) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                needed += android.Manifest.permission.READ_MEDIA_IMAGES
            }
            if (Build.VERSION.SDK_INT >= 34) {
                if (ContextCompat.checkSelfPermission(this, android.Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                    needed += android.Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED
                }
            }
        } else {
            if (ContextCompat.checkSelfPermission(this, android.Manifest.permission.READ_EXTERNAL_STORAGE) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                needed += android.Manifest.permission.READ_EXTERNAL_STORAGE
            }
        }
        if (needed.isNotEmpty()) {
            requestPermissions(needed.toTypedArray(), 1004)
        }
    }

    private fun openBatterySettings() {
        runCatching {
            startActivity(
                if (BuildConfig.FULL_PERMISSIONS) Intent(
                    android.provider.Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
                    android.net.Uri.parse("package:$packageName"))
                else Intent(android.provider.Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)
            )
            if (!BuildConfig.FULL_PERMISSIONS) {
                Toast.makeText(this, "Find Link All and choose \"Don't optimize\"", Toast.LENGTH_LONG).show()
            }
        }.onFailure {
            runCatching {
                startActivity(Intent(
                    android.provider.Settings.ACTION_BATTERY_SAVER_SETTINGS))
            }.onFailure {
                Toast.makeText(this,
                    "Open Settings -> Battery -> Link All -> disable optimisation",
                    Toast.LENGTH_LONG).show()
            }
        }
    }

    private fun openStorageSettings() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            runCatching {
                startActivity(Intent(
                    android.provider.Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                    android.net.Uri.parse("package:$packageName")
                ))
            }.onFailure {
                runCatching {
                    startActivity(Intent(android.provider.Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION))
                }.onFailure {
                    requestMediaPermissions()
                }
            }
        } else {
            requestMediaPermissions()
        }
    }

    private fun openNotificationSettings() {
        runCatching {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                startActivity(Intent(android.provider.Settings.ACTION_CHANNEL_NOTIFICATION_SETTINGS).apply {
                    putExtra(android.provider.Settings.EXTRA_APP_PACKAGE, packageName)
                    putExtra(android.provider.Settings.EXTRA_CHANNEL_ID, "cr_service")
                })
            } else {
                startActivity(Intent(android.provider.Settings.ACTION_APP_NOTIFICATION_SETTINGS).apply {
                    putExtra(android.provider.Settings.EXTRA_APP_PACKAGE, packageName)
                })
            }
        }.onFailure {
            runCatching {
                startActivity(Intent(android.provider.Settings.ACTION_APP_NOTIFICATION_SETTINGS).apply {
                    putExtra(android.provider.Settings.EXTRA_APP_PACKAGE, packageName)
                })
            }.onFailure {
                Toast.makeText(this, "Long-press Link All notification -> Settings -> Minimize", Toast.LENGTH_LONG).show()
            }
        }
    }

    private fun showMagicLinkPairingDialog() {
        // Fallback to directly starting QR scanner since we dropped the programmatic UI dialog
        startQrScanner()
    }

    private fun startQrScanner() {
        val options = GmsBarcodeScannerOptions.Builder()
            .setBarcodeFormats(Barcode.FORMAT_QR_CODE)
            .enableAutoZoom()
            .build()
        val scanner = GmsBarcodeScanning.getClient(this, options)
        scanner.startScan()
            .addOnSuccessListener { barcode: Barcode ->
                val rawValue = barcode.rawValue
                if (!rawValue.isNullOrBlank()) {
                    if (handlePairingInput(rawValue)) {
                        showSnack("QR scanned successfully! Connecting...")
                    } else {
                        showSnack("Invalid QR code format")
                    }
                } else {
                    showSnack("No QR code found")
                }
            }
            .addOnFailureListener { e: java.lang.Exception ->
                showSnack("QR Scan failed: ${e.message}")
            }
    }

    private fun handlePairingInput(input: String): Boolean {
        val cleaned = input.trim()
        val ctx = this
        if (cleaned.startsWith("linkall://pair") || cleaned.startsWith("linkall://pair")) {
            val uri = android.net.Uri.parse(cleaned)
            
            // New QR Code Pairing format with Auth Token
            val id = uri.getQueryParameter("id")
            val token = uri.getQueryParameter("token")
            val fingerprint = uri.getQueryParameter("fingerprint")
            val peerName = uri.getQueryParameter("name")?.let {
                java.net.URLDecoder.decode(it, "UTF-8")
            } ?: "your computer"

            if (id != null && token != null) {
                val ip = uri.getQueryParameter("ip")
                val port = uri.getQueryParameter("port")?.toIntOrNull() ?: 47823
                ContextCompat.startForegroundService(ctx,
                    Intent(ctx, LinkAllService::class.java).apply {
                        action = LinkAllService.ACTION_TRUST_PEER_FROM_QR
                        putExtra(LinkAllService.EXTRA_TARGET_DEVICE_ID, id)
                        putExtra(LinkAllService.EXTRA_TOKEN, token)
                        putExtra(LinkAllService.EXTRA_FINGERPRINT, fingerprint)
                        if (ip != null) {
                            putExtra("ip", ip)
                            putExtra("port", port)
                        }
                    }
                )
                showSnack("Connecting securely to $peerName...")
                return true
            }

            // Legacy IP-based Magic Link format
            val ip = uri.getQueryParameter("ip")
            val port = uri.getQueryParameter("port")?.toIntOrNull() ?: 47823
            if (ip != null) {
                ContextCompat.startForegroundService(ctx,
                    Intent(ctx, LinkAllService::class.java).apply {
                        action = LinkAllService.ACTION_CONNECT_MANUAL
                        putExtra("ip", ip)
                        putExtra("port", port)
                    }
                )
                showSnack("Connecting to $peerName ($ip)...")
                return true
            }
        } else {
            val parts = cleaned.split(":")
            val ip = parts[0].trim()
            val port = if (parts.size > 1) parts[1].trim().toIntOrNull() ?: 47823 else 47823
            if (ip.matches(Regex("""\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}"""))) {
                ContextCompat.startForegroundService(ctx,
                    Intent(ctx, LinkAllService::class.java).apply {
                        action = LinkAllService.ACTION_CONNECT_MANUAL
                        putExtra("ip", ip)
                        putExtra("port", port)
                    }
                )
                showSnack("Connecting to $ip:$port...")
                return true
            }
        }
        return false
    }


}

@Composable
fun CRToast(message: String, isDark: Boolean) {
    Box(
        modifier = Modifier
            .padding(horizontal = 24.dp)
            .background(
                color = if (isDark) Color(0xFF1E1E1E).copy(alpha = 0.95f) else Color.White.copy(alpha = 0.95f),
                shape = RoundedCornerShape(100.dp)
            )
            .border(
                width = 0.5.dp,
                color = if (isDark) Color.White.copy(alpha = 0.1f) else Color.Black.copy(alpha = 0.05f),
                shape = RoundedCornerShape(100.dp)
            )
            .padding(horizontal = 16.dp, vertical = 10.dp),
        contentAlignment = Alignment.Center
    ) {
        Text(
            text = message,
            color = CRTheme.textHigh(isDark),
            fontSize = 13.sp,
            fontWeight = FontWeight.Medium,
            letterSpacing = 0.5.sp
        )
    }
}
