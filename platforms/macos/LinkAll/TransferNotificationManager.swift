import AppKit
import Combine
import SwiftUI
import UserNotifications

// MARK: - Notification Lifecycle State Machine

public enum TransferNotificationState: Equatable, Sendable {
    case queued
    case entering
    case active(autoHideDeadline: Date?)
    case hovered
    case collapsedToNC
    case completing(autoDismissDeadline: Date)
    case failed(errorMessage: String)
    case dismissed
}

public struct TransferNotificationModel: Identifiable, Equatable, Sendable {
    public let id: String
    public let filename: String
    public let fileSizeMB: Double
    public let deviceName: String
    public var progress: Double // 0.0 ... 1.0
    public var bytesPerSecond: Int64
    public var state: TransferNotificationState
    public var isPaused: Bool
    
    public var formattedSpeed: String {
        guard bytesPerSecond > 0 else { return "" }
        let formatter = ByteCountFormatter()
        formatter.allowedUnits = [.useMB, .useKB]
        formatter.countStyle = .file
        return formatter.string(fromByteCount: bytesPerSecond) + "/s"
    }
}

// MARK: - Preferences Store

@MainActor
public final class TransferNotificationPreferences: ObservableObject {
    public static let shared = TransferNotificationPreferences()
    
    @AppStorage("showTransferNotifications") public var showTransferNotifications: Bool = true
    @AppStorage("keepVisibleDuringTransfer") public var keepVisibleDuringTransfer: Bool = false
    @AppStorage("playCompletionSound") public var playCompletionSound: Bool = true
    @AppStorage("minFileSizeThresholdMB") public var minFileSizeThresholdMB: Double = 0.0
}

// MARK: - Transfer Notification Manager

@MainActor
public final class TransferNotificationManager: ObservableObject {
    public static let shared = TransferNotificationManager()
    
    @Published public private(set) var items: [TransferNotificationModel] = []
    
    private var autoCollapseTasks: [String: Task<Void, Never>] = [:]
    private var hoverGraceTasks: [String: Task<Void, Never>] = [:]
    private let preferences = TransferNotificationPreferences.shared
    
    public var visibleItems: [TransferNotificationModel] {
        items.filter { item in
            switch item.state {
            case .entering, .active, .hovered, .completing, .failed:
                return true
            default:
                return false
            }
        }.prefix(3).map { $0 }
    }
    
    public var collapsedCount: Int {
        let visibleCount = visibleItems.count
        let totalActive = items.filter { item in
            switch item.state {
            case .entering, .active, .hovered, .completing, .failed, .collapsedToNC:
                return true
            default:
                return false
            }
        }.count
        return max(0, totalActive - visibleCount)
    }
    
    // MARK: - Public Event Triggers (Event-Driven Updates)
    
    public func onTransferStarted(id: String, filename: String, totalBytes: Int64, deviceName: String) {
        guard preferences.showTransferNotifications else { return }
        let sizeMB = Double(totalBytes) / 1_048_576.0
        guard sizeMB >= preferences.minFileSizeThresholdMB else { return }
        
        let item = TransferNotificationModel(
            id: id,
            filename: filename,
            fileSizeMB: sizeMB,
            deviceName: deviceName,
            progress: 0.0,
            bytesPerSecond: 0,
            state: .entering,
            isPaused: false
        )
        
        items.removeAll { $0.id == id }
        items.append(item)
        
        withAnimation(.spring(response: 0.40, dampingFraction: 0.78)) {
            if let idx = items.firstIndex(where: { $0.id == id }) {
                items[idx].state = .active(autoHideDeadline: Date().addingTimeInterval(4.0))
            }
        }
        
        scheduleAutoCollapse(for: id, duration: 4.0)
    }
    
    public func onProgressUpdated(id: String, progress: Double, bytesPerSecond: Int64) {
        guard let idx = items.firstIndex(where: { $0.id == id }) else { return }
        items[idx].progress = min(1.0, max(0.0, progress))
        items[idx].bytesPerSecond = bytesPerSecond
    }
    
