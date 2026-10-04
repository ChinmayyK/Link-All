// Link All — macOS IPC client
// Communicates with the Rust daemon via Unix domain socket.
// All requests use the IpcRequest JSON protocol defined in ipc.rs.

import Foundation
import AppKit

// ── IPC response model ────────────────────────────────────────────────────────

struct IpcPeerRecord: Codable {
    let id: String
    let friendly_name: String
    let display_name: String?
    let platform: String?
    let status: String
    let last_error: String?
    let trusted: Bool
    let remembered: Bool?
    let sync_enabled: Bool?
    let remote_sync_enabled: Bool?
    let auto_connect: Bool?
    let last_seen: Int?
    let last_discovery_at: Int?
    let last_sync: Int?
    let ip: String?
    let pairing_requested: Bool?
    let outgoing_pairing_waiting: Bool?
    let pairing_pin: String?
    let explicit_disconnect: Bool?
    /// How the last pairing request ended: "accepted", "declined", "cancelled", "expired", "update_needed".
    let pairing_outcome: String?
}

struct IpcStatusResponse: Codable {
    let peers: [IpcPeerRecord]
    let last_sync_at: Int?
    /// Number of remote clipboard items waiting to be applied.
    let pending_clipboard_count: Int?
    /// This device's public-key fingerprint (hex) for display in the Security pane.
    let local_fingerprint: String?
    let local_device_id: String?
    let local_device_name: String?
    /// Active phone call state from a connected Android device (nil if no active call).
    let active_call: IpcActiveCallState?
    let peer_batteries: [IpcPeerBatteryState]?
    let peer_networks: [IpcPeerNetworkState]?
    let peer_storages: [IpcPeerStorageState]?
    let active_transfers: [IpcFileTransferState]?
    let active_speed_tests: [IpcSpeedTestState]?
    /// What stops sync right now, most fundamental first (engine::health).
    let health: [IpcHealthIssue]?
    /// Folder transfers in flight, with how many of their files are done.
    let folders: [IpcFolderProgress]?
}

struct IpcFolderProgress: Codable, Equatable {
    let batch_id: String
    let folder_name: String
    let peer_name: String
    let file_count: Int
    let done_count: Int
    let failed_count: Int
    let outbound: Bool
    /// Whole folder done, 0...1, counting the bytes of files in flight.
    let progress: Double?
    let speed_bps: Int64?
}

struct IpcHealthIssue: Codable, Equatable, Identifiable {
    /// "no_network", "listener_down", "sync_paused", "devices_not_found"
    /// or "connection_blocked".
    let kind: String
    let title: String
    let detail: String
    let device_id: String?
    var id: String { kind + (device_id ?? "") }
}

struct IpcSpeedTestState: Codable {
    let test_id: String?
    let peer_id: String
    let phase: String
    let bytes_transferred: Int64
    let duration_secs: Int
}

struct IpcFileTransferState: Codable {
    let transfer_id: String
    let from_device: String
    let file_name: String
    let bytes_total: Int64
    let bytes_received: Int64
    let percent: Int
    let status: String
    let is_directory: Bool?
    let item_count: Int?
    let batch_id: String?
}

/// Active call state from the daemon's status response.
struct IpcActiveCallState: Codable {
    let device_id: String
    let device_name: String
    let state: String
    let number: String
    let contact_name: String
}

/// Peer battery status.
struct IpcPeerBatteryState: Codable {
    let device_id: String
    let device_name: String
    let level: Int
    let charging: Bool
}

/// Peer network status.
struct IpcPeerNetworkState: Codable {
    let device_id: String
    let device_name: String
    let network_type: String
}

/// Peer storage status.
struct IpcPeerStorageState: Codable {
    let device_id: String
    let device_name: String
    let images_bytes: Int64
    let videos_bytes: Int64
    let apps_bytes: Int64
    let free_bytes: Int64
    let total_bytes: Int64
}

// ── Remote File Explorer models (Phase 3) ─────────────────────────────────────

struct IpcRemoteFileCategoryCounts: Codable {
    let images: UInt32
    let videos: UInt32
    let audio: UInt32
    let documents: UInt32
    let apks: UInt32
    let archives: UInt32
}

