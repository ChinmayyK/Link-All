package app.linkall

// ── JNI Bridge ────────────────────────────────────────────────────────────────
// The prebuilt .so exports Java_app_linkall_LinkAllJni_* symbols.
// Each external fun here must have a matching export in jni_android.rs.

object LinkAllJni {
    init { System.loadLibrary("linkall_core") }

    // ── Event type constants ──────────────────────────────────────────────────
    const val CR_EVENT_NONE                  = 0
    const val CR_EVENT_CLIPBOARD_TEXT        = 1   // auto-applied to local clipboard
    const val CR_EVENT_CLIPBOARD_IMAGE       = 2   // auto-applied
    const val CR_EVENT_CLIPBOARD_FILE        = 3   // auto-applied (legacy)
    const val CR_EVENT_PAIRING_REQUESTED     = 4
    const val CR_EVENT_PEER_CONNECTED        = 5
    const val CR_EVENT_PEER_DISCONNECTED     = 6
    const val CR_EVENT_PEER_DISCOVERED       = 27
    const val CR_EVENT_WARNING               = 7
    const val CR_EVENT_CLIPBOARD_SYNCED      = 8
    // 9, 10 reserved
    const val CR_EVENT_CLIPBOARD_AVAILABLE   = 11  // timeline-first: in feed, not yet applied
    const val CR_EVENT_FILE_TRANSFER_INCOMING  = 12
    const val CR_EVENT_FILE_TRANSFER_PROGRESS  = 13
    const val CR_EVENT_FILE_TRANSFER_COMPLETE  = 14
    const val CR_EVENT_FILE_TRANSFER_FAILED    = 15
    const val CR_EVENT_FILE_TRANSFER_PAUSED    = 20
    const val CR_EVENT_FILE_TRANSFER_RESUMED   = 21
    const val CR_EVENT_ACTIVITY_UPDATED        = 16
    const val CR_EVENT_CALL_STATE_CHANGED       = 17
    const val CR_EVENT_CALL_ACTION              = 18
    const val CR_EVENT_BATTERY_STATE_CHANGED    = 19
    const val CR_EVENT_OUTGOING_PAIRING_WAITING = 29
    /** A pairing request started, ended or changed without another event saying so. Re-read peers. */
    const val CR_EVENT_PAIRING_CHANGED         = 40
    const val CR_EVENT_SYSTEM_HEALTH_UPDATED   = 26
    const val CR_EVENT_REMOTE_FILES_QUERY      = 30
    const val CR_EVENT_REMOTE_THUMBNAIL_REQUEST = 31
    const val CR_EVENT_REMOTE_FILE_PULL_REQUEST = 32
    const val CR_EVENT_REMOTE_FILE_ACTION_REQUEST = 37
    const val CR_EVENT_REMOTE_FILES_RESPONSE   = 33
    /** A peer asks this device to open a link; answer with [ackOpenUrlOnDevice]. */
    const val CR_EVENT_OPEN_URL_ON_DEVICE_REQUESTED = 38
    /** A peer answered a link this device asked it to open; see [eventOpenUrlAckSuccess]. */
    const val CR_EVENT_OPEN_URL_ON_DEVICE_ACK  = 39
    const val CR_EVENT_SPEED_TEST_PROGRESS     = 35
    const val CR_EVENT_SPEED_TEST_COMPLETE     = 36
    /** Every file of a folder transfer finished; see [eventFolderCounts]. */
    const val CR_EVENT_FOLDER_TRANSFER_COMPLETE = 41
    /** A computer answered a remote-control action; see [eventRemoteControlSuccess]. */
    const val CR_EVENT_REMOTE_CONTROL_ACK      = 42
    /** A paired computer asked this phone to start or stop ringing; see [eventFindRing]. */
    const val CR_EVENT_FIND_DEVICE_REQUESTED   = 43
    /** A paired computer offered to call a number from this phone; the number is [eventText]. */
    const val CR_EVENT_DIAL_REQUESTED          = 44
    /** The Scratchpad changed, here or on a paired device. */
    const val CR_EVENT_SCRATCHPAD_CHANGED      = 45

