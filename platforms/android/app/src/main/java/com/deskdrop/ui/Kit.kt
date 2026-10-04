@file:OptIn(ExperimentalFoundationApi::class)

package com.deskdrop.ui

import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.ArrowForward
import androidx.compose.material.icons.rounded.ChevronRight
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.deskdrop.ActivityEntry
import com.deskdrop.ActivityKind
import com.deskdrop.ui.theme.OutfitFontFamily
import com.deskdrop.ui.theme.crPressScale

// Deskdrop's Android UI kit, shared by every screen.
// Plain and typographic: neutral surfaces, one cobalt accent (the Mac app's
// brand blue) used only for actions and state, lists grouped by hairlines.
// No gradients or ambient effects. Shape rule: panels 24dp, icon wells and
// PIN tiles 12dp, buttons full pill.

internal class DdColors(isDark: Boolean) {
    val page = if (isDark) Color(0xFF000000) else Color(0xFFF5F6FA)
    val surface = if (isDark) Color(0xFF131316) else Color(0xFFFFFFFF)
    val surfaceSunk = if (isDark) Color(0xFF1C1C21) else Color(0xFFF1F2F5)
    val line = if (isDark) Color(0xFF26262C) else Color(0xFFE4E5EA)
    val text = if (isDark) Color(0xFFF4F4F6) else Color(0xFF111114)
    val textMuted = if (isDark) Color(0xFF9D9DA6) else Color(0xFF5F6068)
    val accent = if (isDark) Color(0xFF5C95FF) else Color(0xFF0055CC)
    val onAccent = if (isDark) Color(0xFF06122A) else Color.White
    val accentSoft = if (isDark) Color(0xFF5C95FF).copy(alpha = 0.14f) else Color(0xFF0055CC).copy(alpha = 0.08f)
    val live = if (isDark) Color(0xFF3DD68C) else Color(0xFF14935A)
    val warn = if (isDark) Color(0xFFF2B544) else Color(0xFFB7791F)
    val danger = if (isDark) Color(0xFFFF6B6B) else Color(0xFFC62828)
}

@Composable
internal fun rememberDdColors(isDark: Boolean) = remember(isDark) { DdColors(isDark) }

internal object DdType {
    val display = TextStyle(fontFamily = OutfitFontFamily, fontWeight = FontWeight.Bold, fontSize = 32.sp, lineHeight = 36.sp, letterSpacing = (-0.8).sp)
    val pageTitle = TextStyle(fontFamily = OutfitFontFamily, fontWeight = FontWeight.Bold, fontSize = 28.sp, lineHeight = 32.sp, letterSpacing = (-0.6).sp)
    val title = TextStyle(fontFamily = OutfitFontFamily, fontWeight = FontWeight.Bold, fontSize = 17.sp, lineHeight = 22.sp, letterSpacing = (-0.2).sp)
    val body = TextStyle(fontFamily = OutfitFontFamily, fontWeight = FontWeight.Normal, fontSize = 15.sp, lineHeight = 21.sp)
    val label = TextStyle(fontFamily = OutfitFontFamily, fontWeight = FontWeight.Medium, fontSize = 15.sp, lineHeight = 20.sp)
    val small = TextStyle(fontFamily = OutfitFontFamily, fontWeight = FontWeight.Normal, fontSize = 13.sp, lineHeight = 18.sp)
    val mono = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 12.sp, letterSpacing = 0.sp)
}

internal val PanelShape = RoundedCornerShape(24.dp)
internal val WellShape = RoundedCornerShape(12.dp)
internal val PageGutter = 20.dp

/** Large left-aligned page title with an optional trailing slot. */
@Composable
internal fun PageTitle(c: DdColors, title: String, subtitle: String? = null, trailing: @Composable () -> Unit = {}) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(top = 20.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, style = DdType.pageTitle, color = c.text)
            if (subtitle != null) {
                Spacer(Modifier.height(2.dp))
                Text(subtitle, style = DdType.small, color = c.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        trailing()
    }
}

