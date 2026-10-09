import SwiftUI


struct CommandCenterRootView: View {
    @ObservedObject var store: LinkAllStore
    @State private var renameTarget: ManagedDevice?
    @State private var renameDraft = ""
    @State private var density: CRDensityMode = .comfortable

    private var pendingContinuityItems: [IpcActivityEntry] {
        store.activityFeed.filter(\.isApplicable)
    }

    var body: some View {
        HStack(spacing: 0) {
            // Left Column: Navigation Sidebar (240px)
            // All three columns share one colour, split only by dividers.
            CommandSidebarView(store: store)
                .frame(width: 240)
                .background(CRTheme.surface.ignoresSafeArea())
            
            Divider()
            
            // Center Column: Main Workspace (Flexible)
            VStack(spacing: 0) {
                if let issue = store.healthIssues.first {
                    HealthBanner(issue: issue, store: store)
                        .padding(.horizontal, 20)
                        .padding(.top, 14)
                }
                ZStack(alignment: .bottom) {
                    // Content Router
                    Group {
                        switch store.selectedSection {
                        case .devices: 
                            CommandCenterView(store: store)
                        case .clipboard: 
                            TimelineSectionView(store: store, density: density)
                        case .transfers: 
                            TransfersDashboardView(store: store)
                        case .settings: 
                            PreferencesView(store: store)
                        }
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .id(store.selectedSection)
                    .transition(.opacity)
                    .animation(.crSpring, value: store.selectedSection)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            .background(CRTheme.surface)
            
            Divider()
            
            // Right Column: Smart Device Panel (320px)
            LiveDevicePanel(store: store)
                .frame(width: 320)
                .background(CRTheme.surface)
        }
        .ignoresSafeArea(.all, edges: .top)
        .frame(minWidth: 1100, minHeight: 700)
        .background(CRVisualEffect(material: .underWindowBackground, blendingMode: .behindWindow).ignoresSafeArea())
        .sheet(item: $renameTarget) { device in
            // Fallback for store requirements
            Text("Rename \(device.name)")
        }
    }
}

// MARK: - Left Sidebar
struct CommandSidebarView: View {
    @ObservedObject var store: LinkAllStore
    @State private var hoveredSection: DashboardSection? = nil
    @State private var connectingIds: Set<String> = []

    private func nearbyStatus(for device: ManagedDevice) -> String {
        if device.outgoingPairingWaiting {
            if let pin = device.pairingPin, !pin.isEmpty {
                return "Approve on \(device.name) · code \(pin)"
            }
            return "Waiting for approval on \(device.name)"
        }
        if let outcome = device.pairingOutcomeText { return outcome }
        if connectingIds.contains(device.id) || device.connectionState == .connecting {
            return "Connecting…"
        }
        return device.lastError == nil ? "Tap to connect" : "Can't reach · tap to retry"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            // App Branding removed
            // Main Navigation
            VStack(alignment: .leading, spacing: 6) {
                Text("WORKSPACE")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(CRTheme.inkSubtle)
                    .padding(.horizontal, 16)
                    .padding(.top, 40)
                    .padding(.bottom, 4)
                
                ForEach([DashboardSection.devices, .clipboard, .transfers, .settings], id: \.self) { section in
                    SidebarNavItem(
                        section: section, 
                        isSelected: store.selectedSection == section,
                        action: { withAnimation(.crSpring) { store.selectedSection = section } }
                    )
                }
            }
            
            // Connected Devices List
            VStack(alignment: .leading, spacing: 6) {
                Text("YOUR DEVICES")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(CRTheme.inkSubtle)
                    .padding(.horizontal, 16)
                    .padding(.bottom, 4)
                
                if store.connectedDevices.isEmpty {
                    Text("No active devices")
                        .font(.system(size: 13))
                        .foregroundStyle(CRTheme.inkSoft)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 8)
                } else {
                    ForEach(store.connectedDevices) { device in
                        // Only the device the dashboard is showing is highlighted.
                        let isSelected = store.selectedSection == .devices && store.selectedPendingDevice == nil
                            && store.defaultTargetDevice?.id == device.id
                        Button(action: {
                            withAnimation(.crSpring) {
                                store.selectDevice(device.id)
                                store.selectedSection = .devices
                                store.selectedPendingDevice = nil
                            }
                        }) {
                            HStack(spacing: 10) {
                                Circle()
                                    .fill(CRTheme.accentGreen)
                                    .frame(width: 8, height: 8)
                                OSIcon(device.os, size: 13)
                                Text(device.name)
                                    .font(.system(size: 13, weight: isSelected ? .bold : .medium))
                                Spacer()
                            }
                            .foregroundStyle(isSelected ? Color.white : CRTheme.ink)
                            .padding(.horizontal, 12)
                            .padding(.vertical, 8)
                            .background(
                                RoundedRectangle(cornerRadius: 8, style: .continuous)
                                    .fill(isSelected ? CRTheme.brandElectric : Color.clear)
                            )
                            .padding(.horizontal, 12)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
            
            if !store.nearbyTrustedDevices.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Text("NEARBY")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(CRTheme.inkSubtle)
                        .padding(.horizontal, 16)
                        .padding(.bottom, 4)

                    ForEach(store.nearbyTrustedDevices) { device in
                        Button(action: {
                            guard !device.outgoingPairingWaiting else { return }
                            connectingIds.insert(device.id)
                            store.connect(device)
                            // Clear the local "Connecting…" once the dial has
                            // had time to finish; the daemon state takes over.
                            DispatchQueue.main.asyncAfter(deadline: .now() + 8) {
                                connectingIds.remove(device.id)
                            }
                        }) {
                            HStack(spacing: 10) {
                                Circle()
                                    .strokeBorder(device.outgoingPairingWaiting ? CRTheme.accentOrange : CRTheme.inkSubtle, lineWidth: 1.5)
                                    .frame(width: 8, height: 8)
                                OSIcon(device.os, size: 13)
                                    .foregroundStyle(CRTheme.inkSubtle)
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(device.name)
                                        .font(.system(size: 13, weight: .medium))
                                        .foregroundStyle(CRTheme.ink)
                                        .lineLimit(1)
                                    Text(nearbyStatus(for: device))
                                        .font(.system(size: 11))
                                        .foregroundStyle(CRTheme.inkSubtle)
                                        .lineLimit(1)
                                }
                                Spacer()
                            }
                            .padding(.horizontal, 12)
                            .padding(.vertical, 6)
                            .padding(.horizontal, 12)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .help(device.lastError.map { "Last error: \($0)" } ?? "Paired, not connected")
                    }
                }
            }

            if !store.pendingDevices.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Text("PENDING")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(CRTheme.inkSubtle)
                        .padding(.horizontal, 16)
                        .padding(.bottom, 4)
                    
                    ForEach(store.pendingDevices) { device in
                        Button(action: {
                            withAnimation(.crSpring) {
                                store.selectedSection = .devices
                                store.selectedPendingDevice = device
                            }
                        }) {
                            HStack(spacing: 10) {
                                Circle()
                                    .fill(CRTheme.accentOrange)
                                    .frame(width: 8, height: 8)
                                OSIcon(device.os, size: 13)
                                Text(device.name)
                                    .font(.system(size: 13, weight: store.selectedPendingDevice?.id == device.id ? .bold : .medium))
                                Spacer()
                            }
                            .foregroundStyle(store.selectedPendingDevice?.id == device.id ? Color.white : CRTheme.ink)
                            .padding(.horizontal, 12)
                            .padding(.vertical, 8)
                            .background(
                                RoundedRectangle(cornerRadius: 8, style: .continuous)
                                    .fill(store.selectedPendingDevice?.id == device.id ? CRTheme.brandElectric : Color.clear)
                            )
                            .padding(.horizontal, 12)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
            
            Spacer()
        }
    }
}

struct SidebarNavItem: View {
    let section: DashboardSection
    let isSelected: Bool
    let action: () -> Void
    @State private var hovered = false
    
    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Image(systemName: section.icon)
                    .font(.system(size: 14, weight: isSelected ? .bold : .medium))
                    .frame(width: 20)
                Text(section.title)
                    .font(.system(size: 13, weight: isSelected ? .bold : .medium))
                Spacer()
            }
            .foregroundStyle(isSelected ? Color.white : (hovered ? CRTheme.ink : CRTheme.inkSoft))
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background {
                if isSelected {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(CRTheme.brandElectric)
                } else if hovered {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(CRTheme.ink.opacity(0.04))
                }
            }
            .padding(.horizontal, 12)
        }
        .buttonStyle(.plain)
        .onHover { isHovered in
            hovered = isHovered
            if isHovered {
                NSCursor.pointingHand.push()
            } else {
                NSCursor.pop()
            }
        }
        .animation(.crFast, value: hovered)
    }
}

// MARK: - Center Column
struct CommandCenterView: View {
    @ObservedObject var store: LinkAllStore
    @State private var searchQuery = ""
    @State private var showingRemoteExplorer = false

    var body: some View {
        ScrollView(.vertical, showsIndicators: false) {
            VStack(alignment: .leading, spacing: 24) {
                
                // Global Search Placeholder
                HStack {
                    Image(systemName: "magnifyingglass")
                        .foregroundStyle(CRTheme.inkSubtle)
                    TextField("Search Files, Clipboard, Devices...", text: $searchQuery)
                        .textFieldStyle(.plain)
                        .font(.system(size: 15))
                }
                .padding(14)
                .background(CRTheme.surfaceStrong)
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(CRTheme.stroke, lineWidth: 1))
                .padding(.horizontal, 40)
                .padding(.top, 40)
                
                if store.connectedCount == 0 && store.pendingDevices.isEmpty {
                    RadarEmptyStateView()
                        .padding(.top, 60)
                } else {
                    // Transfer Analytics "Wrapped" Widget
                    TransferAnalyticsWidget(store: store)
                        .padding(.horizontal, 40)
                        .padding(.top, 16)
                    
                    // Dynamic Hero Header
                    DynamicHeroHeaderView(store: store)
                
                // 2. Send
                VStack(alignment: .leading, spacing: 16) {
                    Text("Send")
                        .font(.system(size: 20, weight: .bold))
                        .foregroundStyle(CRTheme.ink)
                        .padding(.horizontal, 40)

                    LazyVGrid(columns: tileColumns, spacing: 12) { sendTiles }
                        .padding(.horizontal, 40)
                }

                // 3. Transferring, before devices and history: what is
                // moving right now matters most.
                if !store.batchedTransfers.isEmpty {
                    VStack(alignment: .leading, spacing: 16) {
                        HStack(alignment: .firstTextBaseline, spacing: 8) {
                            Text("Transferring")
                                .font(.system(size: 20, weight: .bold))
                                .foregroundStyle(CRTheme.ink)
                            Text("\(store.batchedTransfers.count)")
                                .font(.system(size: 15, weight: .semibold))
                                .foregroundStyle(CRTheme.inkSoft)
                        }
                        .padding(.horizontal, 40)

                        VStack(spacing: 12) {
                            ForEach(store.batchedTransfers) { transfer in
                                ActiveTransferCard(transfer: transfer, store: store)
                            }
                        }
                        .padding(.horizontal, 40)
                    }
                }

                // 4. More
                VStack(alignment: .leading, spacing: 16) {
                    Text("More")
                        .font(.system(size: 20, weight: .bold))
                        .foregroundStyle(CRTheme.ink)
                        .padding(.horizontal, 40)

                    LazyVGrid(columns: tileColumns, spacing: 12) { moreTiles }
                        .padding(.horizontal, 40)
                }

                // 5. Recent
                VStack(alignment: .leading, spacing: 16) {
                    Text("Recent")
                        .font(.system(size: 20, weight: .bold))
                        .foregroundStyle(CRTheme.ink)
                        .padding(.horizontal, 40)
                    
                    VStack(alignment: .leading, spacing: 0) {
                        // Latest clipboard items and files, the same list as the Clipboard page.
                        let recent = Array(store.timeline.filter { $0.typeLabel != "Connection" }.prefix(4))
                        if recent.isEmpty {
                            Text("Copy something on either device, or send a file. It shows up here.")
                                .font(.system(size: 14))
                                .foregroundStyle(CRTheme.inkSoft)
                                .padding(16)
                        } else {
                            ForEach(Array(recent.enumerated()), id: \.offset) { index, item in
                                ActivityRow(
                                    action: item.title,
                                    time: item.timestamp.relativeTimeString(),
                                    icon: item.iconName
                                )
                                if index < recent.count - 1 {
                                    Divider().padding(.horizontal, 16)
                                }
                            }
                        }
                    }
                    .background(CRTheme.surfaceStrong)
                    .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
                    .padding(.horizontal, 40)
                }
                
                Spacer().frame(height: 60)
                }
            }
        }
        .sheet(isPresented: $showingRemoteExplorer) {
            if let device = store.defaultTargetDevice {
                RemoteExplorerView(store: store, device: device)
            }
        }
    }
    
    // MARK: Tiles
    // Wide rows, three across when there is room and fewer when not, so a
    // narrow window never stacks them into a tall column.
    private let tileColumns = [GridItem(.adaptive(minimum: 200), spacing: 12)]

    @ViewBuilder private var sendTiles: some View {
        transferTile
        clipboardTile
        browseTile
    }
    @ViewBuilder private var moreTiles: some View {
        historyTile
        speedTestTile
        settingsTile
    }

    private var transferTile: some View {
        LaunchpadTile(title: "Files & folders", subtitle: "Send files or entire folders", icon: "paperplane.fill", color: CRTheme.brandElectric) {
            store.presentSendModal()
        }
    }
    private var browseTile: some View {
        LaunchpadTile(title: "Browse Device", subtitle: "Open files on a phone", icon: "internaldrive.fill", color: CRTheme.brandViolet) {
            if store.connectedDevices.first != nil {
                showingRemoteExplorer = true
            }
        }
    }
    private var clipboardTile: some View {
        LaunchpadTile(title: "Clipboard", subtitle: "Send what you last copied", icon: "doc.on.clipboard.fill", color: CRTheme.accentPink) {
            store.pushCurrentClipboard()
        }
    }
    private var historyTile: some View {
        LaunchpadTile(title: "Clipboard history", subtitle: "Everything you copied", icon: "clock.arrow.circlepath", color: CRTheme.brandCyan) {
            withAnimation(.crSpring) { store.selectedSection = .clipboard }
        }
    }
    private var speedTestTile: some View {
        LaunchpadTile(title: "Speed Test", subtitle: "Measure the link", icon: "gauge.with.dots.needle.bottom.50percent", color: CRTheme.brandCyan) {
            if let first = store.connectedDevices.first {
                store.startSpeedTest(deviceId: first.id)
                withAnimation(.crSpring) { store.selectedSection = .transfers }
            }
        }
    }
    private var settingsTile: some View {
        LaunchpadTile(title: "Settings", subtitle: nil, icon: "gearshape.fill", color: CRTheme.inkSubtle) {
            withAnimation(.crSpring) { store.selectedSection = .settings }
        }
    }

}

// Subcomponents
struct LaunchpadTile: View {
    let title: String
    let subtitle: String?
    let icon: String
    let color: Color
    let action: () -> Void
    @State private var hovered = false
    