struct IpcRemoteFileSourceCounts: Codable {
    let whatsapp: UInt32
    let downloads: UInt32
    let camera: UInt32
}

struct IpcRemoteFilesSummary: Codable {
    let type_counts: IpcRemoteFileCategoryCounts
    let source_counts: IpcRemoteFileSourceCounts
}

struct IpcRemoteFileEntry: Codable, Identifiable {
    let file_id: UInt64
    let display_name: String
    let size_bytes: UInt64
    let mime_type: String
    let date_modified: UInt64
    let category: String
    let source: String
    let content_uri: String

    var id: UInt64 { file_id }
}

struct IpcRemoteFilesResult: Codable {
    let summary: IpcRemoteFilesSummary?
    let files: [IpcRemoteFileEntry]
    let total_matching: UInt32
    let error: String?
}

struct IpcRemoteThumbnailResult: Codable {
    let file_id: UInt64
    let data_base64: String?
    let error: String?
}

struct IpcCameraFrameResponse: Codable {
    let frame_base64: String?
}

struct IpcResponse<T: Codable>: Codable {
    let status: String
    let data: T?
    let message: String?
}

// ── IPC Client ────────────────────────────────────────────────────────────────

final class LinkAllIPCClient {
    static let shared = LinkAllIPCClient()

    private var socketPath: String {
        var candidates: [String] = []
        if let runtime = ProcessInfo.processInfo.environment["XDG_RUNTIME_DIR"] {
            candidates.append("\(runtime)/linkall.sock")
        }
        candidates.append("/tmp/linkall-\(getuid())/linkall.sock")
        if let home = ProcessInfo.processInfo.environment["HOME"] {
            candidates.append("\(home)/.linkall.sock")
        }
        for path in candidates {
            if FileManager.default.fileExists(atPath: path) {
                return path
            }
        }
        return candidates.first ?? "/tmp/linkall-\(getuid())/linkall.sock"
    }

    func status() async throws -> IpcStatusResponse {
        let raw = try await send(cmd: ["cmd": "status"])
        let resp = try await decode(IpcResponse<IpcStatusResponse>.self, from: raw)
        guard let data = resp.data else { throw LinkAllIPCError.noData }
        return data
    }

    func ping() async throws {
        _ = try await send(cmd: ["cmd": "ping"])
    }

    func disconnectPeer(deviceId: String) async throws {
        _ = try await send(cmd: ["cmd": "disconnect_peer", "device_id": deviceId])
    }

    /// Send accept or decline call action to a ringing Android device.
    func callAction(action: String, targetDevice: String) async throws {
        _ = try await send(cmd: [
            "cmd": "call_action",
            "action": action,
            "target_device": targetDevice
        ])
    }

    func pauseSync(deviceId: String) async throws {
        _ = try await send(cmd: ["cmd": "pause_sync_peer", "device_id": deviceId])
    }

    func resumeSync(deviceId: String) async throws {
        _ = try await send(cmd: ["cmd": "resume_sync_peer", "device_id": deviceId])
    }

    func forgetDevice(deviceId: String) async throws {
        _ = try await send(cmd: ["cmd": "forget_device", "device_id": deviceId])
    }

    func revokeDevice(deviceId: String) async throws {
        _ = try await send(cmd: ["cmd": "revoke_trusted_device", "device_id": deviceId])
    }

    func setAutoConnect(deviceId: String, enabled: Bool) async throws {
        _ = try await send(cmd: ["cmd": "set_auto_connect", "device_id": deviceId, "enabled": enabled])
    }

    func renameDevice(deviceId: String, displayName: String) async throws {
        _ = try await send(cmd: ["cmd": "rename_trusted_device", "device_id": deviceId, "display_name": displayName])
    }

    func approveTrust(deviceId: String, deviceName: String, pubkeyBytes: Data) async throws {
        _ = try await send(cmd: [
            "cmd": "trust_peer",
            "device_id": deviceId,
            "device_name": deviceName,
            "pubkey_base64": pubkeyBytes.base64EncodedString()
        ])
    }

    func rejectTrust(deviceId: String) async throws {
        _ = try await send(cmd: ["cmd": "reject_peer", "device_id": deviceId])
    }

