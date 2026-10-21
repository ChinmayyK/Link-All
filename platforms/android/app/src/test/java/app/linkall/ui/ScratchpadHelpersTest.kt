package app.linkall.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ScratchpadHelpersTest {
    private val notes = listOf(
        ScratchNote("1", "ssh ops@10.0.0.4", false),
        ScratchNote("2", "Buy milk and eggs", true),
        ScratchNote("3", "https://example.com/docs?a=1.", false),
    )

    @Test fun searchMatchesEveryWordAnywhere() {
        assertEquals(listOf("2"), filterNotes(notes, "EGGS milk").map { it.id })
        assertEquals(notes, filterNotes(notes, "   "))
        assertEquals(emptyList<ScratchNote>(), filterNotes(notes, "nothing like this"))
    }

    @Test fun findsALinkWithoutTrailingPunctuation() {
        assertEquals("https://example.com/docs?a=1", firstLink("see https://example.com/docs?a=1."))
        assertNull(firstLink("no link here, only example.com"))
    }

    @Test fun sayHowOldANoteIs() {
        val now = 1_000_000_000_000L
        assertEquals("just now", ago(now - 10_000, now))
        assertEquals("5 min ago", ago(now - 5 * 60_000, now))
        assertEquals("3 h ago", ago(now - 3 * 3_600_000, now))
        assertEquals("1 day ago", ago(now - 86_400_000, now))
        assertEquals("2 days ago", ago(now - 2 * 86_400_000, now))
        assertEquals("", ago(0, now))
    }

    @Test fun noteKinds() {
        assertEquals(NoteKind.Link, kindOf("https://example.com/docs"))
        assertEquals(NoteKind.Link, kindOf("read later https://example.com/a"))
        assertEquals(NoteKind.Code, kindOf("ssh ops@10.0.0.4"))
        assertEquals(NoteKind.Code, kindOf("git pull\nnpm install"))
        assertEquals(NoteKind.Code, kindOf("```\nlet x = 1\n```"))
        assertEquals(NoteKind.Text, kindOf("Buy milk and eggs"))
        // A long text that merely mentions a link is still text.
        assertEquals(NoteKind.Text, kindOf("The report is at https://example.com/r and it says " + "a lot ".repeat(30)))
    }
}
