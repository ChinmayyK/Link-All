package app.linkall.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.ErrorOutline
import androidx.compose.material.icons.rounded.ExpandLess
import androidx.compose.material.icons.rounded.ExpandMore
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue

/**
 * Everything Link All needs to work well, in one place: what needs fixing first,
 * each with a button that fixes it, then what is fine, folded away until asked.
 */
@Composable
internal fun SetupSection(c: DdColors, items: List<SetupItem>, onFix: (String) -> Unit) {
    val problems = items.filter { !it.ok }
    val fine = items.filter { it.ok }
    var showFine by remember { mutableStateOf(false) }

    Column {
        SectionHeader(c, "Setup", if (problems.isEmpty()) "All set" else "${problems.size} to fix")
        Panel(c) {
            problems.forEachIndexed { i, item ->
                if (i > 0) Hairline(c)
                ListRow(
                    c, Icons.Outlined.ErrorOutline, item.title, item.detail,
                    iconTint = c.warn, iconBackground = c.warn.copy(alpha = 0.12f),
                    trailing = {
                        item.fixLabel?.let { label -> PillButton(c, label, filled = true, compact = true) { onFix(item.kind) } }
                    },
                )
            }
            if (fine.isNotEmpty()) {
                if (problems.isNotEmpty()) Hairline(c)
                ListRow(
                    c, Icons.Outlined.CheckCircle, if (problems.isEmpty()) "Everything is set up" else "Working fine",
                    "${fine.size} checks passed",
                    iconTint = c.live, iconBackground = c.live.copy(alpha = 0.12f),
                    onClick = { showFine = !showFine },
                    trailing = {
                        androidx.compose.material3.Icon(
                            if (showFine) Icons.Rounded.ExpandLess else Icons.Rounded.ExpandMore,
                            contentDescription = if (showFine) "Hide" else "Show",
                            tint = c.textMuted,
                        )
                    },
                )
                if (showFine) {
                    fine.forEach { item ->
                        Hairline(c)
                        ListRow(c, Icons.Outlined.CheckCircle, item.title, item.detail, iconTint = c.live, iconBackground = c.live.copy(alpha = 0.12f))
                    }
                }
            }
        }
    }
}
