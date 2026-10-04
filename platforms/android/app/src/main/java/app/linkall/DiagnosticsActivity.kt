package app.linkall

import app.linkall.ui.theme.*

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material.icons.filled.NetworkCell
import androidx.compose.material.icons.filled.ContentPaste
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.linkall.ui.theme.AppTheme
import app.linkall.ui.theme.CRTheme

class DiagnosticsActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val prefs = getSharedPreferences(LinkAllService.PREFS_NAME, MODE_PRIVATE)
        
        setContent {
            val isDark = prefs.getBoolean("dark_mode", false)
            val isServiceRunning = prefs.getBoolean(LinkAllService.PREF_SERVICE_RUNNING, false)
            val connectedCount = prefs.getInt("connected_count", 0)
            val autoApply = prefs.getBoolean("auto_apply_clipboard", true)
            
            AppTheme(useDarkTheme = isDark) {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = CRTheme.bg(isDark)
                ) {
                    Column(
                        modifier = Modifier
                            .fillMaxSize()
                            .systemBarsPadding()
                            .padding(16.dp)
                    ) {
                        // Header
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.SpaceBetween
                        ) {
                            Text(
                                "Diagnostics",
                                fontSize = 24.sp,
                                fontWeight = FontWeight.Bold,
                                color = CRTheme.textHigh(isDark)
                            )
                            IconButton(onClick = { finish() }) {
                                Icon(Icons.Default.Close, contentDescription = "Close", tint = CRTheme.textMedium(isDark))
                            }
                        }
                        
                        Spacer(modifier = Modifier.height(24.dp))
                        
                        DiagnosticItem(
                            isDark = isDark,
                            icon = Icons.Default.Bolt,
                            title = "Background Service",
                            status = if (isServiceRunning) "Running" else "Stopped",
                            isOk = isServiceRunning,
                            suggestion = if (isServiceRunning) null else "Service is not running.",
                            actionLabel = if (isServiceRunning) null else "Start Service",
                            onAction = {
                                androidx.core.content.ContextCompat.startForegroundService(
                                    this@DiagnosticsActivity,
                                    android.content.Intent(this@DiagnosticsActivity, LinkAllService::class.java).apply {
                                        action = LinkAllService.ACTION_START
                                    }
                                )
                                finish()
                            }
                        )
                        
                        DiagnosticItem(
                            isDark = isDark,
                            icon = Icons.Default.NetworkCell,
                            title = "Local Network",
                            status = if (connectedCount > 0) "Connected to $connectedCount peers" else "Looking for peers",
                            isOk = connectedCount > 0,
                            suggestion = if (connectedCount > 0) null else "No peers found on current network.",
                            actionLabel = if (connectedCount > 0) null else "Scan Again",
                            onAction = {
                                androidx.core.content.ContextCompat.startForegroundService(
                                    this@DiagnosticsActivity,
                                    android.content.Intent(this@DiagnosticsActivity, LinkAllService::class.java).apply {
                                        action = LinkAllService.ACTION_SCAN_NOW
                                    }
                                )
                                finish()
                            }
                        )
                        
                        DiagnosticItem(
                            isDark = isDark,
                            icon = Icons.Default.ContentPaste,
                            title = "Clipboard Sync",
                            status = if (autoApply) "Auto-Apply Enabled" else "Manual/Paused",
                            isOk = autoApply,
                            suggestion = if (autoApply) null else "Enable Auto-Apply for seamless paste.",
                            actionLabel = if (autoApply) null else "Enable",
                            onAction = {
                                prefs.edit().putBoolean("auto_apply_clipboard", true).apply()
                                finish()
                            }
                        )
                        
                        val manufacturer = android.os.Build.MANUFACTURER.lowercase()
                        val isAggressiveOem = manufacturer in listOf("xiaomi", "samsung", "oppo", "vivo", "oneplus", "huawei")
                        
                        if (isAggressiveOem) {
                            DiagnosticItem(
                                isDark = isDark,
                                icon = Icons.Default.Bolt,
                                title = "OEM Battery Restrictions",
                                status = "${manufacturer.replaceFirstChar { it.uppercase() }} may kill background sync",
                                isOk = false,
                                suggestion = "Manually enable 'AutoStart' or remove background restrictions in system settings.",
                                actionLabel = "Open Settings",
                                onAction = {
                                    try {
                                        val intent = android.content.Intent()
                                        when (manufacturer) {
                                            "xiaomi" -> intent.component = android.content.ComponentName("com.miui.securitycenter", "com.miui.permcenter.autostart.AutoStartManagementActivity")
                                            "samsung" -> intent.component = android.content.ComponentName("com.samsung.android.lool", "com.samsung.android.sm.ui.battery.BatteryActivity")
                                            else -> intent.action = android.provider.Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS
                                        }
                                        startActivity(intent)
                                    } catch (e: Exception) {
                                        try {
                                            startActivity(android.content.Intent(android.provider.Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))
                                        } catch (e2: Exception) {
                                            startActivity(android.content.Intent(android.provider.Settings.ACTION_SETTINGS))
                                        }
                                    }
                                }
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun DiagnosticItem(
    isDark: Boolean,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    title: String,
    status: String,
    isOk: Boolean,
    suggestion: String?,
    actionLabel: String? = null,
    onAction: (() -> Unit)? = null
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 8.dp)
            .background(if (isDark) androidx.compose.ui.graphics.Color(0xFF2C2C2E) else androidx.compose.ui.graphics.Color.White, RoundedCornerShape(12.dp))
            .padding(16.dp),
        verticalAlignment = Alignment.Top
    ) {
        Icon(
            icon,
            contentDescription = null,
            tint = if (isOk) CRTheme.accentGreen else CRTheme.accentAmber,
            modifier = Modifier.size(24.dp)
        )
        Spacer(modifier = Modifier.width(16.dp))
        Column {
            Text(
                title,
                fontSize = 16.sp,
                fontWeight = FontWeight.SemiBold,
                color = CRTheme.textHigh(isDark)
            )
            Text(
                status,
                fontSize = 14.sp,
                color = if (isOk) CRTheme.accentGreen else CRTheme.accentAmber
            )
            if (suggestion != null) {
                Spacer(modifier = Modifier.height(4.dp))
                Text(
                    suggestion,
                    fontSize = 12.sp,
                    color = CRTheme.textMedium(isDark)
                )
            }
            if (actionLabel != null && onAction != null) {
                Spacer(modifier = Modifier.height(8.dp))
                Button(
                    onClick = onAction,
                    colors = ButtonDefaults.buttonColors(containerColor = CRTheme.blueSoft),
                    contentPadding = PaddingValues(horizontal = 12.dp, vertical = 4.dp),
                    modifier = Modifier.height(32.dp)
                ) {
                    Text(actionLabel, color = CRTheme.bg(isDark), fontSize = 12.sp, fontWeight = FontWeight.Bold)
                }
            }
        }
    }
}
