// ActivityFeedView.swift — Link All macOS v4
// Live activity feed: day-grouped rows, inline transfer progress, clipboard policy.

import SwiftUI

// MARK: - Activity Feed Root

struct ActivityFeedView: View {
    @EnvironmentObject var store: LinkAllStore
    @State private var searchText  = ""
    @State private var filterKind: KindFilter = .all
    @State private var expandedID: UUID?       = nil

    @State private var filteredEntries: [IpcActivityEntry] = []
    @State private var dayGroups: [(label: String, entries: [IpcActivityEntry])] = []

    private func updateFeed() {
        var entries = store.activityFeed
        if filterKind != .all { entries = entries.filter { filterKind.matches($0.kind) } }
        if !searchText.isEmpty {
            entries = entries.filter {
                $0.summary.localizedCaseInsensitiveContains(searchText)
                || $0.device_name.localizedCaseInsensitiveContains(searchText)
                || ($0.text_preview?.localizedCaseInsensitiveContains(searchText) ?? false)
            }
        }
        filteredEntries = entries

        let cal = Calendar.current
        let byDay = Dictionary(grouping: entries) { entry -> Date in
            cal.startOfDay(for: Date(timeIntervalSince1970: Double(entry.timestamp_ms) / 1000))
        }
        dayGroups = byDay.keys.sorted(by: >).map { day in
            let label: String
            if cal.isDateInToday(day)          { label = "Today" }
            else if cal.isDateInYesterday(day) { label = "Yesterday" }
            else {
                let f = DateFormatter()
                let thisYear = cal.component(.year, from: Date())
                let entryYear = cal.component(.year, from: day)
                f.dateFormat = thisYear == entryYear ? "EEEE, MMMM d" : "MMMM d, yyyy"
                label = f.string(from: day)
            }
            return (label, byDay[day]!)
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            FeedToolbar(searchText: $searchText, filterKind: $filterKind,
                        resultCount: filteredEntries.count)
            CRDivider()

            // Active file transfers
            if !store.batchedTransfers.isEmpty && searchText.isEmpty {
                Section(header: Text("Active Transfers").font(.subheadline).foregroundColor(.secondary)) {
                    ForEach(store.batchedTransfers) { transfer in
                        FileTransferBanner(transfer: transfer)
                        CRDivider().padding(.leading, 54)
                    }
                }
                .background(CRTheme.accentIndigo.opacity(0.030))
                CRDivider()
            }

            // Pending clipboard banner
            let pending = store.activityFeed.filter { $0.isApplicable }
            if !pending.isEmpty && searchText.isEmpty {
                PendingClipboardBanner(items: pending)
                CRDivider()
            }

            // Feed content
            if filteredEntries.isEmpty {
                FeedEmptyState(hasFilter: !searchText.isEmpty || filterKind != .all)
            } else {
                ScrollView(.vertical, showsIndicators: false) {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(Array(dayGroups.enumerated()), id: \.offset) { _, group in
                            // Day header
                            FeedDayHeader(label: group.label)
                            ForEach(group.entries) { entry in
                                ActivityEntryRow(
                                    entry: entry,
                                    isExpanded: expandedID == entry.uuid,
                                    onToggleExpand: {
                                        withAnimation(.crSpring) {
                                            expandedID = expandedID == entry.uuid ? nil : entry.uuid
                                        }
                                    }
                                )
                                CRDivider().padding(.leading, 54).opacity(0.50)
                            }
                        }
                    }
                }
            }
            // Hidden refresh shortcut
            Button("") {
                Task { await store.refreshActivityFeed() }
            }
            .keyboardShortcut("r", modifiers: .command)
            .hidden()
        }
        .frame(minWidth: 380, minHeight: 480)
        .task { await store.refreshActivityFeed() }
        .onChange(of: store.activityFeed) { _ in updateFeed() }
        .onChange(of: searchText) { _ in updateFeed() }
        .onChange(of: filterKind) { _ in updateFeed() }
        .onAppear { updateFeed() }
    }
}

// MARK: - Feed Toolbar

private struct FeedToolbar: View {
    @Binding var searchText: String
    @Binding var filterKind: KindFilter
    let resultCount: Int