    public func onHoverStateChanged(id: String, isHovered: Bool) {
        guard let idx = items.firstIndex(where: { $0.id == id }) else { return }
        
        if isHovered {
            autoCollapseTasks[id]?.cancel()
            hoverGraceTasks[id]?.cancel()
            items[idx].state = .hovered
        } else {
            hoverGraceTasks[id]?.cancel()
            hoverGraceTasks[id] = Task { @MainActor in
                try? await Task.sleep(nanoseconds: 2_000_000_000)
                guard !Task.isCancelled else { return }
                self.collapseToNotificationCenter(id: id)
            }
        }
    }
    
    public func onPauseToggled(id: String) {
        guard let idx = items.firstIndex(where: { $0.id == id }) else { return }
        items[idx].isPaused.toggle()
        autoCollapseTasks[id]?.cancel()
        hoverGraceTasks[id]?.cancel()
        items[idx].state = .hovered
    }
    
    public func onTransferCompleted(id: String, filename: String) {
        autoCollapseTasks[id]?.cancel()
        hoverGraceTasks[id]?.cancel()
        
        if preferences.playCompletionSound {
            NSSound.beep()
        }
        
        withAnimation(.spring(response: 0.35, dampingFraction: 0.82)) {
            if let idx = items.firstIndex(where: { $0.id == id }) {
                items[idx].state = .completing(autoDismissDeadline: Date().addingTimeInterval(5.0))
                items[idx].progress = 1.0
            } else {
                let sizeMB = 0.0
                let item = TransferNotificationModel(
                    id: id,
                    filename: filename,
                    fileSizeMB: sizeMB,
                    deviceName: "Transfer",
                    progress: 1.0,
                    bytesPerSecond: 0,
                    state: .completing(autoDismissDeadline: Date().addingTimeInterval(5.0)),
                    isPaused: false
                )
                items.append(item)
            }
        }
        
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 5_000_000_000)
            self.dismissNotification(id: id)
        }
    }
    
    public func onTransferFailed(id: String, filename: String, error: String) {
        autoCollapseTasks[id]?.cancel()
        hoverGraceTasks[id]?.cancel()
        
        withAnimation(.spring(response: 0.35, dampingFraction: 0.82)) {
            if let idx = items.firstIndex(where: { $0.id == id }) {
                items[idx].state = .failed(errorMessage: error)
            } else {
                let item = TransferNotificationModel(
                    id: id,
                    filename: filename,
                    fileSizeMB: 0.0,
                    deviceName: "Transfer",
                    progress: 0.0,
                    bytesPerSecond: 0,
                    state: .failed(errorMessage: error),
                    isPaused: false
                )
                items.append(item)
            }
        }
    }
    
    public func collapseToNotificationCenter(id: String) {
        guard !preferences.keepVisibleDuringTransfer else { return }
        guard let idx = items.firstIndex(where: { $0.id == id }) else { return }
        
        withAnimation(.spring(response: 0.35, dampingFraction: 0.85)) {
            items[idx].state = .collapsedToNC
        }
        
        postToSystemNotificationCenter(item: items[idx])
    }
    
    public func dismissNotification(id: String) {
        withAnimation(.spring(response: 0.35, dampingFraction: 0.85)) {
            items.removeAll { $0.id == id }
        }
    }
    
    private func scheduleAutoCollapse(for id: String, duration: TimeInterval) {
        guard !preferences.keepVisibleDuringTransfer else { return }
        autoCollapseTasks[id]?.cancel()
        autoCollapseTasks[id] = Task { @MainActor in
            try? await Task.sleep(nanoseconds: UInt64(duration * 1_000_000_000))
            guard !Task.isCancelled else { return }
            self.collapseToNotificationCenter(id: id)
        }
    }
    
    private func postToSystemNotificationCenter(item: TransferNotificationModel) {
        let content = UNMutableNotificationContent()
        content.title = "Transferring \(item.filename)"
        content.body = "\(item.deviceName) • \(Int(item.progress * 100))% complete"
        content.sound = .default
        
        let request = UNNotificationRequest(identifier: item.id, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }
}

// MARK: - Floating Top-Right Window Panel