@Composable
internal fun SectionHeader(c: DdColors, title: String, meta: String?, onMore: (() -> Unit)? = null) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(top = 30.dp, bottom = 10.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(title, style = DdType.title, color = c.text)
        if (meta != null) {
            Spacer(Modifier.width(8.dp))
            Text(meta, style = DdType.small, color = c.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Spacer(Modifier.weight(1f))
        if (onMore != null) {
            Row(
                modifier = Modifier
                    .clip(CircleShape)
                    .combinedClickable(onClick = onMore)
                    .padding(horizontal = 10.dp, vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text("See all", style = DdType.small.copy(fontWeight = FontWeight.Medium), color = c.accent)
                Spacer(Modifier.width(4.dp))
                Icon(Icons.Rounded.ArrowForward, contentDescription = null, tint = c.accent, modifier = Modifier.size(14.dp))
            }
        }
    }
}

@Composable
internal fun Panel(c: DdColors, modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier = modifier
            .fillMaxWidth()
            .clip(PanelShape)
            .background(c.surface)
            .border(1.dp, c.line, PanelShape)
            .animateContentSize(),
        content = content
    )
}

@Composable
internal fun Hairline(c: DdColors) {
    Box(Modifier.padding(start = 70.dp).fillMaxWidth().height(1.dp).background(c.line))
}

@Composable
internal fun IconWell(c: DdColors, icon: ImageVector, tint: Color = c.textMuted, background: Color = c.surfaceSunk, size: Int = 40) {
    Box(
        Modifier.size(size.dp).clip(WellShape).background(background),
        contentAlignment = Alignment.Center
    ) {
        Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size((size / 2).dp))
    }
}

/**
 * The standard list row: icon well, title, one line of detail, and a
 * trailing slot (chevron by default). Tapping anywhere runs [onClick].
 */
@Composable
internal fun ListRow(
    c: DdColors,
    icon: ImageVector,
    title: String,
    detail: String? = null,
    detailColor: Color = c.textMuted,
    iconTint: Color = c.textMuted,
    iconBackground: Color = c.surfaceSunk,
    titleColor: Color = c.text,
    enabled: Boolean = true,
    onClick: (() -> Unit)? = null,
    trailing: @Composable () -> Unit = {
        if (onClick != null) Icon(Icons.Rounded.ChevronRight, contentDescription = null, tint = c.textMuted, modifier = Modifier.size(20.dp))
    }
) {
    val haptic = LocalHapticFeedback.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .graphicsLayer { alpha = if (enabled) 1f else 0.5f }
            .then(
                if (onClick != null) Modifier.combinedClickable(enabled = enabled, onClick = {
                    haptic.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                    onClick()
                }) else Modifier
            )
            .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        IconWell(c, icon, tint = iconTint, background = iconBackground)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(title, style = DdType.label, color = titleColor, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (detail != null) {
                Text(detail, style = DdType.small, color = detailColor, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
        }
        Spacer(Modifier.width(8.dp))
        trailing()
    }
}

@Composable
internal fun SwitchRow(
    c: DdColors,
    icon: ImageVector,
    title: String,
    detail: String?,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit
) {
    val haptic = LocalHapticFeedback.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .combinedClickable(onClick = {
                haptic.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                onCheckedChange(!checked)
            })
            .semantics(mergeDescendants = true) {}
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        IconWell(c, icon, tint = if (checked) c.accent else c.textMuted, background = if (checked) c.accentSoft else c.surfaceSunk)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(title, style = DdType.label, color = c.text)
            if (detail != null) Text(detail, style = DdType.small, color = c.textMuted)
        }
        Spacer(Modifier.width(12.dp))
        Switch(
            checked = checked,
            onCheckedChange = null,
            colors = SwitchDefaults.colors(
                checkedThumbColor = c.onAccent,
                checkedTrackColor = c.accent,
                checkedBorderColor = c.accent,
                uncheckedThumbColor = c.textMuted,
                uncheckedTrackColor = c.surfaceSunk,
                uncheckedBorderColor = c.line
            )
        )
    }
}