    var body: some View {
        HStack(spacing: 10) {
            CRSearchField(placeholder: "Filter activity…", text: $searchText)

            HStack(spacing: 2) {
                ForEach(KindFilter.allCases) { kind in
                    Button(kind.label) { withAnimation(.crFast) { filterKind = kind } }
                        .font(.system(size: 12, weight: filterKind == kind ? .semibold : .regular))
                        .padding(.horizontal, 9).padding(.vertical, 4.5)
                        .background {
                            if filterKind == kind {
                                Capsule()
                                    .fill(CRTheme.brandElectric.opacity(0.09))
                                    .overlay { Capsule().strokeBorder(CRTheme.brandElectric.opacity(0.20), lineWidth: 0.5) }
                            }
                        }
                        .foregroundStyle(filterKind == kind ? CRTheme.brandElectric : CRTheme.inkSoft)
                        .buttonStyle(.plain).animation(.crFast, value: filterKind)
                }
            }

            Spacer()

            if !searchText.isEmpty || filterKind != .all {
                Text("\(resultCount)")
                    .font(.system(size: 11, design: .rounded)).foregroundStyle(CRTheme.inkSubtle)
                    .transition(.opacity)
            }
        }
        .padding(.horizontal, 13).padding(.vertical, 9)
        .background(CRTheme.surfaceElevated)
        .animation(.crFast, value: searchText.isEmpty && filterKind == .all)
    }
}

private enum KindFilter: String, CaseIterable, Identifiable {
    case all, clipboard, files, peers
    var id: String { rawValue }
    var label: String {
        switch self {
        case .all: return "All"; case .clipboard: return "Clipboard"
        case .files: return "Files"; case .peers: return "Peers"
        }
    }
    func matches(_ kind: String) -> Bool {
        switch self {
        case .all:       return true
        case .clipboard: return kind.contains("clipboard")
        case .files:     return kind.contains("file")
        case .peers:     return kind.contains("peer")
        }
    }
}

// MARK: - Day Header

private struct FeedDayHeader: View {
    let label: String
    var body: some View {
        HStack(spacing: 8) {
            Text(label)
                .font(.system(size: 11, weight: .semibold)).foregroundStyle(CRTheme.inkSubtle)
            Rectangle().fill(CRTheme.stroke.opacity(0.35)).frame(height: 0.5)
        }
        .padding(.horizontal, 13).padding(.vertical, 7)
    }
}

// MARK: - Empty State

private struct FeedEmptyState: View {
    var hasFilter: Bool
    var body: some View {
        VStack(spacing: 14) {
            ZStack {
                Circle().fill(CRTheme.inkSubtle.opacity(0.06)).frame(width: 64, height: 64)
                Image(systemName: hasFilter ? "line.3.horizontal.decrease.circle" : "tray")
                    .font(.system(size: 24, weight: .light)).foregroundStyle(CRTheme.inkSubtle)
                    .symbolRenderingMode(.hierarchical)
            }
            VStack(spacing: 4) {
                Text(hasFilter ? "No matching activity" : "No activity yet")
                    .font(.system(size: 13.5, weight: .semibold)).foregroundStyle(CRTheme.ink)
                Text(hasFilter
                     ? "Try clearing the search or changing the filter"
                     : "Clipboard events and file transfers appear here in real time")
                    .font(.system(size: 12)).foregroundStyle(CRTheme.inkSoft)
                    .multilineTextAlignment(.center).lineSpacing(1.5).frame(maxWidth: 260)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity).padding(.vertical, 48)
    }
}

// MARK: - Pending Clipboard Banner

private struct PendingClipboardBanner: View {
    let items: [IpcActivityEntry]
    @EnvironmentObject var store: LinkAllStore
    @State private var isHovered = false

