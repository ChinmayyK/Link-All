package app.linkall.ui

import android.content.Context
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Mouse
import androidx.compose.material3.Icon
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import app.linkall.LinkAllJni
import app.linkall.LinkAllService
import app.linkall.PeerSnapshot
import java.util.concurrent.Executors
import kotlin.math.abs
import kotlin.math.roundToInt

/** How far the pointer moves for a finger's travel; trackpads accelerate, this is the flat part. */
private const val FINGER_GAIN = 1.7f

/** Pixel-like units of scroll per pixel of two-finger travel. */
private const val SCROLL_GAIN = 1.2f

/** Pointer pixels per radian of turn in air-mouse mode. */
private const val AIR_GAIN = 900f

private const val TAP_MAX_MS = 260L
private const val TAP_MAX_TRAVEL_PX = 16f

/** Sends trackpad input for one computer, in order, off the main thread. */
private class PadSender(private val peerId: String, private val context: Context) {
    private val io = Executors.newSingleThreadExecutor()
    private var lastError = 0L

    fun move(dx: Int, dy: Int) = send { LinkAllJni.sendPointerMove(it, peerId, dx, dy) }
    fun scroll(dx: Int, dy: Int) = send { LinkAllJni.sendPointerScroll(it, peerId, dx, dy) }
    fun button(button: Int, down: Boolean) = send { LinkAllJni.sendPointerButton(it, peerId, button, down) }
    fun click(button: Int) {
        button(button, true)
        button(button, false)
    }
    fun text(text: String) = send { LinkAllJni.sendTypeText(it, peerId, text) }
    fun key(key: String) = send { LinkAllJni.sendKeyPress(it, peerId, key) }

    private fun send(call: (Long) -> Int) {
        io.execute {
            val sent = LinkAllService.withLiveEngine(call)
            if (sent != 0) reportOnce(if (sent == null) "Link All isn't running." else "Can't reach that computer.")
        }
    }

    private fun reportOnce(message: String) {
        val now = SystemClock.elapsedRealtime()
        if (now - lastError < 3000) return
        lastError = now
        Handler(Looper.getMainLooper()).post { Toast.makeText(context, message, Toast.LENGTH_SHORT).show() }
    }

    fun close() = io.shutdown()
}

/**
 * The phone as a trackpad and keyboard for [peer]: one finger moves the
 * pointer, a tap clicks, two fingers scroll, a two-finger tap right-clicks.
 * Air mouse points with the phone's turn instead. The computer must have
 * Remote control turned on; it says so on the phone when it doesn't.
 */
@Composable
internal fun TrackpadDialog(c: DdColors, peer: PeerSnapshot, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val sender = remember(peer.id) { PadSender(peer.id, context.applicationContext) }
    DisposableEffect(sender) { onDispose { sender.close() } }
    var air by remember { mutableStateOf(false) }
    var typed by remember { mutableStateOf("") }

    AirMouse(enabled = air, sender = sender)

    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Column(
            Modifier
                .fillMaxSize()
                .background(c.surface)
                .padding(horizontal = PageGutter, vertical = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Outlined.Mouse, contentDescription = null, tint = c.accent, modifier = Modifier.size(22.dp))
                Spacer(Modifier.size(10.dp))
                Column(Modifier.weight(1f)) {
                    Text("Trackpad · ${peer.name}", style = DdType.title, color = c.text, maxLines = 1)
                    Text(
                        if (air) "Point the phone and tap Click" else "Drag to move · tap to click · two fingers scroll",
                        style = DdType.small, color = c.textMuted, maxLines = 1,
                    )
                }
                Icon(
                    Icons.Outlined.Close, contentDescription = "Close", tint = c.text,
                    modifier = Modifier.size(28.dp).clickable(onClick = onDismiss),
                )
            }

            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Air mouse", style = DdType.body, color = c.text, modifier = Modifier.weight(1f))
                Switch(checked = air, onCheckedChange = { air = it })
            }

            val padShape = RoundedCornerShape(22.dp)
            Box(
                Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .clip(padShape)
                    .background(c.surfaceSunk)
                    .border(1.dp, c.line, padShape)
                    .trackpadGestures(sender, pointerMoves = !air),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    if (air) "Scroll with two fingers" else "Touch here",
                    style = DdType.small, color = c.textMuted,
                )
            }

            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                PillButton(c, "Left click", filled = true, modifier = Modifier.weight(1f)) { sender.click(0) }
                PillButton(c, "Right click", filled = false, modifier = Modifier.weight(1f)) { sender.click(1) }
            }

            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for ((label, key) in listOf("Esc" to "escape", "Tab" to "tab", "←" to "left", "↑" to "up", "↓" to "down", "→" to "right")) {
                    PillButton(c, label, filled = false, compact = true, modifier = Modifier.weight(1f)) { sender.key(key) }
                }
            }

            TextField(
                value = typed,
                onValueChange = { new ->
                    sendTypedChange(sender, typed, new)
                    typed = new
                },
                modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("Type to send to ${peer.name}") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                keyboardActions = KeyboardActions(onSend = {
                    sender.key("enter")
                    typed = ""
                }),
            )
            Spacer(Modifier.height(4.dp))
        }
    }
}

