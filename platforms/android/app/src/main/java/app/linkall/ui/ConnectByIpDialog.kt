package app.linkall.ui

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.Lan
import androidx.compose.material.icons.outlined.PhoneAndroid
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.ContextCompat
import app.linkall.LinkAllService
import kotlinx.coroutines.delay

/** A parsed "host[:port]" the user typed, or why it can't be used. */
internal sealed interface ManualAddress {
    data class Valid(val host: String, val port: Int) : ManualAddress
    data class Invalid(val reason: String) : ManualAddress
}

private val HostnamePattern = Regex("""^(?=.{1,253}$)[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?(\.[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*$""")
private val Ipv4Pattern = Regex("""^\d{1,3}(\.\d{1,3}){3}$""")

internal fun parseManualAddress(raw: String): ManualAddress {
    val input = raw.trim()
    if (input.isEmpty()) return ManualAddress.Invalid("Enter the other device's IP address")

    // "[v6]:port", bare v6 (two or more colons), or "host:port".
    val (host, portText) = when {
        input.startsWith("[") -> {
            val end = input.indexOf(']')
            if (end < 0) return ManualAddress.Invalid("Missing ] after the IPv6 address")
            val rest = input.substring(end + 1)
            if (rest.isNotEmpty() && !rest.startsWith(":")) return ManualAddress.Invalid("Use [address]:port")
            input.substring(1, end) to rest.removePrefix(":").ifEmpty { null }
        }
        input.count { it == ':' } >= 2 -> input to null
        input.contains(':') -> input.substringBefore(':') to input.substringAfter(':')
        else -> input to null
    }

    val port = when (portText) {
        null -> LinkAllService.DEFAULT_LINKALL_PORT
        else -> portText.toIntOrNull()?.takeIf { it in 1..65535 }
            ?: return ManualAddress.Invalid("Port must be a number from 1 to 65535")
    }

    val isV6 = host.contains(':')
    val validHost = when {
        isV6 -> host.all { it.isLetterOrDigit() || it == ':' || it == '.' }
        Ipv4Pattern.matches(host) -> host.split('.').all { (it.toIntOrNull() ?: 256) <= 255 }
        else -> HostnamePattern.matches(host)
    }
    if (!validHost) {
        return ManualAddress.Invalid(
            if (Ipv4Pattern.matches(host)) "Each part of an IP address must be 0 to 255"
            else "That doesn't look like an IP address or hostname"
        )
    }
    return ManualAddress.Valid(host, port)
}

private fun ManualAddress.Valid.label(): String {
    val h = if (host.contains(':')) "[$host]" else host
    return if (port == LinkAllService.DEFAULT_LINKALL_PORT) h else "$h:$port"
}

private sealed interface ConnectState {
    data object Editing : ConnectState
    data class Connecting(val address: ManualAddress.Valid) : ConnectState
    data class Failed(val message: String) : ConnectState
}

// connect_once has its own socket timeouts, but if the service is dead or
// the engine never answers the dialog must not spin forever.
private const val CONNECT_TIMEOUT_MS = 20_000L

/**
 * Direct IP connect for networks where mDNS is blocked (guest Wi-Fi,
 * client isolation, VPNs). The service resolves the host, connects, and
 * answers with ACTION_MANUAL_CONNECT_RESULT; a new, untrusted peer then goes
 * through the normal pairing prompt.
 */