    var body: some View {
        HStack(spacing: 14) {
            ZStack {
                Circle().fill(CRTheme.brandElectric.opacity(0.15)).frame(width: 36, height: 36)
                Image(systemName: "doc.on.clipboard.fill")
                    .font(.system(size: 14, weight: .semibold)).foregroundStyle(CRTheme.brandElectric)
            }
            .shadow(color: CRTheme.brandElectric.opacity(0.3), radius: 8, x: 0, y: 2)

            VStack(alignment: .leading, spacing: 4) {
                Text("\(items.count) pending clipboard item\(items.count == 1 ? "" : "s")")
                    .font(.system(size: 13, weight: .bold)).foregroundStyle(CRTheme.ink)
                if let first = items.first {
                    Text("Latest from \(first.device_name)")
                        .font(.system(size: 11.5)).foregroundStyle(CRTheme.inkSoft)
                }
            }
            Spacer()
            Button("Apply Latest") {
                NSHapticFeedbackManager.defaultPerformer.perform(.generic, performanceTime: .default)
                if let first = items.first { Task { await store.applyClipboard(entry: first) } }
            }
            .buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.brandElectric))
        }
        .padding(.horizontal, 16).padding(.vertical, 14)
        .background {
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .fill(isHovered ? CRTheme.surfaceElevated : CRTheme.brandElectric.opacity(0.04))
                .shadow(color: .black.opacity(isHovered ? 0.05 : 0), radius: 10, y: 4)
                .overlay {
                    RoundedRectangle(cornerRadius: 16, style: .continuous)
                        .strokeBorder(isHovered ? CRTheme.stroke : CRTheme.brandElectric.opacity(0.15), lineWidth: 0.5)
                }
        }
        .padding(.horizontal, 12).padding(.vertical, 8)
        .onHover { isHovered = $0 }
        .animation(.crFast, value: isHovered)
    }
}

// MARK: - Activity Entry Row

struct ActivityEntryRow: View {
    let entry:          IpcActivityEntry
    var isExpanded:     Bool
    let onToggleExpand: () -> Void

    @EnvironmentObject var store: LinkAllStore
    @State private var applying  = false
    @State private var isHovered = false

    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            // Kind icon with glow
            ZStack {
                Circle().fill(kindColor.opacity(0.12)).frame(width: 36, height: 36)
                Image(systemName: kindIcon)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(kindColor).symbolRenderingMode(.hierarchical)
            }
            .padding(.top, 2)
            .shadow(color: kindColor.opacity(0.3), radius: 8, x: 0, y: 2)

            // Content
            VStack(alignment: .leading, spacing: 6) {
                // Header line
                HStack(spacing: 0) {
                    Text(entry.device_name)
                        .font(.system(size: 13, weight: .semibold)).foregroundStyle(CRTheme.ink)
                    Text("  ·  ").foregroundStyle(CRTheme.inkFaint).font(.system(size: 10))
                    Text(formattedTime(entry.timestamp_ms))
                        .font(.system(size: 12)).foregroundStyle(CRTheme.inkSoft)
                    Spacer(minLength: 0)
                }

                // Summary
                Text(entry.summary)
                    .font(.system(size: 13.5)).foregroundStyle(CRTheme.ink)
                    .lineLimit(isExpanded ? nil : 2)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.bottom, 2)

                // Relay path visualization
                if !entry.relay_path.isEmpty {
                    HStack(spacing: 6) {
                        Image(systemName: "arrow.uturn.right")
                            .font(.system(size: 10, weight: .semibold)).foregroundStyle(CRTheme.brandElectric)
                        
                        HStack(spacing: 4) {
                            ForEach(Array(entry.relay_path.enumerated()), id: \.offset) { idx, node in
                                Text(node).font(.system(size: 11, weight: .medium)).foregroundStyle(CRTheme.inkSoft)
                                if idx < entry.relay_path.count - 1 {
                                    Image(systemName: "chevron.right").font(.system(size: 8)).foregroundStyle(CRTheme.inkFaint)
                                }
                            }
                        }
                        .padding(.horizontal, 8).padding(.vertical, 4)
                        .background(Capsule().fill(CRTheme.surfaceStrong))
                    }
                }

                // Expanded text preview
                if isExpanded, let preview = entry.text_preview, !preview.isEmpty {
                    ScrollView(.horizontal, showsIndicators: false) {
                        Text(preview)
                            .font(.system(size: 11.5, design: .monospaced)).foregroundStyle(CRTheme.ink)
                            .padding(.horizontal, 12).padding(.vertical, 10)
                    }
                    .background {
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .fill(CRTheme.surfaceElevated)
                            .overlay {
                                RoundedRectangle(cornerRadius: 10, style: .continuous)
                                    .strokeBorder(CRTheme.stroke.opacity(0.50), lineWidth: 0.5)
                            }
                    }
                    .padding(.vertical, 4)
                    .transition(.opacity.combined(with: .scale(scale: 0.95, anchor: .top)))
                }

