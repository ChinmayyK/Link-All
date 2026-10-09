package app.linkall

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.telephony.TelephonyManager
import android.util.Log

class CallStateReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == TelephonyManager.ACTION_PHONE_STATE_CHANGED) {
            val stateStr = intent.getStringExtra(TelephonyManager.EXTRA_STATE)
            val number = intent.getStringExtra(TelephonyManager.EXTRA_INCOMING_NUMBER)
            
            Log.i("CallStateReceiver", "Received phone state change: state=$stateStr, hasNumber=${!number.isNullOrBlank()}")

            val serviceIntent = Intent(context, LinkAllService::class.java).apply {
                action = LinkAllService.ACTION_HANDLE_CALL_STATE
                putExtra(TelephonyManager.EXTRA_STATE, stateStr)
                if (number != null) {
                    putExtra(TelephonyManager.EXTRA_INCOMING_NUMBER, number)
                }
            }
            try {
                androidx.core.content.ContextCompat.startForegroundService(context, serviceIntent)
            } catch (e: Exception) {
                Log.e("CallStateReceiver", "Failed to forward call state to service", e)
            }
        }
    }
}
