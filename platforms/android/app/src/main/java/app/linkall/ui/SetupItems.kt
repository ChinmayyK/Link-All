package app.linkall.ui

import app.linkall.HealthIssue

/** One line of the Setup list. [fixLabel] is null when there is nothing to press. */
data class SetupItem(
    val kind: String,
    val title: String,
    val detail: String,
    val ok: Boolean,
    val fixLabel: String?,
)

/** What Android knows that the engine doesn't, plus the engine's own problems. */
data class SetupState(
    /** Problems from the engine and the Android checks, most important first. */
    val problems: List<HealthIssue>,
    val hasPairedDevice: Boolean,
    val batteryAllowed: Boolean,
    val notificationAccess: Boolean,
    val callContinuityOn: Boolean,
    val postNotifications: Boolean,
    val playBuild: Boolean,
    val sharedFolders: Int,
)

/**
 * Every item of the Setup list: the problems first, as given, then a healthy line
 * for each check that has no problem, so the user can see what was checked.
 * Checks that only matter in some builds or setups say so instead of nagging.
 */
fun setupItems(s: SetupState): List<SetupItem> {
    val items = s.problems.map {
        SetupItem(it.kind, it.title, it.detail, ok = false, fixLabel = healthActionLabel(it.kind))
    }.toMutableList()
    val flagged = s.problems.map { it.kind }.toSet()

    fun healthy(kind: String, title: String, detail: String) {
        if (kind !in flagged) items += SetupItem(kind, title, detail, ok = true, fixLabel = null)
    }

    healthy(
        "battery_restricted", "Stays connected in the background",
        if (s.batteryAllowed) "Android lets Link All run with the screen off."
        else "Not needed until a device is linked.",
    )
    healthy(
        "notification_access", "Notification access",
        when {
            s.notificationAccess -> "Your computer can show who is calling and answer for you."
            s.callContinuityOn -> "Calls need this."
            else -> "Off. Only needed to show or answer calls on your computer."
        },
    )
    if (!s.postNotifications) {
        items += SetupItem(
            "notifications_permission", "Notifications are off",
            "Link All can't tell you about received files or pairing requests.", ok = false, fixLabel = "Turn on",
        )
    } else {
        healthy("notifications_permission", "Notifications", "Link All can tell you about files and pairing requests.")
    }
    if (s.playBuild) {
        healthy(
            "shared_folders", "Folders your computers can browse",
            if (s.sharedFolders == 0) "None shared yet. Add some in Settings." else "${s.sharedFolders} shared.",
        )
    }
    // Problems come first, in the order given; healthy lines keep the order above.
    return items.sortedBy { if (it.ok) 1 else 0 }
}

fun List<SetupItem>.problemCount(): Int = count { !it.ok }
