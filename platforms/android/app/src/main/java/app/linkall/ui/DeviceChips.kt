@file:OptIn(ExperimentalLayoutApi::class)

package app.linkall.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
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

/** Each paired device as a chip, wrapping onto as many rows as needed. */
@Composable
internal fun DeviceChips(c: DdColors, peers: List<PeerSnapshot>, synced: String?) {
    Panel(c) {
        FlowRow(
            Modifier.fillMaxWidth().padding(start = 14.dp, end = 14.dp, top = 14.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            peers.forEach { peer ->
                Row(
                    Modifier
                        .clip(RoundedCornerShape(14.dp))
                        .background(c.surfaceSunk)
                        .padding(start = 10.dp, end = 12.dp, top = 8.dp, bottom = 8.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Icon(
                        osIcon(peer.platform, peer.name), contentDescription = null,
                        tint = if (peer.isConnected) c.accent else c.textMuted,
                        modifier = Modifier.size(15.dp)
                    )
                    Spacer(Modifier.width(7.dp))
                    Text(
                        peer.name, style = DdType.small.copy(fontWeight = FontWeight.SemiBold),
                        color = if (peer.isConnected) c.text else c.textMuted,
                        maxLines = 1, overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.widthIn(max = 140.dp)
                    )
                    Spacer(Modifier.width(7.dp))
                    LiveDot(c, peer.isConnected, 6.dp)
                }
            }
        }
        StatusFooter(c, peers, synced)
    }
}

/** Green when connected; a hollow ring when not. */
@Composable
private fun LiveDot(c: DdColors, live: Boolean, size: androidx.compose.ui.unit.Dp) {
    val base = Modifier.size(size).clip(CircleShape)
    Box(if (live) base.background(c.live) else base.background(c.surface).border(1.5.dp, c.textMuted, CircleShape))
}

/** "● 3 devices linked · synced 2m ago", or "○ Not connected · 2 paired". */
@Composable
private fun StatusFooter(c: DdColors, peers: List<PeerSnapshot>, synced: String?) {
    val linked = peers.count { it.isConnected }
    val lead = when (linked) {
        0 -> "Not connected"
        1 -> "1 device linked"
        else -> "$linked devices linked"
    }
    val tail = if (linked == 0) "${peers.size} paired" else synced?.let { "synced $it" }
    Row(
        Modifier
            .fillMaxWidth()
            .padding(top = 10.dp, bottom = 14.dp, start = 16.dp, end = 16.dp)
            .semantics(mergeDescendants = true) {},
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically
    ) {
        LiveDot(c, linked > 0, 8.dp)
        Spacer(Modifier.width(8.dp))
        Text(lead, style = DdType.small.copy(fontWeight = FontWeight.SemiBold), color = if (linked > 0) c.live else c.textMuted, maxLines = 1)
        if (tail != null) {
            Text(" · $tail", style = DdType.small, color = c.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}