    var body: some View {
        Button(action: action) {
            HStack(spacing: 12) {
                ZStack {
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .fill(color.opacity(0.12))
                        .frame(width: 40, height: 40)
                    Image(systemName: icon)
                        .font(.system(size: 17, weight: .semibold))
                        .foregroundStyle(color)
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(CRTheme.ink)
                        .lineLimit(1)
                    if let subtitle {
                        Text(subtitle)
                            .font(.system(size: 12))
                            .foregroundStyle(CRTheme.inkSoft)
                            .lineLimit(1)
                            .truncationMode(.tail)
                    }
                }
                Spacer(minLength: 4)
                Image(systemName: "chevron.right")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(CRTheme.inkSubtle)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)
            .frame(maxWidth: .infinity, minHeight: 64, alignment: .leading)
            .background(CRTheme.surfaceStrong)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(CRTheme.stroke.opacity(0.5), lineWidth: 1))
            .shadow(color: Color.black.opacity(hovered ? 0.08 : 0.02), radius: hovered ? 10 : 3, y: hovered ? 4 : 1)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { isHovered in
            hovered = isHovered
            if isHovered {
                NSCursor.pointingHand.push()
            } else {
                NSCursor.pop()
            }
        }
        .animation(.crFast, value: hovered)
    }
}

struct SuggestionCard: View {
    let title: String
    let subtitle: String
    let icon: String
    let color: Color
    @State private var hovered = false
    
