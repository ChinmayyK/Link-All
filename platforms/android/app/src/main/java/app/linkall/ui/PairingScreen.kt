package app.linkall.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay

/** Matches the core's PAIRING_TIMEOUT. */
private const val PAIRING_WINDOW_MS = 60_000L

/**
 * Full-screen pairing prompt raised by the service. The responder compares
 * the code and accepts; the initiator only waits. The request lives for
 * PAIRING_WINDOW_MS; when it runs out the screen closes without answering.
 */
@Composable
fun PairingScreen(
    isDark: Boolean,
    deviceName: String,
    pin: String,
    fingerprint: String,
    isInitiator: Boolean = false,
    onApprove: () -> Unit,
    onDeny: () -> Unit,
    onExpire: () -> Unit
) {
    val c = rememberDdColors(isDark)
    val haptic = LocalHapticFeedback.current
    var remainingMs by remember { mutableLongStateOf(PAIRING_WINDOW_MS) }

    LaunchedEffect(Unit) {
        while (remainingMs > 0) {
            delay(100)
            remainingMs -= 100
        }
        haptic.performHapticFeedback(HapticFeedbackType.LongPress)
        onExpire()
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(c.page)
            .systemBarsPadding()
            .padding(horizontal = PageGutter)
    ) {
        Spacer(Modifier.height(48.dp))
        Text(if (isInitiator) "Waiting for approval" else "Pairing request", style = DdType.label, color = c.accent)
        Spacer(Modifier.height(8.dp))
        Text(
            if (isInitiator) "Accept on $deviceName" else "$deviceName wants to link",
            style = DdType.display,
            color = c.text,
            maxLines = 3
        )
        Spacer(Modifier.height(10.dp))
        Text(
            if (isInitiator) "Check that $deviceName shows this same code, then accept there."
            else "Only accept if $deviceName shows this same code.",
            style = DdType.body,
            color = c.textMuted
        )
        Spacer(Modifier.height(32.dp))
        PinTiles(c, pin)

        // The code is what people compare; the full key is there for anyone
        // who wants to check it.
        if (fingerprint.isNotBlank()) {
            var showDetails by remember { mutableStateOf(false) }
            Spacer(Modifier.height(20.dp))
            Text(
                if (showDetails) "Hide details" else "Show details",
                style = DdType.small,
                color = c.accent,
                modifier = Modifier
                    .clickable { showDetails = !showDetails }
                    .padding(vertical = 6.dp)
            )
            if (showDetails) {
                Text("Device fingerprint", style = DdType.small, color = c.textMuted)
                Text(
                    fingerprint.replace(":", "").chunked(4).joinToString(" "),
                    style = DdType.mono,
                    color = c.textMuted
                )
            }
        }

        Spacer(Modifier.weight(1f))
        LinearProgressIndicator(
            progress = { (remainingMs.toFloat() / PAIRING_WINDOW_MS).coerceIn(0f, 1f) },
            modifier = Modifier.fillMaxWidth().height(2.dp).clip(CircleShape),
            color = c.accent,
            trackColor = c.surfaceSunk
        )
        Spacer(Modifier.height(8.dp))
        Text("Expires in ${(remainingMs + 999) / 1000}s", style = DdType.small, color = c.textMuted)
        Spacer(Modifier.height(20.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            PillButton(c, if (isInitiator) "Cancel" else "Decline", filled = false, modifier = Modifier.weight(1f), onClick = onDeny)
            if (!isInitiator) {
                PillButton(c, "Accept", filled = true, modifier = Modifier.weight(1f), onClick = onApprove)
            }
        }
        Spacer(Modifier.height(24.dp))
    }
}
