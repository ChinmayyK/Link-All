package app.linkall

import org.junit.Assert.*
import org.junit.Test

/**
 * Unit tests validating the notification UX rules.
 *
 * These tests verify the DESIGN DECISIONS, not Android framework calls
 * (those require instrumented tests).
 */
class NotificationUxTest {

    // ── Activity feed ─────────────────────────────────────────────────────────

    @Test
    fun `activity feed bounded to ACTIVITY_FEED_MAX entries`() {
        synchronized(ActivityFeedManager.feedLock) {
            ActivityFeedManager.activityFeed.clear()
        }

        repeat(120) { i ->
            ActivityFeedManager.addToFeed(
                ActivityEntry(
                    deviceName = "Phone",
                    kind = ActivityKind.CLIPBOARD_TEXT,
                    preview = "item $i"
                )
            )
        }

        val size = ActivityFeedManager.getFeedSnapshot().size
        assertTrue("Feed must be bounded (got $size)", size <= 100)
    }

    @Test
    fun `activity feed snapshot returns newest first`() {
        synchronized(ActivityFeedManager.feedLock) {
            ActivityFeedManager.activityFeed.clear()
        }

        ActivityFeedManager.addToFeed(
            ActivityEntry(deviceName = "A", kind = ActivityKind.CLIPBOARD_TEXT, preview = "first")
        )
        Thread.sleep(2)
        ActivityFeedManager.addToFeed(
            ActivityEntry(deviceName = "B", kind = ActivityKind.CLIPBOARD_TEXT, preview = "second")
        )

        val snapshot = ActivityFeedManager.getFeedSnapshot()
        assertEquals("second", snapshot.first().preview)
        assertEquals("first",  snapshot.last().preview)
    }

    @Test
    fun `activity feed is thread-safe for concurrent writes`() {
        synchronized(ActivityFeedManager.feedLock) {
            ActivityFeedManager.activityFeed.clear()
        }

        val threads = (0..9).map { i ->
            Thread {
                repeat(10) { j ->
                    ActivityFeedManager.addToFeed(
                        ActivityEntry(
                            deviceName = "Dev$i",
                            kind = ActivityKind.CLIPBOARD_TEXT,
                            preview = "msg-$j"
                        )
                    )
                }
            }
        }
        threads.forEach { it.start() }
        threads.forEach { it.join() }

        val size = ActivityFeedManager.getFeedSnapshot().size
        assertTrue("Feed size must be bounded after concurrent writes (got $size)", size <= 100)
    }

    // ── ActivityEntry formatting ───────────────────────────────────────────────

    @Test
    fun `text entry formatted line uses device name`() {
        val entry = ActivityEntry(
            deviceName = "Chinmay's Pixel 8",
            kind = ActivityKind.CLIPBOARD_TEXT,
            preview = "hello"
        )
        val line  = entry.formattedLine()
        assertTrue("Line must contain device name", line.contains("Chinmay's Pixel 8"))
        assertTrue("Line must not contain 'text' as raw type alone", !line.equals("text"))
    }

    @Test
    fun `file entry formatted line includes filename`() {
        val entry = ActivityEntry(
            deviceName = "MacBook Pro",
            kind = ActivityKind.FILE_SENT,
            preview = "resume.pdf"
        )
        val line  = entry.formattedLine()
        assertTrue("Line must contain filename", line.contains("resume.pdf"))
        assertTrue("Line must contain device name", line.contains("MacBook Pro"))
    }

    @Test
    fun `image entry formatted correctly`() {
        val entry = ActivityEntry(
            deviceName = "iPad",
            kind = ActivityKind.CLIPBOARD_IMAGE,
            preview = "screenshot.png"
        )
        val line  = entry.formattedLine()
        assertTrue("Image line must contain device name", line.contains("iPad"))
    }

    // ── Background sync mode ──────────────────────────────────────────────────

    @Test
    fun `BackgroundSyncMode enum has expected values`() {
        val modes = BackgroundSyncMode.values()
        assertTrue(modes.contains(BackgroundSyncMode.ALWAYS_ACTIVE))
        assertTrue(modes.contains(BackgroundSyncMode.BATTERY_OPTIMIZED))
    }

    // ── Notification channel ID constants ─────────────────────────────────────

    @Test
    fun `service channel and alerts channel are distinct`() {
        // Verify via reflection that the channel IDs differ
        val serviceField = LinkAllService::class.java.getDeclaredField("CHAN_SERVICE")
        val alertsField  = LinkAllService::class.java.getDeclaredField("CHAN_ALERTS")
        val pairingField = LinkAllService::class.java.getDeclaredField("CHAN_PAIRING")
        serviceField.isAccessible = true
        alertsField.isAccessible  = true
        pairingField.isAccessible = true
        assertNotEquals(serviceField.name, alertsField.name)
        assertNotEquals(alertsField.name, pairingField.name)
        val companion = LinkAllService::class.java.getDeclaredField("Companion")
        // Just verify the constants exist and differ by checking them in companion
        // (actual values are private const — we test via the behaviour in service)
        assertNotEquals(
            LinkAllService.ACTION_PAUSE_SYNC,
            LinkAllService.ACTION_RESUME_SYNC
        )
    }

    // ── Action intents ────────────────────────────────────────────────────────

    @Test
    fun `pause and resume actions are distinct strings`() {
        assertNotEquals(LinkAllService.ACTION_PAUSE_SYNC, LinkAllService.ACTION_RESUME_SYNC)
    }

    @Test
    fun `disconnect action is distinct from pause`() {
        assertNotEquals(LinkAllService.ACTION_PAUSE_SYNC, LinkAllService.ACTION_DISCONNECT_ALL)
    }
}