    var body: some View {
        HStack(spacing: 14) {
            Image(systemName: icon)
                .font(.system(size: 20))
                .foregroundStyle(color)
                .frame(width: 24)
            
            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.system(size: 13, weight: .bold))
                    .foregroundStyle(CRTheme.ink)
                Text(subtitle)
                    .font(.system(size: 11.5))
                    .foregroundStyle(CRTheme.inkSoft)
            }
            Spacer()
        }
        .padding(16)
        .frame(width: 260)
        .background(CRTheme.surfaceStrong)
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(CRTheme.stroke.opacity(0.5), lineWidth: 1))
        .scaleEffect(hovered ? 1.02 : 1.0)
        .onHover { hovered = $0 }
        .animation(.crSpring, value: hovered)
    }
}

struct ActivityRow: View {
    let action: String
    let time: String
    let icon: String
    
    var body: some View {
        HStack(spacing: 16) {
            Image(systemName: icon)
                .font(.system(size: 16))
                .foregroundStyle(CRTheme.brandElectric)
            Text(action)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(CRTheme.ink)
                .lineLimit(1)
            Spacer()
            Text(time)
                .font(.system(size: 12))
                .foregroundStyle(CRTheme.inkSoft)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 16)
    }
}

// MARK: - Right Column
struct LiveDevicePanel: View {
    @ObservedObject var store: LinkAllStore
    
