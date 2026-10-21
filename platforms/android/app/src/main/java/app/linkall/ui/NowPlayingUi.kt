package app.linkall.ui

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.MusicNote
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material.icons.rounded.SkipPrevious
import androidx.compose.material3.Icon
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.linkall.LinkAllJni
import app.linkall.LinkAllService
import app.linkall.ui.theme.crPressScale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.json.JSONArray

/** What a paired device is playing, as the engine reports it. */
@Immutable
internal data class NowPlayingItem(
    val deviceId: String,
    val title: String,
    val artist: String,
    val album: String,
    val app: String,
    val playing: Boolean,
    val positionMs: Long,
    val durationMs: Long,
)

internal fun parseNowPlaying(json: String?): List<NowPlayingItem> {
    val array = runCatching { JSONArray(json ?: "[]") }.getOrNull() ?: return emptyList()
    return (0 until array.length()).mapNotNull { i ->
        val o = array.optJSONObject(i) ?: return@mapNotNull null
        NowPlayingItem(
            deviceId = o.optString("device_id"),
            title = o.optString("title"),
            artist = o.optString("artist"),
            album = o.optString("album"),
            app = o.optString("app"),
            playing = o.optBoolean("playing"),
            positionMs = o.optLong("position_ms"),
            durationMs = o.optLong("duration_ms"),
        )
    }.filter { it.title.isNotBlank() }
}

/** What each paired device is playing, by device id, refreshed while on screen. */
@Composable
internal fun rememberNowPlaying(): Map<String, NowPlayingItem> {
    val items by produceState(emptyMap<String, NowPlayingItem>()) {
        while (true) {
            val json = withContext(Dispatchers.IO) { LinkAllService.withLiveEngine { LinkAllJni.nowPlayingJson(it) } }
            value = parseNowPlaying(json).associateBy { it.deviceId }
            delay(2000)
        }
    }
    return items
}

/** The cover for a track, fetched once per track. */
@Composable
private fun rememberCover(item: NowPlayingItem): ImageBitmap? {
    val key = "${item.deviceId}|${item.title}|${item.artist}|${item.album}"
    val cover by produceState<ImageBitmap?>(null, key) {
        value = withContext(Dispatchers.IO) {
            LinkAllService.withLiveEngine { LinkAllJni.nowPlayingArt(it, item.deviceId) }
                ?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
                ?.asImageBitmap()
        }
    }
    return cover
}

@Composable
private fun Cover(c: DdColors, item: NowPlayingItem, size: Int) {
    val cover = rememberCover(item)
    val shape = RoundedCornerShape((size / 5).dp)
    Box(
        Modifier.size(size.dp).clip(shape).background(c.surfaceSunk),
        contentAlignment = Alignment.Center,
    ) {
        if (cover != null) {
            Image(cover, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.size(size.dp))
        } else {
            val (fg, _) = c.tone(Tone.Teal)
            Icon(Icons.Outlined.MusicNote, contentDescription = null, tint = fg, modifier = Modifier.size((size / 2).dp))
        }
    }
}

@Composable
private fun MediaButton(c: DdColors, icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, size: Int = 40, filled: Boolean = false, onClick: () -> Unit) {
    Box(
        Modifier
            .size(size.dp)
            .clip(CircleShape)
            .background(if (filled) c.text else androidx.compose.ui.graphics.Color.Transparent)
            .crPressScale(0.9f) { onClick() }
            .semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, contentDescription = null, tint = if (filled) c.page else c.text, modifier = Modifier.size((size * 0.55f).dp))
    }
}

/** One line on a device card: the cover, the track and a play/pause button. */
@Composable
internal fun NowPlayingMini(c: DdColors, item: NowPlayingItem) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(c.surfaceSunk)
            .padding(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Cover(c, item, 40)
        Spacer(Modifier.width(10.dp))
        Column(Modifier.weight(1f)) {
            Text(item.title, style = DdType.small.copy(fontWeight = FontWeight.SemiBold), color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                listOf(item.artist, item.app).filter { it.isNotBlank() }.joinToString(" · "),
                style = DdType.small, color = c.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis,
            )
        }
        MediaButton(c, if (item.playing) Icons.Rounded.Pause else Icons.Rounded.PlayArrow, if (item.playing) "Pause" else "Play") {
            RemoteActions.media(item.deviceId, "play_pause")
        }
    }
}

private fun clock(ms: Long): String {
    val s = (ms / 1000).coerceAtLeast(0)
    return "%d:%02d".format(s / 60, s % 60)
}

/** The full player for a device page: cover, track, a scrub bar and the controls. */
@Composable
internal fun NowPlayingPanel(c: DdColors, item: NowPlayingItem) {
    var scrubbing by remember(item.deviceId, item.title) { mutableStateOf<Float?>(null) }
    Panel(c) {
        Column(Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Cover(c, item, 64)
                Spacer(Modifier.width(14.dp))
                Column(Modifier.weight(1f)) {
                    Text(item.title, style = DdType.label.copy(fontWeight = FontWeight.SemiBold), color = c.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    if (item.artist.isNotBlank()) {
                        Text(item.artist, style = DdType.small, color = c.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                    if (item.app.isNotBlank()) {
                        Text(item.app, style = DdType.small, color = c.textMuted, maxLines = 1)
                    }
                }
            }
            if (item.durationMs > 0) {
                Spacer(Modifier.height(8.dp))
                Slider(
                    value = scrubbing ?: item.positionMs.toFloat().coerceIn(0f, item.durationMs.toFloat()),
                    onValueChange = { scrubbing = it },
                    onValueChangeFinished = {
                        scrubbing?.let { RemoteActions.media(item.deviceId, "seek", it.toLong()) }
                        scrubbing = null
                    },
                    valueRange = 0f..item.durationMs.toFloat(),
                    colors = SliderDefaults.colors(thumbColor = c.text, activeTrackColor = c.text, inactiveTrackColor = c.line),
                )
                Row {
                    Text(clock((scrubbing?.toLong()) ?: item.positionMs), style = DdType.mono, color = c.textMuted)
                    Spacer(Modifier.weight(1f))
                    Text(clock(item.durationMs), style = DdType.mono, color = c.textMuted)
                }
            }
            Spacer(Modifier.height(6.dp))
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
                MediaButton(c, Icons.Rounded.SkipPrevious, "Previous", 48) { RemoteActions.media(item.deviceId, "previous") }
                Spacer(Modifier.width(18.dp))
                MediaButton(c, if (item.playing) Icons.Rounded.Pause else Icons.Rounded.PlayArrow, if (item.playing) "Pause" else "Play", 60, filled = true) {
                    RemoteActions.media(item.deviceId, "play_pause")
                }
                Spacer(Modifier.width(18.dp))
                MediaButton(c, Icons.Rounded.SkipNext, "Next", 48) { RemoteActions.media(item.deviceId, "next") }
            }
        }
    }
}
