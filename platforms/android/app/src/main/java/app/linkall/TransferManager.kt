package app.linkall

import kotlinx.coroutines.flow.MutableStateFlow

enum class TransferState {
    INCOMING, PROGRESS, PAUSED, CANCELED, FAILED, COMPLETED
}

@androidx.compose.runtime.Immutable
data class TransferProgress(
    val id: String,
    val fileName: String,
    val percent: Int,
    val bytesReceived: Long,
    val totalBytes: Long = 0,
    val speedBps: Long,
    val etaSecs: Long,
    var isPaused: Boolean = false,
    val state: TransferState = TransferState.PROGRESS,
    val peerName: String = "",
    val isOutbound: Boolean = false
)

object TransferManager {
    // Flow to expose active transfers to UI
    val activeTransfersFlow = MutableStateFlow<List<TransferProgress>>(emptyList())
    
    val pendingOutboundTransferIds = java.util.concurrent.ConcurrentHashMap.newKeySet<String>()
    
    // Thread-safe map for real-time updates from JNI threads
    val activeTransfers = java.util.concurrent.ConcurrentHashMap<String, TransferProgress>()
    private var lastPublishTime = 0L

    fun publishActiveTransfers(force: Boolean = false) {
        val now = System.currentTimeMillis()
        if (!force && now - lastPublishTime < 33L) {
            return
        }
        lastPublishTime = now
        activeTransfersFlow.value = activeTransfers.values.toList()
        activeSpeedTestsFlow.value = activeSpeedTests.values.toList()
    }
    
    val activeSpeedTestsFlow = MutableStateFlow<List<SpeedTestProgress>>(emptyList())
    val activeSpeedTests = java.util.concurrent.ConcurrentHashMap<String, SpeedTestProgress>()
}

@androidx.compose.runtime.Immutable
data class SpeedTestProgress(
    val peerId: String,
    val peerName: String,
    val phase: String,
    val bytesTransferred: Long,
    val durationSecs: Int
) {
    val speedBps: Long
        get() = if (durationSecs > 0) bytesTransferred / durationSecs else 0
    
    val speedMbpsString: String
        get() = String.format("%.1f Mbps", (speedBps * 8).toDouble() / 1_000_000.0)
}
