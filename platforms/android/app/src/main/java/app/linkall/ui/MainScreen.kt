@file:OptIn(ExperimentalFoundationApi::class, androidx.compose.foundation.layout.ExperimentalLayoutApi::class)

package app.linkall.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.core.updateTransition
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.animateDp
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.Spring
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.ui.input.pointer.pointerInput
import kotlinx.coroutines.launch
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.ui.layout.LocalPinnableContainer
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.animation.animateContentSize
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Wifi
import androidx.compose.material.icons.filled.SettingsInputAntenna
import androidx.compose.material.icons.filled.LinkOff
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.zIndex
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.ui.graphics.asImageBitmap
import app.linkall.LinkAllService
import app.linkall.ui.getLocalIpAddress
import androidx.compose.material3.TextButton
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.foundation.clickable
import androidx.compose.foundation.selection.selectable
import androidx.compose.ui.semantics.Role
import androidx.compose.foundation.interaction.MutableInteractionSource
import app.linkall.ActivityEntry
import app.linkall.ActivityKind
import app.linkall.PeerSnapshot
import app.linkall.TransferProgress
import app.linkall.SpeedTestProgress
import app.linkall.ui.theme.CRBackground
import app.linkall.ui.theme.CRTheme
import app.linkall.ui.theme.CRTypography
import app.linkall.ui.theme.crGlassCard
import app.linkall.ui.theme.crPressScale

val CRTheme.brandElectric get() = Color(0xFF0066FF)
val CRTheme.brandViolet get() = Color(0xFF8B5CF6)
val CRTheme.brandCyan get() = Color(0xFF06B6D4)
val CRTheme.brandPink get() = Color(0xFFEC4899)
val CRTheme.accentGreen get() = Color(0xFF10B981)
val CRTheme.accentRed get() = Color(0xFFEF4444)
val CRTheme.accentAmber get() = Color(0xFFF59E0B)

enum class AppTab { Home, Files, Activity, Settings }

/** Opens a device's page, from wherever a device is shown. */
internal val LocalOpenDevice = staticCompositionLocalOf<(PeerSnapshot) -> Unit> { {} }