@Composable
internal fun PillButton(
    c: DdColors,
    text: String,
    filled: Boolean,
    modifier: Modifier = Modifier,
    compact: Boolean = false,
    icon: ImageVector? = null,
    textColor: Color? = null,
    onClick: () -> Unit
) {
    val haptic = LocalHapticFeedback.current
    Row(
        modifier = modifier
            .clip(CircleShape)
            .background(if (filled) c.accent else Color.Transparent)
            .then(if (filled) Modifier else Modifier.border(1.dp, c.line, CircleShape))
            .crPressScale(0.97f) {
                haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                onClick()
            }
            .semantics { role = Role.Button }
            .padding(horizontal = if (compact) 14.dp else 22.dp, vertical = if (compact) 8.dp else 14.dp),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically
    ) {
        val fg = textColor ?: if (filled) c.onAccent else c.text
        if (icon != null) {
            Icon(icon, contentDescription = null, tint = fg, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(8.dp))
        }
        Text(text, style = if (compact) DdType.small.copy(fontWeight = FontWeight.Medium) else DdType.label, color = fg, maxLines = 1)
    }
}

@Composable
internal fun MenuItem(c: DdColors, text: String, icon: ImageVector, tint: Color = c.text, onClick: () -> Unit) {
    DropdownMenuItem(
        text = { Text(text, style = DdType.body, color = tint) },
        leadingIcon = { Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size(20.dp)) },
        onClick = onClick
    )
}

/** Dashed box for empty lists. */
@Composable
internal fun EmptyBox(c: DdColors, icon: ImageVector, text: String, content: @Composable ColumnScope.() -> Unit = {}) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(PanelShape)
            .border(1.dp, c.line, PanelShape)
            .padding(20.dp)
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, contentDescription = null, tint = c.textMuted, modifier = Modifier.size(22.dp))
            Spacer(Modifier.width(14.dp))
            Text(text, style = DdType.small, color = c.textMuted)
        }
        content()
    }
}

/** Six digit tiles, split 3 + 3 like the code on the other device. */
@Composable
internal fun PinTiles(c: DdColors, pin: String?, modifier: Modifier = Modifier) {
    val digits = pin?.filter { it.isDigit() }?.takeIf { it.length == 6 }
    Row(
        modifier = modifier
            .fillMaxWidth()
            .semantics(mergeDescendants = true) {
                contentDescription = if (digits != null) "Security code ${digits.toList().joinToString(" ")}" else "Security code loading"
            },
        verticalAlignment = Alignment.CenterVertically
    ) {
        for (i in 0 until 6) {
            if (i == 3) Spacer(Modifier.width(14.dp)) else if (i > 0) Spacer(Modifier.width(6.dp))
            Box(
                modifier = Modifier
                    .weight(1f)
                    .aspectRatio(0.82f)
                    .clip(WellShape)
                    .background(c.surfaceSunk),
                contentAlignment = Alignment.Center
            ) {
                Text(
                    text = digits?.get(i)?.toString() ?: "·",
                    style = TextStyle(fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold, fontSize = 26.sp),
                    color = c.text
                )
            }
        }
    }
}

/**
 * One activity entry. Tap applies it (copy again / open); long-press offers
 * resend and remove when the caller supports them.
 */
