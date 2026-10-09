import SwiftUI
import QuickLook

struct TransfersDashboardView: View {
    @ObservedObject var store: LinkAllStore
    @Environment(\.colorScheme) var colorScheme
    
    var historyItems: [IpcActivityEntry] {
        // A transfer still in flight shows under Active, not as already sent.
        // A file the Mac sent shows once, from its started entry, not again from
        // the receiver's complete entry.
        let active = Set(store.activeTransfers.map(\.id))
        let started = Set(store.activityFeed.compactMap { $0.kind == "file_transfer_started" ? $0.transfer_id : nil })
        return store.activityFeed.filter { entry in
            guard entry.kind == "file_transfer_complete" || entry.kind == "file_transfer_started" || entry.kind == "folder_transfer_complete" else { return false }
            guard let id = entry.transfer_id else { return true }
            if active.contains(id) { return false }
            return !(entry.kind == "file_transfer_complete" && started.contains(id))
        }
    }
    
    @State private var quickLookURL: URL?
    @State private var hoveredItemId: Int64?
    @State private var spaceMonitor: Any?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 32) {
                
                // MARK: - Header
                VStack(alignment: .leading, spacing: 4) {
                    Text("Transfers")
                        .font(.system(size: 28, weight: .semibold, design: .rounded))
                        .foregroundStyle(CRTheme.ink)
                    Text("Manage active files and view recent history")
                        .font(.system(size: 14, weight: .regular))
                        .foregroundStyle(CRTheme.inkSoft)
                }
                .padding(.top, 40)
                .padding(.horizontal, 40)
                
                // MARK: - Active Transfers
                VStack(alignment: .leading, spacing: 16) {
                    Text("ACTIVE")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(CRTheme.inkSoft.opacity(0.6))
                        .kerning(1.2)
                        .padding(.horizontal, 40)
                    
                    if store.batchedTransfers.isEmpty {
                        Text("No active transfers")
                            .font(.system(size: 13, weight: .regular))
                            .foregroundStyle(CRTheme.inkSoft.opacity(0.8))
                            .padding(.horizontal, 40)
                            .padding(.bottom, 8)
                    } else {
                        VStack(spacing: 12) {
                            ForEach(store.batchedTransfers) { transfer in
                                ActiveTransferCard(transfer: transfer, store: store)
                            }
                        }
                        .padding(.horizontal, 40)
                    }
                }
                
                // MARK: - Active Speed Tests
                if !store.activeSpeedTests.isEmpty {
                    VStack(alignment: .leading, spacing: 16) {
                        Text("ACTIVE SPEED TESTS")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(CRTheme.inkSoft.opacity(0.6))
                            .kerning(1.2)
                            .padding(.horizontal, 40)
                        
                        VStack(spacing: 12) {
                            ForEach(store.activeSpeedTests) { test in
                                ActiveSpeedTestCard(test: test, store: store)
                            }
                        }
                        .padding(.horizontal, 40)
                    }
                }
                
                // MARK: - Recent History
                VStack(alignment: .leading, spacing: 16) {
                    Text("RECENT HISTORY")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(CRTheme.inkSoft.opacity(0.6))
                        .kerning(1.2)
                        .padding(.horizontal, 40)
                    
                    if historyItems.isEmpty {
                        Text("No recent file transfers")
                            .font(.system(size: 13, weight: .regular))
                            .foregroundStyle(CRTheme.inkSoft.opacity(0.8))
                            .padding(.horizontal, 40)
                            .padding(.bottom, 40)
                    } else {
                        LazyVStack(spacing: 0) {
                            ForEach(historyItems) { item in
                                TransferHistoryRow(entry: item) {
                                    if let dest = item.dest_path, !dest.isEmpty {
                                        quickLookURL = URL(fileURLWithPath: dest)
                                    }
                                }
                                .onHover { isHovering in
                                    if isHovering { hoveredItemId = item.id }
                                    else if hoveredItemId == item.id { hoveredItemId = nil }
                                }
                                if item.id != historyItems.last?.id {
                                    Divider()
                                        .opacity(0.5)
                                        .padding(.leading, 48) // Align with text content
                                }
                            }
                        }
                        .padding(.horizontal, 40)
                        .padding(.bottom, 40)
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Color.clear)
        .quickLookPreview($quickLookURL)
        .onAppear {
            spaceMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
                if event.keyCode == 49, let hoveredId = hoveredItemId { // Spacebar
                    if let target = historyItems.first(where: { $0.id == hoveredId }), let dest = target.dest_path, !dest.isEmpty {
                        quickLookURL = URL(fileURLWithPath: dest)
                        return nil // Consume event
                    }
                }
                return event
            }
        }
        .onDisappear {
            if let monitor = spaceMonitor {
                NSEvent.removeMonitor(monitor)
            }
        }
    }
}

