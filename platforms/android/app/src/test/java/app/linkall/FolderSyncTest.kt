package app.linkall

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class FolderSyncTest {
    private val now = 1_000_000L
    private fun file(id: String, size: Long = 10, ageMs: Long = 60_000) =
        FolderSync.TreeFile(id, "$id.jpg", size, now - ageMs)

    @Test fun aNewFileIsPending() {
        val f = file("a")
        assertEquals(listOf(f), FolderSync.pending(listOf(f), emptySet(), emptySet(), now))
    }

    @Test fun aSentFileIsSkipped() {
        val f = file("a")
        assertTrue(FolderSync.pending(listOf(f), setOf(f.key), emptySet(), now).isEmpty())
    }

    @Test fun anEditedFileIsSentAgain() {
        val old = file("a", size = 10)
        val edited = file("a", size = 20)
        assertEquals(listOf(edited), FolderSync.pending(listOf(edited), setOf(old.key), emptySet(), now))
    }

    @Test fun aFileStillBeingWrittenWaits() {
        val fresh = file("a", ageMs = 3_000)
        assertTrue(FolderSync.pending(listOf(fresh), emptySet(), emptySet(), now).isEmpty())
    }

    @Test fun aFileAlreadyOnItsWayIsSkipped() {
        val f = file("a")
        assertTrue(FolderSync.pending(listOf(f), emptySet(), setOf(f.key), now).isEmpty())
    }

    @Test fun onlyTheUnsentFilesAreReturned() {
        val sent = file("a")
        val fresh = file("b")
        assertEquals(listOf(fresh), FolderSync.pending(listOf(sent, fresh), setOf(sent.key), emptySet(), now))
    }
}
