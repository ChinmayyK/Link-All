package app.linkall

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.MediaPlayer
import android.media.RingtoneManager
import android.os.Handler
import android.os.Looper
import android.os.VibrationEffect
import android.os.Vibrator
import androidx.core.app.NotificationCompat

/**
 * Rings this phone so its owner can find it, when a paired computer asks and
 * the user has allowed it. Plays on the alarm stream, which sounds even in
 * silent mode, at full volume until "Found it", the computer says stop, or a
 * minute passes. The alarm volume is put back afterwards.
 */
object FindPhone {
    const val ACTION_STOP = "app.linkall.FIND_STOP"
    private const val CHANNEL = "cr_find"
    private const val NOTIF_ID = 3002
    private const val LIMIT_MS = 60_000L

    private val main = Handler(Looper.getMainLooper())
    private var player: MediaPlayer? = null
    private var savedVolume = -1
    private val timeout = Runnable { stop(appContext ?: return@Runnable) }
    private var appContext: Context? = null

    val isRinging: Boolean get() = player != null

    fun start(context: Context) {
        main.post {
            if (player != null) return@post
            val app = context.applicationContext
            appContext = app
            showNotification(app)
            val audio = app.getSystemService(AudioManager::class.java)
            savedVolume = audio.getStreamVolume(AudioManager.STREAM_ALARM)
            audio.setStreamVolume(AudioManager.STREAM_ALARM, audio.getStreamMaxVolume(AudioManager.STREAM_ALARM), 0)
            val uri = RingtoneManager.getDefaultUri(RingtoneManager.TYPE_ALARM)
                ?: RingtoneManager.getDefaultUri(RingtoneManager.TYPE_RINGTONE)
            player = runCatching {
                MediaPlayer().apply {
                    setAudioAttributes(
                        AudioAttributes.Builder()
                            .setUsage(AudioAttributes.USAGE_ALARM)
                            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                            .build()
                    )
                    setDataSource(app, uri)
                    isLooping = true
                    prepare()
                    start()
                }
            }.getOrNull()
            @Suppress("DEPRECATION")
            app.getSystemService(Vibrator::class.java)
                ?.vibrate(VibrationEffect.createWaveform(longArrayOf(0, 700, 500), 0))
            main.postDelayed(timeout, LIMIT_MS)
            if (player == null) stop(app)
        }
    }

    fun stop(context: Context) {
        main.post {
            main.removeCallbacks(timeout)
            val app = context.applicationContext
            player?.let { runCatching { it.stop() }; it.release() }
            player = null
            app.getSystemService(Vibrator::class.java)?.cancel()
            if (savedVolume >= 0) {
                app.getSystemService(AudioManager::class.java)
                    .setStreamVolume(AudioManager.STREAM_ALARM, savedVolume, 0)
                savedVolume = -1
            }
            app.getSystemService(NotificationManager::class.java).cancel(NOTIF_ID)
        }
    }

    private fun showNotification(context: Context) {
        val nm = context.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, "Find my phone", NotificationManager.IMPORTANCE_HIGH).apply {
                description = "Shown while a computer is making this phone ring"
                setSound(null, null)
                enableVibration(false)
                setBypassDnd(true)
            }
        )
        val stop = PendingIntent.getService(
            context, 0,
            Intent(context, LinkAllService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val open = PendingIntent.getActivity(
            context, 0, Intent(context, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        nm.notify(
            NOTIF_ID,
            NotificationCompat.Builder(context, CHANNEL)
                .setSmallIcon(R.mipmap.ic_launcher)
                .setContentTitle("Your phone is ringing")
                .setContentText("A computer is helping you find it.")
                .setCategory(NotificationCompat.CATEGORY_ALARM)
                .setPriority(NotificationCompat.PRIORITY_MAX)
                .setOngoing(true)
                .setContentIntent(open)
                .setFullScreenIntent(open, true)
                .addAction(0, "Found it", stop)
                .build()
        )
    }
}