    private var device: ManagedDevice? {
        store.defaultTargetDevice
    }
    
    var body: some View {
        ScrollView(.vertical, showsIndicators: false) {
            if let dev = device {
                VStack(alignment: .leading, spacing: 28) {
                    
                    // Header Status
                    VStack(alignment: .leading, spacing: 8) {
                        Text(dev.name)
                            .font(.system(size: 24, weight: .bold, design: .rounded))
                            .foregroundStyle(CRTheme.ink)
                        
                        HStack(spacing: 6) {
                            // Still: a forever pulse here redrew the window
                            // every frame, even while it was closed.
                            StatusDot(isOnline: true, size: 8)
                            
                            Text("Connected")
                                .font(.system(size: 13, weight: .bold))
                                .foregroundStyle(CRTheme.accentGreen)
                            
                            if let net = store.peerNetworks.first(where: { $0.deviceId == dev.id }) {
                                Text("· \(net.networkType)")
                                    .font(.system(size: 13))
                                    .foregroundStyle(CRTheme.inkSoft)
                            }
                            
                            if let endpoint = dev.endpoint {
                                Text("· \(endpoint)")
                                    .font(.system(size: 13, design: .monospaced))
                                    .foregroundStyle(CRTheme.inkSoft)
                            }
                        }
                    }
                    
                    // Battery Row
                    if let bat = store.peerBatteries.first(where: { $0.deviceId == dev.id }) {
                        VStack(alignment: .leading, spacing: 6) {
                            HStack {
                                Text("Battery")
                                    .font(.system(size: 13, weight: .semibold))
                                Spacer()
                                Text("\(bat.level)%")
                                    .font(.system(size: 13, weight: .bold, design: .monospaced))
                            }
                            GeometryReader { geo in
                                ZStack(alignment: .leading) {
                                    Capsule().fill(CRTheme.surfaceStrong)
                                    Capsule()
                                        .fill(bat.level < 20 ? CRTheme.accentRed : CRTheme.accentGreen)
                                        .frame(width: geo.size.width * (CGFloat(bat.level) / 100.0))
                                }
                            }
                            .frame(height: 8)
                        }
                    }

                    // Storage Row
                    if let storage = store.peerStorages.first(where: { $0.deviceId == dev.id }) {
                        VStack(alignment: .leading, spacing: 8) {
                            HStack {
                                Text("Storage")
                                    .font(.system(size: 13, weight: .semibold))
                                Spacer()
                                let used = storage.totalBytes - storage.freeBytes
                                let usedGb = Double(used) / 1_000_000_000.0
                                let totalGb = Double(storage.totalBytes) / 1_000_000_000.0
                                Text("\(String(format: "%.1f", usedGb)) GB / \(String(format: "%.1f", totalGb)) GB")
                                    .font(.system(size: 12, weight: .medium))
                                    .foregroundStyle(CRTheme.inkSoft)
                            }
                            
                            GeometryReader { geo in
                                let used = max(0, storage.totalBytes - storage.freeBytes)
                                let total = max(1, storage.totalBytes)
                                let imgRatio = max(0, CGFloat(storage.imagesBytes) / CGFloat(total))
                                let vidRatio = max(0, CGFloat(storage.videosBytes) / CGFloat(total))
                                let otherRatio = max(0, CGFloat(used - storage.imagesBytes - storage.videosBytes) / CGFloat(total))
                                
                                ZStack(alignment: .leading) {
                                    // 1. Full-width background track
                                    Capsule()
                                        .fill(CRTheme.surfaceStrong)
                                        .frame(width: geo.size.width, height: 8)
                                    
                                    // 2. Filled segments
                                    HStack(spacing: 2) {
                                        if imgRatio > 0 {
                                            Rectangle().fill(Color.orange)
                                                .frame(width: max(0, geo.size.width * imgRatio - 1))
                                        }
                                        if vidRatio > 0 {
                                            Rectangle().fill(Color.purple)
                                                .frame(width: max(0, geo.size.width * vidRatio - 1))
                                        }
                                        if otherRatio > 0 {
                                            Rectangle().fill(Color.gray.opacity(0.6))
                                                .frame(width: max(0, geo.size.width * otherRatio - 1))
                                        }
                                    }
                                    .clipShape(Capsule())
                                    .frame(height: 8)
                                }
                            }
                            .frame(height: 8)
                            
                            HStack(spacing: 12) {
                                HStack(spacing: 4) {
                                    Circle().fill(Color.orange).frame(width: 6, height: 6)
                                    Text("Images")
                                }
                                HStack(spacing: 4) {
                                    Circle().fill(Color.purple).frame(width: 6, height: 6)
                                    Text("Videos")
                                }
                                HStack(spacing: 4) {
                                    Circle().fill(Color.gray.opacity(0.3)).frame(width: 6, height: 6)
                                    Text("Other")
                                }
                            }
                            .font(.system(size: 10))
                            .foregroundStyle(CRTheme.inkSubtle)
                        }
                    }

                    // Clipboard History
                    let recentClipboards = store.activityFeed.filter { $0.kind == "clipboard" || $0.kind == "remote_clipboard_available" }.prefix(5)
                    if !recentClipboards.isEmpty {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("Clipboard History")
                                .font(.system(size: 11, weight: .bold))
                                .foregroundStyle(CRTheme.inkSubtle)
                            
                            VStack(spacing: 6) {
                                ForEach(Array(recentClipboards.enumerated()), id: \.offset) { index, clip in
                                    if let text = clip.text_preview {
                                        Button {
                                            Task { @MainActor in
                                                let full = await store.fullText(of: clip) ?? text
                                                NSPasteboard.general.clearContents()
                                                NSPasteboard.general.setString(full, forType: .string)
                                            }
                                        } label: {
                                            HStack {
                                                Text(text)
                                                    .font(.system(size: 13, design: .monospaced))
                                                    .foregroundStyle(index == 0 ? CRTheme.brandElectric : CRTheme.inkSubtle)
                                                    .lineLimit(1)
                                                Spacer()
                                                Image(systemName: "doc.on.clipboard")
                                                    .font(.system(size: 10))
                                                    .foregroundStyle(CRTheme.inkSubtle.opacity(0.5))
                                            }
                                            .padding(10)
                                            .frame(maxWidth: .infinity, alignment: .leading)
                                            .background(index == 0 ? CRTheme.brandElectric.opacity(0.1) : CRTheme.surfaceStrong)
                                            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                                        }
                                        .buttonStyle(.plain)
                                    }
                                }
                            }
                        }
                    }
                }
                .padding(.horizontal, 24)
                .padding(.bottom, 24)
                .padding(.top, 40)
            } else {
                // Not Connected State
                VStack(spacing: 16) {
                    Image(systemName: "antenna.radiowaves.left.and.right")
                        .font(.system(size: 40, weight: .ultraLight))
                        .foregroundStyle(CRTheme.inkSubtle)
                    Text("No active connection")
                        .font(.system(size: 16, weight: .bold))
                        .foregroundStyle(CRTheme.ink)
                    Text("Connect a device on your local network to view live insights here.")
                        .font(.system(size: 13))
                        .foregroundStyle(CRTheme.inkSoft)
                        .multilineTextAlignment(.center)
                }
                .padding(32)
                .frame(maxWidth: .infinity, minHeight: 600, maxHeight: .infinity, alignment: .center)
            }
        }
    }
}

