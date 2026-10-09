package app.linkall

import android.content.SharedPreferences
import org.json.JSONArray

const val PREF_HEALTH_JSON = "health_json"

/**
 * Something stopping sync, from the engine (see Engine::health) or from a
 * check only Android can make. `kind` picks the fix the banner offers.
 */
data class HealthIssue(val kind: String, val title: String, val detail: String, val deviceId: String?)

fun parseHealthIssues(raw: String?): List<HealthIssue> {
    if (raw.isNullOrBlank()) return emptyList()
    return runCatching {
        val array = JSONArray(raw)
        (0 until array.length()).mapNotNull { i ->
            val obj = array.optJSONObject(i) ?: return@mapNotNull null
            HealthIssue(
                kind = obj.optString("kind"),
                title = obj.optString("title"),
                detail = obj.optString("detail"),
                deviceId = if (obj.isNull("device_id")) null else obj.optString("device_id")
            )
        }
    }.getOrDefault(emptyList())
}

fun SharedPreferences.healthIssues(): List<HealthIssue> = parseHealthIssues(getString(PREF_HEALTH_JSON, null))