@Composable
internal fun ActivityRow(
    c: DdColors,
    entry: ActivityEntry,
    onApply: () -> Unit,
    onResend: (() -> Unit)? = null,
    onDelete: (() -> Unit)? = null,
    onTogglePin: (() -> Unit)? = null,
    showClockTime: Boolean = false
) {
    val haptic = LocalHapticFeedback.current
    var menuOpen by remember { mutableStateOf(false) }
    val isLink = entry.preview.startsWith("http")
    val (icon, title) = when (entry.kind) {
        ActivityKind.FILE_SENT -> Icons.Outlined.NorthEast to "Sent to ${entry.deviceName}"
        ActivityKind.FILE_RECEIVED, ActivityKind.FILE_TRANSFER_COMPLETE -> Icons.Outlined.SouthWest to "From ${entry.deviceName}"
        ActivityKind.FILE_TRANSFER_FAILED -> Icons.Outlined.ErrorOutline to "Transfer failed"
        ActivityKind.CLIPBOARD_TEXT -> (if (isLink) Icons.Outlined.Link else Icons.Outlined.ContentCopy) to (if (isLink) "Link" else "Clipboard")
        ActivityKind.CLIPBOARD_IMAGE -> Icons.Outlined.Image to "Image"
        ActivityKind.PEER_CONNECTED -> Icons.Outlined.Wifi to "${entry.deviceName} connected"
        ActivityKind.PEER_DISCONNECTED -> Icons.Outlined.WifiOff to "${entry.deviceName} went offline"
        ActivityKind.WARNING -> Icons.Outlined.ErrorOutline to "Needs attention"
        else -> Icons.Outlined.Sync to entry.deviceName
    }
    val showPreview = entry.preview.isNotBlank() &&
        entry.kind != ActivityKind.PEER_CONNECTED && entry.kind != ActivityKind.PEER_DISCONNECTED
    val hasMenu = onResend != null || onDelete != null || onTogglePin != null

    Box {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .combinedClickable(
                    onClick = {
                        haptic.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onApply()
                    },
                    onLongClick = if (hasMenu) ({
                        haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                        menuOpen = true
                    }) else null
                )
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconWell(c, icon, size = 36)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (entry.isPinned) {
                        Icon(Icons.Outlined.PushPin, contentDescription = "Pinned", tint = c.accent, modifier = Modifier.size(14.dp))
                        Spacer(Modifier.width(5.dp))
                    }
                    Text(title, style = DdType.label, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                if (showPreview) {
                    Text(
                        entry.preview.trim().replace('\n', ' '),
                        style = DdType.small,
                        color = c.textMuted,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis
                    )
                }
            }
            Spacer(Modifier.width(8.dp))
            Text(
                if (showClockTime) clockTime(entry.timestamp) else relativeTime(entry.timestamp),
                style = DdType.mono,
                color = c.textMuted
            )
        }

        if (hasMenu && menuOpen) {
            ActionSheet(
                c,
                icon = Icons.Outlined.History,
                title = entry.deviceName,
                subtitle = entry.preview.trim().replace('\n', ' ').take(80),
                actions = buildList {
                    add(SheetAction(Icons.Outlined.ContentCopy, if (isLink) "Open link" else "Copy again", onClick = onApply))
                    if (onTogglePin != null) add(
                        if (entry.isPinned) SheetAction(Icons.Outlined.PushPin, "Unpin", "Let it age out with the rest", onClick = onTogglePin)
                        else SheetAction(Icons.Outlined.PushPin, "Pin to the top", "Kept above everything else, never cleared", onClick = onTogglePin)
                    )
                    if (onResend != null) add(SheetAction(Icons.Outlined.Replay, "Resend", onClick = onResend))
                    if (onDelete != null) add(SheetAction(Icons.Outlined.DeleteOutline, "Remove", danger = true, onClick = onDelete))
                },
                onDismiss = { menuOpen = false }
            )
        }
    }
}

/**
 * The "+" button: show this phone's pairing QR, scan a computer's, or type
 * an IP. Owns the QR dialog so every screen gets the same flow.
 */
@Composable
internal fun AddDeviceButton(c: DdColors, onScanQr: () -> Unit, onManualIp: () -> Unit) {
    var sheetOpen by remember { mutableStateOf(false) }
    var showQr by remember { mutableStateOf(false) }
    Box(
        modifier = Modifier
            .size(44.dp)
            .clip(CircleShape)
            .background(c.surface)
            .border(1.dp, c.line, CircleShape)
            .crPressScale(0.92f) { sheetOpen = true }
            .semantics { contentDescription = "Add device"; role = Role.Button },
        contentAlignment = Alignment.Center
    ) {
        Icon(Icons.Rounded.Add, contentDescription = null, tint = c.text, modifier = Modifier.size(22.dp))
    }
    if (sheetOpen) {
        DdSheet(
            c, icon = Icons.Rounded.Add, title = "Add a device",
            subtitle = "Pair a computer or another phone",
            onDismiss = { sheetOpen = false }
        ) { close ->
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                SheetOption(c, Icons.Outlined.QrCode2, "Show my pairing code", "Scan it from Link All on your computer") {
                    close { showQr = true }
                }
                SheetOption(c, Icons.Outlined.QrCodeScanner, "Scan a pairing code", "Point the camera at the code on your computer") {
                    close(onScanQr)
                }
                SheetOption(c, Icons.Outlined.Lan, "Connect by IP address", "For networks where devices can't find each other") {
                    close(onManualIp)
                }
            }
        }
    }
    if (showQr) PairQrSheet(c, onDismiss = { showQr = false })
}

