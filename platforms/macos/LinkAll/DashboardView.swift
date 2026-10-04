import SwiftUI
import UniformTypeIdentifiers
import Foundation
import SystemConfiguration
import CoreImage.CIFilterBuiltins
import Carbon.HIToolbox
import QuickLook

// MARK: - Root

struct DashboardRootView: View {
    @ObservedObject var store: LinkAllStore
    @State private var renameTarget:   ManagedDevice?
    @State private var renameDraft     = ""
    @State private var density: CRDensityMode = .comfortable

    private var pendingContinuityItems: [IpcActivityEntry] {
        store.activityFeed.filter(\.isApplicable)
    }

    var body: some View {
        ZStack(alignment: .bottom) {
            DetailContent(store: store, density: $density, beginRename: beginRename)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            
            FloatingNavBar(store: store)
                .padding(.bottom, 32)
                .zIndex(50)
        }
        .background(CRTheme.surfaceStrong.ignoresSafeArea())
        .ignoresSafeArea(edges: .top)
        .overlay(alignment: .bottomTrailing) {
            if !pendingContinuityItems.isEmpty {
                ContinuityStagingDrawer(entries: Array(pendingContinuityItems.prefix(3)), store: store)
                    .padding(.trailing, 26)
                    .padding(.bottom, 24)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
            }
        }
        .animation(.crSpring, value: pendingContinuityItems.isEmpty)
        .sheet(item: $renameTarget) { device in
            RenameDeviceSheet(
                device: device, draft: renameDraft,
                onCancel: { renameTarget = nil },
                onSave:   { store.rename(device, to: $0); renameTarget = nil }
            )
        }
        .sheet(isPresented: $store.showQrCodeSheet) {
            QRCodeSheetView(store: store)
        }
    }

    private func beginRename(_ device: ManagedDevice) {
        renameDraft = device.name; renameTarget = device
    }
}

// MARK: - Floating Dock

// MARK: - Application Sidebar

struct FloatingNavBar: View {
    @ObservedObject var store: LinkAllStore
    @Namespace private var namespace
    
    var body: some View {
        HStack(spacing: 8) {
            ForEach([DashboardSection.devices, .clipboard, .transfers], id: \.self) { section in
                FloatingNavItem(
                    section: section,
                    isSelected: store.selectedSection == section,
                    namespace: namespace
                ) {
                    if store.selectedSection != section {
                        NSHapticFeedbackManager.defaultPerformer.perform(.alignment, performanceTime: .default)
                        withAnimation(.crSpring) { store.selectedSection = section }
                    }
                }
            }
        }
        .padding(6)
        .background(CRTheme.surfaceStrong.opacity(0.85))
        .background(.ultraThinMaterial)
        .clipShape(Capsule())
        .overlay(
            Capsule()
                .strokeBorder(
                    LinearGradient(
                        colors: [CRTheme.stroke.opacity(0.8), CRTheme.stroke.opacity(0.3)],
                        startPoint: .top, endPoint: .bottom
                    ), 
                    lineWidth: 1
                )
        )
        .shadow(color: .black.opacity(0.12), radius: 24, y: 12)
        .shadow(color: .black.opacity(0.04), radius: 8, y: 4)
    }
}

struct FloatingNavItem: View {
    let section: DashboardSection
    let isSelected: Bool
    let namespace: Namespace.ID
    let action: () -> Void
    
    @State private var hovered = false
    
    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                Image(systemName: isSelected ? section.icon + ".fill" : section.icon)
                    .font(.system(size: 14, weight: isSelected ? .bold : .medium))
                    .symbolRenderingMode(.hierarchical)
                
                if isSelected {
                    Text(section.title)
                        .font(.system(size: 13, weight: .bold))
                }
            }
            .foregroundStyle(isSelected ? CRTheme.brandElectric : (hovered ? CRTheme.ink : CRTheme.inkSoft))
            .padding(.horizontal, isSelected ? 16 : 14)
            .padding(.vertical, 10)
            .background {
                if isSelected {
                    Capsule()
                        .fill(CRTheme.ink.opacity(0.08))
                        .matchedGeometryEffect(id: "NAV_TAB", in: namespace)
                } else if hovered {
                    Capsule()
                        .fill(CRTheme.ink.opacity(0.04))
                }
            }
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .animation(.crFast, value: hovered)
    }
}

// MARK: - Detail Content

struct DetailContent: View {
    @ObservedObject var store: LinkAllStore
    @Binding var density: CRDensityMode
    let beginRename: (ManagedDevice) -> Void

