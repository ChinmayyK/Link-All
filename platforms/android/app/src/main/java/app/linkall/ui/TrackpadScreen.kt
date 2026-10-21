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
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Mouse
import androidx.compose.material3.Icon
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
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
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import java.util.concurrent.Executors
import kotlin.math.abs
import kotlin.math.hypot
import kotlin.math.roundToInt

/** Pixel-like units of scroll per pixel of finger travel. */
private const val SCROLL_GAIN = 1.4f

/** Pointer pixels per radian of turn in air-mouse mode. */
private const val AIR_GAIN = 900f

private const val TAP_MAX_MS = 260L
private const val TAP_MAX_TRAVEL_PX = 16f

/** A second touch this soon after a tap, held and dragged, drags (press and hold the left button). */
private const val DRAG_WINDOW_MS = 300L

/** Finger travel that counts as a three-finger swipe, and pinch spread that counts as one zoom step. */
private const val SWIPE_PX = 140f
private const val PINCH_STEP_PX = 70f

/** Everything a mouse has, as the trackpad sends it. Names match the core's. */
private val SHORTCUTS = listOf(
    "Copy" to "copy", "Paste" to "paste", "Cut" to "cut", "Undo" to "undo", "Redo" to "redo",
    "Select all" to "select_all", "Find" to "find", "Save" to "save",
    "New tab" to "new_tab", "Close tab" to "close_tab", "Next tab" to "next_tab", "Prev tab" to "previous_tab",
    "Back" to "back", "Forward" to "forward", "Reload" to "reload",
    "Zoom in" to "zoom_in", "Zoom out" to "zoom_out", "Zoom 100%" to "zoom_reset",
    "Switch app" to "switch_app", "Overview" to "overview", "Show desktop" to "show_desktop",
    "Desktop ←" to "desktop_left", "Desktop →" to "desktop_right",
)

private val KEYS = listOf(
    "Esc" to "escape", "Tab" to "tab", "←" to "left", "↑" to "up", "↓" to "down", "→" to "right",
    "Home" to "home", "End" to "end", "PgUp" to "page_up", "PgDn" to "page_down", "Del" to "delete",
    "Bksp" to "backspace", "Enter" to "enter",
)

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
    fun shortcut(name: String) = send { LinkAllJni.sendShortcut(it, peerId, name) }
    fun modifier(name: String, down: Boolean) = send { LinkAllJni.sendModifier(it, peerId, name, down) }

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

/** What the pad needs to know while a finger moves; the settings are read live. */
private class PadSettings(var sensitivity: Float, var natural: Boolean, var dragLock: Boolean)

/**
 * The phone as a mouse and keyboard for [peer]. One finger moves the pointer
 * and a tap clicks; tap, then touch again and drag, to drag. Two fingers
 * scroll (with momentum), pinch to zoom, and tap with two to right-click.
 * Three fingers swipe between desktops and windows. The strip at the right
 * edge scrolls with one finger. There are buttons for left, middle, right,
 * back and forward, held modifiers, shortcuts, keys and typing.
 */
