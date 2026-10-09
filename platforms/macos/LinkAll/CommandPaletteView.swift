// CommandPaletteView.swift — Link All macOS v4
// ⌘K palette: grouped commands, recent history, live sync-state labels.

import SwiftUI

private enum PaletteSurface {
    static let chromeTop = CRTheme.surface.opacity(0.70)
    static let chromeBottom = CRTheme.surface.opacity(0.85)
    static let stroke = Color.primary.opacity(0.12)
    static let divider = Color.primary.opacity(0.08)
    static let card = Color.primary.opacity(0.04)
    static let cardStrong = Color.primary.opacity(0.07)
    static let rowHover = Color.primary.opacity(0.05)
    static let rowSelected = CRTheme.brandElectric.opacity(0.15)
}

// MARK: - Root

struct CommandPaletteView: View {
    @ObservedObject var store: LinkAllStore
    @State private var query         = ""
    @State private var selectedIndex = 0
    @State private var recentIDs:    [UUID] = []   // tracks recently run commands
    @FocusState private var inputFocused: Bool

    // ── Command catalogue ─────────────────────────────────────────────────────

    private var catalogue: [PaletteGroup] {
        let isSyncPaused = store.settings?.syncEnabled == false
        return [
            PaletteGroup(title: "Navigate", commands: [
                cmd(id: "nav.devices", icon: "desktopcomputer",
                    label: "Devices", hint: "Manage connected peers",
                    tint: CRTheme.brandElectric, shortcut: "⌘1")
                { store.selectedSection = .devices },

                cmd(id: "nav.clipboard", icon: "doc.on.clipboard",
                    label: "Clipboard", hint: "View clipboard history",
                    tint: CRTheme.accentGreen, shortcut: "⌘2")
                { store.selectedSection = .clipboard },

                cmd(id: "nav.transfers", icon: "arrow.left.arrow.right",
                    label: "Transfers", hint: "File transfer history",
                    tint: CRTheme.accentOrange, shortcut: "⌘3")
                { store.selectedSection = .transfers },



                cmd(id: "nav.settings", icon: "slider.horizontal.3",
                    label: "Settings", hint: "Tune sync and network options",
                    tint: CRTheme.sidebarInkSubtle, shortcut: "⌘,")
                { store.selectedSection = .settings },
            ]),

            PaletteGroup(title: "Clipboard", commands: [
                cmd(id: "clip.send_all", icon: "paperplane.fill",
                    label: "Send Clipboard to All",
                    hint: "Push current clipboard to every connected peer",
                    tint: CRTheme.accentBlue)
                { store.sendCurrentClipboard(to: nil) },

                // Per-device send commands — dynamically generated from connected peers.
            ] + store.connectedDevices.map { device in
                cmd(id: "clip.send.\(device.id)", icon: "paperplane",
                    label: "Send to \(device.name)",
                    hint: "Push current clipboard only to \(device.name)",
                    tint: CRTheme.accentMint)
                { store.sendCurrentClipboard(to: device) }
            } + [
                cmd(id: "clip.history", icon: "clock.arrow.circlepath",
                    label: "Open Clipboard History",
                    hint: "Quick Access panel — search, pin, and re-send past items",
                    tint: CRTheme.brandElectric)
                { store.openHistoryPanel() },
            ]),

            PaletteGroup(title: "Network", commands: [
                cmd(id: "net.connect", icon: "network",
                    label: "Manual Connect…",
                    hint: "Connect to a peer by IP address or hostname",
                    tint: CRTheme.accentMint)
                { store.selectedSection = .devices },

                cmd(id: "net.sync_toggle",
                    icon: isSyncPaused ? "play.circle.fill" : "pause.circle.fill",
                    label: isSyncPaused ? "Resume Sync" : "Pause Sync",
                    hint: isSyncPaused
                        ? "Resume clipboard sync across all devices"
                        : "Pause clipboard sync across all devices",
                    tint: isSyncPaused ? CRTheme.accentGreen : CRTheme.accentOrange)
                { store.toggleSync() },

                cmd(id: "net.scan", icon: "antenna.radiowaves.left.and.right",
                    label: "Scan for Devices",
                    hint: "Search the local network for Link All peers",
                    tint: CRTheme.accentTeal)
                { store.scanForDevices() },
            ]),

            // Recent clipboard items as directly-runnable commands.
            // Filtered out when query is empty to keep the default list clean.
        ] + (query.isEmpty ? [] : [
            PaletteGroup(title: "Clipboard History", commands:
                store.timeline
                    .filter { $0.fullText != nil }
                    .prefix(8)
                    .map { item in
                        let preview = item.title.prefix(60).description
                        return cmd(
                            id: "hist.\(item.id)",
                            icon: "doc.on.clipboard",
                            label: preview,
                            hint: "From \(item.sourceDevice) · \(item.timestamp.relativeTimeString())",
                            tint: CRTheme.brandElectric
                        ) { store.copyTimelineItem(item) }
                    }
            ),
        ])
    }

