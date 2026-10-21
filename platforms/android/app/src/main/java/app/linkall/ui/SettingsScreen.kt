package app.linkall.ui

import app.linkall.UpdateChecker
import app.linkall.LinkAllService
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.animation.togetherWith
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
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
    findPhoneEnabled: Boolean,
    shareCodesEnabled: Boolean,
    notificationMirroringEnabled: Boolean,
    autoForwardSms: Boolean,
    autoForwardScreenshots: Boolean,
    deviceName: String,
    deviceId: String,
    onSyncEnabledChange: (Boolean) -> Unit,
    onSyncTextChange: (Boolean) -> Unit,
    onSyncImagesChange: (Boolean) -> Unit,
    onSyncFilesChange: (Boolean) -> Unit,
    onFindPhoneChange: (Boolean) -> Unit,
    onShareCodesChange: (Boolean) -> Unit,
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
    setupItems: List<SetupItem> = emptyList(),
    onSetupFix: (String) -> Unit = {},
    onStorageSettingsClicked: () -> Unit = {},
    onNotificationSettingsClicked: () -> Unit = {}
) {
    val c = rememberDdColors(isDark)
    val context = LocalContext.current
    val ip = remember { getLocalIpAddress() }
    val version = remember {
        runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull() ?: ""
    }

    // One short list of categories; each opens its own page. Back returns to the list.
    var page by rememberSaveable { mutableStateOf<String?>(null) }
    androidx.activity.compose.BackHandler(enabled = page != null) { page = null }
    val prefs = remember { context.getSharedPreferences(LinkAllService.PREFS_NAME, android.content.Context.MODE_PRIVATE) }
    var checkForUpdates by remember { mutableStateOf(UpdateChecker.isEnabled(prefs)) }

    androidx.compose.animation.AnimatedContent(
        targetState = page,
        transitionSpec = {
            val forward = targetState != null
            (androidx.compose.animation.slideInHorizontally { if (forward) it / 4 else -it / 4 } + androidx.compose.animation.fadeIn())
                .togetherWith(androidx.compose.animation.slideOutHorizontally { if (forward) -it / 4 else it / 4 } + androidx.compose.animation.fadeOut())
        },
        label = "settingsPage",
    ) { current ->
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(start = PageGutter, end = PageGutter, bottom = 140.dp)
        ) {
            if (current == null) {
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

                item { SetupSection(c, setupItems, onSetupFix) }

                item {
                    Column {
                        GroupLabel(c, "Preferences")
                        Panel(c) {
                            CategoryRow(
                                c, Icons.Outlined.Sync, Tone.Blue, "Sync",
                                if (!isSyncEnabled) "Paused" else listOfNotNull(
                                    "Text".takeIf { syncText }, "images".takeIf { syncImages }, "files".takeIf { syncFiles }
                                ).joinToString(", ").replaceFirstChar { it.uppercase() }.ifBlank { "Nothing is synced" }
                            ) { page = "sync" }
                            Hairline(c)
                            CategoryRow(
                                c, Icons.Outlined.Devices, Tone.Violet, "Continuity",
                                listOfNotNull(
                                    "Calls".takeIf { callContinuityEnabled },
                                    "notifications".takeIf { notificationMirroringEnabled },
                                    "find my phone".takeIf { findPhoneEnabled },
                                    "codes".takeIf { shareCodesEnabled },
                                ).joinToString(", ").replaceFirstChar { it.uppercase() }.ifBlank { "Off" }
                            ) { page = "continuity" }
                            Hairline(c)
                            CategoryRow(c, Icons.Outlined.Folder, Tone.Green, "Folders", "Shared folders and folder sync") { page = "folders" }
                            Hairline(c)
                            CategoryRow(
                                c, Icons.Outlined.Palette, Tone.Teal, "Appearance",
                                when (themeMode) { "light" -> "Light"; "dark" -> "Dark"; else -> "Match system" }
                            ) { page = "appearance" }
                        }
                    }
                }

                item {
                    Column {
                        GroupLabel(c, "System")
                        Panel(c) {
                            CategoryRow(c, Icons.Outlined.BatteryChargingFull, Tone.Amber, "Battery & permissions", "Keep Link All running in the background") { page = "permissions" }
                            Hairline(c)
                            CategoryRow(c, Icons.Outlined.Tune, Tone.Neutral, "Advanced", "Scan, disconnect all, diagnostics, stop") { page = "advanced" }
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
            } else {
                item { SubpageTitle(c, settingsTitle(current)) { page = null } }
                when (current) {
                    "sync" -> item {
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
                    "continuity" -> item {
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
                            Hairline(c)
                            SwitchRow(c, Icons.Outlined.NotificationsActive, "Find my phone", "Let your computers make this phone ring, even on silent", findPhoneEnabled, onFindPhoneChange)
                            Hairline(c)
                            SwitchRow(c, Icons.Outlined.Pin, "Verification codes", "A code from a text pops up on your computers with Copy and Paste. Needs notification access", shareCodesEnabled, onShareCodesChange)
                        }
                    }
                    "folders" -> {
                        // The Play build can't read all files, so computers browse only the folders picked here.
                        if (!app.linkall.BuildConfig.FULL_PERMISSIONS) {
                            item { SharedFoldersSection(c) }
                        }
                        item { FolderSyncSection(c) }
                    }
                    "appearance" -> item {
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
                    "permissions" -> item {
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
                    "advanced" -> {
                        item {
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
                        if (UpdateChecker.supported) {
                            item {
                                Column(Modifier.padding(top = 20.dp)) {
                                    Panel(c) {
                                        SwitchRow(
                                            c, Icons.Outlined.Sync, "Check for updates",
                                            "Asks GitHub once a day if a newer version exists. Never installs.",
                                            checkForUpdates
                                        ) {
                                            checkForUpdates = it
                                            UpdateChecker.setEnabled(prefs, it)
                                            if (it) UpdateChecker.checkIfDue(context)
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

private fun settingsTitle(page: String) = when (page) {
    "sync" -> "Sync"
    "continuity" -> "Continuity"
    "folders" -> "Folders"
    "appearance" -> "Appearance"
    "permissions" -> "Battery & permissions"
    else -> "Advanced"
}

/** A settings category: tinted icon, name, what's on now, and its page on a tap. */
@Composable
private fun CategoryRow(c: DdColors, icon: androidx.compose.ui.graphics.vector.ImageVector, tone: Tone, title: String, summary: String, onClick: () -> Unit) {
    val (fg, bg) = c.tone(tone)
    ListRow(c, icon, title, detail = summary, iconTint = fg, iconBackground = bg, onClick = onClick)
}

/** A sub-page's title with the way back to the list. */
@Composable
private fun SubpageTitle(c: DdColors, title: String, onBack: () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(top = 16.dp, bottom = 20.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
        TopIconButton(c, Icons.AutoMirrored.Outlined.ArrowBack, "Back", onClick = onBack)
        Spacer(Modifier.width(14.dp))
        Text(title, style = DdType.pageTitle, color = c.text)
    }
}

/** The folders this phone's computers may browse, added with the system folder picker. */
@Composable
private fun SharedFoldersSection(c: DdColors) {
    val context = LocalContext.current
    var folders by remember { mutableStateOf(app.linkall.SharedFolders.list(context)) }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
        if (uri != null) {
            app.linkall.SharedFolders.add(context, uri)
            folders = app.linkall.SharedFolders.list(context)
        }
    }
    Column {
        SectionHeader(c, "Shared folders", null)
        Panel(c) {
            folders.forEach { f ->
                ListRow(
                    c, Icons.Outlined.Folder, f.label,
                    trailing = {
                        Icon(
                            Icons.Rounded.Close,
                            contentDescription = "Stop sharing ${f.label}",
                            tint = c.textMuted,
                            modifier = Modifier.size(20.dp).clickable {
                                app.linkall.SharedFolders.remove(context, f.uri)
                                folders = app.linkall.SharedFolders.list(context)
                            }
                        )
                    }
                )
                Hairline(c)
            }
            ListRow(
                c, Icons.Outlined.CreateNewFolder, "Add a folder",
                if (folders.isEmpty()) "Your computers can browse only the folders you add, like Camera or Documents"
                else "Your computers can browse these folders",
                onClick = { pick.launch(null) }
            )
        }
    }
}