    func sendPairingRequest(deviceId: String) async throws {
        _ = try await send(cmd: ["cmd": "send_pairing_request", "device_id": deviceId])
    }

    func cancelPairingRequest(deviceId: String) async throws {
        _ = try await send(cmd: ["cmd": "cancel_pairing_request", "device_id": deviceId])
    }

    func respondToPairing(deviceId: String, accepted: Bool) async throws {
        _ = try await send(cmd: [
            "cmd": "respond_to_pairing",
            "device_id": deviceId,
            "accepted": accepted
        ])
    }
    
    func generateQrToken() async throws -> String {
        let resp = try await send(cmd: [
            "cmd": "generate_qr_token"
        ])
        
        struct TokenResponse: Decodable {
            let data: TokenData?
            let token: String?
        }
        struct TokenData: Decodable {
            let token: String
        }
        
        let parsed = try await decode(TokenResponse.self, from: resp)
        if let token = parsed.data?.token ?? parsed.token {
            return token
        }
        throw DecodingError.dataCorrupted(.init(codingPath: [], debugDescription: "No token found in response"))
    }
    
    func trustPeerFromQr(deviceId: String, token: String) async throws {
        _ = try await send(cmd: [
            "cmd": "trust_peer_from_qr",
            "device_id": deviceId,
            "token": token
        ])
    }

    // ── Activity Feed ─────────────────────────────────────────────────────────

    func activityRecent(limit: Int = 50) async throws -> [IpcActivityEntry] {
        let raw = try await send(cmd: ["cmd": "activity_recent", "limit": limit])
        let resp = try await decode(IpcResponse<[IpcActivityEntry]>.self, from: raw)
        return resp.data ?? []
    }

    func activitySince(sinceId: Int64) async throws -> [IpcActivityEntry] {
        let raw = try await send(cmd: ["cmd": "activity_since", "since_id": sinceId])
        let resp = try await decode(IpcResponse<[IpcActivityEntry]>.self, from: raw)
        return resp.data ?? []
    }

    func pendingRemoteClipboards() async throws -> [IpcActivityEntry] {
        let raw = try await send(cmd: ["cmd": "pending_remote_clipboards"])
        let resp = try await decode(IpcResponse<[IpcActivityEntry]>.self, from: raw)
        return resp.data ?? []
    }

    /// The full text of a received clipboard item, by its activity id. The
    /// feed's text_preview is cut at 400 characters; this is not.
    func incomingClipboardText(id: Int64) async throws -> String? {
        struct Payload: Codable { let type: String; let text: String? }
        let raw = try await send(cmd: ["cmd": "incoming_clipboard", "id": id])
        let resp = try await decode(IpcResponse<Payload>.self, from: raw)
        guard let p = resp.data, p.type == "text" else { return nil }
        return p.text
    }

    // ── Timeline-first clipboard ──────────────────────────────────────────────

    /// Apply a remote clipboard item from the activity feed by its content hash.
    /// The engine writes the item to the local clipboard and marks it applied.
    func applyClipboard(contentHash: String) async throws {
        _ = try await send(cmd: ["cmd": "apply_clipboard", "content_hash": contentHash])
    }

    // ── Settings ──────────────────────────────────────────────────────────────

    func setTimelineFirstMode(enabled: Bool) async throws {
        _ = try await send(cmd: ["cmd": "set_timeline_first_mode", "enabled": enabled])
    }

    func setAutoApplyClipboard(enabled: Bool) async throws {
        _ = try await send(cmd: ["cmd": "set_auto_apply_clipboard", "enabled": enabled])
    }

    // ── File Transfer ─────────────────────────────────────────────────────────

    /// Send a file to a specific peer, or all peers when targetDevice is nil.
    func sendFile(
        url: URL, 
        targetDeviceId: String? = nil,
        relativePath: String? = nil,
        batchId: String? = nil,
        isDirectory: Bool = false,
        itemCount: Int = 1
    ) async throws -> String {
        var cmd: [String: Any] = [
            "cmd":  "send_file_path",
            "path": url.path,
            "name": relativePath ?? url.lastPathComponent,
            "mime": isDirectory ? "inode/directory" : mimeType(for: url),
            "is_directory": isDirectory,
            "item_count": itemCount
        ]
        if let t = targetDeviceId { cmd["target_device"] = t }
        if let b = batchId { cmd["batch_id"] = b }
        let raw = try await send(cmd: cmd)
        let resp = try await decode(IpcResponse<String>.self, from: raw)
        return resp.data ?? ""
    }