public final class TransferNotificationPanel: NSPanel {
    public init() {
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 360, height: 480),
            styleMask: [.titled, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        titlebarAppearsTransparent = true
        titleVisibility = .hidden
        standardWindowButton(.closeButton)?.isHidden = true
        standardWindowButton(.miniaturizeButton)?.isHidden = true
        standardWindowButton(.zoomButton)?.isHidden = true
        
        level = .floating
        hasShadow = false
        isOpaque = false
        backgroundColor = .clear
        hidesOnDeactivate = false
        ignoresMouseEvents = false
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .ignoresCycle]
    }
    
    public override var canBecomeKey: Bool { false }
    public override var canBecomeMain: Bool { false }
}

// MARK: - SwiftUI Stack & Card Hierarchy

public struct TransferNotificationStackView: View {
    @ObservedObject var manager: TransferNotificationManager = .shared
    
    public var body: some View {
        VStack(alignment: .trailing, spacing: 10) {
            ForEach(manager.visibleItems) { item in
                Group {
                    switch item.state {
                    case .completing:
                        TransferCompletionCardView(item: item, onDismiss: {
                            manager.dismissNotification(id: item.id)
                        })
                    case .failed(let err):
                        TransferErrorCardView(item: item, errorMessage: err, onRetry: {
                            manager.onTransferStarted(id: item.id, filename: item.filename, totalBytes: Int64(item.fileSizeMB * 1_048_576.0), deviceName: item.deviceName)
                        }, onDismiss: {
                            manager.dismissNotification(id: item.id)
                        })
                    default:
                        TransferNotificationCardView(item: item, manager: manager)
                    }
                }
                .transition(.asymmetric(
                    insertion: .move(edge: .trailing).combined(with: .opacity).combined(with: .scale(scale: 0.92)),
                    removal: .move(edge: .trailing).combined(with: .opacity)
                ))
            }
            
            if manager.collapsedCount > 0 {
                CollapsedTransfersIndicatorView(count: manager.collapsedCount)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
            }
            
            Spacer(minLength: 0)
        }
        .padding(.top, 8)
        .padding(.trailing, 8)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
        .animation(.spring(response: 0.38, dampingFraction: 0.82), value: manager.visibleItems.map(\.id))
    }
}

public struct TransferNotificationCardView: View {
    let item: TransferNotificationModel
    @ObservedObject var manager: TransferNotificationManager
    
