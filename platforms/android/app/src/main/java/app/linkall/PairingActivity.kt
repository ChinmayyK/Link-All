package app.linkall

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.SystemBarStyle
import app.linkall.ui.PairingScreen
import app.linkall.ui.theme.AppTheme
import android.content.BroadcastReceiver
import android.content.Context
import android.content.IntentFilter

class PairingActivity : ComponentActivity() {

    companion object {
        const val EXTRA_DEVICE_ID       = "device_id"
        const val EXTRA_DEVICE_NAME     = "device_name"
        const val EXTRA_FINGERPRINT     = "fingerprint"
        const val EXTRA_PIN             = "pin"
        const val EXTRA_IS_INITIATOR    = "is_initiator"
        const val ACTION_PAIRING_RESULT = "app.linkall.PAIRING_RESULT"
        const val EXTRA_APPROVED        = "approved"
    }

    private var targetDeviceId: String? = null

    private val statusReceiver = object : BroadcastReceiver() {
        override fun onReceive(ctx: Context?, intent: Intent?) {
            if (intent?.action == "app.linkall.CLOSE_PAIRING_UI") {
                // A close aimed at another device's request leaves this one up.
                val forDevice = intent.getStringExtra(EXTRA_DEVICE_ID)
                if (forDevice != null && forDevice != targetDeviceId) return
                val toDashboard = Intent(this@PairingActivity, MainActivity::class.java).apply {
                    addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
                }
                startActivity(toDashboard)
                finish()
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
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
        val deviceId    = intent.getStringExtra(EXTRA_DEVICE_ID)   ?: return finish()
        targetDeviceId = deviceId

        val filter = IntentFilter().apply {
            addAction(LinkAllService.ACTION_STATUS_CHANGED)
            addAction("app.linkall.CLOSE_PAIRING_UI")
        }

        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.TIRAMISU) {
            registerReceiver(statusReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("UnspecifiedRegisterReceiverFlag")
            registerReceiver(statusReceiver, filter)
        }
        val deviceName  = intent.getStringExtra(EXTRA_DEVICE_NAME) ?: "Unknown device"
        val fingerprint = intent.getStringExtra(EXTRA_FINGERPRINT) ?: ""
        val pin         = intent.getStringExtra(EXTRA_PIN)         ?: "------"
        val isInitiator = intent.getBooleanExtra(EXTRA_IS_INITIATOR, false)

        val prefs = getSharedPreferences(LinkAllService.PREFS_NAME, MODE_PRIVATE)
        val themeMode = app.linkall.ui.theme.themeModeOf(prefs)

        setContent {
            val isDarkMode = app.linkall.ui.theme.isDarkFor(themeMode)
            AppTheme(useDarkTheme = isDarkMode) {
                // Back is "later", not "no": the request stays on the home
                // screen until it is answered or expires.
                androidx.activity.compose.BackHandler {
                    closeToDashboard()
                }
                PairingScreen(
                    isDark = isDarkMode,
                    deviceName = deviceName,
                    pin = pin,
                    fingerprint = fingerprint,
                    isInitiator = isInitiator,
                    onApprove = { sendResult(deviceId, true) },
                    onDeny = { sendResult(deviceId, false) },
                    // Running out of time is not a decline: the core expires the
                    // request on both devices, so just step out of the way.
                    onExpire = { closeToDashboard() }
                )
            }
        }
    }

    private fun sendResult(deviceId: String, approved: Boolean) {
        val intent = Intent(ACTION_PAIRING_RESULT).apply {
            setPackage(packageName)
            putExtra(EXTRA_DEVICE_ID, deviceId)
            putExtra(EXTRA_APPROVED, approved)
        }
        sendBroadcast(intent)
        closeToDashboard()
    }

    private fun closeToDashboard() {
        val toDashboard = Intent(this@PairingActivity, MainActivity::class.java).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        }
        startActivity(toDashboard)
        finish()
    }

    override fun onDestroy() {
        super.onDestroy()
        try {
            unregisterReceiver(statusReceiver)
        } catch (e: Exception) {
            // Ignore if not registered
        }
    }
}
