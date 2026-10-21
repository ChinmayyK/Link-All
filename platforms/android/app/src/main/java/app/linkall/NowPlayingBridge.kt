package app.linkall

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.ComponentName
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.media.MediaMetadata
import android.media.session.MediaController
import android.media.session.MediaSession
import android.media.session.MediaSessionManager
import android.media.session.PlaybackState
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import org.json.JSONArray
import java.io.ByteArrayOutputStream

/**
 * Now Playing, both ways.
 *
 * What this phone plays (a music app, a podcast, a video) is read from its media
 * sessions and handed to the engine, which tells the paired computers. Reading the
 * sessions needs notification access, the same permission that lets the computers
 * show who is calling; without it nothing is shared.
 *
 * What a paired computer plays shows here as a real media session: the lock screen
 * and the notification shade get its title, artist, cover and a scrub bar, and the
 * buttons and the scrub bar control that computer's player.
 */
class NowPlayingBridge(private val context: Context) {
    private val main = Handler(Looper.getMainLooper())
    private val manager = context.getSystemService(Context.MEDIA_SESSION_SERVICE) as MediaSessionManager
    private val listenerComponent = ComponentName(context, LinkAllNotificationListener::class.java)
    private val notifications = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

    // ── This phone's player, told to the computers ──────────────────────────
    private var watched: MediaController? = null
    private var watchedCallback: MediaController.Callback? = null
    private var sessionsListener: MediaSessionManager.OnActiveSessionsChangedListener? = null

    private val tick = object : Runnable {
        override fun run() {
            tellComputers()
            main.postDelayed(this, 5_000L)
        }
    }

    // ── A computer's player, shown on this phone ────────────────────────────
    private var remote: MediaSession? = null
    private var remoteDevice: String? = null
    private var remoteTrackKey: String? = null
    private var remoteArt: Bitmap? = null

    fun start() {
        main.post {
            runCatching {
                val listener = MediaSessionManager.OnActiveSessionsChangedListener { controllers -> watch(controllers) }
                sessionsListener = listener
                manager.addOnActiveSessionsChangedListener(listener, listenerComponent, main)
                watch(manager.getActiveSessions(listenerComponent))
            }.onFailure {
                // No notification access yet: nothing to read, and nothing is shared.
            }
            main.postDelayed(tick, 5_000L)
            refreshRemote()
        }
    }

    fun stop() {
        main.post {
            main.removeCallbacks(tick)
            sessionsListener?.let { runCatching { manager.removeOnActiveSessionsChangedListener(it) } }
            sessionsListener = null
            unwatch()
            LinkAllService.withLiveEngine { LinkAllJni.clearNowPlaying(it) }
            dropRemote()
        }
    }

    // ── Reading this phone's player ─────────────────────────────────────────

    private fun watch(controllers: List<MediaController>?) {
        // Our own session shows a computer's player; sharing it back would loop.
        val candidates = controllers.orEmpty().filter { it.packageName != context.packageName }
        val chosen = candidates.firstOrNull { it.playbackState?.state == PlaybackState.STATE_PLAYING }
            ?: candidates.firstOrNull { it.metadata != null }
        if (chosen?.sessionToken == watched?.sessionToken) {
            tellComputers()
            return
        }
        unwatch()
        watched = chosen
        chosen?.let { controller ->
            val callback = object : MediaController.Callback() {
                override fun onMetadataChanged(metadata: MediaMetadata?) = tellComputers()
                override fun onPlaybackStateChanged(state: PlaybackState?) = tellComputers()
                override fun onSessionDestroyed() = tellComputers()
            }
            watchedCallback = callback
            controller.registerCallback(callback, main)
        }
        tellComputers()
    }

    private fun unwatch() {
        watchedCallback?.let { runCatching { watched?.unregisterCallback(it) } }
        watchedCallback = null
        watched = null
    }

    private fun tellComputers() {
        val controller = watched
        val metadata = controller?.metadata
        val title = metadata?.getString(MediaMetadata.METADATA_KEY_TITLE).orEmpty()
        if (controller == null || metadata == null || title.isBlank()) {
            LinkAllService.withLiveEngine { LinkAllJni.clearNowPlaying(it) }
            return
        }
        val state = controller.playbackState
        val playing = state?.state == PlaybackState.STATE_PLAYING
        var position = state?.position ?: 0L
        if (playing && state != null) {
            // The position was true when the state was last updated: carry it forward.
            position += ((SystemClock.elapsedRealtime() - state.lastPositionUpdateTime) * state.playbackSpeed).toLong()
        }
        val artist = metadata.getString(MediaMetadata.METADATA_KEY_ARTIST)
            ?: metadata.getString(MediaMetadata.METADATA_KEY_ALBUM_ARTIST).orEmpty()
        val album = metadata.getString(MediaMetadata.METADATA_KEY_ALBUM).orEmpty()
        val duration = metadata.getLong(MediaMetadata.METADATA_KEY_DURATION).coerceAtLeast(0L)
        val app = runCatching {
            val info = context.packageManager.getApplicationInfo(controller.packageName, 0)
            context.packageManager.getApplicationLabel(info).toString()
        }.getOrDefault(controller.packageName)
        val art = coverOf(metadata)?.let { jpegOf(it) }
        LinkAllService.withLiveEngine {
            LinkAllJni.setNowPlaying(it, title, artist, album, app, playing, position.coerceAtLeast(0L), duration, art)
        }
    }

