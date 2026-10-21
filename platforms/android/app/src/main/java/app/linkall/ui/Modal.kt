package app.linkall.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.DevicesOther
import androidx.compose.material.icons.outlined.DriveFolderUpload
import androidx.compose.material.icons.outlined.UploadFile
import androidx.compose.material.icons.rounded.ChevronRight
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.linkall.PeerSnapshot
import kotlinx.coroutines.launch

// One modal for the whole app: a sheet that rises from the bottom, in the
// app's own surface, type and icon wells, instead of the platform dialog.

private val SheetShape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp)

/**
 * Bottom sheet with an icon, title and optional subtitle, then [content].
 * Content gets `close` to animate the sheet away before acting.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun DdSheet(
    c: DdColors,
    icon: ImageVector,
    title: String,
    subtitle: String? = null,
    iconTint: Color = c.accent,
    iconBackground: Color = c.accentSoft,
    onDismiss: () -> Unit,
    content: @Composable ColumnScope.(close: (after: () -> Unit) -> Unit) -> Unit,
) {
    val state = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    val close: (() -> Unit) -> Unit = { after ->
        scope.launch { state.hide() }.invokeOnCompletion {
            onDismiss()
            after()
        }
    }
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = state,
        shape = SheetShape,
        containerColor = c.surface,
        contentColor = c.text,
        tonalElevation = 0.dp,
        scrimColor = Color.Black.copy(alpha = 0.5f),
        dragHandle = {
            Box(
                Modifier
                    .padding(top = 10.dp, bottom = 6.dp)
                    .size(width = 36.dp, height = 4.dp)
                    .clip(CircleShape)
                    .background(c.line)
            )
        },
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .navigationBarsPadding()
                .padding(start = PageGutter, end = PageGutter, top = 8.dp, bottom = 20.dp)
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                IconWell(c, icon, tint = iconTint, background = iconBackground, size = 44)
                Spacer(Modifier.width(14.dp))
                Column(Modifier.weight(1f)) {
                    Text(title, style = DdType.title, color = c.text)
                    if (subtitle != null) {
                        Text(subtitle, style = DdType.small, color = c.textMuted)
                    }
                }
            }
            Spacer(Modifier.height(20.dp))
            content(close)
        }
    }
}

/** One action in an [ActionSheet]. */
internal data class SheetAction(
    val icon: ImageVector,
    val title: String,
    val detail: String? = null,
    val danger: Boolean = false,
    val onClick: () -> Unit,
)

/** The actions for one thing (a device, a feed entry), as a sheet. */
@Composable
internal fun ActionSheet(
    c: DdColors,
    icon: ImageVector,
    title: String,
    subtitle: String?,
    actions: List<SheetAction>,
    onDismiss: () -> Unit,
) {
    DdSheet(c, icon = icon, title = title, subtitle = subtitle, onDismiss = onDismiss) { close ->
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            actions.forEach { a ->
                SheetOption(c, a.icon, a.title, a.detail, danger = a.danger) { close(a.onClick) }
            }
        }
    }
}

/** A large tappable choice inside a sheet: icon well, title, detail, chevron. */
@Composable
internal fun SheetOption(
    c: DdColors,
    icon: ImageVector,
    title: String,
    detail: String?,
    danger: Boolean = false,
    onClick: () -> Unit,
) {
    val haptic = LocalHapticFeedback.current
    val shape = RoundedCornerShape(18.dp)
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(c.surfaceSunk)
            .border(1.dp, c.line, shape)
            .clickable {
                haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                onClick()
            }
            .padding(horizontal = 14.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        IconWell(
            c, icon,
            tint = if (danger) c.danger else c.accent,
            background = if (danger) c.danger.copy(alpha = 0.12f) else c.accentSoft
        )
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(title, style = DdType.label, color = if (danger) c.danger else c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (detail != null) {
                Text(detail, style = DdType.small, color = c.textMuted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
        }
        Spacer(Modifier.width(8.dp))
        if (!danger) Icon(Icons.Rounded.ChevronRight, contentDescription = null, tint = c.textMuted, modifier = Modifier.size(20.dp))
    }
}

/** Explains something and asks to go ahead: message, then the two answers. */
@Composable
internal fun ConfirmSheet(
    c: DdColors,
    icon: ImageVector,
    title: String,
    message: String,
    confirm: String,
    dismiss: String? = "Not now",
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
) {
    DdSheet(c, icon = icon, title = title, onDismiss = onDismiss) { close ->
        Text(message, style = DdType.body, color = c.textMuted)
        Spacer(Modifier.height(24.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            if (dismiss != null) {
                PillButton(c, dismiss, filled = false, modifier = Modifier.weight(1f)) { close {} }
            }
            PillButton(c, confirm, filled = true, modifier = Modifier.weight(1f)) { close(onConfirm) }
        }
    }
}

/**
 * "Files & folders": where to send (when several devices are connected) and
 * what. Android's pickers take files or one folder, never both, so the two
 * kinds are two choices here. `null` target means every connected device.
 */
@Composable
internal fun SendSheet(
    c: DdColors,
    onSend: (folder: Boolean, targetId: String?) -> Unit,
    onDismiss: () -> Unit,
) {
    DeviceChooserSheet(
        c, purpose = app.linkall.ChoicePurpose.Send, icon = Icons.Outlined.DevicesOther,
        title = "Send to which device?",
        emptyHint = "No paired device is connected. Open Link All on your computer, on the same Wi-Fi.",
        actions = listOf(
            ChooserAction(Icons.Outlined.UploadFile, { d -> if (d == null) "Send files to all" else "Send files to ${d.name}" }) { t -> onSend(false, t) },
            ChooserAction(Icons.Outlined.DriveFolderUpload, { d -> if (d == null) "Send a folder to all" else "Send a folder to ${d.name}" }) { t -> onSend(true, t) },
        ),
        onDismiss = onDismiss,
    )
}
