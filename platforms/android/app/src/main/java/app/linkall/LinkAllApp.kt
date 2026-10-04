package app.linkall

import android.app.Activity
import android.app.Application
import android.os.Bundle
import androidx.appcompat.app.AppCompatDelegate
import com.google.android.material.color.DynamicColors

class LinkAllApp : Application(), Application.ActivityLifecycleCallbacks {
    
    companion object {
        var isAppInForeground = false
            private set
    }
    
    private var activityReferences = 0
    private var isActivityChangingConfigurations = false
    override fun onCreate() {
        super.onCreate()
        val prefs = getSharedPreferences(packageName + "_preferences", android.content.Context.MODE_PRIVATE)
        
        // 1. Theme Mode
        val mode = prefs.getInt("theme_mode", AppCompatDelegate.MODE_NIGHT_NO)
        AppCompatDelegate.setDefaultNightMode(mode)

        // 2. Dynamic Colors (Material 3)
        val useDynamic = prefs.getBoolean("use_dynamic_colors", true)
        if (useDynamic) {
            DynamicColors.applyToActivitiesIfAvailable(this)
        }
        
        registerActivityLifecycleCallbacks(this)
    }

    override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) {}
    override fun onActivityStarted(activity: Activity) {
        if (++activityReferences == 1 && !isActivityChangingConfigurations) {
            isAppInForeground = true
        }
    }
    override fun onActivityResumed(activity: Activity) {}
    override fun onActivityPaused(activity: Activity) {}
    override fun onActivityStopped(activity: Activity) {
        isActivityChangingConfigurations = activity.isChangingConfigurations
        if (--activityReferences == 0 && !isActivityChangingConfigurations) {
            isAppInForeground = false
        }
    }
    override fun onActivitySaveInstanceState(activity: Activity, outState: Bundle) {}
    override fun onActivityDestroyed(activity: Activity) {}
}
