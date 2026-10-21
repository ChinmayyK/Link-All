package app.linkall.ui

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.widget.Toast
import app.linkall.LinkAllJni
import app.linkall.LinkAllService
import app.linkall.PeerSnapshot

/**
 * The things a phone asks a paired device to do, as plain calls a screen can make.
 * Each runs off the main thread and says what went wrong, if anything; the
 * device itself decides whether to allow it.
 */
internal object RemoteActions {
    private val main = Handler(Looper.getMainLooper())

    private fun say(context: Context, message: String) {
        if (message.isNotEmpty()) main.post { Toast.makeText(context.applicationContext, message, Toast.LENGTH_LONG).show() }
    }

    /** A media, volume, slide, lock or screen-off key on a computer. */
    fun press(context: Context, peer: PeerSnapshot, action: String) {
        Thread {
            // Null: the engine isn't running. Empty: sent. Otherwise why it could not be sent.
            val problem = LinkAllService.withLiveEngine { handle ->
                if (LinkAllJni.remoteControl(handle, peer.id, action) == 0) ""
                else LinkAllJni.remoteControlError(handle, peer.id) ?: "Couldn't reach ${peer.name}."
            } ?: "Link All isn't running."
            say(context, problem)
        }.start()
    }

    /** Makes a device ring (or stop), so its owner can find it. Waits for its answer. */
    fun ring(context: Context, peer: PeerSnapshot, on: Boolean) {
        Thread {
            val problem = LinkAllService.withLiveEngine { handle -> LinkAllJni.sendFindDevice(handle, peer.id, on) }
                ?: "Link All isn't running."
            say(context, if (problem.isEmpty() && on) "${peer.name} is ringing" else problem)
        }.start()
    }

    /** Play, pause, skip or seek what a device is playing. */
    fun media(peerId: String, action: String, value: Long = 0L) {
        Thread { LinkAllService.withLiveEngine { LinkAllJni.sendMediaCommand(it, peerId, action, value) } }.start()
    }
}