    /// Send a folder and everything in it. The engine walks it and sends a
    /// few files at a time; the folder's files share one batch id.
    func sendFolder(url: URL, targetDeviceId: String?) async throws {
        var cmd: [String: Any] = ["cmd": "send_folder", "path": url.path]
        if let t = targetDeviceId { cmd["target_device"] = t }
        _ = try await send(cmd: cmd)
    }

    /// Ask a connected device to open a link. The device answers with a
    /// feedback event; the daemon opens links it receives itself.
    func openUrlOnDevice(_ url: String, targetDeviceId: String) async throws {
        _ = try await send(cmd: ["cmd": "open_url_on_device", "target_device": targetDeviceId, "url": url])
    }

    /// Tell a device whether we opened the link it sent.
    func ackOpenUrlOnDevice(requesterDeviceId: String, success: Bool, error: String? = nil) async throws {
        var cmd: [String: Any] = ["cmd": "ack_open_url_on_device", "requester_device": requesterDeviceId, "success": success]
        if let error { cmd["error"] = error }
        _ = try await send(cmd: cmd)
    }

    /// Stop a folder transfer, sending or receiving.
    func cancelFolder(batchId: String) async throws {
        _ = try await send(cmd: ["cmd": "cancel_folder", "batch_id": batchId])
    }

    func acceptFileTransfer(transferId: String) async throws {
        _ = try await send(cmd: ["cmd": "accept_file_transfer", "transfer_id": transferId])
    }

    func rejectFileTransfer(transferId: String, reason: String = "user rejected") async throws {
        _ = try await send(cmd: ["cmd": "reject_file_transfer",
                                 "transfer_id": transferId, "reason": reason])
    }

    func cancelFileTransfer(transferId: String) async throws {
        _ = try await send(cmd: ["cmd": "cancel_file_transfer", "transfer_id": transferId])
    }

    func pauseFileTransfer(transferId: String) async throws {
        _ = try await send(cmd: ["cmd": "pause_file_transfer", "transfer_id": transferId])
    }

    func resumeFileTransfer(transferId: String) async throws {
        _ = try await send(cmd: ["cmd": "resume_file_transfer", "transfer_id": transferId])
    }

    func latestCameraFrame(targetDeviceId: String? = nil) async throws -> Data? {
        var cmd: [String: Any] = ["cmd": "latest_camera_frame"]
        if let id = targetDeviceId {
            cmd["target_device"] = id
        }
        let raw = try await send(cmd: cmd)
        let resp = try await decode(IpcResponse<IpcCameraFrameResponse>.self, from: raw)
        guard let b64 = resp.data?.frame_base64 else { return nil }
        return Data(base64Encoded: b64)
    }

    // ── Remote Explorer API ───────────────────────────────────────────────────

    func queryRemoteFiles(
        targetDevice: String,
        summaryOnly: Bool = false,
        category: String? = nil,
        source: String? = nil,
        searchQuery: String? = nil,
        offset: UInt32 = 0,
        limit: UInt32 = 50
    ) async throws -> IpcRemoteFilesResult {
        var cmd: [String: Any] = [
            "cmd": "remote_files_query",
            "target_device": targetDevice,
            "summary_only": summaryOnly,
            "offset": offset,
            "limit": limit
        ]
        if let cat = category { cmd["category"] = cat }
        if let src = source { cmd["source"] = src }
        if let query = searchQuery, !query.isEmpty { cmd["search_query"] = query }
        
        let raw = try await send(cmd: cmd)
        let resp = try await decode(IpcResponse<IpcRemoteFilesResult>.self, from: raw)
        if let err = resp.message { throw LinkAllIPCError.serverError(err) }
        guard let data = resp.data else { throw LinkAllIPCError.noData }
        return data
    }

