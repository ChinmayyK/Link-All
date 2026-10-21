package app.linkall

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.widget.Toast
import kotlin.concurrent.thread

/**
 * "Scratchpad" in the share sheet: shared text becomes a note, with no screen
 * of its own. A link shared from the browser, a command copied from a
 * message or a snippet from a chat is one tap from your other devices.
 */
class ScratchpadShareActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val text = intent?.takeIf { it.action == Intent.ACTION_SEND }
            ?.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()?.trim()
        val subject = intent?.getStringExtra(Intent.EXTRA_SUBJECT)?.trim().orEmpty()
        if (text.isNullOrEmpty()) {
            Toast.makeText(this, "Only text can be saved to the Scratchpad", Toast.LENGTH_SHORT).show()
            finish()
            return
        }
        val note = if (subject.isNotEmpty() && !text.contains(subject)) "$subject\n$text" else text
        val app = applicationContext
        thread {
            val saved = LinkAllService.withLiveEngine { LinkAllJni.scratchpadSave(it, null, note, false) }
            val message = when (saved) {
                0 -> "Saved to Scratchpad"
                null -> "Link All isn't running"
                else -> "Couldn't save the note. It may be too long."
            }
            runOnUiThread {
                Toast.makeText(app, message, Toast.LENGTH_SHORT).show()
                finish()
            }
        }
    }
}
