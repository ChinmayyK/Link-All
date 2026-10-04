// Link All — macOS app state store
// Bridges IPC responses to SwiftUI view models.
// Enforces: no raw UUIDs in public-facing state.

import Foundation
import Combine
import AppKit
import SwiftUI
import ServiceManagement

// MARK: - Notification names

extension Notification.Name {
    /// Posted by LinkAllStore.openHistoryPanel() — observed by AppDelegate.
    static let linkallOpenHistoryPanel = Notification.Name("app.linkall.openHistoryPanel")
    static let linkallOpenCommandPalette = Notification.Name("app.linkall.openCommandPalette")
    static let linkallEnsureDaemon = Notification.Name("app.linkall.ensureDaemon")
}

@MainActor
final class LinkAllStore: ObservableObject {

    // ── Core state ────────────────────────────────────────────────────────────
    @Published var peers: [PeerViewModel] = []
    @Published var activeTrustPrompt: TrustPrompt? = nil
    @Published var isRunning: Bool = false
    @Published var statusLine: String = "Starting…"
    @Published var connectedCount: Int = 0
    /// Number of remote clipboard items pending apply — badges the menu bar icon.
    @Published var pendingClipboardCount: Int = 0
    /// This Mac's public-key fingerprint — shown in Security pane for peer verification.
    /// Populated from the daemon status response; nil until first successful poll.
    @Published var localFingerprint: String? = nil
    @Published var localDeviceId: String? = nil
    @Published var localDeviceName: String? = nil

    // ── Activity feed ─────────────────────────────────────────────────────────
    @Published var activityFeed: [IpcActivityEntry] = []
    @Published var activeTransfers: [FileTransferState] = []
    @Published var healthIssues: [IpcHealthIssue] = []
    @Published var folderProgress: [String: IpcFolderProgress] = [:]
    
    var batchedTransfers: [FileTransferState] {
        var batches: [String: FileTransferState] = [:]
        var singles: [FileTransferState] = []
        
        for t in activeTransfers {
            if let bid = t.batchId {
                if var existing = batches[bid] {
                    existing.bytesReceived += t.bytesReceived
                    existing.totalBytes += t.totalBytes
                    // Merge status (prioritize transferring/failed over pending/complete)
                    if case .failed = t.status { existing.status = t.status }
                    else if case .transferring = t.status, case .complete = existing.status { existing.status = .transferring }
                    
                    if existing.totalBytes > 0 {
                        existing.percent = Int((Double(existing.bytesReceived) / Double(existing.totalBytes)) * 100.0)
                    }
                    batches[bid] = existing
                } else {
                    var initial = t
                    // For the UI, the folder name is the first component of the relative path
                    initial.fileName = t.fileName.components(separatedBy: "/").first ?? t.fileName
                    initial.isDirectory = true
                    if let folder = folderProgress[bid] {
                        initial.fileName = folder.folder_name
                        initial.itemCount = folder.file_count
                        initial.doneCount = folder.done_count + folder.failed_count
                        initial.folderRatio = folder.progress
                        if let speed = folder.speed_bps { initial.speedBps = speed }
                    }
                    batches[bid] = initial
                }
            } else {
                singles.append(t)
            }
        }
        
        return singles + Array(batches.values).sorted(by: { $0.id < $1.id })
    }
    @Published var activeSpeedTests: [SpeedTestState] = []
    @Published var clipboardPolicy = ClipboardPolicy()

    // ── Dashboard UI state ────────────────────────────────────────────────────
    @Published var selectedSection: DashboardSection = .devices
    @Published var selectedPendingDevice: ManagedDevice? = nil
    @Published var toasts: [ToastItem] = []
    @Published var manualConnectAddress: String = ""
    @Published var settings: LinkAllSettingsSnapshot? = nil
    @Published var quickSendContext: QuickSendContext? = nil
    @Published var pendingTrustRequest: DeviceDetailSnapshot? = nil
    @Published var dashboardStatus: StatusSnapshot? = nil
    @Published var pinnedItemIds: Set<Int64> = []
    /// Active phone call from a connected Android device (nil = no active call).
    @Published var activeCall: IncomingCallState? = nil
    private var suppressCallUpdatesUntil: Date? = nil
    /// Battery levels for connected peer devices.
    @Published var peerBatteries: [DeviceBatteryState] = []
    @Published var peerNetworks: [DeviceNetworkState] = []
    @Published var peerStorages: [DeviceStorageState] = []
    
    // Cache for Remote File Explorer to prevent re-fetching on navigation
    @Published var remoteFilesCache: [String: IpcRemoteFilesResult] = [:]

    private var lastActivityId: Int64 = 0
    private var lastMirroredAutoAppliedEntryId: Int64 = 0
    private let ipc: LinkAllIPCClient
    private var pollTimer: Timer?
    private var lastRefreshTime: Date = Date.distantPast
    private var pendingRename: PeerViewModel? = nil
    private var toastWorkItems: [UUID: DispatchWorkItem] = [:]
    private var ipcFailureCount: Int = 0
    private var isCameraPolling = false

    @Published var showQrCodeSheet: Bool = false
    @AppStorage("lastUsedDeviceId") private var lastUsedDeviceId: String = ""

    private var cameraWindowClosedObserver: Any?

    init(ipc: LinkAllIPCClient = .shared) {
        self.ipc = ipc
        startPolling()
        cameraWindowClosedObserver = NotificationCenter.default.addObserver(forName: .linkallCameraWindowClosed, object: nil, queue: .main) { [weak self] _ in
            Task { @MainActor [weak self] in
                self?.stopCameraPolling()
            }
        }
    }
    
    deinit {
        if let obs = cameraWindowClosedObserver {
            NotificationCenter.default.removeObserver(obs)
        }
    }

    // MARK: - Computed / Bridging