// MARK: - Active Transfer Card

struct ActiveTransferCard: View {
    let transfer: FileTransferState
    @ObservedObject var store: LinkAllStore
    @State private var isPulsing = false
    
    var isTransferring: Bool {
        if case .transferring = transfer.status { return true }
        return false
    }
    
    var progressGradient: LinearGradient {
        if case .paused = transfer.status {
            return LinearGradient(colors: [CRTheme.stroke, CRTheme.stroke], startPoint: .leading, endPoint: .trailing)
        } else if case .failed = transfer.status {
            return LinearGradient(colors: [CRTheme.accentRed, CRTheme.accentRed], startPoint: .leading, endPoint: .trailing)
        }
        return LinearGradient(colors: [CRTheme.brandElectric, CRTheme.brandCyan], startPoint: .leading, endPoint: .trailing)
    }
    
    var statusText: String {
        switch transfer.status {
        case .queued: return "Queued for transfer..."
        case .incoming: return "Waiting for approval..."
        case .transferring:
            var text = ""
            if let speed = transfer.speedBps {
                let mbps = Double(speed) / 1_048_576.0
                if mbps >= 1.0 { text += String(format: "%.1f MB/s", mbps) }
                else { text += String(format: "%.0f KB/s", Double(speed) / 1024.0) }
            } else {
                text += "Calculating..."
            }
            if let eta = transfer.etaSecs, eta > 0 {
                text += " • \(eta)s remaining"
            }
            return text
        case .paused: return "Paused"
        case .verifying: return "Verifying..."
        case .complete: return "Complete"
        case .failed(let r): return "Failed: \(r)"
        case .cancelled: return "Cancelled"
        }
    }
    
    var sizeText: String {
        if transfer.isDirectory { return transfer.itemLabel }
        let mbTotal = Double(transfer.totalBytes) / 1_048_576.0
        let mbRecv = Double(transfer.bytesReceived) / 1_048_576.0
        
        if mbTotal >= 1.0 { 
            return String(format: "%.2f / %.2f MB", mbRecv, mbTotal) 
        } else {
            let kbTotal = Double(transfer.totalBytes) / 1_024.0
            let kbRecv = Double(transfer.bytesReceived) / 1_024.0
            if kbTotal >= 1.0 { 
                return String(format: "%.2f / %.2f KB", kbRecv, kbTotal) 
            } else {
                return String(format: "%.0f / %lld B", Double(transfer.bytesReceived), transfer.totalBytes)
            }
        }
    }
    