                // Kind tag + expand hint
                HStack(spacing: 8) {
                    CRTag(text: kindLabel, tint: kindColor)
                    if entry.text_preview != nil {
                        Button(isExpanded ? "Collapse" : "Expand") { onToggleExpand() }
                            .font(.system(size: 11, weight: .medium))
                            .foregroundStyle(CRTheme.brandElectric)
                            .buttonStyle(.plain)
                            .padding(.horizontal, 8).padding(.vertical, 4)
                            .background(Capsule().fill(CRTheme.brandElectric.opacity(0.1)))
                            .contentShape(Rectangle())
                    }
                }
            }
            .animation(.crSpring, value: isExpanded)

            Spacer(minLength: 0)

            // Apply / applied badge
            VStack(alignment: .trailing, spacing: 4) {
                if entry.isApplicable {
                    Button {
                        NSHapticFeedbackManager.defaultPerformer.perform(.generic, performanceTime: .default)
                        applying = true
                        Task { await store.applyClipboard(entry: entry); applying = false }
                    } label: {
                        if applying {
                            ProgressView().controlSize(.mini).padding(.horizontal, 10).padding(.vertical, 6)
                        } else {
                            Label("Apply", systemImage: "doc.on.clipboard.fill")
                                .font(.system(size: 11.5, weight: .bold))
                        }
                    }
                    .buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.brandElectric))
                    .disabled(applying)
                } else if entry.applied_locally {
                    HStack(spacing: 5) {
                        Image(systemName: "checkmark.seal.fill")
                            .font(.system(size: 12)).foregroundStyle(CRTheme.accentGreen)
                        Text("Applied")
                            .font(.system(size: 11.5, weight: .bold)).foregroundStyle(CRTheme.accentGreen)
                    }
                    .padding(.horizontal, 10).padding(.vertical, 6)
                    .background(Capsule().fill(CRTheme.accentGreen.opacity(0.12)))
                }
            }
        }
        .padding(.horizontal, 16).padding(.vertical, 16)
        .background {
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .fill(isHovered ? CRTheme.surfaceElevated : CRTheme.surfaceStrong.opacity(0.5))
                .shadow(color: .black.opacity(isHovered ? 0.05 : 0), radius: 10, y: 4)
                .overlay {
                    RoundedRectangle(cornerRadius: 16, style: .continuous)
                        .strokeBorder(isHovered ? CRTheme.stroke : CRTheme.strokeSoft, lineWidth: 0.5)
                }
        }
        .padding(.horizontal, 12).padding(.vertical, 4)
        .contentShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
        .onHover { isHovered = $0 }
        .animation(.crFast, value: isHovered)
        // ── Right-click context menu ──────────────────────────────────────────
        .contextMenu {
            if let preview = entry.text_preview, !preview.isEmpty {
                Button {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(preview, forType: .string)
                } label: { Label("Copy text", systemImage: "doc.on.clipboard") }

                if entry.isApplicable {
                    Button { Task { await store.applyClipboard(entry: entry) } }
                    label: { Label("Apply to clipboard", systemImage: "doc.on.clipboard.fill") }
                }
                Divider()
            }

            if let dest = entry.dest_path, !dest.isEmpty {
                Button { NSWorkspace.shared.selectFile(dest, inFileViewerRootedAtPath: "") }
                label: { Label("Show in Finder", systemImage: "folder") }
                Divider()
            }

            Button {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(entry.summary, forType: .string)
            } label: { Label("Copy summary", systemImage: "text.quote") }
        }
    }

    // MARK: Kind mapping (string → SF Symbol, colour, label)

    private var kindIcon: String {
        switch entry.kind {
        case "remote_clipboard_available", "clipboard_text": return "doc.on.clipboard"
        case "clipboard_image":        return "photo"
        case "file_transfer_started":  return "arrow.down.circle"
        case "file_transfer_complete": return "checkmark.circle.fill"
        case "file_transfer_failed":   return "xmark.circle"
        case "folder_transfer_complete": return "folder.fill"
        case "peer_connected":         return "wifi"
        case "peer_disconnected":      return "wifi.slash"
        case "sync_paused":            return "pause.circle"
        case "sync_resumed":           return "play.circle"
        case "clipboard_applied":      return "checkmark.circle"
        default:                       return "info.circle"
        }
    }

    private var kindLabel: String {
        switch entry.kind {
        case "remote_clipboard_available":
            return entry.applied_locally ? "Applied" : "Pending"
        case "clipboard_text":        return "Text"
        case "clipboard_image":       return "Image"
        case "file_transfer_started": return "Transfer"
        case "file_transfer_complete":return "Complete"
        case "file_transfer_failed":  return "Failed"
        case "folder_transfer_complete": return "Folder"
        case "peer_connected":        return "Connected"
        case "peer_disconnected":     return "Disconnected"
        case "sync_paused":           return "Paused"
        case "sync_resumed":          return "Resumed"
        default:                      return "Event"
        }
    }

    private var kindColor: Color {
        switch entry.kind {
        case "remote_clipboard_available":
            return entry.applied_locally ? CRTheme.accentGreen : CRTheme.brandElectric
        case "file_transfer_complete", "folder_transfer_complete": return CRTheme.accentGreen
        case "file_transfer_failed":   return CRTheme.accentRed
        case "peer_connected":         return CRTheme.accentGreen
        case "peer_disconnected":      return CRTheme.inkSoft
        case "sync_paused":            return CRTheme.accentOrange
        case "sync_resumed":           return CRTheme.accentGreen
        default:                       return CRTheme.inkSoft
        }
    }

    private func formattedTime(_ ms: Int64) -> String {
        Date(timeIntervalSince1970: Double(ms) / 1000).relativeTimeString()
    }
}

