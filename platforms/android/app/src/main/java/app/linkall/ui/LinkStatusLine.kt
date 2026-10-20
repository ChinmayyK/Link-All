package app.linkall.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.linkall.PeerSnapshot

/**
 * "● 2 devices linked · synced 2m ago", or "○ Not connected · 2 paired",
 * under the app's name. The devices themselves are listed once, under
 * "Your devices".
 */
@Composable
internal fun LinkStatusLine(c: DdColors, peers: List<PeerSnapshot>, synced: String?, modifier: Modifier = Modifier) {
    val linked = peers.count { it.isConnected }
    val lead = when (linked) {
        0 -> "Not connected"
        1 -> "1 device linked"
        else -> "$linked devices linked"
    }
    val tail = if (linked == 0) "${peers.size} paired" else synced?.let { "synced $it" }
    Row(
        modifier.semantics(mergeDescendants = true) {},
        verticalAlignment = Alignment.CenterVertically
    ) {
        LiveDot(c, linked > 0, 7.dp)
        Spacer(Modifier.width(7.dp))
        Text(lead, style = DdType.small.copy(fontWeight = FontWeight.SemiBold), color = if (linked > 0) c.live else c.textMuted, maxLines = 1)
        if (tail != null) {
            Text(" · $tail", style = DdType.small, color = c.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

/** Green when connected; a hollow ring when not. */
@Composable
private fun LiveDot(c: DdColors, live: Boolean, size: androidx.compose.ui.unit.Dp) {
    val base = Modifier.size(size).clip(CircleShape)
    Box(if (live) base.background(c.live) else base.background(c.surface).border(1.5.dp, c.textMuted, CircleShape))
}