    // ── Core engine ───────────────────────────────────────────────────────────
    @JvmStatic external fun initContext(context: android.content.Context)
    @JvmStatic external fun start(deviceName: String?, port: Int, dataDir: String?, fileSaveDir: String?): Long
    @JvmStatic external fun stop(handle: Long)
    @JvmStatic external fun protocolVersion(): Int
    /** A redacted text report for a problem report; [header] is what only the app knows. */
    @JvmStatic external fun diagnosticReport(handle: Long, header: String): String?

    // ── Clipboard push ────────────────────────────────────────────────────────
    @JvmStatic external fun pushText(handle: Long, text: String): Int
    @JvmStatic external fun openUrlOnDevice(handle: Long, targetDeviceId: String, url: String): Int
    /** The devices a purpose ("send", "remote", "browse", "sync_target") can go to, as JSON. */
    @JvmStatic external fun deviceChoicesJson(handle: Long, purpose: String, lastUsed: String?): String?
    /** Asks a computer to press a media, volume or slide key. 0 when sent, -1 when it could not be. */
    @JvmStatic external fun remoteControl(handle: Long, targetDeviceId: String, action: String): Int
    /** Why [remoteControl] could not send, as a sentence, or null. */
    @JvmStatic external fun remoteControlError(handle: Long, targetDeviceId: String): String?
    @JvmStatic external fun eventRemoteControlSuccess(event: Long): Boolean
    // Scratchpad: notes shared between your devices.
    /** The notes as a JSON array, pinned first; each has id, text, pinned, updated_at_ms. */
    @JvmStatic external fun scratchpadListJson(handle: Long): String?
    /** Creates a note ([id] null) or edits one. 0 when saved, -1 when not. */
    @JvmStatic external fun scratchpadSave(handle: Long, id: String?, text: String, pinned: Boolean): Int
    @JvmStatic external fun scratchpadDelete(handle: Long, id: String): Int
    /** Deletes every note that isn't pinned. */
    @JvmStatic external fun scratchpadClear(handle: Long): Int
    /** Rings or stops a computer; waits for its answer. Null when it will, otherwise why it won't. */
    @JvmStatic external fun sendFindDevice(handle: Long, targetDeviceId: String, ring: Boolean): String?
    // Trackpad and keyboard for a computer. Each returns 0 when sent, -1 when it could not be.
    @JvmStatic external fun sendPointerMove(handle: Long, targetDeviceId: String, dx: Int, dy: Int): Int
    /** [button]: 0 left, 1 right, 2 middle. */
    @JvmStatic external fun sendPointerButton(handle: Long, targetDeviceId: String, button: Int, down: Boolean): Int
    @JvmStatic external fun sendPointerScroll(handle: Long, targetDeviceId: String, dx: Int, dy: Int): Int
    @JvmStatic external fun sendTypeText(handle: Long, targetDeviceId: String, text: String): Int
    /** [key]: backspace, enter, escape, tab, left, right, up or down. */
    @JvmStatic external fun sendKeyPress(handle: Long, targetDeviceId: String, key: String): Int
    /** True when a [CR_EVENT_FIND_DEVICE_REQUESTED] says to start ringing, false to stop. */
    @JvmStatic external fun eventFindRing(event: Long): Boolean
    /** Whether paired computers may make this phone ring. Returns 0 on success. */
    @JvmStatic external fun setAllowFindDevice(handle: Long, allow: Boolean): Int
    @JvmStatic external fun ackOpenUrlOnDevice(handle: Long, requesterDeviceId: String, success: Boolean, error: String?): Int
    @JvmStatic external fun eventOpenUrlAckSuccess(event: Long): Boolean
    @JvmStatic external fun pushImage(handle: Long, mimeType: String, data: ByteArray): Int
    @JvmStatic external fun pushFile(handle: Long, name: String, data: ByteArray): Int
    @JvmStatic external fun pushNotification(handle: Long, id: String, packageName: String, title: String, text: String): Int
    @JvmStatic external fun pushBatteryStatus(handle: Long, level: Int, charging: Boolean): Int
    @JvmStatic external fun pushStorageStatus(handle: Long, imagesBytes: Long, videosBytes: Long, appsBytes: Long, freeBytes: Long, totalBytes: Long): Int