// MARK: - IpcActivityEntry identity helper

private extension IpcActivityEntry {
    // Stable UUID derived from the int64 id for use as SwiftUI identity
    var uuid: UUID { UUID(uuidString: String(format: "%08X-0000-0000-0000-%012X", id >> 32, id & 0xFFFFFFFFFFFF)) ?? UUID() }
}

// MARK: - File Transfer Banner

struct FileTransferBanner: View {
    let transfer: FileTransferState
    @EnvironmentObject var store: LinkAllStore
    @State private var isHovered = false

    var body: some View {
        HStack(spacing: 14) {
            ZStack {
                Circle().fill(CRTheme.accentIndigo.opacity(0.15)).frame(width: 36, height: 36)
                Image(systemName: transferIcon)
                    .font(.system(size: 14, weight: .semibold)).foregroundStyle(CRTheme.accentIndigo)
            }
            .shadow(color: CRTheme.accentIndigo.opacity(0.3), radius: 8, x: 0, y: 2)

            VStack(alignment: .leading, spacing: 5) {
                HStack(spacing: 8) {
                    Text(transfer.fileName)
                        .font(.system(size: 13, weight: .semibold)).foregroundStyle(CRTheme.ink).lineLimit(1)
                    if transfer.isDirectory {
                        Text(transfer.itemLabel)
                            .font(.system(size: 10, weight: .medium))
                            .foregroundStyle(CRTheme.inkSoft)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 2)
                            .background(CRTheme.accentIndigo.opacity(0.1))
                            .cornerRadius(4)
                    }
                    Spacer(minLength: 0)
                    StopwatchSizeText(bytesReceived: Double(transfer.bytesReceived), totalBytes: transfer.totalBytes, isDashboard: false)
                        .animation(.linear(duration: 0.25), value: transfer.bytesReceived)
                }

                if case .transferring = transfer.status {
                    VStack(alignment: .leading, spacing: 4) {
                        let prog = transfer.exactRatio
                        StopwatchProgressView(progress: prog, tint: CRTheme.accentIndigo)
                            .animation(.linear(duration: 0.5), value: prog)
                        HStack {
                            Text("\(transfer.isOutbound ? "To" : "From") \(transfer.fromDeviceName)")
                            Spacer()
                        }
                        .font(.system(size: 11)).foregroundStyle(CRTheme.inkSoft)
                    }
                } else if case .paused = transfer.status {
                    VStack(alignment: .leading, spacing: 4) {
                        let prog = transfer.exactRatio
                        StopwatchProgressView(progress: prog, tint: CRTheme.accentOrange)
                            .animation(.linear(duration: 0.5), value: prog)
                        HStack {
                            Text("Paused - \(transfer.isOutbound ? "To" : "From") \(transfer.fromDeviceName)")
                            Spacer()
                        }
                        .font(.system(size: 11)).foregroundStyle(CRTheme.inkSoft)
                    }
                } else {
                    Text(statusText).font(.system(size: 11.5)).foregroundStyle(statusColor)
                }
            }

            // Action buttons
            actionGroup
        }
        .padding(.horizontal, 16).padding(.vertical, 14)
        .background {
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .fill(isHovered ? CRTheme.surfaceElevated : CRTheme.accentIndigo.opacity(0.04))
                .shadow(color: .black.opacity(isHovered ? 0.05 : 0), radius: 10, y: 4)
                .overlay {
                    RoundedRectangle(cornerRadius: 16, style: .continuous)
                        .strokeBorder(isHovered ? CRTheme.stroke : CRTheme.accentIndigo.opacity(0.15), lineWidth: 0.5)
                }
        }
        .padding(.horizontal, 12).padding(.vertical, 8)
        .onHover { isHovered = $0 }
        .animation(.crFast, value: isHovered)
    }

    @ViewBuilder private var actionGroup: some View {
        switch transfer.status {
        case .incoming:
            HStack(spacing: 7) {
                Button("Accept") { store.acceptFileTransfer(transfer) }
                    .buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.accentGreen))
                Button("Reject") { store.rejectFileTransfer(transfer) }
                    .buttonStyle(CRDestructiveButtonStyle())
            }
        case .transferring:
            HStack(spacing: 7) {
                // A folder pauses file by file in the engine; offer only Cancel.
                if !transfer.isDirectory {
                    Button("Pause") { store.pauseFileTransfer(transfer) }
                        .buttonStyle(CRSecondaryButtonStyle())
                }
                Button("Cancel") { store.cancelFileTransfer(transfer) }
                    .buttonStyle(CRDestructiveButtonStyle())
            }
        case .paused:
            HStack(spacing: 7) {
                Button("Resume") { store.resumeFileTransfer(transfer) }
                    .buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.accentIndigo))
                Button("Cancel") { store.cancelFileTransfer(transfer) }
                    .buttonStyle(CRDestructiveButtonStyle())
            }
        default:
            EmptyView()
        }
    }

    private var transferIcon: String {
        if transfer.isDirectory { return "folder.fill" }
        switch transfer.status {
        case .queued:       return "hourglass"
        case .incoming:     return "arrow.down.circle"
        case .transferring: return "arrow.down.circle.fill"
        case .paused:       return "pause.circle.fill"
        case .verifying:    return "checkmark.seal"
        case .complete:     return "checkmark.circle.fill"
        case .failed:       return "xmark.circle.fill"
        case .cancelled:    return "minus.circle.fill"
        }
    }

    private var statusText: String {
        switch transfer.status {
        case .complete(let path): return "Saved to \(path)"
        case .failed(let reason): return reason
        default: return ""
        }
    }

    private var statusColor: Color {
        switch transfer.status {
        case .complete: return CRTheme.accentGreen
        case .failed:   return CRTheme.accentOrange
        default:        return CRTheme.inkSoft
        }
    }
}

