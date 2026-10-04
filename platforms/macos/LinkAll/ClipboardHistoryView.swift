// ClipboardHistoryView.swift — Link All macOS v4
// Spotlight-style quick-access panel. Keyboard-first, day-grouped, live preview expand.

import SwiftUI

private enum QuickAccessSurface {
    static let chromeTop = CRTheme.surface.opacity(0.60)
    static let chromeBottom = CRTheme.surface.opacity(0.75)
    static let stroke = Color.primary.opacity(0.12)
    static let divider = Color.primary.opacity(0.08)
    static let card = Color.primary.opacity(0.04)
    static let cardStrong = Color.primary.opacity(0.08)
    static let rowHover = Color.primary.opacity(0.06)
    static let rowSelected = CRTheme.brandElectric.opacity(0.15)
}

// MARK: - Root Panel

struct QuickAccessHistoryView: View {
    @ObservedObject var store: LinkAllStore
    @State private var search        = ""
    @State private var selectedIndex = 0
    @State private var expandedID:   Int64? = nil          // TimelineItem.id is Int64
    @FocusState private var searchFocused: Bool

    // ── Results ───────────────────────────────────────────────────────────────

    private var results: [TimelineItem] {
        let base = store.timeline
        if search.isEmpty { return Array(base.prefix(40)) }
        return base.filter {
            $0.title.localizedCaseInsensitiveContains(search)        ||
            $0.sourceDevice.localizedCaseInsensitiveContains(search) ||
            $0.typeLabel.localizedCaseInsensitiveContains(search)    ||
            ($0.fullText?.localizedCaseInsensitiveContains(search) ?? false)
        }.prefix(40).map { $0 }
    }

    // Group by calendar day (pinned items float to top when no search)
    private var dayGroups: [(label: String, items: [TimelineItem])] {
        guard search.isEmpty else {
            return results.isEmpty ? [] : [("RESULTS", results)]
        }
        let pinned = results.filter { $0.pinned }
        let rest   = results.filter { !$0.pinned }

        var groups: [(String, [TimelineItem])] = []
        if !pinned.isEmpty { groups.append(("PINNED", pinned)) }

        let cal = Calendar.current
        let byDay = Dictionary(grouping: rest) { cal.startOfDay(for: $0.timestamp) }
        let sorted = byDay.keys.sorted(by: >)
        for day in sorted {
            let label: String
            if cal.isDateInToday(day)     { label = "TODAY" }
            else if cal.isDateInYesterday(day) { label = "YESTERDAY" }
            else {
                let f = DateFormatter(); f.dateFormat = "EEEE, MMM d"; label = f.string(from: day).uppercased()
            }
            if let items = byDay[day] {
                groups.append((label, items))
            }
        }
        return groups
    }

    private var flatResults: [TimelineItem] { dayGroups.flatMap { $0.items } }

