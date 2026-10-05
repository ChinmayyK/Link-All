package app.linkall

import android.app.Notification
import android.content.Intent
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification
import android.util.Log

class LinkAllNotificationListener : NotificationListenerService() {
    companion object {
        private const val TAG = "LinkAllNotifListener"
        private val SKIPPED_CATEGORIES = setOf(
            Notification.CATEGORY_TRANSPORT,
            Notification.CATEGORY_PROGRESS,
            Notification.CATEGORY_NAVIGATION,
        )
        private var instance: LinkAllNotificationListener? = null

        fun getActiveInstance(): LinkAllNotificationListener? = instance

        fun triggerCallAction(action: String): Boolean {
            val inst = instance ?: return false
            return inst.handleCallAction(action)
        }
    }

    /** Title and text last forwarded per notification key; see onNotificationPosted. */
    private val lastSentContent = HashMap<String, String>()

    override fun onCreate() {
        super.onCreate()
        instance = this
        Log.i(TAG, "Notification listener service created")
    }

    override fun onDestroy() {
        super.onDestroy()
        if (instance == this) {
            instance = null
        }
        Log.i(TAG, "Notification listener service destroyed")
    }

    override fun onListenerConnected() {
        super.onListenerConnected()
        instance = this
        Log.i(TAG, "Notification listener connected")
    }

    override fun onListenerDisconnected() {
        super.onListenerDisconnected()
        if (instance == this) {
            instance = null
        }
        Log.i(TAG, "Notification listener disconnected")
    }

    override fun onNotificationPosted(sbn: StatusBarNotification?) {
        super.onNotificationPosted(sbn)
        sbn ?: return
        val notif = sbn.notification ?: return
        val pkg = sbn.packageName ?: ""
        
        // Skip our own notifications or ongoing/system ones that might be noisy
        if (pkg == packageName || pkg == "android" || pkg == "com.android.systemui") return
        if ((notif.flags and Notification.FLAG_ONGOING_EVENT) != 0) return
        // A group summary repeats its child notifications, which are sent on their own.
        if ((notif.flags and Notification.FLAG_GROUP_SUMMARY) != 0) return
        // Media, progress and navigation updates re-post constantly; each send wakes the radio.
        if (notif.category in SKIPPED_CATEGORIES) return

        val title = notif.extras?.getCharSequence(Notification.EXTRA_TITLE)?.toString() ?: ""
        // Messaging apps put the full message in EXTRA_BIG_TEXT; EXTRA_TEXT is often a one-line preview.
        val text = (notif.extras?.getCharSequence(Notification.EXTRA_BIG_TEXT)
            ?: notif.extras?.getCharSequence(Notification.EXTRA_TEXT))?.toString() ?: ""
        val id = sbn.key ?: "${sbn.id}"

        if (title.isBlank() && text.isBlank()) return
        // Apps re-post the same notification (timestamp, badge or silent
        // updates) with unchanged text; the computer already shows it.
        val content = "$title\u0000$text"
        if (lastSentContent[id] == content) return
        lastSentContent[id] = content
        
        // Never log the content: it holds messages and one-time codes.
        Log.d(TAG, "Notification posted: pkg=$pkg")
        
        val intent = Intent(this, LinkAllService::class.java).apply {
            action = LinkAllService.ACTION_PUSH_NOTIFICATION
            putExtra(LinkAllService.EXTRA_NOTIFICATION_ID, id)
            putExtra(LinkAllService.EXTRA_NOTIFICATION_PKG, pkg)
            putExtra(LinkAllService.EXTRA_NOTIFICATION_TITLE, title)
            putExtra(LinkAllService.EXTRA_NOTIFICATION_TEXT, text)
        }
        startService(intent)
    }

    override fun onNotificationRemoved(sbn: StatusBarNotification?) {
        super.onNotificationRemoved(sbn)
        // Dismissed: the same text posted again later is a new notification.
        sbn?.key?.let { lastSentContent.remove(it) }
    }

    fun handleCallAction(actionWanted: String): Boolean {
        val activeNotifs = activeNotifications ?: return false
        Log.i(TAG, "Searching active notifications for call actions... count=${activeNotifs.size}")
        for (sbn in activeNotifs) {
            val notif = sbn.notification ?: continue
            val pkg = sbn.packageName ?: ""
            val category = notif.category ?: ""
            
            // Check if this is a call notification or from dialer/phone app
            val isCall = category == Notification.CATEGORY_CALL || 
                         pkg.contains("dialer") || 
                         pkg.contains("phone") || 
                         pkg.contains("telephony")
                         
            if (isCall) {
                val actions = notif.actions ?: continue
                Log.d(TAG, "Found call notification from package: $pkg with ${actions.size} actions")
                for (action in actions) {
                    val title = action.title?.toString()?.lowercase() ?: continue
                    Log.d(TAG, "Examining action title: $title")
                    
                    if (actionWanted == "accept") {
                        if (title.contains("answer") || title.contains("accept") || title.contains("call")) {
                            try {
                                action.actionIntent.send()
                                Log.i(TAG, "Triggered accept call action successfully via PendingIntent!")
                                return true
                            } catch (e: Exception) {
                                Log.e(TAG, "Failed to send accept call action PendingIntent", e)
                            }
                        }
                    } else if (actionWanted == "decline") {
                        if (title.contains("decline") || title.contains("reject") || title.contains("end") || title.contains("hang") || title.contains("dismiss")) {
                            try {
                                action.actionIntent.send()
                                Log.i(TAG, "Triggered decline call action successfully via PendingIntent!")
                                return true
                            } catch (e: Exception) {
                                Log.e(TAG, "Failed to send decline call action PendingIntent", e)
                            }
                        }
                    }
                }
            }
        }
        Log.w(TAG, "No matching call notification found for action: $actionWanted")
        return false
    }
}
