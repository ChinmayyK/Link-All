package app.linkall

import android.content.Context
import org.json.JSONObject

/** What an action is for; picks the rule for which devices can be chosen. */
enum class ChoicePurpose(val wire: String) {
    Send("send"),
    Remote("remote"),
    Browse("browse"),
    SyncTarget("sync_target"),
}

/** One device in a chooser, as the core ranked it. */
data class DeviceChoice(
    val id: String,
    val name: String,
    val platform: String?,
    val available: Boolean,
    /** Why it can't be chosen right now; null when it can. */
    val reason: String?,
    val lastUsed: Boolean,
)

/** The core's answer: devices best first, and whether "All devices" is offered. */
data class DeviceChoices(val offersAll: Boolean, val choices: List<DeviceChoice>) {
    val available: List<DeviceChoice> get() = choices.filter { it.available }
    val unavailable: List<DeviceChoice> get() = choices.filterNot { it.available }
}

object DeviceChooser {
    private const val PREF_PREFIX = "last_device_"

    /** The device the user chose last for [purpose]. */
    fun lastUsed(context: Context, purpose: ChoicePurpose): String? =
        context.getSharedPreferences(LinkAllService.PREFS_NAME, Context.MODE_PRIVATE).getString(PREF_PREFIX + purpose.wire, null)

    fun remember(context: Context, purpose: ChoicePurpose, deviceId: String) {
        context.getSharedPreferences(LinkAllService.PREFS_NAME, Context.MODE_PRIVATE).edit().putString(PREF_PREFIX + purpose.wire, deviceId).apply()
    }

    /** Asks the engine. Blocks briefly, so call it off the main thread. Null when Link All isn't running. */
    fun load(context: Context, purpose: ChoicePurpose): DeviceChoices? {
        val last = lastUsed(context, purpose)
        val raw = LinkAllService.withLiveEngine { handle -> LinkAllJni.deviceChoicesJson(handle, purpose.wire, last) } ?: return null
        return parse(raw)
    }

    internal fun parse(raw: String): DeviceChoices? = runCatching {
        val root = JSONObject(raw)
        val array = root.getJSONArray("choices")
        DeviceChoices(
            offersAll = root.optBoolean("offers_all"),
            choices = (0 until array.length()).map { i ->
                val o = array.getJSONObject(i)
                DeviceChoice(
                    id = o.getString("id"),
                    name = o.getString("name"),
                    platform = if (o.isNull("platform")) null else o.optString("platform").ifBlank { null },
                    available = o.getBoolean("available"),
                    reason = if (o.isNull("reason")) null else o.optString("reason"),
                    lastUsed = o.optBoolean("last_used"),
                )
            },
        )
    }.getOrNull()
}