@Composable
internal fun TrackpadDialog(c: DdColors, peer: PeerSnapshot, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val prefs = remember { context.getSharedPreferences("linkall", Context.MODE_PRIVATE) }
    val sender = remember(peer.id) { PadSender(peer.id, context.applicationContext) }
    val scope = rememberCoroutineScope()
    val settings = remember {
        PadSettings(
            sensitivity = prefs.getFloat("trackpad_sensitivity", 1.8f),
            natural = prefs.getBoolean("trackpad_natural", true),
            dragLock = false,
        )
    }
    var air by remember { mutableStateOf(false) }
    var natural by remember { mutableStateOf(settings.natural) }
    var sensitivity by remember { mutableStateOf(settings.sensitivity) }
    var dragLock by remember { mutableStateOf(false) }
    var typed by remember { mutableStateOf("") }
    val held = remember { mutableStateOf(setOf<String>()) }
    var showFunctionKeys by remember { mutableStateOf(false) }
    val meta = if (peer.platform == "macOS") "Cmd" else "Win"

    // Whatever is held when the pad closes is let go, and so is a held mouse button.
    DisposableEffect(sender) {
        onDispose {
            held.value.forEach { sender.modifier(it, false) }
            if (settings.dragLock) sender.button(0, false)
            sender.close()
        }
    }

    AirMouse(enabled = air, sender = sender)

    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Column(
            Modifier
                .fillMaxSize()
                .background(c.surface)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = PageGutter, vertical = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Outlined.Mouse, contentDescription = null, tint = c.accent, modifier = Modifier.size(22.dp))
                Spacer(Modifier.size(10.dp))
                Column(Modifier.weight(1f)) {
                    Text("Mouse · ${peer.name}", style = DdType.title, color = c.text, maxLines = 1)
                    Text(
                        if (air) "Point the phone and tap Left click" else "Drag · tap · two fingers scroll · pinch zooms · three swipe",
                        style = DdType.small, color = c.textMuted, maxLines = 1,
                    )
                }
                Icon(
                    Icons.Outlined.Close, contentDescription = "Close", tint = c.text,
                    modifier = Modifier.size(28.dp).clickable(onClick = onDismiss),
                )
            }

            // The pad and, beside it, the strip that scrolls with one finger.
            val padShape = RoundedCornerShape(22.dp)
            Row(Modifier.fillMaxWidth().height(300.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Box(
                    Modifier
                        .weight(1f)
                        .fillMaxHeight()
                        .clip(padShape)
                        .background(c.surfaceSunk)
                        .border(1.dp, c.line, padShape)
                        .trackpadGestures(sender, settings, scope, pointerMoves = !air),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(if (air) "Scroll with two fingers" else "Touch here", style = DdType.small, color = c.textMuted)
                }
                Box(
                    Modifier
                        .width(44.dp)
                        .fillMaxHeight()
                        .clip(padShape)
                        .background(c.surfaceSunk)
                        .border(1.dp, c.line, padShape)
                        .scrollStrip(sender, settings, scope),
                    contentAlignment = Alignment.Center,
                ) {
                    Text("⇅", style = DdType.title, color = c.textMuted)
                }
            }

            // The mouse's own buttons.
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                PillButton(c, "Left", filled = true, compact = true, modifier = Modifier.weight(1f)) { sender.click(0) }
                PillButton(c, "Middle", filled = false, compact = true, modifier = Modifier.weight(1f)) { sender.click(2) }
                PillButton(c, "Right", filled = false, compact = true, modifier = Modifier.weight(1f)) { sender.click(1) }
                PillButton(c, "Back", filled = false, compact = true, modifier = Modifier.weight(1f)) { sender.click(3) }
                PillButton(c, "Fwd", filled = false, compact = true, modifier = Modifier.weight(1f)) { sender.click(4) }
            }
            // Hold the left button, to select text or drag a window, until it is switched off.
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Hold left button", style = DdType.body, color = c.text)
                    Text("Select or drag with one finger", style = DdType.small, color = c.textMuted)
                }
                Switch(checked = dragLock, onCheckedChange = {
                    dragLock = it
                    settings.dragLock = it
                    sender.button(0, it)
                })
            }

            // Held modifiers: Shift, Ctrl, Alt and the Windows or Command key.
            Text("Hold a key while you click or type", style = DdType.small, color = c.textMuted)
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                for ((label, name) in listOf("Shift" to "shift", "Ctrl" to "ctrl", "Alt" to "alt", meta to "meta")) {
                    val on = name in held.value
                    PillButton(c, label, filled = on, compact = true, modifier = Modifier.weight(1f)) {
                        held.value = if (on) held.value - name else held.value + name
                        sender.modifier(name, !on)
                    }
                }
            }

            Text("Shortcuts", style = DdType.small, color = c.textMuted)
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                for ((label, name) in SHORTCUTS) {
                    PillButton(c, label, filled = false, compact = true) { sender.shortcut(name) }
                }
            }

            Text("Keys", style = DdType.small, color = c.textMuted)
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                for ((label, key) in KEYS) {
                    PillButton(c, label, filled = false, compact = true) { sender.key(key) }
                }
                PillButton(c, "F1–F12", filled = showFunctionKeys, compact = true) { showFunctionKeys = !showFunctionKeys }
            }
            if (showFunctionKeys) {
                Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    for (n in 1..12) PillButton(c, "F$n", filled = false, compact = true) { sender.key("f$n") }
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

            Text("Settings", style = DdType.small, color = c.textMuted)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Air mouse (point with the phone)", style = DdType.body, color = c.text, modifier = Modifier.weight(1f))
                Switch(checked = air, onCheckedChange = { air = it })
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Natural scrolling", style = DdType.body, color = c.text)
                    Text("Content follows your fingers", style = DdType.small, color = c.textMuted)
                }
                Switch(checked = natural, onCheckedChange = {
                    natural = it
                    settings.natural = it
                    prefs.edit().putBoolean("trackpad_natural", it).apply()
                })
            }
            Column {
                Text("Pointer speed", style = DdType.body, color = c.text)
                Slider(
                    value = sensitivity,
                    onValueChange = {
                        sensitivity = it
                        settings.sensitivity = it
                    },
                    onValueChangeFinished = { prefs.edit().putFloat("trackpad_sensitivity", sensitivity).apply() },
                    valueRange = 0.8f..4f,
                )
            }
            Spacer(Modifier.height(8.dp))
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

