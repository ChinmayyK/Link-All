package app.linkall

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import android.widget.Toast

/**
 * Whether Android lets Link All keep running in the background, and how the
 * user turns that on.
 *
 * Without it, battery savers pause the app once the screen goes off. Some
 * phones are fast about it: ColorOS (OnePlus, Oppo, Realme) freezes the app
 * and closes its connections about ten seconds after screen-off, so every
 * paired device drops until the phone is unlocked. Every phone's switch for
 * this puts the app on Android's battery-optimisation allow list, which is
 * what [isAllowed] reads.
 */
object BackgroundAccess {
    /** Set once the guide has been shown by itself, after the first pairing. */
    const val PREF_GUIDE_SHOWN = "background_guide_shown"
    /** When the "your phone paused Link All" notification last went out. */
    const val PREF_PAUSED_NOTICE_AT = "background_paused_notice_at"
    /** MainActivity opens the guide when started with this extra. */
    const val EXTRA_SHOW_GUIDE = "show_background_guide"

    fun isAllowed(context: Context): Boolean {
        val pm = context.getSystemService(Context.POWER_SERVICE) as PowerManager
        return pm.isIgnoringBatteryOptimizations(context.packageName)
    }

    /**
     * What to tap once [open] has run. The full build asks Android directly,
     * so there is one step. Google Play doesn't allow that request, so the
     * Play build opens Link All's app info page, and each brand names the
     * switch differently.
     */
    fun steps(): List<String> {
        if (BuildConfig.FULL_PERMISSIONS) return listOf("Tap Allow when your phone asks")
        return when (Build.MANUFACTURER.lowercase()) {
            "oneplus", "oppo", "realme" -> listOf(
                "Tap Battery usage",
                "Choose Allow background activity",
                "Tap Allow to confirm",
            )
            "samsung" -> listOf(
                "Tap Battery",
                "Choose Unrestricted",
            )
            "xiaomi", "redmi", "poco" -> listOf(
                "Turn on Autostart",
                "Tap Battery saver",
                "Choose No restrictions",
            )
            else -> listOf(
                "Tap App battery usage (or Battery)",
                "Choose Unrestricted, or turn on Allow background usage",
            )
        }
    }

    /** Opens the page where the user allows background running. */
    fun open(activity: Activity) {
        val intent = if (BuildConfig.FULL_PERMISSIONS) {
            Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:${activity.packageName}"))
        } else {
            // Link All's own app page is where every phone keeps this switch
            // (OnePlus and Oppo hide the generic list).
            Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${activity.packageName}"))
        }
        runCatching { activity.startActivity(intent) }
            .recoverCatching { activity.startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)) }
            .onFailure {
                Toast.makeText(activity, "Open Settings › Apps › Link All › Battery and allow background use", Toast.LENGTH_LONG).show()
            }
    }
}
