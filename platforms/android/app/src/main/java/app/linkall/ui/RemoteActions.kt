package app.linkall.ui

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.widget.Toast
import app.linkall.LinkAllJni
import app.linkall.LinkAllService
import app.linkall.PeerSnapshot
import app.linkall.parsePeerSnapshots

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

    /**
     * Wakes a sleeping computer with Wake-on-LAN, then watches for it to connect.
     * Sending the packet says nothing about whether the computer heard it, so the
     * answer comes from whether it shows up: a card that isn't set to listen while
     * the computer sleeps never reacts, and this is how that is noticed.
     */
    fun wake(context: Context, peer: PeerSnapshot) {
        Thread {
            val problem = LinkAllService.withLiveEngine { handle -> LinkAllJni.wakePeer(handle, peer.id) }
                ?: "Link All isn't running."
            if (problem.isNotEmpty()) {
                say(context, problem)
                return@Thread
            }
            say(context, "Wake signal sent to ${peer.name}")
            val deadline = System.currentTimeMillis() + 45_000
            while (System.currentTimeMillis() < deadline) {
                Thread.sleep(2_000)
                val awake = LinkAllService.withLiveEngine { handle ->
                    parsePeerSnapshots(LinkAllJni.peersJson(handle)).any { it.id == peer.id && it.isConnected }
                } ?: return@Thread
                if (awake) {
                    say(context, "${peer.name} is awake")
                    return@Thread
                }
            }
            say(context, "${peer.name} didn't wake. Check that Wake-on-LAN is turned on in its network settings, and that it's on a cable.")
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