    // ── Filtering ─────────────────────────────────────────────────────────────

    private var filtered: [PaletteGroup] {
        guard !query.isEmpty else { return catalogue }
        let q = query.lowercased()
        return catalogue.compactMap { group in
            let matched = group.commands.filter {
                $0.label.lowercased().contains(q) || $0.hint.lowercased().contains(q)
            }
            return matched.isEmpty ? nil : PaletteGroup(title: group.title, commands: matched)
        }
    }

    private var flat: [PaletteCommand] { filtered.flatMap { $0.commands } }

    // ── Body ──────────────────────────────────────────────────────────────────

    var body: some View {
        ZStack {
            CRVisualEffect(material: .popover).ignoresSafeArea()
            LinearGradient(
                stops: [
                    .init(color: PaletteSurface.chromeTop, location: 0),
                    .init(color: PaletteSurface.chromeBottom, location: 1)
                ],
                startPoint: .topLeading, endPoint: .bottomTrailing
            ).ignoresSafeArea()

            VStack(spacing: 0) {
                PaletteInputBar(query: $query, focused: $inputFocused, onSubmit: runSelected)

                Rectangle().fill(PaletteSurface.divider).frame(height: 0.5)

                if flat.isEmpty {
                    PaletteEmptyState()
                } else {
                    ScrollView(.vertical, showsIndicators: false) {
                        // Swift doesn't allow mutable state inside body.
                        // Use a helper to compute flat offsets.
                        PaletteResultList(
                            groups:        filtered,
                            flat:          flat,
                            selectedIndex: selectedIndex,
                            showHeaders:   query.isEmpty,
                            recentIDs:     recentIDs,
                            onSelect:      { idx in selectedIndex = idx },
                            onRun:         { idx in selectedIndex = idx; runSelected() }
                        )
                    }
                }

                Rectangle().fill(PaletteSurface.divider).frame(height: 0.5)
                PaletteFooter()
            }
        }
        .frame(width: 600)
        .fixedSize(horizontal: false, vertical: true)
        .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .strokeBorder(PaletteSurface.stroke, lineWidth: 0.5)
        }
        .shadow(color: .black.opacity(0.18), radius: 42, x: 0, y: 22)
        .onAppear { inputFocused = true; selectedIndex = 0 }
        .onChange(of: query) { _ in selectedIndex = 0 }
        .background(
            Group {
                Button("") { navigate(-1) }.keyboardShortcut(.upArrow,   modifiers: [])
                Button("") { navigate(+1) }.keyboardShortcut(.downArrow, modifiers: [])
                Button("") { runSelected() }.keyboardShortcut(.return,   modifiers: [])
            }
            .frame(width: 0, height: 0).opacity(0)
        )
    }

    // MARK: Helpers

    private func navigate(_ delta: Int) {
        let count = flat.count; guard count > 0 else { return }
        let next = min(count - 1, max(0, selectedIndex + delta))
        if next != selectedIndex {
            selectedIndex = next
            NSHapticFeedbackManager.defaultPerformer.perform(.alignment, performanceTime: .default)
        }
    }

    private func runSelected() {
        guard selectedIndex < flat.count else { return }
        NSHapticFeedbackManager.defaultPerformer.perform(.generic, performanceTime: .default)
        let chosen = flat[selectedIndex]
        recentIDs = ([chosen.id] + recentIDs).prefix(5).map { $0 }
        chosen.action()
    }

    private func cmd(id: String, icon: String, label: String, hint: String,
                     tint: Color, shortcut: String? = nil,
                     action: @escaping () -> Void) -> PaletteCommand {
        PaletteCommand(id: id, icon: icon, label: label, hint: hint,
                       tint: tint, shortcut: shortcut, action: action)
    }
}

