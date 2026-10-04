// Link All — macOS data models
// PeerViewModel uses human-readable names everywhere.
// Internal device UUIDs are intentionally absent from this layer.

import Foundation

/// View model for a connected/remembered peer.
/// All display fields are human-readable — no raw UUIDs.
struct PeerViewModel: Identifiable, Equatable {
    /// The stable device UUID is kept internally for IPC calls only.
    /// It is NEVER shown in the UI.
    let id: String

    /// Human-readable name shown in all UI (e.g. "Chinmay's Pixel 8").
    /// If the user has renamed the device locally, this reflects that rename.
    let displayName: String

    /// Platform string: "Android", "macOS", "Windows", "Linux"
    let platform: String?

    // ── Lifecycle state ───────────────────────────────────────────────────────
    let trusted: Bool
    let remembered: Bool
    let connected: Bool
    let connectionStatus: String
    let syncEnabled: Bool
    let remoteSyncEnabled: Bool
    let autoConnect: Bool
    let lastError: String?
    let pairingRequested: Bool
    let outgoingPairingWaiting: Bool
    let pairingPin: String?
    let explicitDisconnect: Bool
    /// How the last pairing request ended; nil while one is pending.
    var pairingOutcome: String? = nil

    // ── Timing ────────────────────────────────────────────────────────────────
    let lastSeen: Date?
    let lastDiscoveryAt: Date?
    let lastSync: Date?
    let ip: String?

    /// Short status description shown under the device name.
    var statusLine: String {
        if pairingRequested { return "Wants to pair" }
        if connectionStatus == "connecting" { return "Reconnecting" }
        if connectionStatus == "failed" {
            if trusted { return "Needs attention" }
            return "Trust required"
        }
        if connected && !syncEnabled { return "Connected · Sync paused" }
        if connected && !remoteSyncEnabled { return "Connected · Paused remotely" }
        if connected { return "Connected · Syncing" }
        if explicitDisconnect { return "Disconnected" }
        if trusted && remembered && autoConnect { return "Ready to reconnect" }
        if !syncEnabled { return "Connected · Sync paused" }
        return "Offline"
    }
}

/// Timeline / activity feed entry.
/// Uses friendly device names — raw UUIDs never appear here.
struct TimelineEntry: Identifiable, Equatable {
    let id: UUID = UUID()
    let timestamp: Date
    /// e.g. "Chinmay's Pixel 8" — human-readable name
    let deviceName: String
    let kind: TimelineEntryKind
    let preview: String

    /// Formatted for the activity feed:
    /// "[Pixel 8] copied OTP" — NOT "[e72a91…] copied text"
    var summaryLine: String {
        switch kind {
        case .text:   return "[\(deviceName)] copied text"
        case .image:  return "[\(deviceName)] copied image"
        case .file:   return "[\(deviceName)] received file: \(preview)"
        }
    }

    /// Notification body: "Copied from Chinmay's Pixel 8"
    var notificationBody: String {
        "Copied from \(deviceName)"
    }
}

enum TimelineEntryKind: String {
    case text, image, file
}

/// Trust dialog model.
/// Shows the friendly name prominently; fingerprint in secondary position.
struct TrustPrompt: Identifiable, Equatable {
    let id: UUID = UUID()
    let deviceId: String           // internal — for IPC approval call only
    let deviceName: String         // shown prominently in the dialog
    let fingerprintDisplay: String // shown in secondary "Fingerprint: A4:F2:91…" row
    let publicKeyBytes: Data
}

// ── Activity Feed — IPC model ─────────────────────────────────────────────────
// Mirrors crate::activity::ActivityEntry as a Codable Swift struct.

struct IpcActivityEntry: Codable, Identifiable, Equatable {
    let id: Int64
    let timestamp_ms: Int64
    let device_id: String
    let device_name: String
    let kind: String
    let summary: String
    let content_hash: String?
    let text_preview: String?
    let file_name: String?
    let file_bytes: Int64?
    let transfer_id: String?
    /// Local filesystem path where a received file was saved — nil for non-file events.
    /// Populated by the daemon when it writes a received file to ~/Downloads (or chosen dir).
    let dest_path: String?
    var applied_locally: Bool
    let relay_path: [String]

    var isApplicable: Bool {
        kind == "remote_clipboard_available" && !applied_locally
    }

    var relayPathDisplay: String {
        relay_path.isEmpty ? "" : relay_path.joined(separator: " → ")
    }

    // Allow decoding from daemons that don't yet emit dest_path.
    private enum CodingKeys: String, CodingKey {
        case id, timestamp_ms, device_id, device_name, kind, summary,
             content_hash, text_preview, file_name, file_bytes,
             transfer_id, dest_path, applied_locally, relay_path
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id              = try c.decode(Int64.self,   forKey: .id)
        timestamp_ms    = try c.decode(Int64.self,   forKey: .timestamp_ms)
        device_id       = try c.decode(String.self,  forKey: .device_id)
        device_name     = try c.decode(String.self,  forKey: .device_name)
        kind            = try c.decode(String.self,  forKey: .kind)
        summary         = try c.decode(String.self,  forKey: .summary)
        content_hash    = try c.decodeIfPresent(String.self,  forKey: .content_hash)
        text_preview    = try c.decodeIfPresent(String.self,  forKey: .text_preview)
        file_name       = try c.decodeIfPresent(String.self,  forKey: .file_name)
        file_bytes      = try c.decodeIfPresent(Int64.self,   forKey: .file_bytes)
        transfer_id     = try c.decodeIfPresent(String.self,  forKey: .transfer_id)
        dest_path       = try c.decodeIfPresent(String.self,  forKey: .dest_path)
        applied_locally = try c.decodeIfPresent(Bool.self,    forKey: .applied_locally) ?? false
        relay_path      = try c.decodeIfPresent([String].self, forKey: .relay_path) ?? []
    }
}