/** The scroll steps a finger lift keeps going with, easing out like a flicked wheel. */
private fun fling(scope: CoroutineScope, vx: Float, vy: Float, send: (Int, Int) -> Unit): Job = scope.launch {
    var x = vx
    var y = vy
    var carryX = 0f
    var carryY = 0f
    while (isActive && (abs(x) > 0.15f || abs(y) > 0.15f)) {
        carryX += x
        carryY += y
        val sx = carryX.roundToInt()
        val sy = carryY.roundToInt()
        carryX -= sx
        carryY -= sy
        if (sx != 0 || sy != 0) send(sx, sy)
        x *= 0.94f
        y *= 0.94f
        delay(16)
    }
}

/** One finger vertical on the strip at the pad's edge: scroll, with momentum. */
private fun Modifier.scrollStrip(sender: PadSender, settings: PadSettings, scope: CoroutineScope): Modifier =
    pointerInput(sender) {
        var momentum: Job? = null
        awaitEachGesture {
            momentum?.cancel()
            val down = awaitFirstDown(requireUnconsumed = false)
            var last = down.position
            var velocity = 0f
            var carry = 0f
            while (true) {
                val event = awaitPointerEvent(PointerEventPass.Main)
                val change = event.changes.firstOrNull() ?: break
                if (!change.pressed) break
                val dy = change.position.y - last.y
                last = change.position
                velocity = dy * 0.6f + velocity * 0.4f
                carry += dy * SCROLL_GAIN * 2f
                val step = carry.roundToInt()
                carry -= step
                if (step != 0) sender.scroll(0, if (settings.natural) -step else step)
                change.consume()
            }
            val v = velocity * SCROLL_GAIN * 2f
            if (abs(v) > 3f) {
                momentum = fling(scope, 0f, if (settings.natural) -v else v) { x, y -> sender.scroll(x, y) }
            }
        }
    }

/**
 * One finger drags the pointer and a short still touch clicks; touch again soon
 * after a tap and drag to drag. Two fingers scroll (with momentum) or, when they
 * spread or pinch, zoom; a short still two-finger touch right-clicks. Three
 * fingers swipe for the overview, the desktop and between desktops, and tap to
 * middle-click. With [pointerMoves] off (air mouse) one finger doesn't move the
 * pointer but a tap still clicks.
 */