    var body: some View {
        VStack(spacing: 0) {
            // TOP SHELL / APPLICATION CHROME
            ContinuityHeaderView(store: store)
                .zIndex(10)

            // CONTENT REGION
            VStack(spacing: 0) {
                // Content — keyed so SwiftUI rebuilds on section change (enables transition)
                Group {
                    switch store.selectedSection {
                    case .devices: DeviceCentricDashboardView(store: store)
                    case .clipboard: TimelineSectionView(store: store, density: density)
                    case .transfers: TransfersDashboardView(store: store)
                    case .settings: PreferencesView(store: store)
                    }
                }
                .id(store.selectedSection)
                .transition(.asymmetric(
                    insertion: .opacity.combined(with: .move(edge: .bottom).combined(with: .scale(scale: 0.98))),
                    removal:   .opacity
                ))
                .animation(.crSpring, value: store.selectedSection)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color.clear) // Rely on CRFluidBackgroundView underneath
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }


}

// MARK: - App Chrome (Top Bar)

struct ContinuityHeaderView: View {
    @ObservedObject var store: LinkAllStore
    @State private var searchText = ""
    @Environment(\.colorScheme) var scheme

    var body: some View {
        HStack(spacing: 16) {
            // Left Context (Window controls clearance + Status Badge)
            HStack(spacing: 12) {
                Spacer().frame(width: 72)
                
                // Status Pill
                if store.connectedCount > 0 {
                    HStack(spacing: 6) {
                        Circle().fill(Color.green).frame(width: 7, height: 7)
                            .crGlow(Color.green.opacity(0.8), radius: 6)
                        Text("\(store.connectedCount) Connected")
                            .font(.system(size: 11, weight: .semibold, design: .rounded))
                            .foregroundStyle(CRTheme.ink)
                    }
                    .padding(.horizontal, 9)
                    .padding(.vertical, 5)
                    .background(Color.green.opacity(0.12), in: Capsule())
                    .overlay(Capsule().stroke(Color.green.opacity(0.3), lineWidth: 0.5))
                } else if store.devices.contains(where: { $0.trustState != .trusted }) {
                    HStack(spacing: 6) {
                        Circle().fill(CRTheme.accentOrange).frame(width: 7, height: 7)
                            .crGlow(CRTheme.accentOrange.opacity(0.8), radius: 6)
                        Text("Ready to pair")
                            .font(.system(size: 11, weight: .semibold, design: .rounded))
                            .foregroundStyle(CRTheme.ink)
                    }
                    .padding(.horizontal, 9)
                    .padding(.vertical, 5)
                    .background(CRTheme.accentOrange.opacity(0.12), in: Capsule())
                    .overlay(Capsule().stroke(CRTheme.accentOrange.opacity(0.3), lineWidth: 0.5))
                } else {
                    HStack(spacing: 6) {
                        StatusDot(isOnline: false, size: 7)
                        Text("Looking for devices")
                            .font(.system(size: 11, weight: .medium, design: .rounded))
                            .foregroundStyle(CRTheme.inkSoft)
                    }
                    .padding(.horizontal, 9)
                    .padding(.vertical, 5)
                    .background(CRTheme.surfaceStrong, in: Capsule())
                    .overlay(Capsule().stroke(CRTheme.stroke.opacity(0.5), lineWidth: 0.5))
                }
            }
            
            Spacer(minLength: 0)
            
            // Command Layer Search (Centered)
            CRSearchField(
                placeholder: store.selectedSection == .clipboard ? "Search clipboard & transfers..." : "Search devices, clipboard, files...",
                text: $searchText,
                shortcut: "⌘K"
            )
            .frame(width: 380)
            
            Spacer(minLength: 0)
            
            // Right Side: Quick Actions Floating Dock Pill
            HStack(spacing: 3) {
                HeaderActionButton(icon: "antenna.radiowaves.left.and.right", tooltip: "Scan Network") {
                    store.scanForDevices()
                }
                HeaderActionButton(icon: "paperplane.fill", tooltip: "Send files or folders") {
                    // Triggers file picker
                }
                HeaderActionButton(icon: "qrcode", tooltip: "Show QR Code") {
                    store.showQrCodeSheet = true
                }
                
                Rectangle()
                    .fill(CRTheme.stroke.opacity(0.6))
                    .frame(width: 1, height: 16)
                    .padding(.horizontal, 4)
                
                HeaderActionButton(icon: "gearshape.fill", tooltip: "Settings") {
                    withAnimation(.crSpring) { store.selectedSection = .settings }
                }
            }
            .padding(.horizontal, 6)
            .padding(.vertical, 4)
            .background {
                Capsule()
                    .fill(CRTheme.surfaceStrong.opacity(0.95))
                    .background(Capsule().fill(.ultraThinMaterial))
                    .overlay(Capsule().stroke(CRTheme.stroke.opacity(0.6), lineWidth: 0.5))
                    .shadow(color: .black.opacity(0.1), radius: 6, y: 2)
            }
        }
        .padding(.horizontal, 20)
        .padding(.top, 14)
        .padding(.bottom, 12)
        .background {
            ZStack {
                scheme == .dark ? CRTheme.surfaceStrong : CRTheme.surfaceElevated
                
                // Subtle bottom border
                VStack {
                    Spacer()
                    Rectangle()
                        .fill(CRTheme.stroke.opacity(scheme == .dark ? 0.6 : 0.3))
                        .frame(height: 1)
                }
            }
        }
    }
}

struct HeaderActionButton: View {
    let icon: String
    let tooltip: String
    let action: () -> Void
    @State private var isHovered = false
    