// MARK: - Result List (extracted to avoid mutable-in-body issue)

private struct PaletteResultList: View {
    let groups:        [PaletteGroup]
    let flat:          [PaletteCommand]
    let selectedIndex: Int
    let showHeaders:   Bool
    let recentIDs:     [UUID]
    let onSelect:      (Int) -> Void
    let onRun:         (Int) -> Void

    var body: some View {
        VStack(spacing: 0) {
            var runningIdx = 0
            ForEach(groups) { group in
                if showHeaders {
                    PaletteGroupHeader(title: group.title)
                }
                ForEach(group.commands) { cmd in
                    let myIdx = runningIdx
                    let isRecent = recentIDs.contains(cmd.id)
                    PaletteRow(command: cmd, isSelected: myIdx == selectedIndex, isRecent: isRecent)
                        .onTapGesture { onRun(myIdx) }
                        .onHover { if $0 { onSelect(myIdx) } }
                    let _ = { runningIdx += 1 }()
                }
            }
        }
        .padding(.horizontal, 8).padding(.vertical, 6)
    }
}

// MARK: - Input Bar

private struct PaletteInputBar: View {
    @Binding var query: String
    var focused: FocusState<Bool>.Binding
    let onSubmit: () -> Void

    var body: some View {
        HStack(spacing: 11) {
            CRAppIconMark(size: 26)

            TextField("Type a command…", text: $query)
                .textFieldStyle(.plain)
                .font(.system(size: 15.5))
                .foregroundStyle(CRTheme.ink)
                .focused(focused)
                .onSubmit { onSubmit() }

            if !query.isEmpty {
                Button { withAnimation(.crFast) { query = "" } } label: {
                    Image(systemName: "xmark.circle.fill")
                        .font(.system(size: 14.5)).foregroundStyle(CRTheme.inkSoft)
                }
                .buttonStyle(.plain).transition(.scale(scale: 0.75).combined(with: .opacity))
            } else {
                KbdChip("⌘K", dark: false)
            }
        }
        .padding(.horizontal, 16).padding(.vertical, 14)
        .background {
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(PaletteSurface.cardStrong)
                .overlay {
                    RoundedRectangle(cornerRadius: 14, style: .continuous)
                        .strokeBorder(PaletteSurface.stroke, lineWidth: 0.5)
                }
        }
        .padding(.horizontal, 12).padding(.top, 12).padding(.bottom, 10)
        .animation(.crFast, value: query.isEmpty)
    }
}

// MARK: - Group Header

private struct PaletteGroupHeader: View {
    let title: String
    var body: some View {
        HStack {
            Text(title.uppercased())
                .font(.system(size: 9.5, weight: .bold)).tracking(1.1)
                .foregroundStyle(CRTheme.inkSubtle)
            Spacer()
        }
        .padding(.horizontal, 10).padding(.top, 10).padding(.bottom, 3)
    }
}

// MARK: - Palette Row

private struct PaletteRow: View {
    let command:    PaletteCommand
    var isSelected: Bool
    var isRecent:   Bool