private fun Modifier.trackpadGestures(
    sender: PadSender,
    settings: PadSettings,
    scope: CoroutineScope,
    pointerMoves: Boolean,
): Modifier = pointerInput(sender, pointerMoves) {
    var lastTapUpAt = 0L
    var momentum: Job? = null
    awaitEachGesture {
        momentum?.cancel()
        val first = awaitFirstDown(requireUnconsumed = false)
        val startedAt = SystemClock.uptimeMillis()
        // Touching again right after a tap and dragging holds the left button.
        val dragging = !settings.dragLock && startedAt - lastTapUpAt < DRAG_WINDOW_MS
        var dragStarted = false
        var maxFingers = 1
        var travel = 0f
        var previousCount = 1
        var lastCentre = first.position
        var startCentre = first.position
        var lastSpread = 0f
        var spreadCarry = 0f
        var carryX = 0f
        var carryY = 0f
        var vx = 0f
        var vy = 0f
        while (true) {
            val event = awaitPointerEvent(PointerEventPass.Main)
            val down = event.changes.filter { it.pressed }
            if (down.isEmpty()) break
            if (down.size > maxFingers) maxFingers = down.size
            val centre = down.map { it.position }.reduce { a, b -> a + b } / down.size.toFloat()
            val spread = if (down.size == 2) hypot(down[0].position.x - down[1].position.x, down[0].position.y - down[1].position.y) else 0f
            if (down.size != previousCount) {
                lastCentre = centre
                lastSpread = spread
                if (down.size >= 3) startCentre = centre
            } else {
                val delta = centre - lastCentre
                travel += abs(delta.x) + abs(delta.y)
                when (down.size) {
                    1 -> if (pointerMoves) {
                        if (dragging && !dragStarted && travel > TAP_MAX_TRAVEL_PX / 2) {
                            dragStarted = true
                            sender.button(0, true)
                        }
                        carryX += delta.x * settings.sensitivity
                        carryY += delta.y * settings.sensitivity
                        val sx = carryX.roundToInt()
                        val sy = carryY.roundToInt()
                        carryX -= sx
                        carryY -= sy
                        if (sx != 0 || sy != 0) sender.move(sx, sy)
                    }
                    2 -> {
                        val spreadChange = spread - lastSpread
                        // Fingers moving apart or together more than they travel: a pinch.
                        if (abs(spreadChange) > abs(delta.x) + abs(delta.y)) {
                            spreadCarry += spreadChange
                            if (abs(spreadCarry) > PINCH_STEP_PX) {
                                sender.shortcut(if (spreadCarry > 0) "zoom_in" else "zoom_out")
                                spreadCarry = 0f
                            }
                        } else {
                            val sign = if (settings.natural) -1 else 1
                            carryX += delta.x * SCROLL_GAIN * sign
                            carryY += delta.y * SCROLL_GAIN * sign
                            vx = delta.x * SCROLL_GAIN * sign * 0.6f + vx * 0.4f
                            vy = delta.y * SCROLL_GAIN * sign * 0.6f + vy * 0.4f
                            val sx = carryX.roundToInt()
                            val sy = carryY.roundToInt()
                            carryX -= sx
                            carryY -= sy
                            if (sx != 0 || sy != 0) sender.scroll(sx, sy)
                        }
                        lastSpread = spread
                    }
                    else -> Unit
                }
            }
            previousCount = down.size
            lastCentre = centre
            event.changes.forEach { it.consume() }
        }
        val held = SystemClock.uptimeMillis() - startedAt
        if (dragStarted) sender.button(0, false)
        val tapped = held <= TAP_MAX_MS && travel <= TAP_MAX_TRAVEL_PX
        when {
            maxFingers >= 3 && !tapped -> {
                val d: Offset = lastCentre - startCentre
                if (abs(d.x) > SWIPE_PX || abs(d.y) > SWIPE_PX) {
                    sender.shortcut(
                        when {
                            abs(d.y) >= abs(d.x) && d.y < 0 -> "overview"
                            abs(d.y) >= abs(d.x) -> "show_desktop"
                            d.x < 0 -> "desktop_right"
                            else -> "desktop_left"
                        }
                    )
                }
            }
            tapped -> {
                if (!settings.dragLock) {
                    sender.click(
                        when (maxFingers) {
                            1 -> 0
                            2 -> 1
                            else -> 2
                        }
                    )
                }
                lastTapUpAt = if (maxFingers == 1) SystemClock.uptimeMillis() else 0L
            }
            maxFingers == 2 && (abs(vx) > 3f || abs(vy) > 3f) -> {
                momentum = fling(scope, vx, vy) { x, y -> sender.scroll(x, y) }
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
