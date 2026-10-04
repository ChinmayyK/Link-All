@file:OptIn(ExperimentalFoundationApi::class)

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
import androidx.compose.material.icons.outlined.Devices
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

enum class AppTab { Home, Activity, Devices, Settings }

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
    onActionStreamCamera: () -> Unit,
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
    onCallContinuityChange: (Boolean) -> Unit,
    onNotificationMirroringChange: (Boolean) -> Unit,
    onAutoForwardSmsChange: (Boolean) -> Unit,
    onAutoForwardScreenshotsChange: (Boolean) -> Unit,
    onDarkModeChange: (Boolean) -> Unit,
    onForgetDevice: (String) -> Unit,
    onOpenDiagnostics: () -> Unit,
    onBatterySettingsClicked: () -> Unit = {},
    onStorageSettingsClicked: () -> Unit = {},
    onNotificationSettingsClicked: () -> Unit = {},
    onDeleteActivity: (ActivityEntry) -> Unit = {},
    onTogglePinActivity: (ActivityEntry) -> Unit = {},
    onClearActivity: () -> Unit = {},
    onResendActivity: (ActivityEntry) -> Unit = {},
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
                                deviceName = deviceName,
                                ambientStatus = ambientStatus,
                                healthIssue = healthIssue,
                                onHealthAction = onHealthAction,
                                peers = peers,
                                feed = feed,
                                activeTransfers = activeTransfers,
                                activeSpeedTests = activeSpeedTests,
                                onActionStartSpeedTest = onActionStartSpeedTest,
                                onActionPushClipboard = onActionPushClipboard,
                                onActionSendQuickContext = {
                                    onActionPushClipboard()
                                },
                                quickContextText = LinkAllService.quickSendContextFlow.collectAsState().value,
                                onActionPairMagicLink = onActionPairMagicLink,
                                onManualIp = onManualIp,
                                onActionSendFiles = onActionSendFiles,
                                onActionSendFolder = onActionSendFolder,
                                onActionStreamCamera = onActionStreamCamera,
                                onApplyClipboard = onApplyClipboard,
                                onActionPauseTransfer = onActionPauseTransfer,
                                onActionResumeTransfer = onActionResumeTransfer,
                                onActionCancelTransfer = onActionCancelTransfer,
                                onForgetPeer = onForgetPeer,
                                onDeleteActivity = onDeleteActivity,
                                onTogglePinActivity = onTogglePinActivity,
                                onResendActivity = onResendActivity,
                                onReplayOnboarding = onReplayOnboarding,
                                onTabSelected = { selectedTab ->
                                    val targetIndex = tabs.indexOf(selectedTab)
                                    if (targetIndex >= 0) {
                                        scope.launch {
                                            pagerState.animateScrollToPage(
                                                targetIndex,
                                                animationSpec = tween(300, easing = androidx.compose.animation.core.FastOutSlowInEasing)
                                            )
                                        }
                                    }
                                },
                                onRespondPairing = onRespondPairing,
                                onCancelPairing = onCancelPairing
                            )
                            AppTab.Activity -> ActivityTab(
                                isDark = isDark,
                                feed = feed,
                                onApply = onApplyClipboard,
                                onResend = onResendActivity,
                                onDelete = onDeleteActivity,
                                onTogglePin = onTogglePinActivity,
                                onClearAll = onClearActivity
                            )
                            AppTab.Devices -> DevicesTab(
                                isDark = isDark,
                                peers = peers,
                                activeSpeedTests = activeSpeedTests,
                                onConnectPeer = onConnectPeer,
                                onDisconnectPeer = onDisconnectPeer,
                                onSendPairingRequest = onSendPairingRequest,
                                onRespondPairing = onRespondPairing,
                                onCancelPairing = onCancelPairing,
                                onForgetPeer = onForgetPeer,
                                onSendFiles = onActionSendFiles,
                                onSendFolder = onActionSendFolder,
                                onSpeedTest = onActionStartSpeedTest,
                                onScanQr = onActionPairMagicLink,
                                onManualIp = onManualIp
                            )
                            AppTab.Settings -> SettingsTab(
                                isDark = isDark,
                                isServiceRunning = isServiceRunning,
                                isSyncEnabled = isSyncEnabled,
                                syncText = syncText,
                                syncImages = syncImages,
                                syncFiles = syncFiles,
                                callContinuityEnabled = callContinuityEnabled,
                                notificationMirroringEnabled = notificationMirroringEnabled,
                                autoForwardSms = autoForwardSms,
                                autoForwardScreenshots = autoForwardScreenshots,
                                deviceName = deviceName,
                                deviceId = deviceId,
                                peers = peers,
                                onSyncEnabledChange = onSyncEnabledChange,
                                onSyncTextChange = onSyncTextChange,
                                onSyncImagesChange = onSyncImagesChange,
                                onSyncFilesChange = onSyncFilesChange,
                                onCallContinuityChange = onCallContinuityChange,
                                onNotificationMirroringChange = onNotificationMirroringChange,
                                onAutoForwardSmsChange = onAutoForwardSmsChange,
                                onAutoForwardScreenshotsChange = onAutoForwardScreenshotsChange,
                                onDarkModeChange = onDarkModeChange,
                                onForgetDevice = onForgetDevice,
                                onStartSync = onStartSync,
                                onResumeSync = onResumeSync,
                                onScanNow = onScanNow,
                                onActionPauseSync = onActionPauseSync,
                                onActionDisconnectAll = onActionDisconnectAll,
                                onActionStopService = onActionStopService,
                                onOpenDiagnostics = onOpenDiagnostics,
                                onBatterySettingsClicked = onBatterySettingsClicked,
                                onStorageSettingsClicked = onStorageSettingsClicked,
                                onNotificationSettingsClicked = onNotificationSettingsClicked
                            )
                        }
                    }
                }
            }
            
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
                    onTabSelected = { selectedTab ->
                        val targetIndex = tabs.indexOf(selectedTab)
                        if (targetIndex >= 0) {
                            scope.launch {
                                pagerState.animateScrollToPage(
                                    targetIndex,
                                    animationSpec = tween(300, easing = androidx.compose.animation.core.FastOutSlowInEasing)
                                )
                            }
                        }
                    },
                    isDark = isDark
                )
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
                    .clickable(
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
    AppTab.Activity -> "Activity"
    AppTab.Devices -> "Devices"
    AppTab.Settings -> "Settings"
}

private val AppTab.icon get() = when (this) {
    AppTab.Home -> Icons.Outlined.Home
    AppTab.Activity -> Icons.Outlined.History
    AppTab.Devices -> Icons.Outlined.Devices
    AppTab.Settings -> Icons.Outlined.Settings
}

private val AppTab.selectedIcon get() = when (this) {
    AppTab.Home -> Icons.Filled.Home
    AppTab.Activity -> Icons.Filled.History
    AppTab.Devices -> Icons.Filled.Devices
    AppTab.Settings -> Icons.Filled.Settings
}