    var body: some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(isHovered ? CRTheme.brandElectric : CRTheme.inkSoft)
                .frame(width: 28, height: 28)
                .background {
                    if isHovered {
                        Circle()
                            .fill(CRTheme.brandElectric.opacity(0.12))
                    }
                }
        }
        .buttonStyle(.plain)
        .help(tooltip)
        .scaleEffect(isHovered ? 1.08 : 1.0)
        .animation(.crSpring, value: isHovered)
        .onHover { isHovered = $0 }
    }
}


// MARK: - Companion Card

struct CompanionDeviceCard: View {
    let device: ManagedDevice
    let connectedPeers: Int
    @State private var isPulsing = false
    @Environment(\.scenePhase) var scenePhase

    var body: some View {
        HStack(spacing: 16) {
            ZStack {
                Circle()
                    .strokeBorder(CRTheme.brandElectric.opacity(isPulsing ? 0 : 0.4), lineWidth: 1.5)
                    .frame(width: 60, height: 60)
                    .scaleEffect(isPulsing ? 1.8 : 0.8)
                Circle()
                    .strokeBorder(CRTheme.brandElectric.opacity(isPulsing ? 0 : 0.2), lineWidth: 1)
                    .frame(width: 60, height: 60)
                    .scaleEffect(isPulsing ? 1.4 : 0.8)
                
                Circle()
                    .fill(CRTheme.brandElectric.opacity(0.12))
                    .frame(width: 64, height: 64)

                HStack(spacing: 4) {
                    Image(systemName: "laptopcomputer")
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(CRTheme.ink.opacity(0.72))
                    Image(systemName: "iphone.gen3")
                        .font(.system(size: 21, weight: .semibold))
                        .foregroundStyle(CRTheme.brandElectric)
                }
            }

            VStack(alignment: .leading, spacing: 5) {
                Text("Companion nearby")
                    .font(.system(size: 10.5, weight: .bold))
                    .tracking(1.1)
                    .foregroundStyle(CRTheme.brandElectric)
                Text(device.name)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(CRTheme.ink)
                    .lineLimit(1)
                HStack(spacing: 6) {
                    StatusDot(isOnline: device.isConnected, size: 6)
                    Text(device.connectionState.label.capitalized)
                        .font(.system(size: 11.5, weight: .medium))
                        .foregroundStyle(CRTheme.inkSoft)
                    if connectedPeers > 1 {
                        Text("·")
                            .foregroundStyle(CRTheme.inkFaint)
                        Text("+\(connectedPeers - 1) more")
                            .font(.system(size: 11.5))
                            .foregroundStyle(CRTheme.inkSubtle)
                    }
                }
            }
        }
        .padding(16)
        .frame(width: 312, alignment: .leading)
        .background {
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .fill(CRTheme.surfaceStrong)
                .overlay {
                    RoundedRectangle(cornerRadius: 16, style: .continuous)
                        .strokeBorder(CRTheme.brandElectric.opacity(0.50), lineWidth: 1)
                }
                .shadow(color: Color.black.opacity(0.15), radius: 8, y: 4)
        }
        .animation(isPulsing ? .easeOut(duration: 2.0).repeatForever(autoreverses: false) : .default, value: isPulsing)
        .onAppear {
            if scenePhase == .active { isPulsing = true }
        }
        .onChange(of: scenePhase) { phase in
            isPulsing = phase == .active
        }
    }
}

// MARK: - Staging Drawer

struct ContinuityStagingDrawer: View {
    let entries: [IpcActivityEntry]
    @ObservedObject var store: LinkAllStore

    private var leadEntry: IpcActivityEntry? { entries.first }