    public var body: some View {
        HStack(spacing: 12) {
            ZStack {
                Circle()
                    .fill(Color.blue.opacity(0.15))
                    .frame(width: 36, height: 36)
                Image(systemName: item.isPaused ? "pause.fill" : "arrow.down.circle.fill")
                    .font(.system(size: 18, weight: .medium))
                    .foregroundColor(.blue)
            }
            
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text(item.filename)
                        .font(.system(size: 13, weight: .semibold, design: .rounded))
                        .lineLimit(1)
                    Spacer()
                    if !item.formattedSpeed.isEmpty {
                        Text(item.formattedSpeed)
                            .font(.system(size: 11, weight: .medium, design: .monospaced))
                            .foregroundColor(.secondary)
                    }
                }
                
                ProgressView(value: item.progress)
                    .progressViewStyle(.linear)
                    .tint(.blue)
                
                HStack {
                    Text("\(Int(item.progress * 100))% • \(item.deviceName)")
                        .font(.system(size: 11, weight: .regular))
                        .foregroundColor(.secondary)
                    Spacer()
                    Button(action: {
                        manager.onPauseToggled(id: item.id)
                    }) {
                        Image(systemName: item.isPaused ? "play.fill" : "pause.fill")
                            .font(.system(size: 10, weight: .bold))
                            .foregroundColor(.secondary)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        .padding(12)
        .frame(width: 330)
        .background(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(.ultraThinMaterial)
                .shadow(color: Color.black.opacity(0.18), radius: 10, x: 0, y: 5)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .stroke(Color.white.opacity(0.2), lineWidth: 1)
        )
        .onHover { hover in
            manager.onHoverStateChanged(id: item.id, isHovered: hover)
        }
    }
}

public struct TransferCompletionCardView: View {
    let item: TransferNotificationModel
    let onDismiss: () -> Void
    
    public var body: some View {
        HStack(spacing: 12) {
            ZStack {
                Circle()
                    .fill(Color.green.opacity(0.15))
                    .frame(width: 36, height: 36)
                Image(systemName: "checkmark.circle.fill")
                    .font(.system(size: 20, weight: .semibold))
                    .foregroundColor(.green)
            }
            
            VStack(alignment: .leading, spacing: 2) {
                Text("✓ Transfer Complete")
                    .font(.system(size: 13, weight: .bold, design: .rounded))
                    .foregroundColor(.primary)
                Text(item.filename)
                    .font(.system(size: 11, weight: .regular))
                    .foregroundColor(.secondary)
                    .lineLimit(1)
            }
            Spacer()
            Button(action: onDismiss) {
                Image(systemName: "xmark")
                    .font(.system(size: 10, weight: .bold))
                    .foregroundColor(.secondary)
            }
            .buttonStyle(.plain)
        }
        .padding(12)
        .frame(width: 330)
        .background(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(.ultraThinMaterial)
                .shadow(color: Color.black.opacity(0.15), radius: 8, x: 0, y: 4)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .stroke(Color.green.opacity(0.3), lineWidth: 1)
        )
    }
}

public struct TransferErrorCardView: View {
    let item: TransferNotificationModel
    let errorMessage: String
    let onRetry: () -> Void
    let onDismiss: () -> Void
    
    public var body: some View {
        HStack(spacing: 12) {
            ZStack {
                Circle()
                    .fill(Color.red.opacity(0.15))
                    .frame(width: 36, height: 36)
                Image(systemName: "exclamationmark.triangle.fill")
                    .font(.system(size: 18, weight: .semibold))
                    .foregroundColor(.red)
            }
            
            VStack(alignment: .leading, spacing: 2) {
                Text("Transfer Failed")
                    .font(.system(size: 13, weight: .bold))
                    .foregroundColor(.red)
                Text(item.filename)
                    .font(.system(size: 11, weight: .regular))
                    .lineLimit(1)
            }
            Spacer()
            HStack(spacing: 6) {
                Button("Retry", action: onRetry)
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)
                Button("Dismiss", action: onDismiss)
                    .buttonStyle(.bordered)
                    .controlSize(.small)
            }
        }
        .padding(12)
        .frame(width: 330)
        .background(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(.ultraThinMaterial)
                .shadow(color: Color.black.opacity(0.18), radius: 8, x: 0, y: 4)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .stroke(Color.red.opacity(0.3), lineWidth: 1)
        )
    }
}

public struct CollapsedTransfersIndicatorView: View {
    let count: Int
    
    public var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "square.stack.3d.up.fill")
                .font(.system(size: 13))
                .foregroundColor(.secondary)
            Text("+\(count) more active transfers in background")
                .font(.system(size: 11, weight: .medium))
                .foregroundColor(.secondary)
            Spacer()
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
        .frame(width: 330)
        .background(
            Capsule(style: .continuous)
                .fill(.ultraThinMaterial)
                .shadow(color: Color.black.opacity(0.12), radius: 6, x: 0, y: 3)
        )
    }
}

// MARK: - Preferences UI View

public struct TransferNotificationPreferencesView: View {
    @ObservedObject var preferences: TransferNotificationPreferences = .shared
    
    public init() {}
    
    public var body: some View {
        Form {
            Section(header: Text("Transfer Notifications").font(.headline)) {
                Toggle("Show transfer notifications", isOn: $preferences.showTransferNotifications)
                Toggle("Keep notification visible during transfer", isOn: $preferences.keepVisibleDuringTransfer)
                Toggle("Play completion sound", isOn: $preferences.playCompletionSound)
                
                HStack {
                    Text("Show notifications only for files larger than:")
                    Spacer()
                    TextField("0", value: $preferences.minFileSizeThresholdMB, format: .number)
                        .frame(width: 60)
                        .multilineTextAlignment(.trailing)
                    Text("MB")
                }
            }
        }
        .padding()
    }
}