struct LegendItem: View {
    let color: Color
    let text: String
    
    var body: some View {
        HStack(spacing: 4) {
            Circle().fill(color).frame(width: 6, height: 6)
            Text(text).font(.system(size: 11)).foregroundStyle(CRTheme.inkSoft)
        }
    }
}

struct MetricCard: View {
    let label: String
    let value: String
    let icon: String
    
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Image(systemName: icon)
                .foregroundStyle(CRTheme.inkSubtle)
                .font(.system(size: 14))
            
            VStack(alignment: .leading, spacing: 2) {
                Text(value)
                    .font(.system(size: 15, weight: .bold))
                    .foregroundStyle(CRTheme.ink)
                Text(label)
                    .font(.system(size: 11))
                    .foregroundStyle(CRTheme.inkSoft)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(CRTheme.surfaceStrong)
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(CRTheme.stroke, lineWidth: 1))
    }
}



struct TransferAnalyticsWidget: View {
    @ObservedObject var store: LinkAllStore
    
    var timeSavedString: String {
        let bytesPerSecond = 5.0 * 1024.0 * 1024.0
        let secondsSaved = Double(store.totalBytesTransferred) / bytesPerSecond
        if secondsSaved < 60 {
            return "\(Int(secondsSaved))s"
        } else if secondsSaved < 3600 {
            return "\(Int(secondsSaved / 60)) min"
        } else {
            return String(format: "%.1f hrs", secondsSaved / 3600.0)
        }
    }
    