    // ── Permission Error ──────────────────────────────────────────────────────
    @JvmStatic external fun sendPermissionError(handle: Long, deviceId: String, feature: String, message: String): Int

    // ── Common event accessors ────────────────────────────────────────────────
    @JvmStatic external fun eventText(event: Long): String?
    @JvmStatic external fun eventDeviceId(event: Long): String?
    @JvmStatic external fun eventBinaryData(event: Long): ByteArray?
    @JvmStatic external fun eventDeviceName(event: Long): String?
    @JvmStatic external fun eventMimeType(event: Long): String?

    // ── Event poll ────────────────────────────────────────────────────────────
    @JvmStatic external fun pollEvent(handle: Long): Long
    /** Blocks until the engine emits an event; 0 once the engine is gone. */
    @JvmStatic external fun waitEvent(handle: Long): Long
    /** Wakes a thread blocked in [waitEvent] with an "interrupt" warning event. */
    @JvmStatic external fun interruptWait(handle: Long)
    @JvmStatic external fun eventType(event: Long): Int
    @JvmStatic external fun freeEvent(event: Long)

    // ── Common event accessors ────────────────────────────────────────────────
    @JvmStatic external fun eventFileName(event: Long): String?
    @JvmStatic external fun eventFingerprint(event: Long): String?

    // ── Timeline-first clipboard ──────────────────────────────────────────────
    /** 1 if the ClipboardReceived event was auto-applied; 0 if timeline-first. */
    @JvmStatic external fun eventAutoApplied(event: Long): Int
    /** Activity feed entry ID (-1 if not applicable). */
    @JvmStatic external fun eventActivityId(event: Long): Long
    /** Apply a remote clipboard item to the local clipboard by its content hash. */
    @JvmStatic external fun applyClipboardByHash(engineHandle: Long, hash: String): Int
    /** Mark a peer as trusted after the user approves the pairing prompt. */
    @JvmStatic external fun trustPeer(engineHandle: Long, deviceId: String): Int
    /**
     * Trust a peer via QR code and send the auth token. [fingerprint] is the
     * key the code shows; the core refuses a peer that presented another one.
     */
    @JvmStatic external fun trustPeerFromQr(engineHandle: Long, deviceId: String, token: String, fingerprint: String?): Int
    /** Reject a peer after the user denies the pairing prompt. */
    @JvmStatic external fun rejectPeer(engineHandle: Long, deviceId: String): Int
    /** Forget a previously connected device. */
    @JvmStatic external fun forgetPeer(engineHandle: Long, deviceId: String): Int
    /** Send a pairing request to an untrusted device. */
    @JvmStatic external fun sendPairingRequest(engineHandle: Long, deviceId: String): Int
    /** Respond to an incoming pairing request. */
    @JvmStatic external fun respondToPairing(engineHandle: Long, deviceId: String, accepted: Boolean): Int
    /** Withdraw our pending pairing request; the other device's prompt closes too. */
    @JvmStatic external fun cancelPairingRequest(engineHandle: Long, deviceId: String): Int

    // ── File transfer accessors ───────────────────────────────────────────────
    @JvmStatic external fun eventTransferId(event: Long): String?
    @JvmStatic external fun eventTransferFileName(event: Long): String?
    @JvmStatic external fun eventTransferProgressPercent(event: Long): Int
    @JvmStatic external fun eventTransferBytesReceived(event: Long): Long
    @JvmStatic external fun eventTransferSpeedBps(event: Long): Long
    @JvmStatic external fun eventTransferEtaSecs(event: Long): Long
    /** 1 = this device is sending, 0 = receiving, -1 = not a progress event. */
    @JvmStatic external fun eventTransferIsOutbound(event: Long): Int
    @JvmStatic external fun eventTransferTotalBytes(event: Long): Long
    @JvmStatic external fun eventTransferDestPath(event: Long): String?
    /** Accept an incoming file transfer (identified by hex transfer ID). */
    @JvmStatic external fun acceptFileTransfer(engineHandle: Long, transferIdHex: String): Int
    /** Reject an incoming file transfer. */
    @JvmStatic external fun rejectFileTransfer(engineHandle: Long, transferIdHex: String): Int
    /** Cancel an active file transfer. */
    @JvmStatic external fun cancelFileTransfer(engineHandle: Long, transferIdHex: String): Int

