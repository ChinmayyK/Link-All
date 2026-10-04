package app.linkall.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import app.linkall.PeerSnapshot

/**
 * First run: find a computer, then show the pairing code while the other
 * side accepts. Finishes as soon as the chosen device trusts this phone.
 */
@Composable
fun OnboardingScreen(
    isDark: Boolean,
    peers: List<PeerSnapshot>,
    onConnectPeer: (PeerSnapshot) -> Unit,
    onCancelPairing: (PeerSnapshot) -> Unit,
    onSendSampleText: (PeerSnapshot) -> Unit,
    onScanQr: () -> Unit,
    onManualIp: () -> Unit,
    onComplete: () -> Unit
) {
    val c = rememberDdColors(isDark)
    val haptic = LocalHapticFeedback.current
    var selectedPeerId by remember { mutableStateOf<String?>(null) }
    val selectedPeer = peers.find { it.id == selectedPeerId }

    LaunchedEffect(selectedPeer?.trusted) {
        if (selectedPeer?.trusted == true) onComplete()
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(c.page)
            .systemBarsPadding()
    ) {
        AnimatedContent(
            targetState = selectedPeer != null,
            transitionSpec = { fadeIn(tween(220)) togetherWith fadeOut(tween(140)) },
            label = "onboarding"
        ) { pairing ->
            if (pairing && selectedPeer != null) {
                PairStep(
                    c,
                    selectedPeer,
                    onRetry = { onConnectPeer(selectedPeer) },
                    onBack = {
                        onCancelPairing(selectedPeer)
                        selectedPeerId = null
                    }
                )
            } else {
                FindStep(
                    c = c,
                    isDark = isDark,
                    peers = peers.filter { !it.trusted },
                    onScanQr = onScanQr,
                    onManualIp = onManualIp,
                    onPick = { peer ->
                        haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                        selectedPeerId = peer.id
                        onConnectPeer(peer)
                    }
                )
            }
        }
    }
}

@Composable
private fun FindStep(
    c: DdColors,
    isDark: Boolean,
    peers: List<PeerSnapshot>,
    onScanQr: () -> Unit,
    onManualIp: () -> Unit,
    onPick: (PeerSnapshot) -> Unit
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = PageGutter)
            .widthIn(max = 560.dp)
    ) {
        Spacer(Modifier.height(40.dp))
        Image(
            painter = painterResource(if (isDark) app.linkall.R.drawable.dark_logo else app.linkall.R.drawable.light_logo),
            contentDescription = null,
            modifier = Modifier.size(56.dp)
        )
        Spacer(Modifier.height(28.dp))
        Text("Link this phone to your computer", style = DdType.display, color = c.text)
        Spacer(Modifier.height(10.dp))
        Text(
            "One clipboard and instant file sharing between your devices, over your own network. No account, no cloud.",
            style = DdType.body,
            color = c.textMuted
        )
        Spacer(Modifier.height(28.dp))
        PillButton(c, "Scan QR code", filled = true, icon = Icons.Outlined.QrCodeScanner, modifier = Modifier.fillMaxWidth(), onClick = onScanQr)
        Spacer(Modifier.height(10.dp))
        PillButton(c, "Enter IP address", filled = false, icon = Icons.Outlined.Lan, modifier = Modifier.fillMaxWidth(), onClick = onManualIp)

        SectionHeader(c, "On your network", if (peers.isEmpty()) null else "Tap to pair")
        if (peers.isEmpty()) {
            EmptyBox(c, Icons.Outlined.Radar, "Looking for computers… Open Link All on your computer and keep both on the same Wi-Fi.") {
                Spacer(Modifier.height(16.dp))
                LinearProgressIndicator(
                    modifier = Modifier.fillMaxWidth().height(2.dp).clip(CircleShape),
                    color = c.accent,
                    trackColor = c.surfaceSunk
                )
            }
        } else {
            Panel(c) {
                peers.forEachIndexed { i, peer ->
                    if (i > 0) Hairline(c)
                    ListRow(
                        c,
                        osIcon(peer.platform, peer.name),
                        peer.name,
                        detail = peer.ip ?: "On your network",
                        iconTint = c.accent,
                        iconBackground = c.accentSoft,
                        onClick = { onPick(peer) },
                        trailing = { PillButton(c, "Pair", filled = true, compact = true) { onPick(peer) } }
                    )
                }
            }
        }
        Spacer(Modifier.height(40.dp))
    }
}

@Composable
private fun PairStep(c: DdColors, peer: PeerSnapshot, onRetry: () -> Unit, onBack: () -> Unit) {
    // Driven by the request itself: the core expires it after a minute and
    // says how it ended, so this screen never guesses with its own timer.
    val ended = !peer.outgoingPairingWaiting && !peer.trusted && peer.pairingOutcome != null
    val timedOut = ended && peer.pairingOutcome == "expired"
    val declined = ended && peer.pairingOutcome == "declined"
    val outdated = ended && peer.pairingOutcome == "update_needed"
    val pin = peer.pairingPin.takeIf { peer.outgoingPairingWaiting }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = PageGutter)
    ) {
        Spacer(Modifier.height(40.dp))
        Text(
            when {
                declined -> "${peer.name} declined"
                outdated -> "Update Link All on ${peer.name}"
                timedOut -> "No answer from ${peer.name}"
                pin != null -> "Check the code"
                else -> "Connecting to ${peer.name}"
            },
            style = DdType.display,
            color = c.text
        )
        Spacer(Modifier.height(10.dp))
        Text(
            when {
                declined -> "Ask again if that was a mistake."
                outdated -> "It runs an older Link All that turns pairing requests down on its own. Update it, then try again."
                timedOut -> "Make sure Link All is open on it and both devices are on the same Wi-Fi."
                pin != null -> "${peer.name} shows a pairing request with a code. Accept it there if it matches this one."
                else -> "A pairing request will appear on ${peer.name} in a moment."
            },
            style = DdType.body,
            color = c.textMuted
        )
        Spacer(Modifier.height(32.dp))

        when {
            pin != null -> {
                PinTiles(c, pin)
                Spacer(Modifier.height(16.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    androidx.compose.material3.Icon(Icons.Outlined.Lock, contentDescription = null, tint = c.textMuted, modifier = Modifier.size(14.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("Codes match only when nobody is in between.", style = DdType.small, color = c.textMuted)
                }
                PairingCountdown(c, peer)
            }
            ended -> PillButton(c, "Try again", filled = true, modifier = Modifier.fillMaxWidth(), onClick = onRetry)
            else -> LinearProgressIndicator(
                modifier = Modifier.fillMaxWidth().height(2.dp).clip(CircleShape),
                color = c.accent,
                trackColor = c.surfaceSunk
            )
        }

        Spacer(Modifier.height(24.dp))
        PillButton(c, if (ended) "Back" else "Cancel", filled = false, modifier = Modifier.fillMaxWidth(), onClick = onBack)
        Spacer(Modifier.height(40.dp))
    }
}