@Composable
private fun PairQrSheet(c: DdColors, onDismiss: () -> Unit) {
    val uri = remember { "deskdrop://${getLocalIpAddress()}:47823" }
    val bitmap by produceState<android.graphics.Bitmap?>(null, uri) {
        value = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Default) {
            runCatching {
                val matrix = com.google.zxing.qrcode.QRCodeWriter()
                    .encode(uri, com.google.zxing.BarcodeFormat.QR_CODE, 512, 512)
                val pixels = IntArray(matrix.width * matrix.height) { i ->
                    if (matrix.get(i % matrix.width, i / matrix.width)) android.graphics.Color.BLACK
                    else android.graphics.Color.WHITE
                }
                android.graphics.Bitmap.createBitmap(pixels, matrix.width, matrix.height, android.graphics.Bitmap.Config.RGB_565)
            }.getOrNull()
        }
    }
    DdSheet(
        c, icon = Icons.Outlined.QrCode2, title = "Scan from your computer",
        subtitle = "Link All on your computer → Add device → Scan",
        onDismiss = onDismiss
    ) { close ->
        Box(
            Modifier
                .fillMaxWidth()
                .clip(PanelShape)
                .background(Color.White)
                .padding(20.dp),
            contentAlignment = Alignment.Center
        ) {
            Box(Modifier.fillMaxWidth(0.8f).aspectRatio(1f), contentAlignment = Alignment.Center) {
                bitmap?.let {
                    androidx.compose.foundation.Image(
                        it.asImageBitmap(),
                        contentDescription = "Pairing QR code",
                        modifier = Modifier.fillMaxSize()
                    )
                } ?: Text("Generating…", style = DdType.small, color = Color.Black)
            }
        }
        Spacer(Modifier.height(10.dp))
        Text(uri, style = DdType.mono, color = c.textMuted, modifier = Modifier.align(Alignment.CenterHorizontally))
        Spacer(Modifier.height(18.dp))
        PillButton(c, "Done", filled = true, modifier = Modifier.fillMaxWidth()) { close {} }
    }
}

// ---------------------------------------------------------------- helpers

internal fun isPhoneName(name: String) =
    listOf(
        "phone", "pixel", "galaxy", "oneplus", "nord", "android", "realme", "redmi", "xiaomi", "poco",
        "samsung", "motorola", "moto ", "vivo", "oppo", "iqoo", "infinix", "tecno", "huawei", "honor"
    ).any { name.contains(it, ignoreCase = true) }

internal fun relativeTime(timestampMs: Long): String {
    val secs = (System.currentTimeMillis() - timestampMs) / 1000
    return when {
        secs < 60 -> "now"
        secs < 3600 -> "${secs / 60}m"
        secs < 86_400 -> "${secs / 3600}h"
        else -> "${secs / 86_400}d"
    }
}

internal fun clockTime(timestampMs: Long): String =
    java.text.SimpleDateFormat("HH:mm", java.util.Locale.getDefault()).format(java.util.Date(timestampMs))

/** "2m ago" style label for a Unix-seconds timestamp; null when unknown. */
internal fun agoLabel(unixSecs: Long?): String? {
    if (unixSecs == null || unixSecs < 1_000_000_000L) return null
    val ago = System.currentTimeMillis() / 1000 - unixSecs
    return when {
        ago < 0 -> null
        ago < 60 -> "just now"
        ago < 3600 -> "${ago / 60}m ago"
        ago < 86_400 -> "${ago / 3600}h ago"
        else -> "${ago / 86_400}d ago"
    }
}