// MARK: - Clipboard Policy View

struct ClipboardPolicyView: View {
    @EnvironmentObject var store: LinkAllStore
    @State private var timelineFirst = false
    @State private var autoApply     = false

    var body: some View {
        Form {
            Section {
                Toggle("Timeline-first mode", isOn: $timelineFirst)
                    .onChange(of: timelineFirst) { v in Task { await store.setTimelineFirstMode(enabled: v) } }
                Text("Remote clipboard items land in the activity feed instead of immediately overwriting your clipboard. Tap Apply on any item to paste it.")
                    .font(.caption).foregroundStyle(.secondary)
            } header: { Text("Clipboard Behaviour") }

            if timelineFirst {
                Section {
                    Toggle("Auto-apply from trusted devices", isOn: $autoApply)
                        .onChange(of: autoApply) { v in Task { await store.setAutoApplyClipboard(enabled: v) } }
                    Text("Items from trusted peers still apply automatically, as if timeline-first mode were off for those devices.")
                        .font(.caption).foregroundStyle(.secondary)
                } header: { Text("Auto-Apply") }
            }
        }
        .formStyle(.grouped).padding()
        .onAppear { timelineFirst = store.clipboardPolicy.timelineFirstMode
                    autoApply     = store.clipboardPolicy.autoApply }
    }
}