    var body: some View {
        ZStack {
            CRVisualEffect(material: .hudWindow, blendingMode: .behindWindow).ignoresSafeArea()

            VStack(spacing: 0) {
                // Search bar
                QASearchBar(text: $search, focused: $searchFocused)
                    .padding(.horizontal, 24).padding(.vertical, 16)

                panelSeparator

                // Just-copied strip (only when not searching)
                if let ctx = store.quickSendContext, !ctx.text.isEmpty, search.isEmpty {
                    QuickSendStrip(store: store, context: ctx)
                        .padding(.horizontal, 16).padding(.top, 12).padding(.bottom, 4)
                }

                // Content
                if flatResults.isEmpty {
                    QAEmptyState(hasSearch: !search.isEmpty)
                } else {
                    ScrollViewReader { proxy in
                        ScrollView(.vertical, showsIndicators: false) {
                            LazyVStack(spacing: 0) {
                                ForEach(Array(dayGroups.enumerated()), id: \.offset) { _, group in
                                    QAGroupLabel(text: group.label,
                                                 icon: group.label == "PINNED" ? "pin.fill" : "clock",
                                                 tint: group.label == "PINNED"
                                                     ? CRTheme.accentGold
                                                     : CRTheme.inkSubtle)
                                    ForEach(Array(group.items.enumerated()), id: \.element.id) { _, item in
                                        let globalIdx = flatResults.firstIndex(where: { $0.id == item.id }) ?? 0
                                        QuickRow(
                                            item:        item,
                                            store:       store,
                                            isSelected:  globalIdx == selectedIndex,
                                            isExpanded:  expandedID == item.id,
                                            onTap:       {
                                                store.copyTimelineItem(item)
                                                if store.connectedCount > 0 {
                                                    store.sendTimelineItem(item, to: nil)
                                                }
                                            },
                                            onExpand:    {
                                                withAnimation(.crSpring) {
                                                    expandedID = expandedID == item.id ? nil : item.id
                                                }
                                            }
                                        )
                                        .id(item.id)
                                    }
                                }
                            }
                            .padding(.horizontal, 12).padding(.bottom, 12)
                        }
                        .onChange(of: selectedIndex) { idx in
                            guard idx < flatResults.count else { return }
                            withAnimation { proxy.scrollTo(flatResults[idx].id, anchor: .center) }
                        }
                    }
                }

                panelSeparator
                QAFooter()
            }
        }
        .frame(width: 680, height: 540)
        .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .strokeBorder(Color.white.opacity(0.15), lineWidth: 0.5)
        }
        .preferredColorScheme(.dark)
        .shadow(color: .black.opacity(0.4), radius: 60, x: 0, y: 30)
        .onAppear { searchFocused = true }
        .onChange(of: search) { _ in selectedIndex = 0; expandedID = nil }
        .background(
            Group {
                Button("") { navigate(-1) }.keyboardShortcut(.upArrow,   modifiers: [])
                Button("") { navigate(+1) }.keyboardShortcut(.downArrow, modifiers: [])
                Button("") { runSelected() }.keyboardShortcut(.return,   modifiers: [])
                Button("") {
                    if !search.isEmpty {
                        search = ""
                    } else {
                        NSApp.keyWindow?.close()
                    }
                }.keyboardShortcut(.escape, modifiers: [])
                Button("") { NSApp.keyWindow?.close() }
                    .keyboardShortcut("w", modifiers: .command)
            }
            .frame(width: 0, height: 0).opacity(0)
        )
    }

    // MARK: Helpers

    private var panelSeparator: some View {
        Rectangle().fill(QuickAccessSurface.divider).frame(height: 0.5)
    }

    private func navigate(_ delta: Int) {
        let upperBound = flatResults.count - 1
        guard upperBound >= 0 else { return }
        let next = Swift.min(upperBound, Swift.max(0, selectedIndex + delta))
        if next != selectedIndex {
            selectedIndex = next
            NSHapticFeedbackManager.defaultPerformer.perform(.alignment, performanceTime: .default)
        }
    }

    private func runSelected() {
        guard selectedIndex < flatResults.count else { return }
        NSHapticFeedbackManager.defaultPerformer.perform(.generic, performanceTime: .default)
        store.copyTimelineItem(flatResults[selectedIndex])
    }
}

// MARK: - Search Bar

private struct QASearchBar: View {
    @Binding var text: String
    var focused: FocusState<Bool>.Binding

    var body: some View {
        HStack(spacing: 16) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 22, weight: .light))
                .foregroundStyle(CRTheme.inkSoft)
                .frame(width: 24)

            TextField("Search clipboard history…", text: $text)
                .textFieldStyle(.plain)
                .font(.system(size: 28, weight: .regular, design: .rounded))
                .foregroundStyle(CRTheme.ink)
                .focused(focused)

            if !text.isEmpty {
                Button { withAnimation(.crFast) { text = "" } } label: {
                    Image(systemName: "xmark.circle.fill")
                        .font(.system(size: 18))
                        .foregroundStyle(CRTheme.inkSoft)
                }
                .buttonStyle(.plain)
                .transition(.scale(scale: 0.75).combined(with: .opacity))
            } else {
                HStack(spacing: 4) {
                    KbdChip("⌘", dark: false)
                    KbdChip("⇧", dark: false)
                    KbdChip("V", dark: false)
                }
            }
        }
        .animation(.crFast, value: text.isEmpty)
    }
}

