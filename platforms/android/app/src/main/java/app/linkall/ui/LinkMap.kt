@file:OptIn(ExperimentalLayoutApi::class)

package app.linkall.ui

import androidx.compose.foundation.Canvas
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
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.linkall.PeerSnapshot

/** The map draws up to this many devices around the phone; more than that become chips. */
internal const val LINK_MAP_MAX = 4

private val MapTop = 16.dp
private val PeerLabelLine = 18.dp
/** Where the lines leave the device row: under its tiles and their names. */
private val LinesFrom = MapTop + 48.dp + 6.dp + PeerLabelLine + 4.dp
/** Top of the phone tile, leaving the lines room to curve. */
private val PhoneTop = LinesFrom + 34.dp
private val MapHeight = PhoneTop + 60.dp + 6.dp + PeerLabelLine
private val PeerTile = 48.dp
private val PhoneTile = 60.dp
private val PeerLabelWidth = 84.dp

/**
 * This phone in the middle, each paired device above it, joined by a line:
 * solid while connected, dashed while not. Drawn once; nothing animates.
 */
@Composable
internal fun LinkMap(c: DdColors, deviceName: String, peers: List<PeerSnapshot>, synced: String?) {
    val n = peers.size.coerceIn(1, LINK_MAP_MAX)
    Panel(c) {
        BoxWithConstraints(Modifier.fillMaxWidth().height(MapHeight).padding(horizontal = 8.dp)) {
            val slot = maxWidth / n
            Canvas(Modifier.matchParentSize()) {
                val phone = Offset(size.width / 2, PhoneTop.toPx() - 4.dp.toPx())
                val fromY = LinesFrom.toPx()
                peers.take(LINK_MAP_MAX).forEachIndexed { i, peer ->
                    val x = (slot * i + slot / 2).toPx()
                    val midY = (fromY + phone.y) / 2
                    val path = Path().apply {
                        moveTo(x, fromY)
                        cubicTo(x, midY, phone.x, midY, phone.x, phone.y)
                    }
                    drawPath(
                        path,
                        color = if (peer.isConnected) c.accent.copy(alpha = 0.6f) else c.textMuted.copy(alpha = 0.35f),
                        style = Stroke(
                            width = 1.5.dp.toPx(),
                            cap = StrokeCap.Round,
                            pathEffect = if (peer.isConnected) null else PathEffect.dashPathEffect(floatArrayOf(4.dp.toPx(), 5.dp.toPx()))
                        )
                    )
                    drawCircle(
                        if (peer.isConnected) c.accent else c.textMuted.copy(alpha = 0.5f),
                        radius = 2.5.dp.toPx(), center = Offset(x, fromY)
                    )
                }
            }
            peers.take(LINK_MAP_MAX).forEachIndexed { i, peer ->
                PeerNode(
                    c, peer,
                    Modifier.offset(x = slot * i + (slot - PeerLabelWidth) / 2, y = MapTop)
                )
            }
            PhoneNode(
                c, deviceName,
                Modifier.align(Alignment.TopCenter).offset(y = PhoneTop)
            )
        }
        StatusFooter(c, peers, synced)
    }
}

/** Past [LINK_MAP_MAX] devices: one chip each, wrapping onto as many rows as needed. */
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

@Composable
private fun PeerNode(c: DdColors, peer: PeerSnapshot, modifier: Modifier) {
    Column(modifier.width(PeerLabelWidth), horizontalAlignment = Alignment.CenterHorizontally) {
        Box {
            Box(
                Modifier
                    .size(PeerTile)
                    .clip(RoundedCornerShape(16.dp))
                    .background(if (peer.isConnected) c.accentSoft else c.surfaceSunk),
                contentAlignment = Alignment.Center
            ) {
                Icon(
                    osIcon(peer.platform, peer.name), contentDescription = null,
                    tint = if (peer.isConnected) c.accent else c.textMuted,
                    modifier = Modifier.size(22.dp)
                )
            }
            Box(Modifier.align(Alignment.TopEnd).offset(x = 3.dp, y = (-3).dp)) {
                LiveDot(c, peer.isConnected, 12.dp, ring = true)
            }
        }
        Spacer(Modifier.height(6.dp))
        Text(
            peer.name, style = DdType.small.copy(fontWeight = FontWeight.Medium),
            color = if (peer.isConnected) c.text else c.textMuted,
            maxLines = 1, overflow = TextOverflow.Ellipsis, textAlign = TextAlign.Center
        )
    }
}

@Composable
private fun PhoneNode(c: DdColors, deviceName: String, modifier: Modifier) {
    Column(modifier.widthIn(max = 200.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Box(
            Modifier
                .size(PhoneTile)
                .clip(RoundedCornerShape(20.dp))
                .background(c.accent),
            contentAlignment = Alignment.Center
        ) {
            Icon(osIcon(DeviceOs.Android), contentDescription = null, tint = c.onAccent, modifier = Modifier.size(26.dp))
        }
        Spacer(Modifier.height(6.dp))
        Text(
            deviceName.ifBlank { "This phone" }, style = DdType.small.copy(fontWeight = FontWeight.SemiBold),
            color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis
        )
    }
}

/** Green when connected; a hollow ring when not. [ring] adds a page-coloured edge so it sits on a tile. */
@Composable
private fun LiveDot(c: DdColors, live: Boolean, size: androidx.compose.ui.unit.Dp, ring: Boolean = false) {
    val base = Modifier.size(size).clip(CircleShape)
    val edged = if (ring) base.background(c.surface).padding(2.dp).clip(CircleShape) else base
    Box(if (live) edged.background(c.live) else edged.background(c.surface).border(1.5.dp, c.textMuted, CircleShape))
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
