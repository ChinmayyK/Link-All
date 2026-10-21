package app.linkall

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.widget.Toast
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Tells the user what an action did, including when it did nothing. A tap on
 * "send my clipboard" with no device connected used to leave only a log line.
 * The app's snackbar shows it while the app is on screen; otherwise a toast does.
 */
object ActionReceipts {
    private val flow = MutableSharedFlow<String>(extraBufferCapacity = 8)
    val messages: SharedFlow<String> get() = flow

    /** Set by the main screen while it is on screen, so [say] knows whether a snackbar can show. */
    val appVisible = AtomicBoolean(false)

    fun say(context: Context, text: String) {
        if (appVisible.get() && flow.tryEmit(text)) return
        Handler(Looper.getMainLooper()).post {
            Toast.makeText(context.applicationContext, text, Toast.LENGTH_SHORT).show()
        }
    }

    /** "Sent to MacBook Pro", "Sent to Desk and Mac", "Sent to 4 devices". */
    fun clipboardSent(names: List<String>): String = when (names.size) {
        0 -> "Sent"
        1 -> "Sent to ${names[0]}"
        2 -> "Sent to ${names[0]} and ${names[1]}"
        else -> "Sent to ${names.size} devices"
    }

    const val NO_DEVICE = "No device is connected"
    const val CLIPBOARD_EMPTY = "Nothing to send: the clipboard is empty"
    const val CLIPBOARD_FAILED = "Couldn't send. Try again"
    const val FILE_NOT_SHARED = "That file isn't shared on this phone any more"

    /** "Syncing 3 new files from Camera to MacBook Pro". */
    fun folderSync(count: Int, folder: String, target: String): String =
        "Syncing $count new ${if (count == 1) "file" else "files"} from $folder to $target"
}