    var body: some View {
        HStack(spacing: 12) {
            ZStack {
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .fill(command.tint.opacity(isSelected ? 0.22 : 0.10))
                    .frame(width: 26, height: 26)
                Image(systemName: command.icon)
                    .font(.system(size: 11.5, weight: .semibold))
                    .foregroundStyle(command.tint).symbolRenderingMode(.hierarchical)
            }

            HStack(spacing: 8) {
                Text(command.label)
                    .font(.system(size: 13, weight: isSelected ? .semibold : .medium))
                    .foregroundStyle(CRTheme.ink.opacity(isSelected ? 1.0 : 0.86))
                    .layoutPriority(1)
                    
                if isRecent {
                    CRTag(text: "recent", tint: CRTheme.brandElectric)
                }

                Text(command.hint)
                    .font(.system(size: 12))
                    .foregroundStyle(CRTheme.inkSoft.opacity(isSelected ? 0.95 : 0.65))
                    .lineLimit(1)
                    .truncationMode(.tail)
            }

            Spacer(minLength: 0)

            HStack(spacing: 5) {
                if let sc = command.shortcut {
                    Text(sc)
                        .font(.system(size: 10, weight: .semibold, design: .rounded))
                        .foregroundStyle(CRTheme.inkSubtle.opacity(isSelected ? 0.95 : 0.78))
                        .padding(.horizontal, 6).padding(.vertical, 2.5)
                        .background {
                            RoundedRectangle(cornerRadius: 5, style: .continuous)
                                .fill(PaletteSurface.cardStrong)
                                .overlay {
                                    RoundedRectangle(cornerRadius: 5, style: .continuous)
                                        .strokeBorder(PaletteSurface.stroke, lineWidth: 0.5)
                                }
                        }
                }
                if isSelected { KbdChip("↵", dark: false).transition(.opacity) }
            }
        }
        .padding(.horizontal, 10).padding(.vertical, 5)
        .background {
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .fill(isSelected ? PaletteSurface.rowSelected : .clear)
                .overlay {
                    if isSelected {
                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                            .strokeBorder(CRTheme.brandElectric.opacity(0.18), lineWidth: 0.5)
                    }
                }
        }
        .contentShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .animation(.crFast, value: isSelected)
    }
}

// MARK: - Empty State

private struct PaletteEmptyState: View {
    var body: some View {
        VStack(spacing: 11) {
            Image(systemName: "questionmark.circle")
                .font(.system(size: 26, weight: .ultraLight))
                .foregroundStyle(CRTheme.inkSubtle).symbolRenderingMode(.hierarchical)
            Text("No matching commands")
                .font(.system(size: 13)).foregroundStyle(CRTheme.inkSoft)
        }
        .frame(maxWidth: .infinity).padding(.vertical, 30)
    }
}

// MARK: - Footer

private struct PaletteFooter: View {
    var body: some View {
        HStack(spacing: 16) {
            PaletteHint(keys: ["↑", "↓"], label: "select")
            PaletteHint(keys: ["↵"],       label: "run")
            PaletteHint(keys: ["Esc"],      label: "close")
            Spacer()
        }
        .padding(.horizontal, 16).padding(.vertical, 10)
    }
}

private struct PaletteHint: View {
    let keys: [String]; let label: String
    var body: some View {
        HStack(spacing: 3) {
            ForEach(keys, id: \.self) { KbdChip($0, dark: false) }
            Text(label).font(.system(size: 10.5)).foregroundStyle(CRTheme.inkSubtle)
        }
    }
}

// MARK: - Data Models

private struct PaletteGroup: Identifiable {
    let id    = UUID()
    let title: String
    let commands: [PaletteCommand]
}

struct PaletteCommand: Identifiable {
    let id:       UUID
    let icon:     String
    let label:    String
    let hint:     String
    var tint:     Color   = CRTheme.brandElectric
    var shortcut: String? = nil
    let action:   () -> Void

    init(id: String, icon: String, label: String, hint: String,
         tint: Color, shortcut: String? = nil, action: @escaping () -> Void) {
        self.id       = UUID(uuidString: id) ?? UUID()
        self.icon     = icon; self.label = label; self.hint = hint
        self.tint     = tint; self.shortcut = shortcut; self.action = action
    }
}

// MARK: - Accent alias (local)
