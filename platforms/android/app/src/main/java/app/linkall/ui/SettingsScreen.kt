package app.linkall.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

fun getLocalIpAddress(): String {
    try {
        var preferredIp: String? = null
        var fallbackIp: String? = null
        val en = java.net.NetworkInterface.getNetworkInterfaces() ?: return "Unknown IP"
        while (en.hasMoreElements()) {
            val intf = en.nextElement()
            val name = intf.name.lowercase()
            val enumIpAddr = intf.inetAddresses
            while (enumIpAddr.hasMoreElements()) {
                val inetAddress = enumIpAddr.nextElement()
                if (!inetAddress.isLoopbackAddress && inetAddress is java.net.Inet4Address) {
                    val host = inetAddress.hostAddress ?: continue
                    if (host.isEmpty()) continue
                    if (name.startsWith("wlan") || name.startsWith("eth") || name.startsWith("en") || name.startsWith("ap")) {
                        return host
                    } else if (!name.startsWith("rmnet") && !name.startsWith("tun") && !name.startsWith("ccmni") && !name.startsWith("pdp") && !name.startsWith("ppp") && !name.startsWith("wireguard")) {
                        if (preferredIp == null) preferredIp = host
                    } else {
                        if (fallbackIp == null) fallbackIp = host
                    }
                }
            }
        }
        return preferredIp ?: fallbackIp ?: "Unknown IP"
    } catch (ex: Exception) {
        // Ignore
    }
    return "Unknown IP"
}