    // ── Speed test accessors ──────────────────────────────────────────────────
    @JvmStatic external fun eventSpeedTestBytes(event: Long): Long
    @JvmStatic external fun eventSpeedTestDuration(event: Long): Int
    @JvmStatic external fun eventSpeedTestPhase(event: Long): String?
    /** Pause an active file transfer. */
    @JvmStatic external fun pauseFileTransfer(engineHandle: Long, transferIdHex: String): Int
    /** Resume a paused file transfer. */
    @JvmStatic external fun resumeFileTransfer(engineHandle: Long, transferIdHex: String): Int
    /** Start a speed test with the given peer. */
    @JvmStatic external fun startSpeedTest(engineHandle: Long, deviceId: String, durationSecs: Int): Int

    /**
     * Connect to a peer discovered via Android NSD.
     * Returns 0 on success, -1 on error.
     */
    @JvmStatic external fun connectToPeer(handle: Long, ip: String, port: Int): Int
    @JvmStatic external fun reportDiscoveredPeer(handle: Long, deviceId: String, deviceName: String, ip: String, port: Int): Int
    @JvmStatic external fun initiatePairing(handle: Long, deviceId: String): Int
    @JvmStatic external fun disconnectPeer(handle: Long, deviceId: String): Int
    @JvmStatic external fun reconnectPeer(handle: Long, deviceId: String): Boolean

    /**
     * Returns this engine's stable device UUID as a hyphenated string
     * (e.g. "550e8400-e29b-41d4-a716-446655440000"), or null on error.
     * Used to filter self-connections during NSD resolution.
     */
    @JvmStatic external fun getDeviceId(handle: Long): String?
    @JvmStatic external fun peersJson(handle: Long): String?
    @JvmStatic external fun healthJson(handle: Long): String?

    // ── Folder transfers ─────────────────────────────────────────────────────
    /** Folder transfers in flight: JSON array of {batch_id, folder_name, peer_name, file_count, done_count, failed_count, outbound}. */
    @JvmStatic external fun foldersJson(handle: Long): String?
    /** [files, failed] for a FOLDER_TRANSFER_COMPLETE event. */
    @JvmStatic external fun eventFolderCounts(event: Long): IntArray?
    /** True when a transfer event is for one file of a folder; the folder reports instead. */
    @JvmStatic external fun eventTransferInFolder(event: Long): Boolean
    /**
     * Send one file of a folder from a detached descriptor (Rust closes it).
     * Blocks while the folder already has its fill of files in flight, so call
     * it from a background thread, file after file. Null means stop sending.
     */
    @JvmStatic external fun sendFolderItemFd(
        handle: Long,
        fd: Int,
        relPath: String,
        folderName: String,
        targetDeviceId: String?,
        batchId: String,
        fileCount: Int
    ): String?
    /** Report the folder once its last files finish. Call after the last [sendFolderItemFd]. */
    @JvmStatic external fun finishFolderSend(handle: Long, batchId: String)
    @JvmStatic external fun cancelFolder(handle: Long, batchId: String): Int
    @JvmStatic external fun sendFilePath(
        handle: Long,
        path: String,
        displayName: String,
        mimeType: String,
        targetDeviceId: String?,
        batchId: String?,
        isDirectory: Boolean,
        itemCount: Int
    ): String?

    /**
     * Send from a file descriptor detached from a ParcelFileDescriptor.
     * The engine takes ownership of [fd] and closes it.
     */
    @JvmStatic external fun sendFileFd(
        handle: Long,
        fd: Int,
        displayName: String,
        mimeType: String,
        targetDeviceId: String?
    ): String?