    var dataString: String {
        let gb = Double(store.totalBytesTransferred) / (1024.0 * 1024.0 * 1024.0)
        let mb = Double(store.totalBytesTransferred) / (1024.0 * 1024.0)
        if gb >= 1.0 {
            return String(format: "%.1f GB", gb)
        } else {
            return String(format: "%.1f MB", mb)
        }
    }
    
    var body: some View {
        if store.totalBytesTransferred > 0 {
            HStack(spacing: 16) {
                HStack(spacing: 12) {
                    ZStack {
                        Circle()
                            .fill(Color(nsColor: .controlAccentColor).opacity(0.1))
                            .frame(width: 38, height: 38)
                        
                        Image(systemName: "sparkles")
                            .font(.system(size: 17, weight: .semibold))
                            .foregroundStyle(Color(nsColor: .controlAccentColor))
                    }
                    
                    VStack(alignment: .leading, spacing: 2) {
                        Text("LINK ALL WRAPPED")
                            .font(.system(size: 10, weight: .bold, design: .rounded))
                            .foregroundStyle(Color.secondary)
                            .tracking(0.5)
                        
                        HStack(spacing: 5) {
                            Text("You've beamed")
                                .font(.system(size: 14, weight: .medium, design: .rounded))
                                .foregroundStyle(Color.primary)
                            
                            Text(dataString)
                                .font(.system(size: 16, weight: .heavy, design: .rounded))
                                .foregroundStyle(Color.primary)
                        }
                    }
                }
                
                Spacer(minLength: 12)
                
                HStack(spacing: 8) {
                    Image(systemName: "bolt.shield.fill")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(Color(nsColor: .controlAccentColor))
                    
                    VStack(alignment: .leading, spacing: 0) {
                        Text("TIME SAVED")
                            .font(.system(size: 9, weight: .bold, design: .rounded))
                            .foregroundStyle(Color.secondary)
                            .tracking(0.4)
                        
                        Text(timeSavedString)
                            .font(.system(size: 14, weight: .bold, design: .rounded))
                            .foregroundStyle(Color.primary)
                    }
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 8)
                .background(
                    Capsule(style: .continuous)
                        .fill(Color(nsColor: .windowBackgroundColor))
                        .overlay(
                            Capsule(style: .continuous)
                                .stroke(Color.primary.opacity(0.06), lineWidth: 1)
                        )
                )
            }
            .padding(.horizontal, 18)
            .padding(.vertical, 14)
            .background(
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .fill(Color(nsColor: .controlBackgroundColor))
                    .shadow(color: Color.black.opacity(0.04), radius: 6, x: 0, y: 2)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .stroke(Color.primary.opacity(0.05), lineWidth: 1)
            )
            .shadow(color: Color(nsColor: .controlAccentColor).opacity(0.08), radius: 12, x: 0, y: 4)
        }
    }
}


