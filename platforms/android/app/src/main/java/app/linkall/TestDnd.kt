package app.linkall

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.draganddrop.dragAndDropTarget
import androidx.compose.ui.Modifier
import androidx.compose.ui.draganddrop.toAndroidDragEvent

@OptIn(ExperimentalFoundationApi::class)
fun testModifier(): Modifier = Modifier.dragAndDropTarget(
    shouldStartDragAndDrop = { true },
    target = object : androidx.compose.ui.draganddrop.DragAndDropTarget {
        override fun onDrop(event: androidx.compose.ui.draganddrop.DragAndDropEvent): Boolean {
            val androidEvent = event.toAndroidDragEvent()
            val clipData = androidEvent.clipData
            return clipData != null
        }
    }
)