@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
fun MainScreen(
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
    peers: kotlinx.collections.immutable.ImmutableList<PeerSnapshot>,
    feed: kotlinx.collections.immutable.ImmutableList<ActivityEntry>,
    ambientStatus: String,
    healthIssue: app.linkall.HealthIssue? = null,
    onHealthAction: (app.linkall.HealthIssue) -> Unit = {},
    setupItems: List<SetupItem> = emptyList(),
    onSetupFix: (String) -> Unit = {},
    activeTransfers: kotlinx.collections.immutable.ImmutableList<TransferProgress>,
    activeSpeedTests: kotlinx.collections.immutable.ImmutableList<SpeedTestProgress> = kotlinx.collections.immutable.persistentListOf(),
    onActionStartSpeedTest: (String) -> Unit = {},
    onStartSync: () -> Unit,
    onResumeSync: () -> Unit,
    onScanNow: () -> Unit,
    onActionPushClipboard: () -> Unit,
    onActionPairMagicLink: () -> Unit,
    onManualIp: () -> Unit,
    onActionPauseSync: () -> Unit,
    onActionDisconnectAll: () -> Unit,
    onActionStopService: () -> Unit,
    onActionPauseTransfer: (String) -> Unit,
    onActionResumeTransfer: (String) -> Unit,
    onActionCancelTransfer: (String) -> Unit,
    onActionAcceptTransfer: (String) -> Unit = {},
    onActionRejectTransfer: (String) -> Unit = {},
    onActionSendFiles: (String?) -> Unit,
    onActionSendFolder: (String?) -> Unit,
    onDropFiles: (String, List<android.net.Uri>) -> Unit = { _, _ -> },
    onApplyClipboard: (ActivityEntry) -> Unit,
    onTrustPeer: (PeerSnapshot) -> Unit,
    onRejectPeer: (PeerSnapshot) -> Unit,
    onConnectPeer: (PeerSnapshot) -> Unit,
    onDisconnectPeer: (PeerSnapshot) -> Unit,
    onForgetPeer: (PeerSnapshot) -> Unit,
    onSendPairingRequest: (PeerSnapshot) -> Unit,
    onRespondPairing: (PeerSnapshot, Boolean) -> Unit,
    onCancelPairing: (PeerSnapshot) -> Unit,
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
    onForgetDevice: (String) -> Unit,
    onOpenDiagnostics: () -> Unit,
    onBatterySettingsClicked: () -> Unit = {},
    onStorageSettingsClicked: () -> Unit = {},
    onNotificationSettingsClicked: () -> Unit = {},
    onDeleteActivity: (ActivityEntry) -> Unit = {},
    onTogglePinActivity: (ActivityEntry) -> Unit = {},
    onClearActivity: () -> Unit = {},
    onReplayOnboarding: () -> Unit = {}
) {
    val scope = rememberCoroutineScope()
    val tabs = AppTab.values()
    val pagerState = androidx.compose.foundation.pager.rememberPagerState(
        initialPage = 0,
        pageCount = { tabs.size }
    )

    val currentTab = tabs[pagerState.targetPage.coerceIn(0, tabs.size - 1)]

    val hasConnectedDevices = remember(peers) { peers.any { it.isConnected } }

    var browsingPeerId by rememberSaveable { mutableStateOf<String?>(null) }
    var devicePageId by rememberSaveable { mutableStateOf<String?>(null) }
    var notesOpen by rememberSaveable { mutableStateOf(false) }
    var activityFilter by rememberSaveable { mutableStateOf(ActivityFilter.All) }
    // The keyboard covers the dock anyway; hiding it while typing keeps it off the search boxes.
    val keyboardOpen = androidx.compose.foundation.layout.WindowInsets.isImeVisible
    fun goTo(tab: AppTab) {
        val target = tabs.indexOf(tab)
        scope.launch {
            // Home to Settings used to slide through Files and Activity,
            // building both on the way. Jump next to the tab, then slide one page.
            val from = pagerState.currentPage
            if (kotlin.math.abs(target - from) > 1) {
                pagerState.scrollToPage(if (target > from) target - 1 else target + 1)
            }
            pagerState.animateScrollToPage(
                target,
                animationSpec = tween(220, easing = androidx.compose.animation.core.FastOutSlowInEasing)
            )
        }
    }
    CompositionLocalProvider(
        LocalBrowseFiles provides { peer: PeerSnapshot -> browsingPeerId = peer.id },
        LocalOpenDevice provides { peer: PeerSnapshot -> devicePageId = peer.id },
    ) {
        CRBackground(isDark = isDark, hasConnectedDevices = hasConnectedDevices, flat = true) {
        Box(modifier = Modifier.fillMaxSize().systemBarsPadding()) {
            Column(modifier = Modifier.fillMaxSize()) {
                Box(modifier = Modifier.weight(1f)) {
                    androidx.compose.foundation.pager.HorizontalPager(
                        state = pagerState,
                        modifier = Modifier.fillMaxSize()
                    ) { page ->
                        when (tabs[page]) {
                            AppTab.Home -> HomeTab(
                                isDark = isDark,
                                healthIssue = healthIssue,
                                moreIssues = if (healthIssue != null) (setupItems.problemCount() - 1).coerceAtLeast(0) else 0,
                                onHealthAction = onHealthAction,
                                peers = peers,
                                feed = feed,
                                activeTransfers = activeTransfers,
                                activeSpeedTests = activeSpeedTests,
                                onActionPushClipboard = onActionPushClipboard,
                                onActionSendQuickContext = { onActionPushClipboard() },
                                quickContextText = LinkAllService.quickSendContextFlow.collectAsState().value,
                                onActionPairMagicLink = onActionPairMagicLink,
                                onManualIp = onManualIp,
                                onActionSendFiles = onActionSendFiles,
                                onActionSendFolder = onActionSendFolder,
                                onApplyClipboard = onApplyClipboard,
                                onActionPauseTransfer = onActionPauseTransfer,
                                onActionResumeTransfer = onActionResumeTransfer,
                                onActionCancelTransfer = onActionCancelTransfer,
                                onDeleteActivity = onDeleteActivity,
                                onTogglePinActivity = onTogglePinActivity,
                                onReplayOnboarding = onReplayOnboarding,
                                onOpenNotes = { notesOpen = true },
                                onTabSelected = { selectedTab ->
                                    // Home's "See all" is everything, not the last filter picked.
                                    if (selectedTab == AppTab.Activity) activityFilter = ActivityFilter.All
                                    goTo(selectedTab)
                                },
                                onRespondPairing = onRespondPairing,
                                onSendPairingRequest = onSendPairingRequest,
                                onCancelPairing = onCancelPairing
                            )
                            AppTab.Files -> FilesTab(
                                isDark = isDark,
                                peers = peers,
                                feed = feed,
                                activeTransfers = activeTransfers,
                                onActionSendFiles = onActionSendFiles,
                                onActionSendFolder = onActionSendFolder,
                                onActionPauseTransfer = onActionPauseTransfer,
                                onActionResumeTransfer = onActionResumeTransfer,
                                onActionCancelTransfer = onActionCancelTransfer,
                                onSeeAll = {
                                    activityFilter = ActivityFilter.Files
                                    goTo(AppTab.Activity)
                                },
                            )
                            AppTab.Activity -> ActivityTab(
                                isDark = isDark,
                                feed = feed,
                                filter = activityFilter,
                                onFilterChange = { activityFilter = it },
                                onApply = onApplyClipboard,
                                onDelete = onDeleteActivity,
                                onTogglePin = onTogglePinActivity,
                                onClearAll = onClearActivity
                            )
                            AppTab.Settings -> SettingsTab(
                                isDark = isDark,
                                isServiceRunning = isServiceRunning,
                                isSyncEnabled = isSyncEnabled,
                                syncText = syncText,
                                syncImages = syncImages,
                                syncFiles = syncFiles,
                                callContinuityEnabled = callContinuityEnabled,
                                findPhoneEnabled = findPhoneEnabled,
                                shareCodesEnabled = shareCodesEnabled,
                                notificationMirroringEnabled = notificationMirroringEnabled,
                                autoForwardSms = autoForwardSms,
                                autoForwardScreenshots = autoForwardScreenshots,
                                deviceName = deviceName,
                                deviceId = deviceId,
                                onSyncEnabledChange = onSyncEnabledChange,
                                onSyncTextChange = onSyncTextChange,
                                onSyncImagesChange = onSyncImagesChange,
                                onSyncFilesChange = onSyncFilesChange,
                                onFindPhoneChange = onFindPhoneChange,
                                onShareCodesChange = onShareCodesChange,
                                onCallContinuityChange = onCallContinuityChange,
                                onNotificationMirroringChange = onNotificationMirroringChange,
                                onAutoForwardSmsChange = onAutoForwardSmsChange,
                                onAutoForwardScreenshotsChange = onAutoForwardScreenshotsChange,
                                themeMode = themeMode,
                                onThemeModeChange = onThemeModeChange,
                                onStartSync = onStartSync,
                                onScanNow = onScanNow,
                                onActionDisconnectAll = onActionDisconnectAll,
                                onActionStopService = onActionStopService,
                                onOpenDiagnostics = onOpenDiagnostics,
                                onBatterySettingsClicked = onBatterySettingsClicked,
                                onStorageSettingsClicked = onStorageSettingsClicked,
                                onNotificationSettingsClicked = onNotificationSettingsClicked,
                                setupItems = setupItems,
                                onSetupFix = onSetupFix
                            )
                        }
                    }
                }
            }

            // The floating dock, over a fade so content scrolls under it softly.
            // Hidden while typing: the keyboard covers it anyway.
            if (!keyboardOpen) {
                Box(
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .fillMaxWidth()
                        .height(120.dp)
                        .background(
                            androidx.compose.ui.graphics.Brush.verticalGradient(
                                colors = listOf(
                                    Color.Transparent,
                                    CRTheme.bg(isDark).copy(alpha = 0.8f),
                                    CRTheme.bg(isDark)
                                )
                            )
                        )
                )
                Box(
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .padding(bottom = 24.dp)
                ) {
                    BottomDock(
                        currentTab = currentTab,
                        onTabSelected = { goTo(it) },
                        isDark = isDark
                    )
                }
            }

            // A device's page slides in over the tabs; Back closes it.
            val devicePeer = devicePageId?.let { id -> peers.firstOrNull { it.id == id } }
            if (devicePageId != null && devicePeer == null) {
                LaunchedEffect(devicePageId) { devicePageId = null } // forgotten meanwhile
            }
            androidx.compose.animation.AnimatedVisibility(
                visible = devicePeer != null,
                enter = androidx.compose.animation.slideInHorizontally { it / 3 } + fadeIn(tween(220)),
                exit = androidx.compose.animation.slideOutHorizontally { it / 3 } + fadeOut(tween(180)),
            ) {
                // Keep the last device while the page animates away.
                var shown by remember { mutableStateOf<PeerSnapshot?>(null) }
                if (devicePeer != null) shown = devicePeer
                shown?.let { peer ->
                    DeviceScreen(
                        isDark = isDark,
                        peer = peer,
                        speedTest = activeSpeedTests.find { it.peerId == peer.id },
                        onBack = { devicePageId = null },
                        onSendFiles = { onActionSendFiles(peer.id) },
                        onSendFolder = { onActionSendFolder(peer.id) },
                        onSendClipboard = onActionPushClipboard,
                        onConnect = { onConnectPeer(peer) },
                        onDisconnect = { onDisconnectPeer(peer) },
                        onForget = { onForgetPeer(peer) },
                        onPair = { onSendPairingRequest(peer) },
                        onRespondPairing = { accept -> onRespondPairing(peer, accept) },
                        onCancelPairing = { onCancelPairing(peer) },
                        onSpeedTest = { onActionStartSpeedTest(peer.id) },
                    )
                }
            }

            // Notes, from the button on Home: over the tabs and the dock.
            androidx.compose.animation.AnimatedVisibility(
                visible = notesOpen,
                enter = androidx.compose.animation.slideInHorizontally { it / 3 } + fadeIn(tween(220)),
                exit = androidx.compose.animation.slideOutHorizontally { it / 3 } + fadeOut(tween(180)),
            ) {
                NotesScreen(isDark = isDark, onBack = { notesOpen = false })
            }

            // Over everything, dock included; Back closes it.
            browsingPeerId?.let { id ->
                peers.firstOrNull { it.id == id }?.let { peer ->
                    RemoteBrowserScreen(isDark, peer, onClose = { browsingPeerId = null })
                } ?: LaunchedEffect(id) { browsingPeerId = null } // forgotten meanwhile
            }
        }
    }
    }
}