    var body: some View {
        if let leadEntry {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 10) {
                    ZStack {
                        RoundedRectangle(cornerRadius: 12, style: .continuous)
                            .fill(CRTheme.brandElectric.opacity(0.10))
                            .frame(width: 38, height: 38)
                        Image(systemName: leadEntry.text_preview.map(isLikelyURL) == true ? "link" : "text.cursor")
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(CRTheme.brandElectric)
                    }

                    VStack(alignment: .leading, spacing: 3) {
                        Text(isLikelyOTP(leadEntry.text_preview) ? "One-time code ready" : "Calm continuity staging")
                            .font(.system(size: 13.5, weight: .semibold))
                            .foregroundStyle(CRTheme.ink)
                        Text(leadEntry.device_name)
                            .font(.system(size: 11.5, weight: .medium))
                            .foregroundStyle(CRTheme.inkSoft)
                    }

                    Spacer(minLength: 0)

                    if entries.count > 1 {
                        Text("+\(entries.count - 1)")
                            .font(.system(size: 11, weight: .bold, design: .rounded))
                            .foregroundStyle(CRTheme.brandElectric)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 5)
                            .background {
                                Capsule().fill(CRTheme.brandElectric.opacity(0.10))
                            }
                    }
                }

                if let preview = leadEntry.text_preview, !preview.isEmpty {
                    Text(preview)
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundStyle(CRTheme.inkSoft)
                        .lineLimit(3)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 9)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background {
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .fill(Color.white.opacity(0.6))
                                .overlay {
                                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                                        .strokeBorder(CRTheme.stroke.opacity(0.36), lineWidth: 0.5)
                                }
                        }
                }

                HStack(spacing: 8) {
                    Button(isLikelyOTP(leadEntry.text_preview) ? "Copy code" : "Copy") {
                        NSHapticFeedbackManager.defaultPerformer.perform(.generic, performanceTime: .default)
                        Task { await store.applyClipboard(entry: leadEntry) }
                    }
                    .buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.brandElectric))

                    if let preview = leadEntry.text_preview,
                       let url = URL(string: preview.trimmingCharacters(in: .whitespacesAndNewlines)),
                       isLikelyURL(preview)
                    {
                        Button("Open Link") {
                            NSWorkspace.shared.open(url)
                        }
                        .buttonStyle(CRSecondaryButtonStyle())
                    }

                    Spacer(minLength: 0)
                }
            }
            .padding(16)
            .frame(width: 336, alignment: .leading)
            .background {
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .fill(CRTheme.surfaceStrong)
                    .overlay {
                        RoundedRectangle(cornerRadius: 16, style: .continuous)
                            .strokeBorder(CRTheme.stroke, lineWidth: 1)
                    }
                    .shadow(color: Color.black.opacity(0.15), radius: 8, y: 4)
            }
            .overlay(alignment: .bottomLeading) {
                // Animated shrinking progress line
                Rectangle()
                    .fill(CRTheme.brandElectric)
                    .frame(height: 3)
                    .frame(maxWidth: progress * 336, alignment: .leading)
                    .clipShape(RoundedRectangle(cornerRadius: 1.5))
                    .padding(.horizontal, 16)
                    .padding(.bottom, 6)
            }
            .offset(x: offset)
            .opacity(isDismissed ? 0 : (1.0 - Double(offset / 100.0)))
            .gesture(
                DragGesture()
                    .onChanged { gesture in
                        if gesture.translation.width > 0 {
                            offset = gesture.translation.width
                        }
                    }
                    .onEnded { gesture in
                        let predictedOffset = gesture.translation.width + (gesture.velocity.width * 0.2)
                        if predictedOffset > 100 {
                            withAnimation(.easeOut(duration: 0.2)) {
                                offset = 300
                                isDismissed = true
                            }
                        } else {
                            withAnimation(.crSpring) {
                                offset = 0
                            }
                        }
                    }
            )
            .onAppear {
                progress = 1.0
                withAnimation(.linear(duration: 5.0)) {
                    progress = 0.0
                }
            }
        }
    }
    
    @State private var progress: CGFloat = 1.0
    @State private var offset: CGFloat = 0
    @State private var isDismissed: Bool = false

    private func isLikelyOTP(_ text: String?) -> Bool {
        guard let text else { return false }
        let condensed = text.lowercased()
        let digitCount = text.filter(\.isNumber).count
        return digitCount >= 6 && digitCount <= 8 && ["otp", "code", "auth", "verify"].contains { condensed.contains($0) }
    }

    private func isLikelyURL(_ text: String?) -> Bool {
        guard let text else { return false }
        let value = text.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return value.hasPrefix("http://") || value.hasPrefix("https://")
    }
}

// MARK: - Timeline Section
