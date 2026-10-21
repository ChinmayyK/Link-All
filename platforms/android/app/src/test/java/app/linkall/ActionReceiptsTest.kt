package app.linkall

import org.junit.Assert.assertEquals
import org.junit.Test

class ActionReceiptsTest {
    @Test fun clipboardSentNamesOneTwoOrManyDevices() {
        assertEquals("Sent to Mac", ActionReceipts.clipboardSent(listOf("Mac")))
        assertEquals("Sent to Mac and PC", ActionReceipts.clipboardSent(listOf("Mac", "PC")))
        assertEquals("Sent to 3 devices", ActionReceipts.clipboardSent(listOf("Mac", "PC", "Linux")))
    }

    @Test fun folderSyncPluralisesTheFileCount() {
        assertEquals("Syncing 1 new file from Camera to Mac", ActionReceipts.folderSync(1, "Camera", "Mac"))
        assertEquals("Syncing 12 new files from Camera to Mac", ActionReceipts.folderSync(12, "Camera", "Mac"))
    }
}