// MARK: - Group Label

private struct QAGroupLabel: View {
    let text: String; let icon: String; let tint: Color
    var body: some View {
        HStack(spacing: 4) {
            Image(systemName: icon).font(.system(size: 9, weight: .bold)).foregroundStyle(tint)
            Text(text).font(.system(size: 9.5, weight: .bold)).tracking(1.1).foregroundStyle(tint)
            Spacer()
        }
        .padding(.horizontal, 10).padding(.top, 10).padding(.bottom, 3)
    }
}

// MARK: - Quick Send Strip

private struct QuickSendStrip: View {
    @ObservedObject var store: LinkAllStore
    let context: QuickSendContext
    private var connectedDevices: [ManagedDevice] { store.connectedDevices }

    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            HStack(spacing: 6) {
                // Pulsing dot
                ZStack {
                    Circle().fill(CRTheme.brandElectric.opacity(0.20)).frame(width: 14, height: 14)
                    Circle().fill(CRTheme.brandElectric).frame(width: 5.5, height: 5.5)
                }
                .crGlow(CRTheme.brandElectric, radius: 4)

                Text("JUST COPIED")
                    .font(.system(size: 9.5, weight: .bold)).tracking(1.0)
                    .foregroundStyle(CRTheme.brandElectric)
                Spacer()
                Text(context.timestamp.relativeTimeString())
                    .font(.system(size: 10.5)).foregroundStyle(CRTheme.inkSubtle)
            }

            Text(context.text)
                .font(.system(size: 12.5, weight: .medium, design: .monospaced))
                .foregroundStyle(CRTheme.ink.opacity(0.88))
                .lineLimit(2).fixedSize(horizontal: false, vertical: true)

