package app.linkall

import app.linkall.ui.SetupState
import app.linkall.ui.problemCount
import app.linkall.ui.setupItems
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SetupItemsTest {
    private fun state(
        problems: List<HealthIssue> = emptyList(),
        paired: Boolean = true,
        battery: Boolean = true,
        access: Boolean = true,
        calls: Boolean = true,
        notifications: Boolean = true,
        play: Boolean = false,
        folders: Int = 0,
    ) = SetupState(problems, paired, battery, access, calls, notifications, play, folders)

    private fun issue(kind: String) = HealthIssue(kind, "title $kind", "detail $kind", null)

    @Test fun everythingFineIsAllHealthyLines() {
        val items = setupItems(state())
        assertEquals(0, items.problemCount())
        assertTrue(items.all { it.ok })
    }

    @Test fun problemsComeFirstInTheOrderGiven() {
        val items = setupItems(state(problems = listOf(issue("sync_paused"), issue("battery_restricted")), battery = false))
        assertEquals(listOf("sync_paused", "battery_restricted"), items.take(2).map { it.kind })
        assertEquals(2, items.problemCount())
    }

    @Test fun aFlaggedCheckIsNotAlsoListedAsHealthy() {
        val items = setupItems(state(problems = listOf(issue("notification_access")), access = false))
        assertEquals(1, items.count { it.kind == "notification_access" })
        assertFalse(items.first { it.kind == "notification_access" }.ok)
    }

    @Test fun notificationAccessOffIsOnlyHealthyAdviceWhenCallsAreOff() {
        val items = setupItems(state(access = false, calls = false))
        val line = items.first { it.kind == "notification_access" }
        assertTrue(line.ok)
        assertTrue(line.detail.startsWith("Off"))
    }

    @Test fun deniedNotificationsIsAProblemWithAFixButton() {
        val line = setupItems(state(notifications = false)).first { it.kind == "notifications_permission" }
        assertFalse(line.ok)
        assertEquals("Turn on", line.fixLabel)
    }

    @Test fun sharedFoldersOnlyAppearInThePlayBuild() {
        assertTrue(setupItems(state(play = false)).none { it.kind == "shared_folders" })
        val line = setupItems(state(play = true, folders = 2)).first { it.kind == "shared_folders" }
        assertEquals("2 shared.", line.detail)
    }

    @Test fun withNoPairedDeviceBatteryLimitsAreNotNagged() {
        val line = setupItems(state(paired = false, battery = false)).first { it.kind == "battery_restricted" }
        assertTrue(line.ok)
    }
}