    var connectionBanner: String { statusLine }
    var devices: [ManagedDevice] { peers.map(ManagedDevice.init) }
    // outgoingPairingWaiting on a trusted, connected peer means the session
    // is up but the peer no longer trusts us - nothing works until they
    // approve, so it must not be presented as connected.
    // Sorted by name so the sidebar doesn't reshuffle each time a device reconnects.
    var connectedDevices: [ManagedDevice] {
        devices
            .filter { $0.isConnected && $0.trustState == .trusted && !$0.outgoingPairingWaiting }
            .sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
    }
    var pendingDevices: [ManagedDevice] { 
        devices.filter { device in
            if device.trustState == .trusted { return false } // Trusted devices go to connectedDevices or are hidden when disconnected
            if device.pairingRequested || device.outgoingPairingWaiting { return true }
            
            // For untrusted devices not actively pairing, only show them if they are actively broadcasting mDNS right now.
            // Since mDNS can be flaky, we give a 60-second grace period.
            let now = Date()
            if let lastDiscovery = device.lastDiscoveryAt, now.timeIntervalSince(lastDiscovery) < 60 {
                return true
            }
            if let lastSeen = device.lastSeen, now.timeIntervalSince(lastSeen) < 60 {
                return true
            }
            return false
        }
    }
    // Paired devices that are on the network right now but not connected
    // (e.g. a firewall drops the dial). Hiding them made a paired machine
    // simply vanish from the sidebar with no hint why.
    var nearbyTrustedDevices: [ManagedDevice] {
        let now = Date()
        return devices.filter { device in
            guard device.trustState == .trusted else { return false }
            if device.outgoingPairingWaiting { return true }
            guard !device.isConnected else { return false }
            let seen = [device.lastDiscoveryAt, device.lastSeen].compactMap { $0 }.max()
            return seen.map { now.timeIntervalSince($0) < 60 } ?? false
        }
    }
    var status: StatusSnapshot? { dashboardStatus }

    /// Make a device the one the dashboard shows and sends to.
    func selectDevice(_ id: String) {
        objectWillChange.send() // @AppStorage inside an ObservableObject doesn't publish.
        lastUsedDeviceId = id
    }

    var defaultTargetDevice: ManagedDevice? {
        let connected = connectedDevices
        if !lastUsedDeviceId.isEmpty, let last = connected.first(where: { $0.id == lastUsedDeviceId }) {
            return last
        }
        return connected.first
    }

    var timeline: [TimelineItem] {
        // A file this Mac sends logs a "started" entry here and a "complete" entry
        // reported by the receiver; show it once, under the device that sent it.
        let started = Set(activityFeed.compactMap { $0.kind == "file_transfer_started" ? $0.transfer_id : nil })
        return activityFeed
            .filter { !($0.kind == "file_transfer_complete" && $0.transfer_id.map(started.contains) == true) }
            .prefix(80)
            .map { TimelineItem(entry: $0, pinned: pinnedItemIds.contains($0.id)) }
    }

    // MARK: - Lifecycle

    // ── Clipboard watcher ─────────────────────────────────────────────────────
    // Polls NSPasteboard on a background thread and fires callbacks on change.
    // ClipboardSetter prevents echo: applying a received clipboard increments
    // suppressCount so the watcher skips that event and doesn't re-push it.
    private let watcher = ClipboardWatcher()
    private lazy var setter = ClipboardSetter(watcher: watcher)

    func start() {
        startPolling()
        startWatchingClipboard()
        Task {
            try? await Task.sleep(nanoseconds: 500_000_000)
            await refresh()
            scanForDevices()
        }
    }

    func stop() {
        pollTimer?.invalidate()
        pollTimer = nil
        watcher.stop()
    }
    
