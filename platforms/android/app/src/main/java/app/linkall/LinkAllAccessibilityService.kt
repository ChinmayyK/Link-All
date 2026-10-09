package app.linkall

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.AccessibilityServiceInfo
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.util.Log
import android.view.accessibility.AccessibilityEvent

class LinkAllAccessibilityService : AccessibilityService() {

    override fun onServiceConnected() {
        super.onServiceConnected()
        val info = AccessibilityServiceInfo()
        info.eventTypes = AccessibilityEvent.TYPE_VIEW_TEXT_SELECTION_CHANGED or AccessibilityEvent.TYPE_VIEW_CLICKED
        info.feedbackType = AccessibilityServiceInfo.FEEDBACK_GENERIC
        // We only want to run in the background to bypass Android 10+ clipboard restrictions if needed.
        this.serviceInfo = info
        Log.d("LinkAll", "AccessibilityService connected for clipboard fallback.")
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent) {
        // AccessibilityServices on Android 10+ are exempt from background clipboard read restrictions!
        // We just need the service to be enabled. The actual clipboard listening happens in ClipboardSyncManager/TransferManager,
        // but if they fail, we can proactively read the clipboard here on copy actions if we want to build a deep integration.
        
        // For now, simply having the AccessibilityService running and enabled gives the app background clipboard access.
        // If we want to proactively capture text when a user clicks 'Copy':
        // We can listen to accessibility events, but since Android 10 allows background clipboard reads if an AccessibilityService is enabled,
        // we might not even need to manually parse the text here.
    }

    override fun onInterrupt() {
        Log.d("LinkAll", "AccessibilityService interrupted.")
    }
}
