import SwiftUI
import UniformTypeIdentifiers
import Foundation
import SystemConfiguration
import CoreImage.CIFilterBuiltins
import Carbon.HIToolbox
import QuickLook


struct TimelineSectionView: View {
    @ObservedObject var store: LinkAllStore
    let density: CRDensityMode
    @State private var search     = ""
    @State private var filterKind = "all"
    @State private var quickLookURL: URL?
    @State private var hoveredItemId: Int64?
    @State private var spaceMonitor: Any?
    private let filters = [("all","All"),("text","Text"),("image","Image"),("file","File")]

    private var pinnedItems: [TimelineItem] {
        guard search.isEmpty else { return [] }
        return store.timeline.filter { $0.pinned }
    }

    private var filteredItems: [TimelineItem] {
        var base = store.timeline.filter { !$0.pinned }
        if filterKind != "all" { base = base.filter { $0.typeLabel.lowercased().contains(filterKind) } }
        if !search.isEmpty {
            base = base.filter {
                $0.title.localizedCaseInsensitiveContains(search)         ||
                $0.sourceDevice.localizedCaseInsensitiveContains(search)  ||
                ($0.fullText?.localizedCaseInsensitiveContains(search) ?? false)
            }
        }
        return base
    }

    var body: some View {
        VStack(spacing: 0) {
            // Search + filter bar (below sticky toolbar, scrolls with content on small windows)
            VStack(alignment: .leading, spacing: 12) {
                CRSearchField(placeholder: "Search copied text, links, files, and images across devices...", text: $search, shortcut: "⌘F")
                filterRow
            }
            .padding(.horizontal, 24).padding(.vertical, 14)
            .background(CRTheme.surface)

            CRDivider()

            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0, pinnedViews: []) {
                    VStack(alignment: .leading, spacing: 0) {
                        // Pinned group
                        if !pinnedItems.isEmpty {
                            groupLabel("PINNED", icon: "pin.fill", tint: CRTheme.accentGold)
                                .padding(.horizontal, 20).padding(.top, 16).padding(.bottom, 6)
                            VStack(spacing: density.cardSpacing) {
                                ForEach(pinnedItems) { item in
                                    TimelineCard(item: item, store: store, density: density)
                                        .onHover { isHovering in
                                            if isHovering { hoveredItemId = item.id }
                                            else if hoveredItemId == item.id { hoveredItemId = nil }
                                        }
                                }
                            }
                            .padding(.horizontal, 20)
                            .padding(.bottom, 14)

                            groupLabel("RECENT", icon: "clock", tint: CRTheme.inkSubtle)
                                .padding(.horizontal, 20).padding(.bottom, 6)
                        }

                        // Main list
                        if filteredItems.isEmpty && store.timeline.isEmpty {
                            CREmptyState(
                                systemImage: "doc.text.magnifyingglass",
                                title: "Nothing here yet",
                                message: "Copied text, images, and files will appear once the daemon is running."
                            )
                        } else if filteredItems.isEmpty {
                            CREmptyState(
                                systemImage: "magnifyingglass",
                                title: "No results",
                                message: search.isEmpty ? "Try a different filter." : "No items match \"\(search)\".",
                                accent: CRTheme.accentIndigo,
                                actionLabel: "Clear search",
                                onAction: { search = "" }
                            )
                        } else {
                            LazyVGrid(columns: [GridItem(.flexible(), spacing: 16), GridItem(.flexible(), spacing: 16)], spacing: 16) {
                                ForEach(filteredItems) { item in
                                    TimelineCard(item: item, store: store, density: density)
                                        .modifier(MasonryGridModifier())
                                        .transition(.scale(scale: 0.95).combined(with: .opacity))
                                        .animation(.crSpring, value: filteredItems.count)
                                        .onHover { isHovering in
                                            if isHovering { hoveredItemId = item.id }
                                            else if hoveredItemId == item.id { hoveredItemId = nil }
                                        }
                                }
                            }
                            .padding(.horizontal, 20)
                            .padding(.top, pinnedItems.isEmpty ? 16 : 0)
                        }
                    }
                    .padding(.bottom, 24)
                }
            }
        }
        .quickLookPreview($quickLookURL)
        .onAppear {
            spaceMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
                if event.keyCode == 49, let hoveredId = hoveredItemId { // Spacebar
                    if let target = store.timeline.first(where: { $0.id == hoveredId }), let path = target.filePath, !path.isEmpty {
                        quickLookURL = URL(fileURLWithPath: path)
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

    private var filterRow: some View {
        HStack(spacing: 6) {
            ForEach(filters, id: \.0) { key, label in
                Button(action: { withAnimation(.crSpring) { filterKind = key } }) {
                    Text(label)
                        .font(.system(size: 12, weight: filterKind == key ? .bold : .medium, design: .rounded))
                        .foregroundStyle(filterKind == key ? Color.white : CRTheme.inkSoft)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 5)
                        .background {
                            Capsule()
                                .fill(filterKind == key ? CRTheme.brandElectric : CRTheme.surfaceStrong.opacity(0.6))
                                .overlay {
                                    Capsule().strokeBorder(
                                        filterKind == key ? Color.white.opacity(0.3) : CRTheme.stroke.opacity(0.6),
                                        lineWidth: filterKind == key ? 1.0 : 0.5
                                    )
                                }
                                .shadow(color: filterKind == key ? CRTheme.brandElectric.opacity(0.4) : .clear, radius: 6, y: 2)
                        }
                }
                .buttonStyle(.plain)
                .animation(.crSpring, value: filterKind)
            }
            Spacer()
            if !search.isEmpty || filterKind != "all" {
                HStack(spacing: 5) {
                    Circle().fill(CRTheme.brandElectric).frame(width: 6, height: 6)
                    Text("\(filteredItems.count) result\(filteredItems.count == 1 ? "" : "s")")
                        .font(.system(size: 12, weight: .semibold, design: .rounded))
                        .foregroundStyle(CRTheme.inkSoft)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 4)
                .background(CRTheme.surfaceStrong.opacity(0.8), in: Capsule())
                .transition(.scale(scale: 0.8).combined(with: .opacity))
            }
        }
        .animation(.crSpring, value: search.isEmpty && filterKind == "all")
    }

    @ViewBuilder private func groupLabel(_ text: String, icon: String, tint: Color) -> some View {
        HStack(spacing: 5) {
            Image(systemName: icon).font(.system(size: 9, weight: .semibold)).foregroundStyle(tint)
            Text(text).font(.system(size: 10, weight: .bold)).tracking(1.0).foregroundStyle(tint)
        }
    }
}

// MARK: - Spatial Modifiers

struct MasonryGridModifier: ViewModifier {
    func body(content: Content) -> some View {
        GeometryReader { proxy in
            let y = proxy.frame(in: .global).minY
            // Approximating screen height for 3D tilt calculations
            let screenH = NSScreen.main?.frame.height ?? 1000
            let normalizedY = (y / screenH)
            
            content
                .rotation3DEffect(
                    .degrees(Double((normalizedY - 0.5) * 6)),
                    axis: (x: 1, y: 0, z: 0),
                    perspective: 0.8
                )
                .scaleEffect(1.0 - (abs(normalizedY - 0.5) * 0.03))
                .opacity(normalizedY > 0.9 ? 0.4 : 1.0)
        }
        // Fixed minimum height to prevent the geometry reader from collapsing
        .frame(minHeight: 100) 
    }
}

// MARK: - Devices Section


struct TimelineCard: View {
    let item:    TimelineItem
    @ObservedObject var store: LinkAllStore
    var density: CRDensityMode = .comfortable
    @State private var isHovered = false

    private var accent: Color {
        switch item.iconName {
        case "doc.on.clipboard": return CRTheme.accentBlue
        case "photo":            return CRTheme.accentPurple
        case "doc.fill":         return CRTheme.accentIndigo
        case "wifi":             return CRTheme.accentGreen
        case "wifi.slash":       return CRTheme.inkSoft
        default:                 return CRTheme.accentBlue
        }
    }

    private var charCount: String? {
        guard let t = item.fullText, !t.isEmpty else { return nil }
        return t.count > 999 ? "\(t.count / 1000)k chars" : "\(t.count) chars"
    }

    var body: some View {
        HStack(spacing: 0) {
            // Left accent stripe
            RoundedRectangle(cornerRadius: 1.5).fill(accent)
                .frame(width: 2.5)
                .padding(.vertical, density.rowPadding)
                .padding(.leading, 11)

            VStack(alignment: .leading, spacing: density == .compact ? 5 : 8) {
                // Header
                HStack(alignment: .center, spacing: 9) {
                    if density == .comfortable {
                        CRIconChip(systemName: item.iconName, tint: accent, size: 28)
                    }

                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 6) {
                            Text(item.title)
                                .font(.system(size: density == .compact ? 12.5 : 13, weight: .semibold))
                                .foregroundStyle(CRTheme.ink).lineLimit(1)
                            if item.pinned {
                                Image(systemName: "pin.fill")
                                    .font(.system(size: 9)).foregroundStyle(CRTheme.accentGold.opacity(0.85))
                                    .rotationEffect(.degrees(45))
                            }
                        }
                        HStack(spacing: 4) {
                            // The device name gives way first, so tag, size and time stay whole.
                            CRTag(text: item.typeLabel, tint: accent)
                            if let cc = charCount {
                                Text("·").foregroundStyle(CRTheme.inkFaint).font(.system(size: 9))
                                Text(cc).foregroundStyle(CRTheme.inkSubtle).lineLimit(1).layoutPriority(1)
                            }
                            Text("·").foregroundStyle(CRTheme.inkFaint).font(.system(size: 9))
                            Image(systemName: "desktopcomputer").font(.system(size: 9))
                                .foregroundStyle(CRTheme.inkSubtle)
                            Text(item.sourceDevice).lineLimit(1).truncationMode(.middle)
                            Text("·").foregroundStyle(CRTheme.inkFaint).font(.system(size: 9))
                            Text(item.timestamp.relativeTimeString()).lineLimit(1).layoutPriority(1)
                        }
                        .font(.system(size: 10.5)).foregroundStyle(CRTheme.inkSoft)
                    }

                    Spacer(minLength: 0)
                }

                // Text preview
                if let preview = item.fullText, !preview.isEmpty, density == .comfortable {
                    Text(preview)
                        .font(.system(size: 11, design: .monospaced))
                        .foregroundStyle(CRTheme.inkSoft)
                        .lineLimit(isHovered ? 4 : 1)
                        .padding(.horizontal, 9).padding(.vertical, 6)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background {
                            RoundedRectangle(cornerRadius: 6, style: .continuous)
                                .fill(CRTheme.surface)
                                .overlay {
                                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                                        .strokeBorder(CRTheme.stroke.opacity(0.40), lineWidth: 0.5)
                                }
                        }
                        .animation(.crSpring, value: isHovered)
                }

                // Action bar
                if isHovered {
                    HStack(spacing: 6) {
                        if item.fullText != nil {
                            Button("Copy") {
                                NSHapticFeedbackManager.defaultPerformer.perform(.generic, performanceTime: .default)
                                store.copyTimelineItem(item)
                            }
                            .buttonStyle(CRPrimaryButtonStyle())
                            .help("Copy to clipboard")
                        }
                        Menu {
                            Button("Send to all devices") { store.sendTimelineItem(item, to: nil) }
                            if !store.connectedDevices.isEmpty { Divider() }
                            ForEach(store.connectedDevices) { d in
                                Button(d.name) { store.sendTimelineItem(item, to: d) }
                            }
                        } label: {
                            Label("Send", systemImage: "paperplane.fill")
                                .font(.system(size: 12, weight: .medium))
                        }
                        .buttonStyle(CRSecondaryButtonStyle()).menuIndicator(.hidden)
                        .help("Send to device")

                        Button(item.pinned ? "Unpin" : "Pin") {
                            store.pinTimelineItem(item, pinned: !item.pinned)
                        }
                        .buttonStyle(CRSecondaryButtonStyle())
                        .help(item.pinned ? "Unpin item" : "Pin item")

                        Spacer()

                        Button { store.deleteTimelineItem(item) } label: {
                            Image(systemName: "trash").font(.system(size: 11.5, weight: .medium))
                        }
                        .buttonStyle(CRDestructiveButtonStyle())
                        .help("Delete item")
                    }
                    .transition(.opacity.combined(with: .move(edge: .bottom)))
                }
            }
            .padding(.horizontal, 11).padding(.vertical, density.rowPadding)
        }
        .crCard(cornerRadius: density.cardRadius, highlighted: isHovered, accent: accent)
        .onHover { isHovered = $0 }
        .animation(.crFast, value: isHovered)
    }
}

// MARK: - Device Card