    var percentText: String {
        // A folder's bytes in flight say nothing about the whole folder.
        if transfer.isDirectory { return transfer.exactPercentString }
        if transfer.totalBytes > 0 {
            let ratio = max(0, min(1, Double(transfer.bytesReceived) / Double(transfer.totalBytes)))
            return String(format: "%.1f%%", ratio * 100.0)
        } else {
            return "0.0%"
        }
    }
    
    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .center, spacing: 16) {
                // Icon
                ZStack {
                    Circle()
                        .fill(CRTheme.brandElectric.opacity(isPulsing && isTransferring ? 0.25 : 0.1))
                        .frame(width: 44, height: 44)
                        .scaleEffect(isPulsing && isTransferring ? 1.15 : 1.0)
                        .animation(.easeInOut(duration: 1.0).repeatForever(autoreverses: true), value: isPulsing)
                    
                    Image(systemName: transfer.isDirectory ? "folder.fill" : "doc.fill")
                        .font(.system(size: 20))
                        .foregroundStyle(CRTheme.brandElectric)
                        .scaleEffect(isPulsing && isTransferring ? 1.05 : 1.0)
                        .animation(.easeInOut(duration: 1.0).repeatForever(autoreverses: true), value: isPulsing)
                }
                .whileWindowVisible($isPulsing)
                
                // Details
                VStack(alignment: .leading, spacing: 4) {
                    HStack(alignment: .center, spacing: 6) {
                        Text(transfer.fileName)
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(CRTheme.ink)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        
                        if transfer.isDirectory {
                            Text(transfer.itemLabel)
                                .font(.system(size: 11, weight: .medium))
                                .foregroundStyle(CRTheme.inkSoft)
                                .padding(.horizontal, 6)
                                .padding(.vertical, 2)
                                .background(CRTheme.brandElectric.opacity(0.1))
                                .cornerRadius(6)
                        }
                    }
                    
                    if #available(macOS 13.0, *) {
                        Text(statusText)
                            .font(.system(size: 12, weight: .regular))
                            .foregroundStyle(CRTheme.inkSoft)
                            .lineLimit(1)
                            .truncationMode(.tail)
                            .contentTransition(.numericText())
                            .animation(.linear(duration: 0.25), value: statusText)
                    } else {
                        Text(statusText)
                            .font(.system(size: 12, weight: .regular))
                            .foregroundStyle(CRTheme.inkSoft)
                            .lineLimit(1)
                            .truncationMode(.tail)
                    }
                }
                
                Spacer(minLength: 16)
                
                // Actions
                HStack(spacing: 8) {
                    if case .incoming = transfer.status {
                        actionButton(icon: "checkmark", label: "Accept", color: .green) { store.acceptFileTransfer(transfer) }
                        actionButton(icon: "xmark", label: "Reject", color: .red) { store.rejectFileTransfer(transfer) }
                    } else if case .transferring = transfer.status {
                        if !transfer.isDirectory {
                            actionButton(icon: "pause.fill", label: "Pause") { store.pauseFileTransfer(transfer) }
                        }
                        actionButton(icon: "xmark", label: "Cancel") { store.cancelFileTransfer(transfer) }
                    } else if case .paused = transfer.status {
                        actionButton(icon: "play.fill", label: "Resume") { store.resumeFileTransfer(transfer) }
                        actionButton(icon: "xmark", label: "Cancel") { store.cancelFileTransfer(transfer) }
                    } else if case .complete = transfer.status {
                        actionButton(icon: "checkmark", label: "Dismiss", color: .green) { store.cancelFileTransfer(transfer) }
                    }
                }
            }
            
            // Progress Bar Section
            if case .incoming = transfer.status {
                // Hide for incoming
            } else if case .failed = transfer.status {
                // Hide for failed
            } else {
                VStack(spacing: 8) {
                    HStack {
                        if #available(macOS 13.0, *) {
                            Text(percentText)
                                .font(.system(size: 11, weight: .medium).monospacedDigit())
                                .foregroundStyle(CRTheme.ink)
                                .contentTransition(.numericText())
                                .animation(.linear(duration: 0.15), value: percentText)
                            
                            Spacer()
                            
                            Text(sizeText)
                                .font(.system(size: 11, weight: .regular).monospacedDigit())
                                .foregroundStyle(CRTheme.inkSoft)
                                .contentTransition(.numericText())
                                .animation(.linear(duration: 0.15), value: sizeText)
                        } else {
                            Text(percentText)
                                .font(.system(size: 11, weight: .medium).monospacedDigit())
                                .foregroundStyle(CRTheme.ink)
                            
                            Spacer()
                            
                            Text(sizeText)
                                .font(.system(size: 11, weight: .regular).monospacedDigit())
                                .foregroundStyle(CRTheme.inkSoft)
                        }
                    }
                    
                    // Gradient Bar
                    GeometryReader { geo in
                        ZStack(alignment: .leading) {
                            RoundedRectangle(cornerRadius: 5)
                                .fill(CRTheme.inkSoft.opacity(0.1))
                                .frame(height: 10)
                            
                            RoundedRectangle(cornerRadius: 5)
                                .fill(progressGradient)
                                .frame(width: max(0, geo.size.width * CGFloat(transfer.exactRatio)), height: 10)
                                .animation(.linear(duration: 0.5), value: transfer.exactRatio)
                        }
                    }
                    .frame(height: 10)
                }
                .padding(.top, 20)
            }
        }
        .padding(24)
        .background(CRTheme.surfaceStrong)
        .crCard(cornerRadius: 24)
    }
    
    private func actionButton(icon: String, label: String, color: Color = CRTheme.inkSoft, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.system(size: 12, weight: .bold))
                .foregroundStyle(color)
                .frame(width: 32, height: 32)
                .background(Color.black.opacity(0.04))
                .clipShape(Circle())
        }
        .buttonStyle(.plain)
        .crHoverScale(scale: 1.05)
        .accessibilityLabel(label)
    }
}

// MARK: - History Row

private struct TransferHistoryRow: View {
    let entry: IpcActivityEntry
    var onPreview: (() -> Void)? = nil
    
