package app.linkall.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.BatteryAlert
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties

/**
 * Walks the user through letting Link All run in the background. The steps
 * stay on screen while they go to Settings, and when they come back the
 * dialog says whether it worked ([allowed] is re-read on resume).
 */
@Composable
internal fun BackgroundAccessDialog(
    isDark: Boolean,
    allowed: Boolean,
    steps: List<String>,
    onOpenSettings: () -> Unit,
    onDismiss: () -> Unit,
) {
    val c = rememberDdColors(isDark)
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Column(
            Modifier
                .padding(horizontal = PageGutter)
                .widthIn(max = 480.dp)
                .fillMaxWidth()
                .clip(PanelShape)
                .background(c.surface)
                .border(1.dp, c.line, PanelShape)
                .padding(20.dp)
        ) {
            if (allowed) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    IconWell(c, Icons.Outlined.CheckCircle, tint = c.live, background = c.live.copy(alpha = 0.12f))
                    Spacer(Modifier.width(14.dp))
                    Column {
                        Text("Link All can keep running", style = DdType.title, color = c.text)
                        Text("Devices stay linked with the screen off", style = DdType.small, color = c.textMuted)
                    }
                }
                Spacer(Modifier.height(20.dp))
                PillButton(c, "Done", filled = true, modifier = Modifier.fillMaxWidth(), onClick = onDismiss)
                return@Column
            }

            Row(verticalAlignment = Alignment.CenterVertically) {
                IconWell(c, Icons.Outlined.BatteryAlert, tint = c.warn, background = c.warn.copy(alpha = 0.12f))
                Spacer(Modifier.width(14.dp))
                Column {
                    Text("Keep Link All running", style = DdType.title, color = c.text)
                    Text("So devices stay linked when the screen is off", style = DdType.small, color = c.textMuted)
                }
            }
            Spacer(Modifier.height(16.dp))
            Text(
                "Your phone pauses apps in the background to save battery. When it pauses Link All, your devices disconnect until you unlock the phone.",
                style = DdType.body,
                color = c.textMuted,
            )
            Spacer(Modifier.height(16.dp))
            steps.forEachIndexed { i, step ->
                Row(Modifier.padding(vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                    Box(
                        Modifier.size(26.dp).clip(CircleShape).background(c.accentSoft),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text("${i + 1}", style = DdType.small.copy(fontWeight = FontWeight.SemiBold), color = c.accent)
                    }
                    Spacer(Modifier.width(12.dp))
                    Text(step, style = DdType.body, color = c.text)
                }
            }
            Spacer(Modifier.height(20.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                PillButton(c, "Not now", filled = false, modifier = Modifier.weight(1f), onClick = onDismiss)
                PillButton(c, "Open settings", filled = true, modifier = Modifier.weight(1f), onClick = onOpenSettings)
            }
        }
    }
}