    func reconnect() {
        // Restart background polling — stop then re-start after a short delay
        stop()
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
            self.start()
        }
    }
    
    func rescanNetwork() {
        scanForDevices()
    }
    
    func enableAutoApply() {
        if !clipboardPolicy.autoApply {
            Task {
                await setAutoApplyClipboard(enabled: true)
            }
        }
    }

    // ── Analytics ─────────────────────────────────────────────────────────────
    @AppStorage("totalBytesTransferred") public var totalBytesTransferred: Int = 0
    @AppStorage("totalFilesTransferred") public var totalFilesTransferred: Int = 0

    // ── Clipboard watcher setup ───────────────────────────────────────────────

    private func startWatchingClipboard() {
        watcher.onTextChange = { [weak self] text in
            self?.handleLocalClipboardText(text)
        }
        watcher.onImageChange = { [weak self] data, mimeType in
            self?.handleLocalClipboardImage(data, mimeType: mimeType)
        }
        watcher.onFileChange = { [weak self] urls in
            self?.handleLocalClipboardFiles(urls)
        }
        watcher.start()
    }

    /// User copied text — update quick-send strip and push to peers if connected.
    private func handleLocalClipboardText(_ text: String) {
        // Always update the quick-send strip (shown in history panel).
        quickSendContext = QuickSendContext(text: text, timestamp: Date())
        guard UserDefaults.standard.bool(forKey: "magicClipboardEnabled") else { return }
        // Pushed even with no device connected: the daemon records the copy in
        // the activity feed first, so local clipboard history stays complete.
        Task { [weak self] in
            guard let self else { return }
            // Text goes inline — the daemon cannot read the OS clipboard itself.
            _ = try? await self.ipc.sendPushText(text, targetDeviceId: nil)
        }
    }

    private func handleLocalClipboardImage(_ data: Data, mimeType: String) {
        guard UserDefaults.standard.bool(forKey: "magicClipboardEnabled") else { return }
        guard connectedCount > 0 else { return }
        Task { [weak self] in
            guard let self else { return }
            _ = try? await self.ipc.sendPushImage(data, mimeType: mimeType)
        }
    }

    private func handleLocalClipboardFiles(_ urls: [URL]) {
        guard UserDefaults.standard.bool(forKey: "magicClipboardEnabled") else { return }
        guard connectedCount > 0 else { return }
        let readable = urls.filter { FileManager.default.fileExists(atPath: $0.path) }
        guard !readable.isEmpty else { return }
        if readable.count == 1, let first = readable.first {
            sendFile(url: first)
            return
        }

        Task {
            guard let archiveURL = await buildClipboardArchive(from: readable) else {
                showToast(
                    title: "Archive failed",
                    body: "Could not bundle copied files for transfer",
                    tint: CRTheme.accentOrange
                )
                return
            }
            sendFile(url: archiveURL)
            showToast(
                title: "Bundled \(readable.count) files",
                body: "Sending a zip archive to connected devices",
                tint: CRTheme.accentBlue
            )
        }
    }

    /// Apply received clipboard text locally without triggering the watcher callback.
    func applyClipboardLocally(text: String) { setter.setText(text) }
    func applyClipboardImageLocally(_ data: Data, mimeType: String) { setter.setImage(data, mimeType: mimeType) }

    /// Adaptive poll rate:
    ///  • 0.25 s when peers are connected — near-instant sync feedback in the UI
    ///  • 3.0 s when idle — no peers, daemon is quiet, conserve resources
    /// The timer reschedules itself after each tick so the interval adjusts
    /// immediately when the connection state changes.
    private func startPolling() {
        schedulePollTick()
    }

    private func schedulePollTick() {
        pollTimer?.invalidate()
        let hasActiveTransfers = activeTransfers.contains { t in
            switch t.status {
            case .transferring, .verifying, .incoming: return true
            default: return false
            }
        }
        let interval: TimeInterval = hasActiveTransfers ? 0.25 : (connectedCount > 0 ? 1.5 : 3.0)
        pollTimer = Timer.scheduledTimer(withTimeInterval: interval, repeats: false) { [weak self] _ in
            Task { @MainActor [weak self] in
                await self?.refresh()
                self?.schedulePollTick()   // reschedule with updated interval
            }
        }
    }

    func refresh() async {
        let now = Date()
        if now.timeIntervalSince(lastRefreshTime) < 0.08 {
            return
        }
        lastRefreshTime = now
        do {
            let s = try await ipc.status()
            ipcFailureCount = 0
            isRunning      = true
            connectedCount = s.peers.filter { $0.status == "connected" && $0.trusted }.count
            let reconnectingCount = s.peers.filter { $0.status == "connecting" }.count
            let reconnectableCount = s.peers.filter {
                $0.status != "connected" &&
                $0.status != "connecting" &&
                $0.trusted &&
                ($0.remembered ?? true) &&
                ($0.auto_connect ?? true)
            }.count
            statusLine = whenStatusLine(
                connectedCount: connectedCount,
                reconnectingCount: reconnectingCount,
                reconnectableCount: reconnectableCount
            )
            var seenIds = Set<String>()
            var uniquePeers = [PeerViewModel]()
            // Deduplicate peers by ID (handles cases where a device is discovered via both IPv4 and IPv6)
            for p in s.peers {
                if !seenIds.contains(p.id) {
                    seenIds.insert(p.id)
                    uniquePeers.append(makePeerViewModel(p))
                }
            }
            if self.peers != uniquePeers {
                self.peers = uniquePeers
            }
            
            if connectedCount == 0 {
                watcher.stop()
            } else {
                watcher.start()
            }

            pendingClipboardCount = s.pending_clipboard_count ?? 0
            let health = s.health ?? []
            if health != healthIssues { healthIssues = health }
            let folders = Dictionary((s.folders ?? []).map { ($0.batch_id, $0) }, uniquingKeysWith: { a, _ in a })
            if folders != folderProgress { folderProgress = folders }
            if let fp = s.local_fingerprint { localFingerprint = fp }
            if let id = s.local_device_id { localDeviceId = id }
            if let name = s.local_device_name { localDeviceName = name }
            
            if let ats = s.active_transfers {
                activeTransfers = ats.map { t in
                    let status: FileTransferStatus
                    switch t.status {
                    case "pending": status = .incoming
                    case "queued": status = .queued
                    case "paused": status = .paused
                    case "incoming": status = .incoming
                    case "verifying": status = .verifying
                    case "failed": status = .failed(reason: "Unknown Error")
                    case "cancelled": status = .cancelled
                    case "complete": status = .complete(destPath: "")
                    default: status = .transferring
                    }
                    return FileTransferState(
                        id: t.transfer_id,
                        fromDeviceName: t.from_device,
                        fileName: t.file_name,
                        totalBytes: t.bytes_total,
                        isDirectory: t.is_directory ?? false,
                        itemCount: t.item_count ?? 1,
                        batchId: t.batch_id,
                        bytesReceived: t.bytes_received,
                        percent: t.percent,
                        speedBps: t.speed_bps,
                        etaSecs: t.eta_secs,
                        status: status,
                        isOutbound: t.is_outbound ?? false
                    )
                }
            } else {
                activeTransfers = []
            }
            
            if let ast = s.active_speed_tests {
                let newTests = ast.filter { $0.phase != "Idle" }.map { t in
                    SpeedTestState(
                        id: t.peer_id,
                        testId: t.test_id,
                        phase: t.phase,
                        bytesTransferred: t.bytes_transferred,
                        durationSecs: t.duration_secs
                    )
                }
                if self.activeSpeedTests != newTests {
                    self.activeSpeedTests = newTests
                }
            } else {
                if !self.activeSpeedTests.isEmpty { self.activeSpeedTests = [] }
            }

            // ── Call continuity: update active call state ─────────────────────
            // Only mutate when the value actually changes — prevents SwiftUI
            // from tearing down & rebuilding the call banner every 0.25 s,
            // which was silently breaking button hit-testing (mouse-down and
            // mouse-up land on different view instances when the view rebuilds
            // between the two events).
            if let suppress = suppressCallUpdatesUntil, suppress > Date() {
                // Ignore status updates for activeCall during optimistic UI wait
            } else {
                if let call = s.active_call, call.state.lowercased() != "idle" {
                    let incoming = IncomingCallState(
                        deviceId: call.device_id,
                        deviceName: call.device_name,
                        state: call.state,
                        phoneNumber: call.number,
                        contactName: call.contact_name
                    )
                    if activeCall != incoming {
                        activeCall = incoming
                    }
                } else if activeCall != nil {
                    activeCall = nil
                }
            }

            // ── Battery Sync (F20) ────────────────────────────────────────────
            let incomingBatteries = (s.peer_batteries ?? []).map { pb in
                DeviceBatteryState(
                    deviceId: pb.device_id,
                    deviceName: pb.device_name,
                    level: pb.level,
                    charging: pb.charging
                )
            }
            if peerBatteries != incomingBatteries {
                peerBatteries = incomingBatteries
            }

            // ── Network Sync ──────────────────────────────────────────────────
            let incomingNetworks = (s.peer_networks ?? []).map { pn in
                DeviceNetworkState(
                    deviceId: pn.device_id,
                    deviceName: pn.device_name,
                    networkType: pn.network_type
                )
            }
            if peerNetworks != incomingNetworks {
                peerNetworks = incomingNetworks
            }

            // ── Storage Sync ──────────────────────────────────────────────────
            let incomingStorages = (s.peer_storages ?? []).map { ps in
                DeviceStorageState(
                    deviceId: ps.device_id,
                    deviceName: ps.device_name,
                    imagesBytes: ps.images_bytes,
                    videosBytes: ps.videos_bytes,
                    appsBytes: ps.apps_bytes,
                    freeBytes: ps.free_bytes,
                    totalBytes: ps.total_bytes
                )
            }
            if peerStorages != incomingStorages {
                peerStorages = incomingStorages
            }

            // ── Camera Streaming ──────────────────────────────────────────────
            if !isCameraPolling {
                if let frameData = try? await ipc.latestCameraFrame() {
                    startFastCameraPolling(initialFrame: frameData)
                }
            }

            dashboardStatus = StatusSnapshot(
                peerCount:    connectedCount,
                trustedCount: s.peers.filter { $0.trusted }.count,
                lastSyncAt:   s.peers.compactMap { $0.last_sync }
                    .max().map { Date(timeIntervalSince1970: TimeInterval($0)) },
                syncEnabled:  true,
                daemonVersion: nil
            )
            if lastActivityId > 0 {
                await pollActivityFeedIncremental()
            } else {
                await primeActivityFeed()
            }
        } catch {
            ipcFailureCount += 1
            isRunning       = false
            // Without the daemon a call can be neither followed nor acted on.
            if activeCall != nil { activeCall = nil }
            statusLine      = ipcFailureCount >= 3
                ? "Daemon not running"
                : "Reconnecting to daemon…"
            dashboardStatus = nil
            if case LinkAllIPCError.connectionFailed = error {
                NotificationCenter.default.post(name: .linkallEnsureDaemon, object: nil)
            }
        }
    }

    // MARK: - Camera Streaming

    private func startFastCameraPolling(initialFrame: Data) {
        guard !isCameraPolling else { return }
        isCameraPolling = true
        DispatchQueue.main.async {
            CameraPreviewWindowController.shared.showWindow(nil)
            CameraPreviewWindowController.shared.updateFrame(data: initialFrame)
        }
        
        Task { [weak self] in
            while self?.isCameraPolling == true {
                try? await Task.sleep(nanoseconds: 33_000_000) // ~30 fps
                guard let self = self else { break }
                do {
                    if let frameData = try await self.ipc.latestCameraFrame() {
                        DispatchQueue.main.async {
                            CameraPreviewWindowController.shared.updateFrame(data: frameData)
                        }
                    } else {
                        self.isCameraPolling = false
                        DispatchQueue.main.async {
                            CameraPreviewWindowController.shared.close()
                        }
                        break
                    }
                } catch {
                    self.isCameraPolling = false
                    break
                }
            }
        }
    }

    func stopCameraPolling() {
        isCameraPolling = false
    }
    // MARK: - Device actions (ManagedDevice variants)

    func disconnect(_ device: ManagedDevice) {
        Task { try? await ipc.disconnectPeer(deviceId: device.id); await refresh() }
    }
    func connect(_ device: ManagedDevice) {
        Task {
            try? await ipc.setAutoConnect(deviceId: device.id, enabled: true)
            try? await ipc.reconnectPeer(deviceId: device.id)
            await refresh()
        }
    }
    func trust(_ device: ManagedDevice) {
        Task { try? await ipc.approveTrust(deviceId: device.id, deviceName: device.name, pubkeyBytes: Data()); await refresh() }
    }
    func reject(_ device: ManagedDevice) {
        Task { try? await ipc.rejectTrust(deviceId: device.id); await refresh() }
    }
    func revoke(_ device: ManagedDevice) {
        Task { try? await ipc.revokeDevice(deviceId: device.id); await refresh() }
    }

    func forget(_ device: ManagedDevice) {
        Task { try? await ipc.forgetDevice(deviceId: device.id); await refresh() }
    }
    func rename(_ device: ManagedDevice, to newName: String) {
        Task { try? await ipc.renameDevice(deviceId: device.id, displayName: newName); await refresh() }
    }
    func sendPairingRequest(_ device: ManagedDevice) {
        Task { try? await ipc.sendPairingRequest(deviceId: device.id); try? await Task.sleep(nanoseconds: 200_000_000); await refresh() }
    }
    func cancelPairingRequest(_ device: ManagedDevice) {
        Task { try? await ipc.cancelPairingRequest(deviceId: device.id); try? await Task.sleep(nanoseconds: 200_000_000); await refresh() }
    }
    func respondToPairing(_ device: ManagedDevice, accepted: Bool) {
        Task { try? await ipc.respondToPairing(deviceId: device.id, accepted: accepted); try? await Task.sleep(nanoseconds: 200_000_000); await refresh() }
    }
    
    func generateQrToken() async throws -> String {
        try await ipc.generateQrToken()
    }
    
    func connectAndPair(deviceId: String) {
        Task {
            _ = try? await ipc.send(cmd: ["cmd": "send_pairing_request", "device_id": deviceId])
            await refresh()
        }
    }

    // MARK: - Device actions (PeerViewModel variants)

    func pauseSync(_ peer: PeerViewModel)        { Task { try? await ipc.pauseSync(deviceId: peer.id);     await refresh() } }
    func resumeSync(_ peer: PeerViewModel)       { Task { try? await ipc.resumeSync(deviceId: peer.id);    await refresh() } }
    func forgetDevice(_ peer: PeerViewModel)     { Task { try? await ipc.forgetDevice(deviceId: peer.id);  await refresh() } }
    func revokeTrust(_ peer: PeerViewModel)      { Task { try? await ipc.revokeDevice(deviceId: peer.id);  await refresh() } }
    func disconnect(_ peer: PeerViewModel)       { Task { try? await ipc.disconnectPeer(deviceId: peer.id); await refresh() } }
    func toggleAutoConnect(_ peer: PeerViewModel) {
        Task { try? await ipc.setAutoConnect(deviceId: peer.id, enabled: !peer.autoConnect); await refresh() }
    }

    func beginRename(_ peer: PeerViewModel) {
        pendingRename = peer
        NotificationCenter.default.post(name: .beginRename, object: peer)
    }
    func applyRename(deviceId: String, newName: String) {
        Task { try? await ipc.renameDevice(deviceId: deviceId, displayName: newName); await refresh() }
    }

    func connectManual() {
        let addr = manualConnectAddress.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !addr.isEmpty else { return }
        Task {
            do {
                try await ipc.connectManual(address: addr)
                manualConnectAddress = ""
                showToast(title: "Connecting…", body: "Initiated connection to \(addr)", tint: CRTheme.accentBlue)
                // Poll a few times to pick up the peer once the handshake completes.
                try? await Task.sleep(nanoseconds: 800_000_000)
                await refresh()
                try? await Task.sleep(nanoseconds: 1_200_000_000)
                await refresh()
            } catch {
                showToast(
                    title: "Connection failed",
                    body: error.localizedDescription,
                    tint: CRTheme.accentRed
                )
            }
        }
    }

    /// Connect to a specific host IP entered by the user (e.g. from the menu bar dialog).
    func connectManual(host: String) {
        let addr = host.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !addr.isEmpty else { return }
        Task {
            do {
                try await ipc.connectManual(address: addr)
                showToast(title: "Connecting to \(addr)…", body: "Handshake in progress", tint: CRTheme.accentBlue)
                try? await Task.sleep(nanoseconds: 900_000_000)
                await refresh()
                try? await Task.sleep(nanoseconds: 1_500_000_000)
                await refresh()
            } catch {
                showToast(title: "Connection failed", body: error.localizedDescription, tint: CRTheme.accentRed)
            }
        }
    }


    // Single-URL convenience wrappers
    func sendFile(url: URL, to device: ManagedDevice?) {
        sendFiles(urls: [url], to: device)
    }
    func sendFile(url: URL, toPeer deviceId: String? = nil) {
        sendFiles(urls: [url], toPeer: deviceId)
    }

    func sendFiles(urls: [URL], to device: ManagedDevice?) {
        if let device = device {
            lastUsedDeviceId = device.id
        }
        Task {
            for url in urls {
                await processAndSend(url: url, targetDeviceId: device?.id)
            }
        }
    }
    func sendFiles(urls: [URL], toPeer deviceId: String? = nil) {
        if let id = deviceId {
            lastUsedDeviceId = id
        }
        Task {
            for url in urls {
                await processAndSend(url: url, targetDeviceId: deviceId)
            }
        }
    }
    
    /// Send what this Mac last copied to the default device (all devices
    /// when there is none), with a toast saying how it went.
    func pushCurrentClipboard() {
        guard connectedCount > 0 else {
            showToast(
                title: "No Devices Connected",
                body: "Connect a device to push clipboard.",
                tint: CRTheme.inkSoft,
                systemImage: "wifi.slash",
                ttl: 2.5
            )
            return
        }
        Task {
            do {
                try await ipc.sendClipboardCurrent(targetDeviceId: defaultTargetDevice?.id)
                showToast(
                    title: "Clipboard Synced",
                    body: "Pushed to all connected devices.",
                    tint: CRTheme.accentGreen,
                    systemImage: "arrow.up.circle.fill",
                    ttl: 2.0
                )
            } catch {
                showToast(
                    title: "Sync Failed",
                    body: error.localizedDescription,
                    tint: Color.red,
                    systemImage: "exclamationmark.triangle",
                    ttl: 3.0
                )
            }
        }
    }

    /// Send files when the caller has no specific device in mind. With one
    /// connected device they go straight to it; with several, the send
    /// modal asks (all devices unless one is picked). `completion` says
    /// whether anything was sent. Returns false when nothing can be sent.
    @discardableResult
    func sendFilesChoosingTarget(urls: [URL], completion: ((Bool) -> Void)? = nil) -> Bool {
        guard !urls.isEmpty else { completion?(false); return false }
        let connected = connectedDevices
        guard connected.count > 1 else {
            guard let only = connected.first else { completion?(false); return false }
            sendFiles(urls: urls, to: only)
            completion?(true)
            return true
        }
        presentSendModal(urls: urls, completion: completion)
        return true
    }

    /// The send modal: pick the device (all by default), then drop or choose
    /// files and folders, or confirm `urls` already chosen.
    func presentSendModal(urls: [URL]? = nil, completion: ((Bool) -> Void)? = nil) {
        let devices = connectedDevices
        guard !devices.isEmpty else {
            showToast(title: "No Devices Connected", body: "Connect a device to send files or folders.", tint: CRTheme.inkSoft, systemImage: "wifi.slash")
            completion?(false)
            return
        }
        LinkAllModal.shared.present { [weak self] dismiss in
            SendModalCard(
                devices: devices,
                urls: urls,
                onSend: { chosen, target in
                    dismiss()
                    guard let self else { return }
                    if let target { self.sendFiles(urls: chosen, to: target) }
                    else { self.sendFiles(urls: chosen, toPeer: nil) }
                    completion?(true)
                },
                onCancel: { dismiss(); completion?(false) }
            )
        }
    }

    private func processAndSend(url: URL, targetDeviceId: String?) async {
        let isFolder = (try? url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) ?? false
        do {
            if isFolder {
                try await ipc.sendFolder(url: url, targetDeviceId: targetDeviceId)
            } else {
                _ = try await ipc.sendFile(url: url, targetDeviceId: targetDeviceId)
            }
        } catch {
            showToast(
                title: "Couldn't send \(url.lastPathComponent)",
                body: error.localizedDescription,
                tint: CRTheme.accentRed,
                systemImage: "exclamationmark.triangle",
                ttl: 5.0
            )
        }
    }

    // MARK: - Trust prompts (legacy TrustPrompt model)

    func approveTrust(_ prompt: TrustPrompt) {
        Task {
            try? await ipc.approveTrust(deviceId: prompt.deviceId, deviceName: prompt.deviceName, pubkeyBytes: prompt.publicKeyBytes)
            activeTrustPrompt = nil; await refresh()
        }
    }
    func rejectTrust(_ prompt: TrustPrompt) {
        Task { try? await ipc.rejectTrust(deviceId: prompt.deviceId); activeTrustPrompt = nil }
    }

    // MARK: - Device discovery
    func scanForDevices() {
        Task {
            _ = try? await ipc.send(cmd: ["cmd": "rescan_peers"])
            // Give discovery 1.5 s to find peers, then refresh the peer list.
            try? await Task.sleep(nanoseconds: 1_500_000_000)
            await refresh()
            showToast(title: "Scan complete", body: "Refreshed peer list", tint: CRTheme.accentBlue)
        }
    }

    /// Reject all currently-untrusted device requests in one action.
    func rejectAll() {
        let untrusted = devices.filter { $0.trustState == .untrusted }
        guard !untrusted.isEmpty else { return }
        Task {
            for device in untrusted {
                try? await ipc.rejectTrust(deviceId: device.id)
            }
            await refresh()
            showToast(title: "Requests Dismissed", body: "Rejected \(untrusted.count) pending devices", tint: CRTheme.accentRed)
        }
    }

    // MARK: - Timeline

    /// Copy item text to pasteboard without marking it applied.
    /// Apply is a separate explicit action (the Apply button / context menu).
    func copyTimelineItem(_ item: TimelineItem) {
        if let text = item.fullText {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
            showToast(title: "Copied", body: String(text.prefix(60)), tint: CRTheme.accentGreen)
        }
    }

    func sendTimelineItem(_ item: TimelineItem, to device: ManagedDevice?) {
        Task {
            guard let entry = activityFeed.first(where: { $0.id == item.id }),
                  let hash = entry.content_hash else { return }
            try? await ipc.sendClipboardByHash(hash: hash, targetDeviceId: device?.id)
            let target = device?.name ?? "all devices"
            showToast(title: "Sent", body: "Clipboard sent to \(target)", tint: CRTheme.accentBlue)
        }
    }

    func pinTimelineItem(_ item: TimelineItem, pinned: Bool) {
        if pinned { pinnedItemIds.insert(item.id) } else { pinnedItemIds.remove(item.id) }
    }

    func deleteTimelineItem(_ item: TimelineItem) {
        activityFeed.removeAll { $0.id == item.id }
        pinnedItemIds.remove(item.id)
    }

    /// Synthesise a minimal feed entry from a legacy TimelineEntry.
    /// Uses JSON round-trip via the custom Codable init so we don't need a
    /// memberwise initialiser on IpcActivityEntry.
    func addTimelineEntry(_ entry: TimelineEntry) {
        let dict: [String: Any] = [
            "id":             Int64(Date().timeIntervalSince1970 * 1000),
            "timestamp_ms":   Int64(entry.timestamp.timeIntervalSince1970 * 1000),
            "device_id":      "",
            "device_name":    entry.deviceName,
            "kind":           entry.kind.rawValue,
            "summary":        entry.preview,
            "text_preview":   entry.preview,
            "applied_locally": false,
            "relay_path":     [String]()
        ]
        guard
            let data    = try? JSONSerialization.data(withJSONObject: dict),
            let synthetic = try? JSONDecoder().decode(IpcActivityEntry.self, from: data)
        else { return }
        activityFeed.insert(synthetic, at: 0)
        if activityFeed.count > 200 { activityFeed.removeLast() }
    }

    // MARK: - Command palette actions

    /// Toggle sync on/off — used by command palette ⌘K and menu bar.
    func toggleSync() {
        guard var s = settings else { return }
        s.syncEnabled = !s.syncEnabled
        saveSettings(s)
        let state = s.syncEnabled ? "resumed" : "paused"
        showToast(title: "Sync \(state)", body: s.syncEnabled
            ? "Clipboard sync is now active"
            : "Clipboard sync paused — no events will be forwarded",
            tint: s.syncEnabled ? CRTheme.accentGreen : CRTheme.accentOrange)
    }

    /// Open the Quick Access history panel — triggered by command palette.
    func openHistoryPanel() {
        // Post a notification that AppDelegate listens to — keeps store decoupled from UI.
        NotificationCenter.default.post(name: .linkallOpenHistoryPanel, object: nil)
    }

    func openCommandPalette() {
        NotificationCenter.default.post(name: .linkallOpenCommandPalette, object: nil)
    }

    /// Send the current local clipboard to all (or one) connected peer.
    func sendCurrentClipboard(to device: ManagedDevice?) {
        if let device = device {
            lastUsedDeviceId = device.id
        }
        Task {
            try? await ipc.sendClipboardCurrent(targetDeviceId: device?.id)
            let target = device?.name ?? "all devices"
            showToast(title: "Sent", body: "Clipboard sent to \(target)", tint: CRTheme.accentBlue)
        }
    }

    func sendPushText(_ text: String, to device: ManagedDevice?) {
        if let device = device {
            lastUsedDeviceId = device.id
        }
        Task {
            try? await ipc.sendPushText(text, targetDeviceId: device?.id)
            let target = device?.name ?? "all devices"
            showToast(title: "Sent", body: "Text sent to \(target)", tint: CRTheme.accentBlue)
        }
    }

    func sendQuickContext(to device: ManagedDevice) {
        guard let context = quickSendContext else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(context.text, forType: .string)
        sendCurrentClipboard(to: device)
    }

    // MARK: - Remote Explorer (Phase 3)
    
    func queryRemoteFiles(
        targetDevice: String,
        summaryOnly: Bool = false,
        category: String? = nil,
        source: String? = nil,
        searchQuery: String? = nil,
        offset: UInt32 = 0,
        limit: UInt32 = 50
    ) async throws -> IpcRemoteFilesResult {
        return try await ipc.queryRemoteFiles(
            targetDevice: targetDevice,
            summaryOnly: summaryOnly,
            category: category,
            source: source,
            searchQuery: searchQuery,
            offset: offset,
            limit: limit
        )
    }

    func requestRemoteThumbnail(targetDevice: String, fileId: UInt64, sizePx: UInt32 = 256) async throws -> Data? {
        return try await ipc.requestRemoteThumbnail(targetDevice: targetDevice, fileId: fileId, sizePx: sizePx)
    }

    func pullRemoteFile(targetDevice: String, fileId: UInt64) async throws {
        try await ipc.requestRemoteFilePull(targetDevice: targetDevice, fileId: fileId)
    }

    func performRemoteFileAction(targetDevice: String, fileId: UInt64, action: String, newName: String? = nil) async throws {
        try await ipc.requestRemoteFileAction(targetDevice: targetDevice, fileId: fileId, action: action, newName: newName)
    }

    private func buildClipboardArchive(from urls: [URL]) async -> URL? {
        guard !urls.isEmpty else { return nil }

        return await Task.detached { () -> URL? in
            let stamp = ISO8601DateFormatter()
                .string(from: Date())
                .replacingOccurrences(of: ":", with: "-")
            let tempRoot = FileManager.default.temporaryDirectory
                .appendingPathComponent("linkall-clipboard-archives", isDirectory: true)
            let stagingDir = tempRoot.appendingPathComponent(UUID().uuidString, isDirectory: true)
            let archiveURL = tempRoot.appendingPathComponent("Link All Bundle \(stamp).zip")

            do {
                try FileManager.default.createDirectory(
                    at: stagingDir,
                    withIntermediateDirectories: true,
                    attributes: nil
                )

                var stagedNames = Set<String>()
                for source in urls {
                    let stagedName = self.uniqueClipboardItemName(for: source.lastPathComponent, existing: &stagedNames)
                    let stagedURL = stagingDir.appendingPathComponent(stagedName)
                    try FileManager.default.createSymbolicLink(at: stagedURL, withDestinationURL: source)
                }

                if FileManager.default.fileExists(atPath: archiveURL.path) {
                    try FileManager.default.removeItem(at: archiveURL)
                }

                let process = Process()
                process.executableURL = URL(fileURLWithPath: "/usr/bin/zip")
                process.currentDirectoryURL = stagingDir
                process.arguments = ["-r", "-q", archiveURL.path] + stagedNames.sorted()

                try process.run()
                process.waitUntilExit()

                guard process.terminationStatus == 0,
                      FileManager.default.fileExists(atPath: archiveURL.path) else {
                    throw NSError(
                        domain: "LinkAllArchive",
                        code: Int(process.terminationStatus),
                        userInfo: nil
                    )
                }

                DispatchQueue.global().asyncAfter(deadline: .now() + .seconds(1800)) {
                    try? FileManager.default.removeItem(at: archiveURL)
                    try? FileManager.default.removeItem(at: stagingDir)
                }
                return archiveURL
            } catch {
                try? FileManager.default.removeItem(at: stagingDir)
                try? FileManager.default.removeItem(at: archiveURL)
                NSLog("Link All: failed to archive clipboard files: \(error.localizedDescription)")
                return nil
            }
        }.value
    }

    nonisolated private func uniqueClipboardItemName(for baseName: String, existing: inout Set<String>) -> String {
        guard !existing.contains(baseName) else {
            let stem = URL(fileURLWithPath: baseName).deletingPathExtension().lastPathComponent
            let ext = URL(fileURLWithPath: baseName).pathExtension
            var index = 2
            while true {
                let candidate = ext.isEmpty ? "\(stem) \(index)" : "\(stem) \(index).\(ext)"
                if !existing.contains(candidate) {
                    existing.insert(candidate)
                    return candidate
                }
                index += 1
            }
        }
        existing.insert(baseName)
        return baseName
    }

    // MARK: - Activity Feed

    @MainActor
    func refreshActivityFeed() async {
        do {
            let entries    = try await ipc.activityRecent(limit: 100)
            activityFeed   = entries
            lastActivityId = entries.first?.id ?? 0
            mirrorAutoAppliedClipboardIfNeeded(entries: entries)
        } catch {}
    }

    @MainActor
    func pollActivityFeedIncremental() async {
        do {
            let newEntries = try await ipc.activitySince(sinceId: lastActivityId)
            if !newEntries.isEmpty {
                activityFeed.insert(contentsOf: newEntries.reversed(), at: 0)
                if activityFeed.count > 200 { activityFeed = Array(activityFeed.prefix(200)) }
                lastActivityId = newEntries.map(\.id).max() ?? lastActivityId
                mirrorAutoAppliedClipboardIfNeeded(entries: newEntries)
                
                // Only fire user-visible notifications for events the user cares about:
                // remote clipboard items arriving and completed incoming file transfers.
                // Local copies, peer connections, sync events, etc. are silently absorbed
                // into the activity feed without triggering system notifications or toasts.
                for entry in newEntries {
                    if entry.kind == "file_transfer_complete" {
                        if let bytes = entry.file_bytes {
                            self.totalBytesTransferred += Int(bytes)
                            self.totalFilesTransferred += 1
                        }
                    }
                    
                    switch entry.kind {
                    case "remote_clipboard_available", "file_transfer_complete", "folder_transfer_complete", "remote_notification":
                        NotificationCenter.default.post(name: NSNotification.Name("linkallActivityReceived"), object: entry)
                    default:
                        break
                    }
                }
            }
            // pendingClipboardCount relies on daemon status, do not recompute locally from limited feed.
        } catch {}
    }

    // MARK: - Clipboard policy

    @MainActor
    func applyClipboard(entry: IpcActivityEntry) async {
        guard let hash = entry.content_hash else { return }
        do {
            try await ipc.applyClipboard(contentHash: hash)
            if let idx = activityFeed.firstIndex(where: { $0.id == entry.id }) {
                activityFeed[idx].applied_locally = true
            }
            // Decrement immediately so the menu bar badge updates without waiting for next poll.
            pendingClipboardCount = max(0, pendingClipboardCount - 1)
            // Apply to local pasteboard via ClipboardSetter (suppresses echo back to peers).
            if let text = await fullText(of: entry) {
                applyClipboardLocally(text: text)
            }
        } catch {}
    }

    @MainActor
    func setTimelineFirstMode(enabled: Bool) async {
        clipboardPolicy.timelineFirstMode = enabled
        try? await ipc.setTimelineFirstMode(enabled: enabled)
    }

    @MainActor
    func setAutoApplyClipboard(enabled: Bool) async {
        clipboardPolicy.autoApply = enabled
        try? await ipc.setAutoApplyClipboard(enabled: enabled)
    }

    @MainActor
    private func primeActivityFeed() async {
        do {
            let entries = try await ipc.activityRecent(limit: 20)
            activityFeed = entries
            lastActivityId = entries.first?.id ?? 0
            mirrorAutoAppliedClipboardIfNeeded(entries: entries)
        } catch {}
    }

    @MainActor
    private func mirrorAutoAppliedClipboardIfNeeded(entries: [IpcActivityEntry]) {
        let pendingMirror = entries
            .filter {
                $0.kind == "remote_clipboard_available" &&
                $0.applied_locally &&
                $0.id > lastMirroredAutoAppliedEntryId &&
                !($0.text_preview?.isEmpty ?? true)
            }
            .sorted { $0.id < $1.id }

        guard !pendingMirror.isEmpty else { return }
        // Mark them now so the next poll doesn't apply them a second time.
        lastMirroredAutoAppliedEntryId = pendingMirror.last!.id
        Task { @MainActor in
            for entry in pendingMirror {
                if let text = await fullText(of: entry) { applyClipboardLocally(text: text) }
            }
        }
    }

    /// A clipboard entry's whole text. The feed only carries a 400-character
    /// preview, and writing that to the pasteboard cut long copies short; the
    /// daemon keeps the full text of each received item by its id.
    func fullText(of entry: IpcActivityEntry) async -> String? {
        if let full = try? await ipc.incomingClipboardText(id: entry.id), !full.isEmpty { return full }
        return entry.text_preview
    }

    // MARK: - Settings

    func saveSettings(_ snapshot: LinkAllSettingsSnapshot) {
        settings = snapshot
        // startOnLogin is OS-level (LaunchAgent) — handle separately from daemon settings.
        applyLoginItemState(enabled: snapshot.startOnLogin)
        Task {
            do {
                try await ipc.saveSettings(snapshot)
                await refresh()
                showToast(title: "Settings saved", body: "Changes applied", tint: CRTheme.accentGreen)
            } catch {
                showToast(title: "Save failed", body: error.localizedDescription, tint: CRTheme.accentRed)
            }
        }
    }

    /// Registers/unregisters Link All as a login item via SMAppService (macOS 13+).
    private func applyLoginItemState(enabled: Bool) {
        if #available(macOS 13.0, *) {
            let svc = SMAppService.mainApp
            do {
                if enabled  { if svc.status != .enabled  { try svc.register()   } }
                else        { if svc.status == .enabled  { try svc.unregister() } }
            } catch {
                NSLog("Link All: login item \(enabled ? "register" : "unregister") error: \(error)")
            }
        }
    }

    // MARK: - Command palette

    func performCommand(_ command: String) {
        let cmd = command.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if cmd.hasPrefix("/history") || cmd.hasPrefix("/timeline") { selectedSection = .clipboard }
        else if cmd.hasPrefix("/devices") { selectedSection = .devices }
        else if cmd.hasPrefix("/settings") || cmd.hasPrefix("/prefs") { selectedSection = .settings }
        else if cmd.hasPrefix("/connect ") {
            manualConnectAddress = String(command.dropFirst(9))
            selectedSection = .devices
            connectManual()
        }
    }

    // MARK: - File transfers

    @MainActor func acceptFileTransfer(_ t: FileTransferState) {
        Task { try? await ipc.acceptFileTransfer(transferId: t.id); updateTransferStatus(id: t.id, status: .transferring) }
    }
    @MainActor func rejectFileTransfer(_ t: FileTransferState) {
        Task { try? await ipc.rejectFileTransfer(transferId: t.id); activeTransfers.removeAll { $0.id == t.id } }
    }
    @MainActor func cancelFileTransfer(_ t: FileTransferState) {
        // A folder row stands for the whole folder: stop all of it.
        if t.isDirectory, let batchId = t.batchId {
            Task { try? await ipc.cancelFolder(batchId: batchId); activeTransfers.removeAll { $0.batchId == batchId } }
            return
        }
        Task { try? await ipc.cancelFileTransfer(transferId: t.id); activeTransfers.removeAll { $0.id == t.id } }
    }

    @MainActor
    func pauseFileTransfer(_ t: FileTransferState) {
        Task { try? await ipc.pauseFileTransfer(transferId: t.id); updateTransferStatus(id: t.id, status: .paused) }
    }
    
    @MainActor
    func resumeFileTransfer(_ t: FileTransferState) {
        Task { try? await ipc.resumeFileTransfer(transferId: t.id); updateTransferStatus(id: t.id, status: .transferring) }
    }

    @MainActor
    func startSpeedTest(deviceId: String, durationSecs: Int = 10) {
        Task { try? await ipc.startSpeedTest(deviceId: deviceId, durationSecs: durationSecs) }
    }

    @MainActor
    func upsertTransfer(_ t: FileTransferState) {
        let isNew = !activeTransfers.contains(where: { $0.id == t.id })
        if let idx = activeTransfers.firstIndex(where: { $0.id == t.id }) { activeTransfers[idx] = t }
        else { activeTransfers.insert(t, at: 0) }
        
        if isNew {
            TransferNotificationManager.shared.onTransferStarted(id: t.id, filename: t.fileName, totalBytes: t.totalBytes, deviceName: t.fromDeviceName)
        } else {
            TransferNotificationManager.shared.onProgressUpdated(id: t.id, progress: t.exactRatio, bytesPerSecond: t.speedBps ?? 0)
        }
        
        switch t.status {
        case .complete:
            TransferNotificationManager.shared.onTransferCompleted(id: t.id, filename: t.fileName)
        case .failed(let reason):
            TransferNotificationManager.shared.onTransferFailed(id: t.id, filename: t.fileName, error: reason)
        default:
            break
        }
    }

    @MainActor
    private func updateTransferStatus(id: String, status: FileTransferStatus) {
        guard let idx = activeTransfers.firstIndex(where: { $0.id == id }) else { return }
        var t = activeTransfers[idx]
        t.status = status
        activeTransfers[idx] = t
        
        switch status {
        case .complete:
            TransferNotificationManager.shared.onTransferCompleted(id: id, filename: t.fileName)
        case .failed(let reason):
            TransferNotificationManager.shared.onTransferFailed(id: id, filename: t.fileName, error: reason)
        default:
            break
        }
    }

    // MARK: - Toast system

    func showToast(
        title: String,
        body: String,
        tint: Color,
        systemImage: String = "sparkles.rectangle.stack",
        detail: String? = nil,
        ttl: TimeInterval? = 4.0,
        progress: Double? = nil,
        primaryAction: ToastAction? = nil,
        secondaryAction: ToastAction? = nil
    ) {
        let toast = ToastItem(
            title: title,
            body: body,
            tint: tint,
            systemImage: systemImage,
            detail: detail,
            ttl: ttl,
            progress: progress,
            primaryAction: primaryAction,
            secondaryAction: secondaryAction
        )
        withAnimation(.spring(response: 0.45, dampingFraction: 0.82, blendDuration: 0.1)) {
            toasts.append(toast)
        }

        guard let ttl else { return }

        let work = DispatchWorkItem { [weak self] in
            self?.dismissToast(id: toast.id)
        }
        toastWorkItems[toast.id] = work
        DispatchQueue.main.asyncAfter(deadline: .now() + ttl, execute: work)
    }

    func dismissToast(id: UUID) {
        toastWorkItems[id]?.cancel()
        toastWorkItems.removeValue(forKey: id)
        withAnimation(.spring(response: 0.45, dampingFraction: 0.82, blendDuration: 0.1)) {
            toasts.removeAll { $0.id == id }
        }
    }

    // MARK: - Call Continuity

    func acceptCall() {
        NSSound.beep()
        guard let call = activeCall else { return }
        suppressCallUpdatesUntil = Date().addingTimeInterval(3.0)
        // Temporarily mark as offhook locally for immediate feedback
        var updated = call
        updated.state = "offhook"
        activeCall = updated
        Task {
            try? await ipc.callAction(action: "accept", targetDevice: call.deviceId)
        }
    }

    func declineCall() {
        NSSound.beep()
        guard let call = activeCall else { return }
        suppressCallUpdatesUntil = Date().addingTimeInterval(3.0)
        withAnimation(.crSpring) { activeCall = nil }
        Task {
            try? await ipc.callAction(action: "decline", targetDevice: call.deviceId)
        }
    }

    func routeAudio(to route: String) {
        guard let call = activeCall else { return }
        Task {
            try? await ipc.callAction(action: "audio_\(route)", targetDevice: call.deviceId)
        }
    }

    // MARK: - Mapping

    private func makePeerViewModel(_ raw: IpcPeerRecord) -> PeerViewModel {
        PeerViewModel(
            id:          raw.id,
            displayName: raw.display_name?.isEmpty == false ? raw.display_name! : raw.friendly_name,
            platform:    raw.platform,
            trusted:     raw.trusted,
            remembered:  raw.remembered ?? true,
            connected:   raw.status == "connected",
            connectionStatus: raw.status,
            syncEnabled: raw.sync_enabled ?? true,
            remoteSyncEnabled: raw.remote_sync_enabled ?? true,
            autoConnect: raw.auto_connect ?? true,
            lastError:   raw.last_error,
            pairingRequested: raw.pairing_requested ?? false,
            outgoingPairingWaiting: raw.outgoing_pairing_waiting ?? false,
            pairingPin: raw.pairing_pin,
            explicitDisconnect: raw.explicit_disconnect ?? false,
            pairingOutcome: raw.pairing_outcome,
            lastSeen:    raw.last_seen.map { Date(timeIntervalSince1970: TimeInterval($0)) },
            lastDiscoveryAt: raw.last_discovery_at.map { Date(timeIntervalSince1970: TimeInterval($0)) },
            lastSync:    raw.last_sync.map { Date(timeIntervalSince1970: TimeInterval($0)) },
            ip:          raw.ip
        )
    }

    private func whenStatusLine(connectedCount: Int, reconnectingCount: Int, reconnectableCount: Int) -> String {
        if connectedCount > 0 {
            return "\(connectedCount) device\(connectedCount == 1 ? "" : "s") connected"
        }
        if reconnectingCount > 0 {
            return "Reconnecting to nearby devices…"
        }
        if reconnectableCount > 0 {
            return "Trusted devices ready to reconnect"
        }
        return "Ready — no devices nearby"
    }
}

extension Notification.Name {
    static let beginRename = Notification.Name("LinkAllBeginRename")
}