@Composable
fun BottomDock(
    currentTab: AppTab,
    onTabSelected: (AppTab) -> Unit,
    isDark: Boolean
) {
    val haptic = LocalHapticFeedback.current
    val dock = remember(isDark) { DdColors(isDark) }

    // The selected tab grows into a labelled pill; the rest stay icon-only.
    Row(
        modifier = Modifier
            .shadow(18.dp, CircleShape, ambientColor = Color(0xFF0B1B3F), spotColor = Color(0xFF0B1B3F))
            .clip(CircleShape)
            .background(dock.surface)
            .border(1.dp, dock.line, CircleShape)
            .padding(6.dp),
        horizontalArrangement = Arrangement.spacedBy(2.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        AppTab.values().forEach { tab ->
            val selected = tab == currentTab
            val bg by androidx.compose.animation.animateColorAsState(
                if (selected) dock.accentSoft else Color.Transparent, label = "dockBg"
            )
            Row(
                modifier = Modifier
                    .clip(CircleShape)
                    .background(bg)
                    .selectable(
                        selected = selected,
                        role = Role.Tab,
                        interactionSource = remember { MutableInteractionSource() },
                        indication = null
                    ) {
                        haptic.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onTabSelected(tab)
                    }
                    .animateContentSize(spring(dampingRatio = 0.8f, stiffness = 500f))
                    .padding(horizontal = 16.dp, vertical = 12.dp)
                    .semantics { contentDescription = tab.label },
                verticalAlignment = Alignment.CenterVertically
            ) {
                Icon(
                    imageVector = if (selected) tab.selectedIcon else tab.icon,
                    contentDescription = null,
                    tint = if (selected) dock.accent else dock.textMuted,
                    modifier = Modifier.size(22.dp)
                )
                if (selected) {
                    Spacer(modifier = Modifier.width(8.dp))
                    Text(
                        tab.label,
                        style = androidx.compose.ui.text.TextStyle(
                            fontFamily = app.linkall.ui.theme.OutfitFontFamily,
                            fontWeight = FontWeight.Medium,
                            fontSize = 14.sp
                        ),
                        color = dock.accent,
                        maxLines = 1
                    )
                }
            }
        }
    }
}

private val AppTab.label get() = when (this) {
    AppTab.Home -> "Home"
    AppTab.Files -> "Files"
    AppTab.Activity -> "Activity"
    AppTab.Settings -> "Settings"
}

private val AppTab.icon get() = when (this) {
    AppTab.Home -> Icons.Outlined.Home
    AppTab.Files -> Icons.Outlined.Folder
    AppTab.Activity -> Icons.Outlined.History
    AppTab.Settings -> Icons.Outlined.Settings
}

private val AppTab.selectedIcon get() = when (this) {
    AppTab.Home -> Icons.Filled.Home
    AppTab.Files -> Icons.Filled.Folder
    AppTab.Activity -> Icons.Filled.History
    AppTab.Settings -> Icons.Filled.Settings
}