/** Turns an edit of the text field into the keystrokes that make the computer match it. */
private fun sendTypedChange(sender: PadSender, old: String, new: String) {
    val common = old.commonPrefixWith(new).length
    repeat(old.length - common) { sender.key("backspace") }
    val added = new.substring(common)
    if (added.isNotEmpty()) sender.text(added)
}

/**
 * One finger drags the pointer, a short still touch clicks, two fingers
 * scroll and a short still two-finger touch right-clicks. With
 * [pointerMoves] off (air mouse) one finger does nothing but a tap still clicks.
 */
private fun Modifier.trackpadGestures(sender: PadSender, pointerMoves: Boolean): Modifier =
    pointerInput(sender, pointerMoves) {
        awaitEachGesture {
            val first = awaitFirstDown(requireUnconsumed = false)
            val startedAt = SystemClock.uptimeMillis()
            var maxFingers = 1
            var travel = 0f
            var previousCount = 1
            var lastCentre = first.position
            var carryX = 0f
            var carryY = 0f
            while (true) {
                val event = awaitPointerEvent(PointerEventPass.Main)
                val down = event.changes.filter { it.pressed }
                if (down.isEmpty()) break
                if (down.size > maxFingers) maxFingers = down.size
                val centre = down.map { it.position }.reduce { a, b -> a + b } / down.size.toFloat()
                if (down.size == previousCount) {
                    val delta = centre - lastCentre
                    travel += abs(delta.x) + abs(delta.y)
                    if (down.size == 1) {
                        if (pointerMoves) {
                            carryX += delta.x * FINGER_GAIN
                            carryY += delta.y * FINGER_GAIN
                            val sx = carryX.roundToInt()
                            val sy = carryY.roundToInt()
                            carryX -= sx
                            carryY -= sy
                            if (sx != 0 || sy != 0) sender.move(sx, sy)
                        }
                    } else {
                        // Fingers moving down the screen scroll the page down.
                        carryX += delta.x * SCROLL_GAIN
                        carryY += delta.y * SCROLL_GAIN
                        val sx = carryX.roundToInt()
                        val sy = carryY.roundToInt()
                        carryX -= sx
                        carryY -= sy
                        if (sx != 0 || sy != 0) sender.scroll(-sx, -sy)
                    }
                }
                previousCount = down.size
                lastCentre = centre
                event.changes.forEach { it.consume() }
            }
            val held = SystemClock.uptimeMillis() - startedAt
            if (held <= TAP_MAX_MS && travel <= TAP_MAX_TRAVEL_PX) {
                when (maxFingers) {
                    1 -> sender.click(0)
                    2 -> sender.click(1)
                    else -> sender.click(2)
                }
            }
        }
    }

/** While [enabled], the phone's turn moves the pointer. */
@Composable
private fun AirMouse(enabled: Boolean, sender: PadSender) {
    val context = LocalContext.current
    DisposableEffect(enabled, sender) {
        val manager = context.getSystemService(Context.SENSOR_SERVICE) as? SensorManager
        val gyro = manager?.getDefaultSensor(Sensor.TYPE_GYROSCOPE)
        if (!enabled || manager == null || gyro == null) {
            if (enabled && gyro == null) {
                Toast.makeText(context, "This phone has no gyroscope.", Toast.LENGTH_SHORT).show()
            }
            return@DisposableEffect onDispose {}
        }
        val listener = object : SensorEventListener {
            private var lastNs = 0L
            private var carryX = 0f
            private var carryY = 0f

            override fun onSensorChanged(event: SensorEvent) {
                if (lastNs != 0L) {
                    val dt = (event.timestamp - lastNs) / 1_000_000_000f
                    // Held upright, turning left or right spins about the phone's long axis
                    // and tilting up or down about its short one.
                    carryX += -event.values[1] * AIR_GAIN * dt
                    carryY += -event.values[0] * AIR_GAIN * dt
                    val sx = carryX.roundToInt()
                    val sy = carryY.roundToInt()
                    carryX -= sx
                    carryY -= sy
                    if (sx != 0 || sy != 0) sender.move(sx, sy)
                }
                lastNs = event.timestamp
            }

            override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) = Unit
        }
        manager.registerListener(listener, gyro, SensorManager.SENSOR_DELAY_GAME)
        onDispose { manager.unregisterListener(listener) }
    }
}