@Composable
fun SettingsTab(
    isDark: Boolean,
    isServiceRunning: Boolean,
    isSyncEnabled: Boolean,
    syncText: Boolean,
    syncImages: Boolean,
    syncFiles: Boolean,
    callContinuityEnabled: Boolean,
    notificationMirroringEnabled: Boolean,
    autoForwardSms: Boolean,
    autoForwardScreenshots: Boolean,
    deviceName: String,
    deviceId: String,
    onSyncEnabledChange: (Boolean) -> Unit,
    onSyncTextChange: (Boolean) -> Unit,
    onSyncImagesChange: (Boolean) -> Unit,
    onSyncFilesChange: (Boolean) -> Unit,
    onCallContinuityChange: (Boolean) -> Unit,
    onNotificationMirroringChange: (Boolean) -> Unit,
    onAutoForwardSmsChange: (Boolean) -> Unit,
    onAutoForwardScreenshotsChange: (Boolean) -> Unit,
    themeMode: String,
    onThemeModeChange: (String) -> Unit,
    onStartSync: () -> Unit,
    onScanNow: () -> Unit,
    onActionDisconnectAll: () -> Unit,
    onActionStopService: () -> Unit,
    onOpenDiagnostics: () -> Unit,
    onBatterySettingsClicked: () -> Unit = {},
    onStorageSettingsClicked: () -> Unit = {},
    onNotificationSettingsClicked: () -> Unit = {}
) {
    val c = rememberDdColors(isDark)
    val context = LocalContext.current
    val ip = remember { getLocalIpAddress() }
    val version = remember {
        runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull() ?: ""
    }

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = PageGutter, end = PageGutter, bottom = 140.dp)
    ) {
        item {
            Column {
                PageTitle(c, "Settings")
                Spacer(Modifier.height(20.dp))
                Panel(c) {
                    ListRow(
                        c, osIcon(DeviceOs.Android), deviceName.ifBlank { "This phone" },
                        detail = when {
                            !isServiceRunning -> "Service stopped"
                            !isSyncEnabled -> "Sync paused · $ip"
                            else -> "Active · $ip"
                        },
                        detailColor = if (isServiceRunning && isSyncEnabled) c.live else c.warn,
                        iconTint = c.accent,
                        iconBackground = c.accentSoft
                    )
                }
            }
        }

        item {
            Column {
                SectionHeader(c, "Sync", null)
                Panel(c) {
                    SwitchRow(c, Icons.Outlined.Sync, "Sync", "Turn off to pause everything", isSyncEnabled, onSyncEnabledChange)
                    if (isSyncEnabled) {
                        Hairline(c)
                        SwitchRow(c, Icons.Outlined.TextFields, "Text", null, syncText, onSyncTextChange)
                        Hairline(c)
                        SwitchRow(c, Icons.Outlined.Image, "Images", null, syncImages, onSyncImagesChange)
                        Hairline(c)
                        SwitchRow(c, Icons.Outlined.FilePresent, "Files", "Saved to Downloads", syncFiles, onSyncFilesChange)
                    }
                }
            }
        }

        item {
            Column {
                SectionHeader(c, "Continuity", null)
                Panel(c) {
                    // The Play build may not read SMS, or all photos in the
                    // background (see src/play/AndroidManifest.xml).
                    if (app.linkall.BuildConfig.FULL_PERMISSIONS) {
                        SwitchRow(c, Icons.Outlined.Sms, "SMS codes", "Copy one-time codes to your computer", autoForwardSms, onAutoForwardSmsChange)
                        Hairline(c)
                        SwitchRow(c, Icons.Outlined.Screenshot, "Screenshots", "Send new screenshots automatically", autoForwardScreenshots, onAutoForwardScreenshotsChange)
                        Hairline(c)
                    }
                    SwitchRow(
                        c, Icons.Outlined.Call, "Calls",
                        if (app.linkall.BuildConfig.FULL_PERMISSIONS) "Needs Phone, Contacts and Call log access"
                        else "Show incoming calls on your computer. Needs Phone access",
                        callContinuityEnabled, onCallContinuityChange
                    )
                    Hairline(c)
                    SwitchRow(c, Icons.Outlined.Notifications, "Notifications", "Mirror phone notifications", notificationMirroringEnabled, onNotificationMirroringChange)
                }
            }
        }

        item {
            Column {
                SectionHeader(c, "Appearance", null)
                Panel(c) {
                    Column(Modifier.padding(horizontal = 16.dp, vertical = 14.dp)) {
                        Text("Theme", style = DdType.label, color = c.text)
                        Spacer(Modifier.height(10.dp))
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            listOf("system" to "Match system", "light" to "Light", "dark" to "Dark").forEach { (mode, label) ->
                                FilterChip(c, label, selected = themeMode == mode) { onThemeModeChange(mode) }
                            }
                        }
                    }
                }
            }
        }

        item {
            Column {
                SectionHeader(c, "Permissions", null)
                Panel(c) {
                    ListRow(c, Icons.Outlined.BatteryChargingFull, "Battery", "Allow background use so clips arrive instantly", onClick = onBatterySettingsClicked)
                    Hairline(c)
                    // The Play build has no all-files access to ask for.
                    if (app.linkall.BuildConfig.FULL_PERMISSIONS) {
                        ListRow(c, Icons.Outlined.Folder, "All files access", "Lets your computer browse this phone's files", onClick = onStorageSettingsClicked)
                        Hairline(c)
                    }
                    ListRow(c, Icons.Outlined.NotificationsOff, "Hide the Link All notification", "Turn off \"Link All\" here. Sync keeps running", onClick = onNotificationSettingsClicked)
                }
            }
        }

        // Rarely needed; pausing is the Sync switch above, and devices are
        // forgotten from the Devices tab.
        item {
            Column {
                SectionHeader(c, "Advanced", null)
                Panel(c) {
                    if (!isServiceRunning) {
                        ListRow(c, Icons.Outlined.PlayCircleOutline, "Start service", onClick = onStartSync)
                        Hairline(c)
                    }
                    ListRow(c, Icons.Outlined.Radar, "Scan for devices", onClick = onScanNow)
                    Hairline(c)
                    ListRow(c, Icons.Outlined.LinkOff, "Disconnect all", onClick = onActionDisconnectAll)
                    Hairline(c)
                    ListRow(c, Icons.Outlined.MonitorHeart, "Diagnostics", onClick = onOpenDiagnostics)
                    Hairline(c)
                    ListRow(
                        c, Icons.Outlined.PowerSettingsNew, "Stop service",
                        detail = "Link All stops until you open it again",
                        iconTint = c.danger,
                        titleColor = c.danger,
                        onClick = onActionStopService
                    )
                }
            }
        }

        item {
            Column(Modifier.fillMaxWidth().padding(top = 28.dp, start = 4.dp)) {
                Text("Link All ${version}".trim(), style = DdType.label, color = c.text)
                Text("No cloud, no account, no telemetry.", style = DdType.small, color = c.textMuted)
                if (deviceId.isNotBlank()) {
                    Spacer(Modifier.height(6.dp))
                    Text(deviceId, style = DdType.mono, color = c.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}
