package app.linkall.ui

import android.widget.Toast
import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.DevicesOther
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.outlined.RadioButtonUnchecked
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.linkall.ChoicePurpose
import app.linkall.DeviceChoice
import app.linkall.DeviceChooser
import app.linkall.DeviceChoices
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** One button under the device list. [label] gets the chosen device (null for "All devices"). */
internal class ChooserAction(
    val icon: ImageVector,
    val label: (DeviceChoice?) -> String,
    val onChosen: (targetId: String?) -> Unit,
)

private const val ALL = "*all*"

/**
 * The one way every feature asks "which device?". The core ranks the paired
 * devices for [purpose]; the one used last is already selected, so the usual
 * case is one tap on the button. Devices that can't be used right now sit
 * below, greyed, each saying why. Nothing is sent until a button is pressed.
 */
@Composable
internal fun DeviceChooserSheet(
    c: DdColors,
    purpose: ChoicePurpose,
    icon: ImageVector,
    title: String,
    emptyHint: String,
    actions: List<ChooserAction>,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    val loaded: DeviceChoices? by produceState<DeviceChoices?>(null, purpose) {
        value = withContext(Dispatchers.IO) { DeviceChooser.load(context, purpose) }
    }
    val choices = loaded
    var picked by remember(choices) {
        mutableStateOf(choices?.available?.let { list -> (list.firstOrNull { it.lastUsed } ?: list.firstOrNull())?.id })
    }

    val subtitle = when {
        choices == null -> "Looking for your devices…"
        choices.available.isEmpty() -> null
        choices.available.size == 1 -> "Only ${choices.available.first().name} is connected"
        else -> "${choices.available.size} devices are connected"
    }

    DdSheet(c, icon = icon, title = title, subtitle = subtitle, onDismiss = onDismiss) { close ->
        if (choices == null) return@DdSheet
        if (choices.available.isEmpty()) {
            Text(emptyHint, style = DdType.body, color = c.textMuted)
            Spacer(Modifier.height(16.dp))
        }
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            if (choices.offersAll) {
                ChooserRow(c, Icons.Outlined.DevicesOther, "All devices", "${choices.available.size} connected", picked == ALL, true) { picked = ALL }
            }
            choices.available.forEach { d ->
                ChooserRow(
                    c, osIcon(d.platform, d.name), d.name,
                    listOfNotNull(d.platform, "used last".takeIf { d.lastUsed }).joinToString(" · ").ifBlank { null },
                    picked == d.id, true,
                ) { picked = d.id }
            }
        }
        if (choices.unavailable.isNotEmpty()) {
            Spacer(Modifier.height(18.dp))
            Text("Not available", style = DdType.small, color = c.textMuted)
            Spacer(Modifier.height(8.dp))
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                choices.unavailable.forEach { d ->
                    ChooserRow(c, osIcon(d.platform, d.name), d.name, d.reason, false, false) {
                        Toast.makeText(context, "${d.name}: ${d.reason}", Toast.LENGTH_LONG).show()
                    }
                }
            }
        }
        if (choices.available.isNotEmpty()) {
            Spacer(Modifier.height(20.dp))
            val chosen = choices.available.firstOrNull { it.id == picked }
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                actions.forEachIndexed { i, a ->
                    PillButton(
                        c, a.label(chosen), filled = i == 0, modifier = Modifier.fillMaxWidth(), icon = a.icon,
                    ) {
                        val target = picked
                        if (target == null) return@PillButton
                        if (target != ALL) DeviceChooser.remember(context, purpose, target)
                        close { a.onChosen(target.takeIf { it != ALL }) }
                    }
                }
            }
        }
    }
}

/** A device row: logo, name, detail and a selection mark. Greyed and not selectable when [enabled] is false. */
@Composable
private fun ChooserRow(
    c: DdColors,
    icon: ImageVector,
    title: String,
    detail: String?,
    selected: Boolean,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    val shape = RoundedCornerShape(18.dp)
    val border by animateColorAsState(if (selected) c.accent else c.line, label = "chooser-border")
    Row(
        Modifier
            .fillMaxWidth()
            .alpha(if (enabled) 1f else 0.55f)
            .clip(shape)
            .background(if (selected) c.accentSoft else c.surfaceSunk)
            .border(if (selected) 2.dp else 1.dp, border, shape)
            .clickable(onClick = onClick)
            .semantics(mergeDescendants = true) {
                role = Role.RadioButton
                this.selected = selected
                if (!enabled) contentDescription = "$title, not available${detail?.let { ": $it" } ?: ""}"
            }
            .padding(horizontal = 14.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconWell(c, icon, tint = if (selected) c.accent else c.textMuted, background = if (selected) c.surface else c.accentSoft)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(title, style = DdType.label, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (detail != null) Text(detail, style = DdType.small, color = c.textMuted, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
        if (enabled) {
            Spacer(Modifier.width(10.dp))
            Icon(
                if (selected) Icons.Rounded.CheckCircle else Icons.Outlined.RadioButtonUnchecked,
                contentDescription = null,
                tint = if (selected) c.accent else c.textMuted,
                modifier = Modifier.size(22.dp),
            )
        }
    }
}