    var formattedTime: String {
        let date = Date(timeIntervalSince1970: TimeInterval(entry.timestamp_ms) / 1000.0)
        let formatter = DateFormatter()
        formatter.timeStyle = .short
        return formatter.string(from: date)
    }
    
    /// The core only records a save path for incoming files; outgoing ones carry the local device's name.
    var isReceived: Bool { !(entry.dest_path ?? "").isEmpty }

    var displaySize: String {
        // Folder entries carry a file count ("15 files") instead of a byte size.
        guard let b = entry.file_bytes else { return entry.text_preview ?? "Unknown size" }
        let mb = Double(b) / 1_048_576.0
        if mb >= 1.0 { return String(format: "%.1f MB", mb) }
        let kb = Double(b) / 1_024.0
        if kb >= 1.0 { return String(format: "%.0f KB", kb) }
        return "\(b) B"
    }
    
    var body: some View {
        HStack(alignment: .center, spacing: 16) {
            // Icon
            Image(systemName: isReceived ? "arrow.down.doc" : "arrow.up.doc")
                .font(.system(size: 18, weight: .light))
                .foregroundStyle(CRTheme.inkSoft.opacity(0.8))
                .frame(width: 32)
            
            // Details
            VStack(alignment: .leading, spacing: 2) {
                Text(entry.file_name ?? "Unknown File")
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(CRTheme.ink)
                    .lineLimit(1)
                    .truncationMode(.middle)
                
                Text(isReceived ? "Received from \(entry.device_name) • \(displaySize)" : "Sent • \(displaySize)")
                    .font(.system(size: 12, weight: .regular))
                    .foregroundStyle(CRTheme.inkSoft)
            }
            
            Spacer(minLength: 16)
            
            // Time
            Text(formattedTime)
                .font(.system(size: 12, weight: .regular))
                .foregroundStyle(CRTheme.inkSoft.opacity(0.6))
            
            // Actions
            if let dest = entry.dest_path, !dest.isEmpty {
                HStack(spacing: 12) {
                    Button { onPreview?() } label: {
                        Image(systemName: "eye.fill")
                            .font(.system(size: 14))
                            .foregroundStyle(CRTheme.inkSoft)
                    }
                    .buttonStyle(.plain)
                    .crHoverScale(scale: 1.05)
                    .help("Quick Look (Space)")
                    
                    Button {
                        let url = URL(fileURLWithPath: dest)
                        NSWorkspace.shared.activateFileViewerSelecting([url])
                    } label: {
                        Image(systemName: "folder.fill")
                            .font(.system(size: 14))
                            .foregroundStyle(CRTheme.inkSoft)
                    }
                    .buttonStyle(.plain)
                    .crHoverScale(scale: 1.05)
                    .help("Show in Finder")
                }
                .padding(.leading, 8)
            }
        }
        .padding(.vertical, 12)
        .contentShape(Rectangle())
    }
}

// MARK: - Active Speed Test Card

struct ActiveSpeedTestCard: View {
    let test: SpeedTestState
    @ObservedObject var store: LinkAllStore
    @State private var isHovered = false
    
    var body: some View {
        HStack(spacing: 16) {
            // Icon
            ZStack {
                Circle().fill(CRTheme.brandCyan.opacity(0.12))
                    .frame(width: 44, height: 44)
                Image(systemName: test.phase == "Receiving" ? "arrow.down.circle.fill" : "arrow.up.circle.fill")
                    .font(.system(size: 20))
                    .foregroundStyle(CRTheme.brandCyan)
            }
            
            // Details
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text("Speed Test (\(test.phase))")
                        .font(.system(size: 14, weight: .semibold, design: .rounded))
                        .foregroundStyle(CRTheme.ink)
                    
                    if let peer = store.peers.first(where: { $0.id == test.id }) {
                        CRTag(text: peer.displayName, tint: CRTheme.brandViolet)
                    }
                }
                
                Text("\(test.speedMbpsString)")
                    .font(.system(size: 13, weight: .medium, design: .monospaced))
                    .foregroundStyle(CRTheme.brandCyan)
            }
            
            Spacer()
            
            // Progress / Status
            VStack(alignment: .trailing, spacing: 4) {
                Text("\(test.durationSecs)s")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(CRTheme.inkFaint)
            }
        }
        .padding(16)
        .background(CRTheme.surfaceStrong)
        .crCard(cornerRadius: CRTheme.radiusMedium, highlighted: isHovered, accent: CRTheme.brandCyan)
        .onHover { isHovered = $0 }
    }
}