@Composable
internal fun ConnectByIpDialog(
    isDark: Boolean,
    onDismiss: () -> Unit,
    onConnected: (label: String) -> Unit,
    onPairingLink: (String) -> Boolean,
) {
    val c = rememberDdColors(isDark)
    val context = LocalContext.current
    val ownIp = remember { getLocalIpAddress() }
    val recent = remember { LinkAllService.recentManualAddresses(context) }

    var input by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var state by remember { mutableStateOf<ConnectState>(ConnectState.Editing) }
    val focus = remember { FocusRequester() }

    val connecting = state as? ConnectState.Connecting

    // Result from the service for the address we asked for.
    DisposableEffect(connecting) {
        if (connecting == null) return@DisposableEffect onDispose { }
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(ctx: Context?, intent: Intent?) {
                if (intent?.getStringExtra(LinkAllService.EXTRA_MANUAL_HOST) != connecting.address.host) return
                if (intent.getBooleanExtra(LinkAllService.EXTRA_MANUAL_OK, false)) {
                    onConnected(connecting.address.label())
                } else {
                    state = ConnectState.Failed(
                        intent.getStringExtra(LinkAllService.EXTRA_MANUAL_ERROR)
                            ?: "Couldn't reach ${connecting.address.label()}."
                    )
                }
            }
        }
        ContextCompat.registerReceiver(
            context, receiver,
            IntentFilter(LinkAllService.ACTION_MANUAL_CONNECT_RESULT),
            ContextCompat.RECEIVER_NOT_EXPORTED
        )
        onDispose { runCatching { context.unregisterReceiver(receiver) } }
    }

    LaunchedEffect(connecting) {
        if (connecting == null) return@LaunchedEffect
        delay(CONNECT_TIMEOUT_MS)
        state = ConnectState.Failed("No answer from ${connecting.address.label()}. Make sure Link All is open on that device.")
    }

    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }

    fun submit(text: String = input) {
        if (text.trim().startsWith("linkall://")) {
            if (onPairingLink(text)) onDismiss() else error = "That pairing link isn't valid"
            return
        }
        when (val parsed = parseManualAddress(text)) {
            is ManualAddress.Invalid -> error = parsed.reason
            is ManualAddress.Valid -> {
                error = null
                state = ConnectState.Connecting(parsed)
                runCatching {
                    ContextCompat.startForegroundService(context, Intent(context, LinkAllService::class.java).apply {
                        action = LinkAllService.ACTION_CONNECT_MANUAL
                        putExtra("ip", parsed.host)
                        putExtra("port", parsed.port)
                    })
                }.onFailure { state = ConnectState.Failed("Link All's background service couldn't start.") }
            }
        }
    }

    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Column(
            Modifier
                .padding(horizontal = PageGutter)
                .widthIn(max = 480.dp)
                .fillMaxWidth()
                .clip(PanelShape)
                .background(c.surface)
                .border(1.dp, c.line, PanelShape)
                .padding(20.dp)
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                IconWell(c, Icons.Outlined.Lan, tint = c.accent, background = c.accentSoft)
                Spacer(Modifier.width(14.dp))
                Column {
                    Text("Connect by IP", style = DdType.title, color = c.text)
                    Text("For networks where devices can't find each other", style = DdType.small, color = c.textMuted)
                }
            }

            Spacer(Modifier.height(20.dp))

            val fieldShape = RoundedCornerShape(16.dp)
            val borderColor = when {
                error != null || state is ConnectState.Failed -> c.danger
                else -> c.line
            }
            BasicTextField(
                value = input,
                onValueChange = {
                    input = it
                    error = null
                    if (state is ConnectState.Failed) state = ConnectState.Editing
                },
                enabled = connecting == null,
                singleLine = true,
                textStyle = DdType.body.copy(color = c.text),
                cursorBrush = SolidColor(c.accent),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, autoCorrect = false, imeAction = ImeAction.Go),
                keyboardActions = KeyboardActions(onGo = { submit() }),
                modifier = Modifier.fillMaxWidth().focusRequester(focus),
                decorationBox = { inner ->
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .clip(fieldShape)
                            .background(c.surfaceSunk)
                            .border(1.dp, borderColor, fieldShape)
                            .padding(horizontal = 16.dp, vertical = 14.dp)
                    ) {
                        if (input.isEmpty()) {
                            Text("192.168.1.50 or 192.168.1.50:${LinkAllService.DEFAULT_LINKALL_PORT}", style = DdType.body, color = c.textMuted)
                        }
                        inner()
                    }
                }
            )

            val message = error ?: (state as? ConnectState.Failed)?.message
            if (message != null) {
                Spacer(Modifier.height(8.dp))
                Text(message, style = DdType.small, color = c.danger)
            }

            if (recent.isNotEmpty() && connecting == null) {
                Spacer(Modifier.height(16.dp))
                Text("Recent", style = DdType.small, color = c.textMuted)
                Spacer(Modifier.height(8.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    recent.take(3).forEach { address ->
                        Row(
                            Modifier
                                .clip(CircleShape)
                                .background(c.surfaceSunk)
                                .clickable { input = address; submit(address) }
                                .padding(horizontal = 12.dp, vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Icon(Icons.Outlined.History, contentDescription = null, tint = c.textMuted, modifier = Modifier.size(14.dp))
                            Spacer(Modifier.width(6.dp))
                            Text(address, style = DdType.small, color = c.text, maxLines = 1)
                        }
                    }
                }
            }

            Spacer(Modifier.height(16.dp))
            Row(
                Modifier
                    .fillMaxWidth()
                    .clip(WellShape)
                    .background(c.accentSoft)
                    .padding(12.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Icon(Icons.Outlined.PhoneAndroid, contentDescription = null, tint = c.accent, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(10.dp))
                Column {
                    Text("This phone's address", style = DdType.small, color = c.textMuted)
                    Text("$ownIp:${LinkAllService.DEFAULT_LINKALL_PORT}", style = DdType.label, color = c.text)
                }
            }

            Spacer(Modifier.height(20.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                PillButton(c, "Cancel", filled = false, modifier = Modifier.weight(1f), onClick = onDismiss)
                if (connecting != null) {
                    Row(
                        Modifier.weight(1f).clip(CircleShape).background(c.accent).padding(vertical = 14.dp),
                        horizontalArrangement = Arrangement.Center,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        CircularProgressIndicator(color = c.onAccent, strokeWidth = 2.dp, modifier = Modifier.size(16.dp))
                        Spacer(Modifier.width(10.dp))
                        Text("Connecting", style = DdType.label, color = c.onAccent)
                    }
                } else {
                    PillButton(
                        c,
                        if (state is ConnectState.Failed) "Try again" else "Connect",
                        filled = true,
                        modifier = Modifier.weight(1f),
                        onClick = { submit() }
                    )
                }
            }
        }
    }
}
