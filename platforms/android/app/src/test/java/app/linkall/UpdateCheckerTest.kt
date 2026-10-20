package app.linkall

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class UpdateCheckerTest {
    @Test fun higherVersionsAreNewer() {
        assertTrue(UpdateChecker.isNewer("2.1.0", "2.0.2"))
        assertTrue(UpdateChecker.isNewer("2.0.10", "2.0.9"))
        assertTrue(UpdateChecker.isNewer("3.0.0", "2.9.9"))
    }

    @Test fun sameOrOlderIsNotNewer() {
        assertFalse(UpdateChecker.isNewer("2.0.2", "2.0.2"))
        assertFalse(UpdateChecker.isNewer("2.0.1", "2.0.2"))
        assertFalse(UpdateChecker.isNewer("2.0.2", "2.0.2-debug"))
    }

    @Test fun releaseJsonGivesTheVersionAndPage() {
        val u = UpdateChecker.parse("""{"tag_name":"v2.1.0","html_url":"https://github.com/ChinmayyK/Link-All/releases/tag/v2.1.0"}""")
        assertEquals("2.1.0", u?.version)
    }

    @Test fun draftsPrereleasesAndForeignLinksAreIgnored() {
        assertNull(UpdateChecker.parse("""{"tag_name":"v9.0.0","prerelease":true,"html_url":"https://github.com/ChinmayyK/Link-All/x"}"""))
        assertNull(UpdateChecker.parse("""{"tag_name":"v9.0.0","html_url":"https://evil.example/Link-All/"}"""))
    }
}