    private fun coverOf(metadata: MediaMetadata): Bitmap? =
        metadata.getBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART)
            ?: metadata.getBitmap(MediaMetadata.METADATA_KEY_ART)
            ?: metadata.getBitmap(MediaMetadata.METADATA_KEY_DISPLAY_ICON)

    /** A cover as a JPEG of at most 300 pixels, small enough to send. */
    private fun jpegOf(source: Bitmap): ByteArray? = runCatching {
        val scale = 300f / maxOf(source.width, source.height)
        val bitmap = if (scale < 1f) {
            Bitmap.createScaledBitmap(source, (source.width * scale).toInt().coerceAtLeast(1), (source.height * scale).toInt().coerceAtLeast(1), true)
        } else source
        ByteArrayOutputStream().use { out ->
            bitmap.compress(Bitmap.CompressFormat.JPEG, 82, out)
            out.toByteArray()
        }
    }.getOrNull()

    /** A computer asked to control this phone's player. */
    fun onCommand(command: String) {
        main.post {
            val (action, value) = command.substringBefore(':') to (command.substringAfter(':', "0").toLongOrNull() ?: 0L)
            val controls = watched?.transportControls ?: return@post
            when (action) {
                "play" -> controls.play()
                "pause" -> controls.pause()
                "play_pause" -> if (watched?.playbackState?.state == PlaybackState.STATE_PLAYING) controls.pause() else controls.play()
                "next" -> controls.skipToNext()
                "previous" -> controls.skipToPrevious()
                "seek" -> controls.seekTo(value)
            }
        }
    }

    // ── Showing a computer's player ─────────────────────────────────────────

    /** Re-reads what the paired computers play and updates the session to match. */
    fun refreshRemote() {
        main.post {
            val json = LinkAllService.withLiveEngine { LinkAllJni.nowPlayingJson(it) }
            val list = runCatching { JSONArray(json ?: "[]") }.getOrDefault(JSONArray())
            // Computers only: a phone's own player is the phone's.
            var chosen: org.json.JSONObject? = null
            for (i in 0 until list.length()) {
                val item = list.optJSONObject(i) ?: continue
                if (item.optString("platform") == "Android") continue
                if (chosen == null || (item.optBoolean("playing") && !chosen.optBoolean("playing"))) chosen = item
            }
            if (chosen == null) {
                dropRemote()
                return@post
            }
            showRemote(chosen)
        }
    }

    private fun showRemote(item: org.json.JSONObject) {
        val device = item.optString("device_id")
        val title = item.optString("title")
        val artist = item.optString("artist")
        val album = item.optString("album")
        val playing = item.optBoolean("playing")
        val position = item.optLong("position_ms")
        val duration = item.optLong("duration_ms")
        val computer = item.optString("device_name")
        val key = "$device|$title|$artist|$album"
        val session = remote ?: MediaSession(context, "LinkAllRemote").also { created ->
            created.setCallback(object : MediaSession.Callback() {
                override fun onPlay() = command("play")
                override fun onPause() = command("pause")
                override fun onSkipToNext() = command("next")
                override fun onSkipToPrevious() = command("previous")
                override fun onSeekTo(pos: Long) = command("seek", pos)
            }, main)
            created.isActive = true
            remote = created
        }
        remoteDevice = device
        if (key != remoteTrackKey) {
            remoteTrackKey = key
            remoteArt = LinkAllService.withLiveEngine { LinkAllJni.nowPlayingArt(it, device) }
                ?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
        }
        session.setMetadata(
            MediaMetadata.Builder()
                .putString(MediaMetadata.METADATA_KEY_TITLE, title)
                .putString(MediaMetadata.METADATA_KEY_ARTIST, artist)
                .putString(MediaMetadata.METADATA_KEY_ALBUM, if (album.isBlank()) "on $computer" else "$album · on $computer")
                .putLong(MediaMetadata.METADATA_KEY_DURATION, duration)
                .apply { remoteArt?.let { putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, it) } }
                .build()
        )
        session.setPlaybackState(
            PlaybackState.Builder()
                .setActions(
                    PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE or PlaybackState.ACTION_PLAY_PAUSE or
                        PlaybackState.ACTION_SKIP_TO_NEXT or PlaybackState.ACTION_SKIP_TO_PREVIOUS or PlaybackState.ACTION_SEEK_TO
                )
                .setState(
                    if (playing) PlaybackState.STATE_PLAYING else PlaybackState.STATE_PAUSED,
                    position, if (playing) 1f else 0f, SystemClock.elapsedRealtime(),
                )
                .build()
        )
        notifications.createNotificationChannel(
            NotificationChannel(CHANNEL, "Now playing on your computer", NotificationManager.IMPORTANCE_LOW).apply {
                description = "What a paired computer is playing, with controls"
                setShowBadge(false)
            }
        )
        val notification = Notification.Builder(context, CHANNEL)
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentTitle(title)
            .setContentText(if (artist.isBlank()) "on $computer" else "$artist · on $computer")
            .setLargeIcon(remoteArt)
            .setOngoing(playing)
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .setStyle(Notification.MediaStyle().setMediaSession(session.sessionToken).setShowActionsInCompactView())
            .build()
        runCatching { notifications.notify(NOTIF_ID, notification) }
    }

    private fun command(action: String, value: Long = 0L) {
        val device = remoteDevice ?: return
        Thread { LinkAllService.withLiveEngine { LinkAllJni.sendMediaCommand(it, device, action, value) } }.start()
    }

    private fun dropRemote() {
        remote?.let {
            it.isActive = false
            it.release()
        }
        remote = null
        remoteDevice = null
        remoteTrackKey = null
        remoteArt = null
        runCatching { notifications.cancel(NOTIF_ID) }
    }

    private companion object {
        const val CHANNEL = "cr_now_playing"
        const val NOTIF_ID = 3100
    }
}