struct RadarEmptyStateView: View {
    @State private var rotation: Double = 0
    @State private var opacityPulse: Double = 0.4
    
    var body: some View {
        VStack(spacing: 32) {
            ZStack {
                // Radar Rings
                ForEach(0..<4) { i in
                    Circle()
                        .strokeBorder(CRTheme.brandCyan.opacity(0.15 - Double(i) * 0.03), lineWidth: 1)
                        .frame(width: CGFloat(100 + i * 80), height: CGFloat(100 + i * 80))
                }
                
                // Sweeping Radar Line
                Circle()
                    .fill(
                        AngularGradient(
                            gradient: Gradient(colors: [CRTheme.brandElectric.opacity(0.0), CRTheme.brandElectric.opacity(0.6)]),
                            center: .center,
                            startAngle: .degrees(-90),
                            endAngle: .degrees(0)
                        )
                    )
                    .frame(width: 340, height: 340)
                    .rotationEffect(.degrees(rotation))
                    .mask(Circle().frame(width: 340, height: 340))
                
                // Core Node
                Circle()
                    .fill(CRTheme.brandElectric)
                    .frame(width: 16, height: 16)
                    .shadow(color: CRTheme.brandElectric.opacity(0.8), radius: 8)
                    .overlay(
                        Circle()
                            .strokeBorder(Color.white.opacity(0.5), lineWidth: 2)
                            .scaleEffect(1.5)
                            .opacity(opacityPulse)
                    )
            }
            .frame(height: 360)
            
            VStack(spacing: 8) {
                Text("Scanning Mesh Network...")
                    .font(.system(size: 20, weight: .bold))
                    .foregroundStyle(CRTheme.ink)
                
                Text("Make sure your phone is on the same Wi-Fi and Link All is open.")
                    .font(.system(size: 14))
                    .foregroundStyle(CRTheme.inkSoft)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 280)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        // Spins only while the window is on screen; closed, it kept the app
        // redrawing at full frame rate whenever no device was connected.
        .background(WindowVisibilityReader { visible in
            if visible {
                withAnimation(.linear(duration: 4.0).repeatForever(autoreverses: false)) {
                    rotation = 360
                }
                withAnimation(.easeInOut(duration: 1.5).repeatForever(autoreverses: true)) {
                    opacityPulse = 0.0
                }
            } else {
                var still = Transaction()
                still.disablesAnimations = true
                withTransaction(still) {
                    rotation = 0
                    opacityPulse = 0.4
                }
            }
        })
    }
}
