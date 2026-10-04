package com.deskdrop

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import android.util.Log

/**
 * Starts Deskdrop automatically after device boot.
 *
 * Also handles:
 *   - MY_PACKAGE_REPLACED — restart after app update
 *   - LOCKED_BOOT_COMPLETED — for Android 7+ direct-boot compatibility
 *
 * Requires RECEIVE_BOOT_COMPLETED permission in AndroidManifest.xml.
 * Service is only started if the user had sync enabled before the reboot.
 */
class BootReceiver : BroadcastReceiver() {

    companion object {
        private const val TAG = "DeskdropBoot"
    }

    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            Intent.ACTION_BOOT_COMPLETED,
            Intent.ACTION_LOCKED_BOOT_COMPLETED,
            Intent.ACTION_MY_PACKAGE_REPLACED -> {
                val prefs = context.getSharedPreferences(DeskdropService.PREFS_NAME, Context.MODE_PRIVATE)

                // Respect user setting — don't auto-start if they stopped it intentionally
                val syncEnabled = prefs.getBoolean("sync_enabled", true)
                if (!syncEnabled) {
                    Log.i(TAG, "Boot received but sync_enabled=false — not auto-starting")
                    return
                }

                Log.i(TAG, "Boot received (${intent.action}) — starting Link All")
                startService(context)
            }
        }
    }

    private fun startService(context: Context) {
        runCatching {
            val serviceIntent = Intent(context, DeskdropService::class.java).apply {
                action = DeskdropService.ACTION_START
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(serviceIntent)
            } else {
                context.startService(serviceIntent)
            }
        }.onFailure { ex ->
            Log.e(TAG, "Failed to start Link All service at boot", ex)
        }
    }
}