            quickSendTargets
        }
        .padding(14)
        .background {
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(Color.white.opacity(0.06))
        }
    }

    @ViewBuilder
    private var quickSendTargets: some View {
        if !connectedDevices.isEmpty {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    ForEach(connectedDevices) { device in
                        Button {
                            store.sendQuickContext(to: device)
                        } label: {
                            HStack(spacing: 5) {
                                OSIcon(device.os, size: 10)
                                Text(device.name).font(.system(size: 11, weight: .medium))
                            }
                            .foregroundStyle(CRTheme.ink.opacity(0.90))
                            .padding(.horizontal, 9).padding(.vertical, 5)
                            .background {
                                Capsule()
                                    .fill(Color.white.opacity(0.1))
                            }
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
    }
}

// MARK: - Quick Row

private struct QuickRow: View {
    let item:       TimelineItem
    @ObservedObject var store: LinkAllStore
    var isSelected: Bool
    var isExpanded: Bool
    let onTap:      () -> Void
    let onExpand:   () -> Void

    @State private var hovered = false

    private var accent: Color {
        switch item.iconName {
        case "photo":    return CRTheme.accentPurple
        case "doc.fill": return CRTheme.accentIndigo
        case "wifi":     return CRTheme.accentGreen
        default:         return CRTheme.brandElectric
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Main row
            HStack(spacing: 14) {
                ZStack {
                    Circle()
                        .fill(accent.opacity(isSelected ? 0.3 : 0.15))
                        .frame(width: 38, height: 38)
                    Image(systemName: item.iconName)
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(accent).symbolRenderingMode(.hierarchical)
                }
                .shadow(color: accent.opacity(isSelected ? 0.4 : 0), radius: 8, y: 2)

                VStack(alignment: .leading, spacing: 3) {
                    Text(item.title)
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(CRTheme.ink.opacity(isSelected ? 1.0 : 0.90))
                        .lineLimit(1)
                    HStack(spacing: 4) {
                        Text(item.typeLabel).foregroundStyle(accent.opacity(0.80))
                        Text("·").foregroundStyle(CRTheme.inkFaint).font(.system(size: 10))
                        Text(item.sourceDevice).lineLimit(1).truncationMode(.middle)
                        Text("·").foregroundStyle(CRTheme.inkFaint).font(.system(size: 10))
                        Text(item.timestamp.relativeTimeString())
                    }
                    .font(.system(size: 11.5)).foregroundStyle(CRTheme.inkSoft)
                }

                Spacer(minLength: 0)

                // Right actions — visible on select/hover
                if isSelected || hovered {
                    HStack(spacing: 8) {
                        if item.fullText != nil {
                            Button {
                                onExpand()
                            } label: {
                                Image(systemName: isExpanded ? "chevron.up" : "chevron.down")
                                    .font(.system(size: 12, weight: .bold))
                                    .foregroundStyle(CRTheme.inkSubtle)
                                    .padding(8)
                                    .background(Circle().fill(QuickAccessSurface.card))
                            }
                            .buttonStyle(.plain)
                        }
                        KbdChip("↵", dark: false)
                    }
                    .transition(.opacity.combined(with: .scale(scale: 0.85)))
                }

                if item.pinned && !(isSelected || hovered) {
                    Image(systemName: "pin.fill")
                        .font(.system(size: 12))
                        .foregroundStyle(CRTheme.accentGold.opacity(0.80))
                        .rotationEffect(.degrees(45))
                }
            }
            .padding(.horizontal, 14).padding(.vertical, 10)

            // Expanded preview
            if isExpanded, let preview = item.fullText, !preview.isEmpty {
                Text(preview)
                    .font(.system(size: 12.5, design: .monospaced))
                    .foregroundStyle(CRTheme.ink.opacity(0.88))
                    .padding(.horizontal, 14).padding(.bottom, 12)
                    .fixedSize(horizontal: false, vertical: true)
                    .lineLimit(8)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .background {
            if isSelected {
                Rectangle().fill(Color.white.opacity(0.12))
            } else if hovered {
                Rectangle().fill(Color.white.opacity(0.05))
            }
        }
        .contentShape(Rectangle())
        .onTapGesture { onTap() }
        .onHover { hovered = $0 }
        .animation(.crFast, value: isSelected)
        .animation(.crFast, value: hovered)
    }
}

// MARK: - Empty State

private struct QAEmptyState: View {
    var hasSearch: Bool
    var body: some View {
        VStack(spacing: 11) {
            Image(systemName: hasSearch ? "magnifyingglass" : "doc.on.clipboard")
                .font(.system(size: 26, weight: .ultraLight))
                .foregroundStyle(CRTheme.inkSubtle)
                .symbolRenderingMode(.hierarchical)
            Text(hasSearch ? "No results" : "Clipboard is empty")
                .font(.system(size: 13.5, weight: .medium))
                .foregroundStyle(CRTheme.inkSoft)
            if !hasSearch {
                Text("Copy something to get started")
                    .font(.system(size: 12)).foregroundStyle(CRTheme.inkSubtle)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity).padding(.vertical, 44)
    }
}

// MARK: - Footer

private struct QAFooter: View {
    var body: some View {
        HStack(spacing: 14) {
            QAHint(keys: ["↑", "↓"], label: "navigate")
            QAHint(keys: ["↵"],       label: "copy")
            QAHint(keys: ["⌘", "↵"],  label: "send to all")
            QAHint(keys: ["Space"],    label: "preview")
            Spacer()
            QAHint(keys: ["Esc"], label: "close")
        }
        .padding(.horizontal, 16).padding(.vertical, 10)
        .overlay(alignment: .top) {
            Rectangle().fill(Color.white.opacity(0.1)).frame(height: 0.5)
        }
    }
}

private struct QAHint: View {
    let keys: [String]; let label: String
    var body: some View {
        HStack(spacing: 3) {
            ForEach(keys, id: \.self) { KbdChip($0, dark: false) }
            Text(label).font(.system(size: 10.5)).foregroundStyle(CRTheme.inkSubtle)
        }
    }
}