    /**
     * Push updated sync settings to the running engine atomically.
     * Avoids restarting the service just to update a toggle.
     * Returns 0 on success, -1 if the handle is invalid.
     */
    @JvmStatic external fun applySyncSettings(
        handle: Long,
        syncEnabled: Boolean,
        syncText: Boolean,
        syncImages: Boolean,
        syncFiles: Boolean,
    ): Int

    // ── Call continuity ───────────────────────────────────────────────────────
    /** Push phone call state (ringing/offhook/idle) to all connected peers. */
    @JvmStatic external fun pushCallState(
        handle: Long, state: String, number: String, contactName: String
    ): Int
    /** Get the call state string from a CR_EVENT_CALL_STATE_CHANGED event. */
    @JvmStatic external fun eventCallState(event: Long): String?
    /** Get the phone number from a CR_EVENT_CALL_STATE_CHANGED event. */
    @JvmStatic external fun eventCallNumber(event: Long): String?
    /** Get the contact name from a CR_EVENT_CALL_STATE_CHANGED event. */
    @JvmStatic external fun eventCallContactName(event: Long): String?
    /** Get the action string ("accept"/"decline") from a CR_EVENT_CALL_ACTION event. */
    @JvmStatic external fun eventCallAction(event: Long): String?
    // ── Battery synchronization (F20) ─────────────────────────────────────────
    // ── Network lifecycle ─────────────────────────────────────────────────────
    /**
     * Notify the Rust engine that Android's default network is available again
     * (e.g., after Doze, Wi-Fi reconnect, airplane mode toggle).
     * Triggers immediate reconnection to all known trusted peers.
     */
    @JvmStatic external fun notifyNetworkRestored(handle: Long): Int

    /**
     * Notify the Rust engine whether the Android device is sleeping (Doze mode / screen off).
     * The engine uses this to relax heartbeat timeouts to zero-drain the battery.
     */
    @JvmStatic external fun notifySleepState(handle: Long, isAsleep: Boolean): Int
    @JvmStatic external fun setBackgroundRestricted(handle: Long, restricted: Boolean): Int

    // ── Remote Explorer (Phase 2) ─────────────────────────────────────────────
    @JvmStatic external fun eventRequestId(event: Long): String?
    @JvmStatic external fun eventSummaryOnly(event: Long): Boolean
    @JvmStatic external fun eventFileId(event: Long): Long
    @JvmStatic external fun eventThumbnailSizePx(event: Long): Int
    @JvmStatic external fun eventOffset(event: Long): Int
    @JvmStatic external fun eventLimit(event: Long): Int
    @JvmStatic external fun eventFileCategory(event: Long): String?
    @JvmStatic external fun eventFileSource(event: Long): String?
    @JvmStatic external fun eventSearchQuery(event: Long): String?
    @JvmStatic external fun sendRemoteFilesResponse(
        handle: Long,
        requestId: String,
        targetDeviceId: String,
        summaryJson: String?,
        filesJson: String?,
        totalMatching: Int,
        error: String?
    ): Int
    @JvmStatic external fun sendRemoteThumbnailResponse(
        handle: Long,
        requestId: String,
        targetDeviceId: String,
        fileId: Long,
        data: ByteArray?,
        error: String?
    ): Int

    // Remote browsing, this phone as the client. All three block; call off the main thread.
    /** The device's listing as RemoteFilesResult JSON, or {"error": "..."}. */
    @JvmStatic external fun queryRemoteFiles(
        handle: Long, deviceId: String, summaryOnly: Boolean, category: String?,
        searchQuery: String?, offset: Int, limit: Int
    ): String?
    @JvmStatic external fun requestRemoteThumbnail(handle: Long, deviceId: String, fileId: Long, sizePx: Int): ByteArray?
    /** Asks the device to send one listed file; it arrives as a normal transfer. */
    @JvmStatic external fun pullRemoteFile(handle: Long, deviceId: String, fileId: Long): Int

    @JvmStatic external fun setSyncEnabled(handle: Long, enabled: Boolean): Int
}
