package app.linkall

import android.content.Context
import android.content.SharedPreferences
import kotlinx.coroutines.flow.MutableStateFlow
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/**
 * Looks for a newer Link All on GitHub, at most once a day, and only tells the
 * user: it never installs anything. The Google Play build is updated by Play,
 * so it never checks. Failures are silent.
 */
object UpdateChecker {
    data class Update(val version: String, val url: String)

    const val PREF_ENABLED = "update_check_enabled"
    private const val PREF_LAST_CHECK = "update_last_check_ms"
    private const val PREF_LATEST_VERSION = "update_latest_version"
    private const val PREF_LATEST_URL = "update_latest_url"
    private const val EVERY_MS = 24L * 60 * 60 * 1000
    private const val LATEST = "https://api.github.com/repos/ChinmayyK/Link-All/releases/latest"

    /** The update to offer, or null. */
    val available = MutableStateFlow<Update?>(null)

    val supported: Boolean get() = BuildConfig.FULL_PERMISSIONS

    fun isEnabled(prefs: SharedPreferences) = supported && prefs.getBoolean(PREF_ENABLED, true)

    fun setEnabled(prefs: SharedPreferences, enabled: Boolean) {
        prefs.edit().putBoolean(PREF_ENABLED, enabled).apply()
        if (!enabled) available.value = null
    }

    /** Shows what the last check found, then checks again if a day has passed. */
    fun checkIfDue(context: Context) {
        val prefs = context.getSharedPreferences(LinkAllService.PREFS_NAME, Context.MODE_PRIVATE)
        if (!isEnabled(prefs)) return
        val current = currentVersion(context)
        val known = prefs.getString(PREF_LATEST_VERSION, null)
        val knownUrl = prefs.getString(PREF_LATEST_URL, null)
        available.value = if (known != null && knownUrl != null && isNewer(known, current)) Update(known, knownUrl) else null

        val now = System.currentTimeMillis()
        if (now - prefs.getLong(PREF_LAST_CHECK, 0L) < EVERY_MS) return
        Thread {
            val latest = runCatching { fetchLatest() }.getOrNull() ?: return@Thread
            prefs.edit()
                .putLong(PREF_LAST_CHECK, now)
                .putString(PREF_LATEST_VERSION, latest.version)
                .putString(PREF_LATEST_URL, latest.url)
                .apply()
            if (isEnabled(prefs)) available.value = if (isNewer(latest.version, current)) latest else null
        }.start()
    }

    private fun currentVersion(context: Context): String =
        runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull() ?: ""

    private fun fetchLatest(): Update? {
        val conn = URL(LATEST).openConnection() as HttpURLConnection
        try {
            conn.connectTimeout = 8_000
            conn.readTimeout = 8_000
            conn.setRequestProperty("Accept", "application/vnd.github+json")
            if (conn.responseCode != 200) return null
            return parse(conn.inputStream.bufferedReader().use { it.readText() })
        } finally {
            conn.disconnect()
        }
    }

    internal fun parse(json: String): Update? {
        val release = JSONObject(json)
        if (release.optBoolean("draft") || release.optBoolean("prerelease")) return null
        val version = release.optString("tag_name").removePrefix("v")
        val url = release.optString("html_url")
        // Only ever open a page on the project's own repository.
        if (version.isEmpty() || !url.startsWith("https://github.com/ChinmayyK/Link-All/")) return null
        return Update(version, url)
    }

    /** True when [latest] is a higher major.minor.patch than [current]. */
    internal fun isNewer(latest: String, current: String): Boolean {
        fun parts(v: String) = v.split('.', '-', '+').take(3).map { it.takeWhile(Char::isDigit).toIntOrNull() ?: 0 }
        val a = parts(latest)
        val b = parts(current)
        for (i in 0 until 3) {
            val x = a.getOrElse(i) { 0 }
            val y = b.getOrElse(i) { 0 }
            if (x != y) return x > y
        }
        return false
    }
}