    func requestRemoteThumbnail(targetDevice: String, fileId: UInt64, sizePx: UInt32 = 256) async throws -> Data? {
        let cmd: [String: Any] = [
            "cmd": "remote_thumbnail_request",
            "target_device": targetDevice,
            "file_id": fileId,
            "size_px": sizePx
        ]
        let raw = try await send(cmd: cmd)
        let resp = try await decode(IpcResponse<IpcRemoteThumbnailResult>.self, from: raw)
        if let err = resp.message { throw LinkAllIPCError.serverError(err) }
        guard let b64 = resp.data?.data_base64 else { return nil }
        return Data(base64Encoded: b64)
    }

    func requestRemoteFilePull(targetDevice: String, fileId: UInt64) async throws {
        let cmd: [String: Any] = [
            "cmd": "remote_file_pull_request",
            "target_device": targetDevice,
            "file_id": fileId
        ]
        _ = try await send(cmd: cmd)
    }

    func requestRemoteFileAction(targetDevice: String, fileId: UInt64, action: String, newName: String?) async throws {
        var cmd: [String: Any] = [
            "cmd": "remote_file_action_request",
            "target_device": targetDevice,
            "file_id": fileId,
            "action": action
        ]
        if let nn = newName {
            cmd["new_name"] = nn
        }
        _ = try await send(cmd: cmd)
    }

    // ── Internal Sender ───────────────────────────────────────────────────────────────

    private func mimeType(for url: URL) -> String {
        let ext = url.pathExtension.lowercased()
        let map: [String: String] = [
            "pdf": "application/pdf", "png": "image/png", "jpg": "image/jpeg",
            "jpeg": "image/jpeg", "gif": "image/gif", "txt": "text/plain",
            "zip": "application/zip", "tar": "application/x-tar",
            "gz": "application/gzip", "mp4": "video/mp4", "mov": "video/quicktime"
        ]
        return map[ext] ?? "application/octet-stream"
    }

    // ── Transport (internal so store can issue ad-hoc commands) ──────────────

    private lazy var persistentConnection = PersistentIPCConnection(socketPath: self.socketPath)

    func send(cmd: [String: Any]) async throws -> Data {
        var lastError: Error = LinkAllIPCError.connectionFailed
        for attempt in 0..<3 {
            do {
                return try await persistentConnection.send(cmd: cmd)
            } catch LinkAllIPCError.connectionFailed, LinkAllIPCError.disconnected, LinkAllIPCError.socketFailed {
                lastError = LinkAllIPCError.connectionFailed
                if attempt < 2 {
                    try? await Task.sleep(nanoseconds: 200_000_000) // 200 ms
                }
            } catch {
                throw error   // non-connection errors propagate immediately
            }
        }
        throw lastError
    }

    /// Perform JSON decoding on a background thread to avoid blocking the caller's actor (e.g. MainActor).
    private func decode<T: Decodable>(_ type: T.Type, from data: Data) async throws -> T {
        try await Task.detached(priority: .userInitiated) {
            try JSONDecoder().decode(type, from: data)
        }.value
    }
}

enum LinkAllIPCError: Error, Equatable, LocalizedError {
    case socketFailed
    case connectionFailed
    case noData
    case disconnected
    case serverError(String)

    var errorDescription: String? {
        switch self {
        case .socketFailed:
            return "Failed to communicate with the local background service."
        case .connectionFailed:
            return "Could not connect to the local background service."
        case .noData:
            return "Received an empty response from the local service."
        case .disconnected:
            return "The background service disconnected unexpectedly."
        case .serverError(let msg):
            if msg == "PermissionError" {
                return "Permission Required: The remote device has not granted file access permissions. Please accept the prompt on the device."
            }
            return msg
        }
    }
}

// MARK: - Dashboard extensions

extension LinkAllIPCClient {

    /// Initiate an outbound TCP connection to a manually-entered address.
    /// Address format: "host:port" or bare "host" (uses daemon's configured port).
    /// Daemon resolves DNS — hostname or IP both work.
    func connectManual(address: String) async throws {
        let parts = address.split(separator: ":", maxSplits: 1)
        var cmd: [String: Any] = ["cmd": "connect_manual"]
        if parts.count == 2, let port = Int(parts[1]) {
            cmd["host"] = String(parts[0])
            cmd["port"] = port
        } else {
            cmd["host"] = address
        }
        _ = try await send(cmd: cmd)
    }
    