// ── Speed Test model ────────────────────────────────────────────────────────────

struct SpeedTestState: Identifiable, Equatable {
    let id: String // peer_id
    let testId: String?
    let phase: String // "Idle", "Sending", "Receiving"
    let bytesTransferred: Int64
    let durationSecs: Int
    
    var speedBps: Int64 {
        if durationSecs == 0 { return 0 }
        return bytesTransferred / Int64(durationSecs)
    }
    
    var speedMbpsString: String {
        let mbps = Double(speedBps * 8) / 1_000_000.0
        return String(format: "%.1f Mbps", mbps)
    }
}

// ── File transfer progress model ──────────────────────────────────────────────

struct FileTransferState: Identifiable, Equatable {
    let id: String          // hex transfer ID
    let fromDeviceName: String
    var fileName: String
    var totalBytes: Int64
    var isDirectory: Bool = false
    var itemCount: Int = 1
    var batchId: String? = nil
    /// Files of a folder already done; nil for single files.
    var doneCount: Int? = nil
    /// Whole folder done, 0...1, from the engine; nil for single files.
    var folderRatio: Double? = nil
    
    var bytesReceived: Int64 = 0
    var percent: Int = 0
    var speedBps: Int64? = nil
    var etaSecs: Int64? = nil
    var status: FileTransferStatus = .incoming
    /// This Mac is sending the file; fromDeviceName is then the receiving device.
    var isOutbound: Bool = false

    /// "Receiving from X" or "Sending to X", for one-line status text.
    var directionLabel: String {
        isOutbound ? "Sending to \(fromDeviceName)" : "Receiving from \(fromDeviceName)"
    }

    var exactRatio: Double {
        // Only a few files of a folder are in flight at once, so a folder's
        // progress is files done, not bytes of the files in flight.
        if isDirectory, let ratio = folderRatio {
            return min(1.0, max(0.0, ratio))
        }
        if isDirectory, let done = doneCount, itemCount > 0 {
            return min(1.0, Double(done) / Double(itemCount))
        }
        if totalBytes > 0 {
            return min(1.0, max(0.0, Double(bytesReceived) / Double(totalBytes)))
        }
        return Double(percent) / 100.0
    }

    /// "37 of 121 files" for a folder in flight, else "121 items".
    var itemLabel: String {
        let noun = itemCount == 1 ? "file" : "files"
        if let done = doneCount { return "\(done) of \(itemCount) \(noun)" }
        return "\(itemCount) items"
    }

    var exactPercentString: String {
        if isDirectory, doneCount != nil {
            return "\(Int((exactRatio * 100).rounded()))%"
        }
        if totalBytes > 0 {
            return String(format: "%.1f%%", exactRatio * 100.0)
        }
        return "\(percent)%"
    }

    var formattedSize: String {
        let mb = Double(totalBytes) / 1_048_576.0
        if mb >= 1.0 { return String(format: "%.1f MB", mb) }
        let kb = Double(totalBytes) / 1_024.0
        if kb >= 1.0 { return String(format: "%.0f KB", kb) }
        return "\(totalBytes) B"
    }

    var formattedProgressSize: String {
        let mbTotal = Double(totalBytes) / 1_048_576.0
        let mbRecv = Double(bytesReceived) / 1_048_576.0
        if mbTotal >= 1.0 { return String(format: "%.2f / %.2f MB", mbRecv, mbTotal) }
        
        let kbTotal = Double(totalBytes) / 1_024.0
        let kbRecv = Double(bytesReceived) / 1_024.0
        if kbTotal >= 1.0 { return String(format: "%.2f / %.2f KB", kbRecv, kbTotal) }
        
        return "\(bytesReceived) / \(totalBytes) B"
    }
}

enum FileTransferStatus: Equatable {
    case incoming, queued, transferring, paused, verifying, complete(destPath: String), failed(reason: String), cancelled
}

// ── Clipboard apply policy preference ────────────────────────────────────────

struct ClipboardPolicy: Equatable {
    var timelineFirstMode: Bool = true   // default: timeline-first
    var autoApply: Bool = true           // default: auto-apply incoming clipboard
    var autoApplyDebounceMs: Int = 500
}

// ── Call continuity ──────────────────────────────────────────────────────────

/// Active phone call state propagated from a connected Android device.
/// Used by the incoming-call banner overlay on macOS.
struct IncomingCallState: Equatable {
    let deviceId: String
    let deviceName: String
    /// "ringing", "offhook", "idle"
    var state: String
    let phoneNumber: String
    let contactName: String

    /// Human-friendly display: contact name if available, otherwise phone number.
    var displayName: String {
        contactName.isEmpty ? (phoneNumber.isEmpty ? "Unknown Caller" : phoneNumber) : contactName
    }

    var isRinging: Bool { state.lowercased() == "ringing" }
    var isOffhook: Bool { state.lowercased() == "offhook" }
}

/// Device battery state (F20).
struct DeviceBatteryState: Equatable, Identifiable {
    var id: String { deviceId }
    let deviceId: String
    let deviceName: String
    let level: Int
    let charging: Bool
}

/// Device network status.
struct DeviceNetworkState: Equatable, Identifiable {
    var id: String { deviceId }
    let deviceId: String
    let deviceName: String
    let networkType: String
}

/// Device storage status.
struct DeviceStorageState: Equatable, Identifiable {
    var id: String { deviceId }
    let deviceId: String
    let deviceName: String
    let imagesBytes: Int64
    let videosBytes: Int64
    let appsBytes: Int64
    let freeBytes: Int64
    let totalBytes: Int64
}