    /// Reconnect to a peer by its ID, using all historical endpoints.
    func reconnectPeer(deviceId: String) async throws {
        let cmd: [String: Any] = ["cmd": "reconnect_peer", "device_id": deviceId]
        _ = try await send(cmd: cmd)
    }

    /// Re-push a previously-received clipboard item (by hash) to connected peers.
    func sendClipboardByHash(hash: String, targetDeviceId: String?) async throws {
        var cmd: [String: Any] = ["cmd": "push_clipboard_hash", "hash": hash]
        if let id = targetDeviceId { cmd["target_device_id"] = id }
        _ = try await send(cmd: cmd)
    }

    /// Push the current local clipboard text to connected peers.
    ///
    /// The text is read here and sent inline: the daemon has no OS clipboard
    /// access of its own, so its `push_clipboard` command finds nothing to send.
    func sendClipboardCurrent(targetDeviceId: String?) async throws {
        let (text, image) = await MainActor.run { () -> (String?, Data?) in
            let pb = NSPasteboard.general
            if let text = pb.string(forType: .string), !text.isEmpty { return (text, nil) }
            // Screenshots and "Copy Image" put only image data on the pasteboard.
            if let png = pb.data(forType: .png) { return (nil, png) }
            if let tiff = pb.data(forType: .tiff),
               let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]) {
                return (nil, png)
            }
            return (nil, nil)
        }
        if let text {
            try await sendPushText(text, targetDeviceId: targetDeviceId)
        } else if let image {
            // The daemon's push_image has no per-peer variant; images go to every connected peer.
            try await sendPushImage(image, mimeType: "image/png")
        }
    }

    /// Push image bytes to all connected peers.
    func sendPushImage(_ data: Data, mimeType: String) async throws {
        _ = try await send(cmd: [
            "cmd":         "push_image",
            "mime":        mimeType,
            "data_base64": data.base64EncodedString(),
        ])
    }

    /// Push arbitrary text to connected peers without reading the OS clipboard.
    func sendPushText(_ text: String, targetDeviceId: String?) async throws {
        var cmd: [String: Any] = ["cmd": "push_text", "text": text]
        if let id = targetDeviceId {
            cmd["cmd"] = "push_text_to"
            cmd["target"] = id
        }
        _ = try await send(cmd: cmd)
    }

    /// Persist settings changes to the daemon — partial patch, only set fields are applied.
    func saveSettings(_ snapshot: LinkAllSettingsSnapshot) async throws {
        let cmd: [String: Any] = [
            "cmd":                              "save_settings",
            "port":                             snapshot.port,
            "device_name":                      snapshot.deviceName,
            "sync_enabled":                     snapshot.syncEnabled,
            "sync_text":                        snapshot.syncText,
            "sync_images":                      snapshot.syncImages,
            "sync_files":                       snapshot.syncFiles,
            "history_limit":                    snapshot.historyLimit,
            "max_history_text_bytes":           snapshot.maxHistoryTextBytes,
            "max_payload_bytes":                snapshot.maxPayloadBytes,
            "clipboard_poll_ms":                snapshot.clipboardPollMs,
            "max_pushes_per_sec":               snapshot.maxPushesPerSec,
            "rate_limit_burst":                 snapshot.rateLimitBurst,
            "smart_sync_duplicate_window_ms":   snapshot.smartSyncDuplicateWindowMs,
            "smart_sync_debounce_ms":           snapshot.smartSyncDebounceMs,
            "block_sensitive_text":             snapshot.blockSensitiveText,
            "require_tofu_confirmation":        snapshot.requireTofuConfirmation,
            "show_receive_notification":        snapshot.showReceiveNotification,
            "ignore_patterns":                  snapshot.ignorePatterns,
        ]
        _ = try await send(cmd: cmd)
    }
    func startSpeedTest(deviceId: String, durationSecs: Int = 10) async throws {
        let cmd: [String: Any] = [
            "cmd": "start_speed_test",
            "device_id": deviceId,
            "duration_secs": durationSecs
        ]
        _ = try await send(cmd: cmd)
    }
}
