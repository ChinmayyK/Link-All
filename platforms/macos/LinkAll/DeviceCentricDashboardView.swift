import SwiftUI
import UniformTypeIdentifiers
import Foundation
import SystemConfiguration
import CoreImage.CIFilterBuiltins
import Carbon.HIToolbox
import QuickLook

struct DevicesSectionView: View {
    @ObservedObject var store: LinkAllStore
    let rename: (ManagedDevice) -> Void
    @State private var showingFileImporter = false
    @State private var pendingFileTarget:  ManagedDevice?

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 14) {
                // 2x2 Widget Grid
                LazyVGrid(columns: [GridItem(.flexible(), spacing: 14), GridItem(.flexible(), spacing: 14)], spacing: 14) {
                    MagicLinkPairingCard(store: store)
                    FileShareCard(store: store) { pendingFileTarget = $0; showingFileImporter = true }
                }
                .padding(.top, 18)

                if store.devices.isEmpty {
                    CREmptyState(
                        systemImage: "wifi.slash", title: "No devices discovered",
                        message: "Nearby devices on Wi-Fi or hotspot will appear here. Remembered devices can be pulled back with a fresh scan.",
                        accent: CRTheme.brandElectric,
                        actionLabel: "Scan again",
                        onAction: { store.scanForDevices() }
                    )
                } else {
                    LazyVStack(spacing: 7) {
                        ForEach(store.devices) { DeviceCard(device: $0, store: store, rename: rename) }
                    }
                }
            }
            .padding(.horizontal, 20).padding(.bottom, 24)
        }
        .fileImporter(isPresented: $showingFileImporter, allowedContentTypes: [.item, .folder], allowsMultipleSelection: true) { result in
            if case let .success(urls) = result {
                store.sendFiles(urls: urls, to: pendingFileTarget)
                pendingFileTarget = nil
            }
        }
    }
}

// MARK: - Trust Section

struct TrustSectionView: View {
    @ObservedObject var store: LinkAllStore
    let rename: (ManagedDevice) -> Void
    private var attention: [ManagedDevice] { store.devices.filter { $0.trustState != .trusted } }

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 8) {
                // Attention banner
                if !attention.isEmpty {
                    HStack(spacing: 10) {
                        ZStack {
                            Circle().fill(CRTheme.accentOrange.opacity(0.10)).frame(width: 28, height: 28)
                            Image(systemName: "exclamationmark.triangle.fill")
                                .font(.system(size: 12, weight: .semibold))
                                .foregroundStyle(CRTheme.accentOrange)
                        }
                        Text("\(attention.count) device\(attention.count == 1 ? "" : "s") awaiting your decision")
                            .font(.system(size: 13, weight: .semibold)).foregroundStyle(CRTheme.ink)
                        Spacer()
                    }
                    .padding(.horizontal, 14).padding(.vertical, 10)
                    .background {
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .fill(CRTheme.accentOrange.opacity(0.06))
                            .overlay {
                                RoundedRectangle(cornerRadius: 10, style: .continuous)
                                    .strokeBorder(CRTheme.accentOrange.opacity(0.18), lineWidth: 0.5)
                            }
                    }
                    .padding(.top, 18)
                }

                if attention.isEmpty {
                    CREmptyState(
                        systemImage: "checkmark.shield.fill", title: "All clear",
                        message: "No trust prompts right now. New devices appear here when they request access.",
                        accent: CRTheme.accentGreen
                    )
                } else {
                    LazyVStack(spacing: 7) {
                        ForEach(attention) { DeviceCard(device: $0, store: store, rename: rename, emphasizeTrust: true) }
                    }
                }
            }
            .padding(.horizontal, 20).padding(.bottom, 24)
        }
    }
}

// MARK: - Timeline Card
struct DeviceCard: View {
    let device: ManagedDevice
    @ObservedObject var store: LinkAllStore
    let rename: (ManagedDevice) -> Void
    var emphasizeTrust: Bool = false
    @State private var isHovered = false
    @State private var showingRemoteExplorer = false
    @Environment(\.colorScheme) var colorScheme
    private var isDark: Bool { colorScheme == .dark }

    private var accent: Color { emphasizeTrust ? CRTheme.accentOrange : device.connectionState.color }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .center, spacing: 11) {
                ZStack {
                    Circle().fill(accent.opacity(0.12))
                        .frame(width: 38, height: 38)
                    OSIcon(device.os, size: 18)
                        .foregroundStyle(accent)
                }

                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        Text(device.name)
                            .font(.system(size: 14, weight: .semibold)).foregroundStyle(CRTheme.ink)
                            .lineLimit(1)
                            .layoutPriority(1)
                        
                        HStack(spacing: 3) {
                            StatusDot(isOnline: device.isConnected, size: 6)
                            Text(device.connectionState.label)
                                .font(.system(size: 11, weight: .medium))
                                .foregroundStyle(device.connectionState.color)
                        }
                        
                        Spacer(minLength: 8)
                        
                        if let battery = store.peerBatteries.first(where: { $0.deviceId == device.id }) {
                            BatteryIndicatorPill(level: battery.level, charging: battery.charging)
                        }
                        if let net = store.peerNetworks.first(where: { $0.deviceId == device.id }) {
                            NetworkIndicatorPill(type: net.networkType)
                        }
                    }
                    HStack(spacing: 6) {
                        CRTag(text: device.trustState.rawValue.capitalized, tint: device.trustState.color)
                        if device.canReconnect {
                            CRTag(text: "Auto reconnect", tint: CRTheme.brandElectric)
                        }
                        if let ep = device.endpoint {
                            Text("·").foregroundStyle(CRTheme.inkFaint).font(.system(size: 10))
                            Label(ep, systemImage: "network")
                                .lineLimit(1).truncationMode(.middle)
                                .font(.system(size: 10.5)).foregroundStyle(CRTheme.inkSoft)
                        }
                        if let seen = device.lastSeen {
                            Text("·").foregroundStyle(CRTheme.inkFaint).font(.system(size: 10))
                            Text("Seen \(seen.relativeTimeString())")
                                .font(.system(size: 10.5)).foregroundStyle(CRTheme.inkSoft)
                        }
                    }
                }
                Spacer()
            }
            .padding(.horizontal, 14).padding(.vertical, 12)

            // Fingerprint (hover)
            if let fp = device.fingerprint, isHovered {
                CRDivider().padding(.horizontal, 14)
                HStack(spacing: 5) {
                    Image(systemName: "key.fill").font(.system(size: 9)).foregroundStyle(CRTheme.inkSubtle)
                    Text(fp).font(.system(size: 10, weight: .medium, design: .monospaced))
                        .foregroundStyle(CRTheme.inkSubtle).lineLimit(1).truncationMode(.middle)
                }
                .padding(.horizontal, 14).padding(.vertical, 6)
                .transition(.opacity)
            }

            // Error
            if let err = device.lastError, !err.isEmpty {
                CRDivider().padding(.horizontal, 14)
                Label(err, systemImage: "exclamationmark.triangle.fill")
                    .font(.system(size: 11)).foregroundStyle(CRTheme.accentOrange)
                    .padding(.horizontal, 14).padding(.vertical, 6)
            }

            // Actions
            CRDivider()
            HStack(spacing: 8) {
                if device.isConnected {
                    ModernDeviceCardButton(icon: "internaldrive.fill", color: CRTheme.brandViolet, help: "Remote File Explorer") { showingRemoteExplorer = true }
                    ModernDeviceCardButton(icon: "gauge.with.dots.needle.bottom.50percent", color: CRTheme.brandCyan, help: "Speed Test") { 
                        store.startSpeedTest(deviceId: device.id)
                        withAnimation { store.selectedSection = .transfers }
                    }
                    ModernDeviceCardButton(icon: "wifi.slash", color: CRTheme.accentOrange, help: "Disconnect") { store.disconnect(device) }
                } else if device.trustState == .trusted {
                    ModernDeviceCardButton(icon: "antenna.radiowaves.left.and.right", color: CRTheme.brandElectric, help: "Connect") { store.connect(device) }
                }
                
                if device.trustState != .trusted {
                    if device.pairingRequested || device.outgoingPairingWaiting {
                        if let pin = device.pairingPin, !pin.isEmpty {
                            Text("PIN: \(pin)")
                                .font(.system(.body, design: .monospaced).weight(.bold))
                                .foregroundStyle(CRTheme.ink)
                                .padding(.horizontal, 8)
                        }
                        if device.pairingRequested {
                            ModernDeviceCardButton(icon: "checkmark", color: CRTheme.accentGreen, help: "Accept") { store.respondToPairing(device, accepted: true) }
                            ModernDeviceCardButton(icon: "xmark", color: CRTheme.accentRed, help: "Decline") { store.respondToPairing(device, accepted: false) }
                        } else {
                            Text("Waiting...")
                                .foregroundStyle(CRTheme.inkSoft)
                                .font(.system(size: 13))
                            ModernDeviceCardButton(icon: "xmark", color: CRTheme.inkSoft, help: "Cancel request") { store.cancelPairingRequest(device) }
                        }
                    } else {
                        ModernDeviceCardButton(icon: "link", color: CRTheme.brandElectric, help: "Pair") { store.sendPairingRequest(device) }
                    }
                    ModernDeviceCardButton(icon: "trash", color: CRTheme.inkSoft, help: "Remove") { store.forget(device) }
                } else {
                    ModernDeviceCardButton(icon: "pencil", color: CRTheme.inkSoft, help: "Rename") { rename(device) }
                    ModernDeviceCardButton(icon: "lock.slash", color: CRTheme.accentOrange, help: "Revoke Trust") { store.revoke(device) }
                    ModernDeviceCardButton(icon: "trash", color: CRTheme.accentRed, help: "Forget") { Task { try? await LinkAllIPCClient.shared.forgetDevice(deviceId: device.id); store.scanForDevices() } }
                }
                Spacer()
            }
            .padding(.horizontal, 14).padding(.vertical, 9)
        }
        .crCard(cornerRadius: CRTheme.radiusSmall, highlighted: isHovered, accent: accent)
        .onHover { isHovered = $0 }
        .animation(.crFast, value: isHovered)
        .sheet(isPresented: $showingRemoteExplorer) {
            RemoteExplorerView(store: store, device: device)
        }
    }
}

// MARK: - File Share Card

struct FileShareCard: View {
    @ObservedObject var store: LinkAllStore
    let chooseTarget: (ManagedDevice?) -> Void
    @State private var hovered = false
    
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                ZStack {
                    Circle().fill(CRTheme.brandElectric.opacity(0.12)).frame(width: 44, height: 44)
                    Image(systemName: "arrow.up.doc.fill").font(.system(size: 18, weight: .semibold)).foregroundStyle(CRTheme.brandElectric)
                }
                Spacer()
                if !store.connectedDevices.isEmpty {
                    Menu("Send to…") {
                        ForEach(store.connectedDevices) { d in Button(d.name) { chooseTarget(d) } }
                    }
                    .buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.brandElectric))
                } else {
                    Button("Send Files & Folders") { chooseTarget(nil) }
                        .buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.brandElectric))
                }
            }
            
            VStack(alignment: .leading, spacing: 4) {
                Text("Files & Folders")
                    .font(.system(size: 15, weight: .bold, design: .rounded))
                    .foregroundStyle(CRTheme.ink)
                Text("Send files or entire folders, and your clipboard, straight across your local network. Nothing goes through a cloud.")
                    .font(.system(size: 12.5))
                    .foregroundStyle(CRTheme.inkSoft)
                    .lineSpacing(2)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(18)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(CRTheme.surfaceStrong)
        .crCard(cornerRadius: CRTheme.radiusMedium, highlighted: hovered, accent: CRTheme.brandElectric)
        .scaleEffect(hovered ? 1.02 : 1.0)
        .onHover { hovered = $0 }
        .animation(.crSpring, value: hovered)
    }
}

// MARK: - Rename Sheet (auto-focuses text field)

struct RenameDeviceSheet: View {
    let device: ManagedDevice
    @State var draft: String
    let onCancel: () -> Void
    let onSave:   (String) -> Void
    @FocusState private var fieldFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            VStack(alignment: .leading, spacing: 3) {
                Text("Rename Device").font(.system(size: 18, weight: .bold)).foregroundStyle(CRTheme.ink)
                Text("Give \(device.rawName) a friendly name.")
                    .font(.system(size: 12.5)).foregroundStyle(CRTheme.inkSoft)
            }
            TextField("Device name", text: $draft)
                .crInput()
                .focused($fieldFocused)
                .onSubmit { if !draft.isEmpty { onSave(draft) } }
            HStack {
                Spacer()
                Button("Cancel", action: onCancel).buttonStyle(CRSecondaryButtonStyle())
                Button("Save") { onSave(draft) }.buttonStyle(CRPrimaryButtonStyle())
                        .disabled(draft.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(24).frame(width: 330)
        .background(CRTheme.surfaceStrong.ignoresSafeArea())
        .onAppear { fieldFocused = true }
    }
}

// MARK: - Device-Centric Dashboard (O+ Connect Style)

struct DeviceCentricDashboardView: View {
    @ObservedObject var store: LinkAllStore
    @State private var showingFilePicker = false
    @State private var pendingFileTarget: ManagedDevice?
    @Environment(\.colorScheme) var scheme

    private var attention: [ManagedDevice] { store.devices.filter { $0.trustState != .trusted } }

    var body: some View {
        ScrollView {
            VStack(spacing: 0) {
                // Header
                HStack {
                    Text("Ecosystem")
                        .font(.system(size: 28, weight: .semibold, design: .rounded))
                        .foregroundStyle(CRTheme.ink)
                    Spacer()
                }
                .padding(.horizontal, 40)
                .padding(.top, 40)
                .padding(.bottom, 20)

                // Attention Section
                if !attention.isEmpty {
                    VStack(alignment: .leading, spacing: 12) {
                        HStack(spacing: 8) {
                            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(CRTheme.accentOrange)
                            Text("Awaiting your decision").font(.system(size: 14, weight: .semibold)).foregroundStyle(CRTheme.ink)
                        }
                        .padding(.horizontal, 40)
                        
                        LazyVStack(spacing: 12) {
                            ForEach(attention) { device in
                                UntrustedDeviceCard(device: device, store: store)
                                    .padding(.horizontal, 40)
                            }
                        }
                    }
                    .padding(.bottom, 30)
                }

                // Active Transfers Section
                if !store.batchedTransfers.isEmpty {
                    VStack(alignment: .leading, spacing: 12) {
                        HStack(spacing: 8) {
                            Image(systemName: "arrow.up.arrow.down").foregroundStyle(CRTheme.brandElectric)
                            Text("Active Transfers").font(.system(size: 14, weight: .semibold)).foregroundStyle(CRTheme.ink)
                        }
                        .padding(.horizontal, 40)
                        
                        LazyVStack(spacing: 12) {
                            ForEach(store.batchedTransfers) { transfer in
                                ActiveTransferCard(transfer: transfer, store: store)
                                    .padding(.horizontal, 40)
                            }
                        }
                    }
                    .padding(.bottom, 30)
                }

                // Active Speed Tests Section
                if !store.activeSpeedTests.isEmpty {
                    VStack(alignment: .leading, spacing: 12) {
                        HStack(spacing: 8) {
                            Image(systemName: "gauge.with.dots.needle.bottom.50percent").foregroundStyle(CRTheme.brandCyan)
                            Text("Speed Tests").font(.system(size: 14, weight: .semibold)).foregroundStyle(CRTheme.ink)
                        }
                        .padding(.horizontal, 40)
                        
                        LazyVStack(spacing: 12) {
                            ForEach(store.activeSpeedTests) { test in
                                ActiveSpeedTestCard(test: test, store: store)
                                    .padding(.horizontal, 40)
                            }
                        }
                    }
                    .padding(.bottom, 30)
                }

                // Trusted & Connected Devices Section
                VStack(alignment: .leading, spacing: 14) {
                    HStack(spacing: 8) {
                        Image(systemName: "checkmark.shield.fill").foregroundStyle(CRTheme.brandElectric)
                        Text("Remembered Devices").font(.system(size: 14, weight: .semibold)).foregroundStyle(CRTheme.ink)
                    }
                    .padding(.horizontal, 40)
                    
                    let trustedDevices = store.devices.filter { $0.trustState == .trusted }
                    if !trustedDevices.isEmpty {
                        LazyVGrid(columns: [GridItem(.flexible(), spacing: 16), GridItem(.flexible(), spacing: 16)], spacing: 16) {
                            ForEach(trustedDevices, id: \.id) { device in
                                CompactDeviceCard(device: device, store: store) {
                                    pendingFileTarget = device
                                    showingFilePicker = true
                                }
                            }
                        }
                        .padding(.horizontal, 40)
                    } else {
                        CompactEmptyState(store: store)
                            .padding(.horizontal, 40)
                    }
                }
                .padding(.bottom, 32)
                
                // Quick Actions Grid
                VStack(alignment: .leading, spacing: 14) {
                    HStack(spacing: 8) {
                        Image(systemName: "wand.and.stars").foregroundStyle(CRTheme.brandElectric)
                        Text("Discovery & Quick Tools").font(.system(size: 14, weight: .semibold)).foregroundStyle(CRTheme.ink)
                    }
                    .padding(.horizontal, 40)
                    
                    LazyVGrid(columns: [GridItem(.flexible(), spacing: 16), GridItem(.flexible(), spacing: 16)], spacing: 16) {
                        MagicLinkPairingCard(store: store)
                        FileShareCard(store: store) { pendingFileTarget = $0; showingFilePicker = true }
                    }
                    .padding(.horizontal, 40)
                }
                .padding(.bottom, 40)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.clear)
        .fileImporter(isPresented: $showingFilePicker, allowedContentTypes: [.item, .folder], allowsMultipleSelection: true) { result in
            if case .success(let urls) = result {
                store.sendFiles(urls: urls, to: pendingFileTarget)
                pendingFileTarget = nil
            }
        }
    }
}

// MARK: - Hero Device Layouts

struct CompactEmptyState: View {
    @ObservedObject var store: LinkAllStore

    var body: some View {
        HStack(spacing: 18) {
            ZStack {
                Circle().fill(CRTheme.brandElectric.opacity(0.12)).frame(width: 52, height: 52)
                Image(systemName: "macbook.and.iphone").foregroundStyle(CRTheme.brandElectric).font(.system(size: 24, weight: .light))
            }
            
            VStack(alignment: .leading, spacing: 4) {
                Text("No remembered devices yet")
                    .font(.system(size: 15, weight: .bold, design: .rounded))
                    .foregroundStyle(CRTheme.ink)
                Text("Once you pair a device above or scan your local network, it will stay remembered here for instant clipboard and file sharing.")
                    .font(.system(size: 12.5))
                    .foregroundStyle(CRTheme.inkSoft)
                    .lineSpacing(2)
                    .fixedSize(horizontal: false, vertical: true)
            }
            
            Spacer(minLength: 12)
            
            Button {
                store.scanForDevices()
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "antenna.radiowaves.left.and.right")
                    Text("Scan Nearby")
                }
            }
            .buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.brandElectric))
        }
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(CRTheme.surfaceStrong)
        .crCard(cornerRadius: CRTheme.radiusMedium)
    }
}

struct ModernDeviceCardButton: View {
    let icon: String
    let color: Color
    let help: String
    let action: () -> Void
    @State private var isHovered = false
    
    var body: some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.system(size: 13, weight: .semibold))
                .frame(width: 28, height: 28)
                .foregroundStyle(isHovered ? color : CRTheme.inkSoft)
                .background(isHovered ? color.opacity(0.15) : CRTheme.inkSoft.opacity(0.08), in: Circle())
        }
        .buttonStyle(.plain)
        .help(help)
        .scaleEffect(isHovered ? 1.04 : 1.0)
        .onHover { isHovered = $0 }
        .animation(.spring(response: 0.2, dampingFraction: 0.7), value: isHovered)
    }
}

struct CompactDeviceCard: View {
    let device: ManagedDevice
    @ObservedObject var store: LinkAllStore
    let onSendFiles: () -> Void
    @Environment(\.colorScheme) var colorScheme
    @State private var isHovered = false
    @State private var showingRemoteExplorer = false

    var body: some View {
        HStack(spacing: 16) {
            // Icon
            ZStack {
                Circle()
                    .fill(CRTheme.brandElectric.opacity(0.1))
                    .frame(width: 44, height: 44)
                
                OSIcon(device.os, size: 20)
                    .foregroundStyle(CRTheme.brandElectric)
            }
            
            VStack(alignment: .leading, spacing: 8) {
                // Top Row: Name and Status Indicators
                HStack(alignment: .center) {
                    Text(device.name)
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(CRTheme.ink)
                        .lineLimit(1)
                        .truncationMode(.tail)
                    
                    Spacer(minLength: 8)
                    
                    // Status Indicators at top right
                    HStack(spacing: 6) {
                        if let net = store.peerNetworks.first(where: { $0.deviceId == device.id }) {
                            NetworkIndicatorPill(type: net.networkType)
                        }
                        if let battery = store.peerBatteries.first(where: { $0.deviceId == device.id }) {
                            BatteryIndicatorPill(level: battery.level, charging: battery.charging)
                        }
                    }
                }
                
                // Bottom Row: Connection Status and Actions
                HStack(alignment: .center) {
                    HStack(spacing: 6) {
                        Circle()
                            .fill(device.isConnected ? CRTheme.accentGreen : CRTheme.inkSoft)
                            .frame(width: 6, height: 6)
                        Text(device.isConnected ? "Connected" : "Offline")
                            .font(.system(size: 12, weight: .medium))
                            .foregroundStyle(device.isConnected ? CRTheme.accentGreen : CRTheme.inkSoft)
                    }
                    
                    Spacer(minLength: 8)
                    
                    // Action Buttons
                    HStack(spacing: 8) {
                        if device.isConnected {
                            ModernDeviceCardButton(
                                icon: "bell.badge.fill",
                                color: CRTheme.accentBlue,
                                help: "Ping",
                                action: {
                                    Task { try? await LinkAllIPCClient.shared.sendPushText("__LINKALL_PING__", targetDeviceId: device.id) }
                                    NSHapticFeedbackManager.defaultPerformer.perform(.generic, performanceTime: .default)
                                }
                            )
                            
                            ModernDeviceCardButton(
                                icon: "folder.fill",
                                color: CRTheme.brandElectric,
                                help: "Send files or folders",
                                action: onSendFiles
                            )
                            
                            ModernDeviceCardButton(
                                icon: "internaldrive.fill",
                                color: CRTheme.brandViolet,
                                help: "Remote File Explorer",
                                action: { showingRemoteExplorer = true }
                            )
                            
                            ModernDeviceCardButton(
                                icon: "gauge.with.dots.needle.bottom.50percent",
                                color: CRTheme.brandCyan,
                                help: "Speed Test",
                                action: { 
                                    store.startSpeedTest(deviceId: device.id)
                                    withAnimation { store.selectedSection = .transfers }
                                }
                            )
                            
                            ModernDeviceCardButton(
                                icon: "xmark",
                                color: CRTheme.accentRed,
                                help: "Disconnect",
                                action: { store.disconnect(device) }
                            )
                        } else {
                            ModernDeviceCardButton(
                                icon: "antenna.radiowaves.left.and.right",
                                color: CRTheme.brandElectric,
                                help: "Connect",
                                action: { store.connect(device) }
                            )
                        }
                    }
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .background(CRTheme.surfaceElevated)
        .crCard(cornerRadius: CRTheme.radiusLarge)
        .overlay {
            RoundedRectangle(cornerRadius: CRTheme.radiusLarge, style: .continuous)
                .strokeBorder(CRTheme.brandElectric.opacity(isHovered ? 0.3 : 0.0), lineWidth: 1)
        }
        .onHover { isHovered = $0 }
        .animation(.crSpring, value: isHovered)
        .sheet(isPresented: $showingRemoteExplorer) {
            RemoteExplorerView(store: store, device: device)
        }
    }
}

// MARK: - Untrusted Device Card (Premium UI)

struct UntrustedDeviceCard: View {
    let device: ManagedDevice
    @ObservedObject var store: LinkAllStore
    @Environment(\.colorScheme) var colorScheme
    @State private var isHovered = false
    @State private var pulse = false
    @Environment(\.scenePhase) var scenePhase

    var body: some View {
        HStack(spacing: 20) {
            // Animated Icon
            ZStack {
                Circle()
                    .fill(CRTheme.accentOrange.opacity(0.15))
                    .frame(width: 56, height: 56)
                    .scaleEffect(pulse ? 1.15 : 1.0)
                    .opacity(pulse ? 0.8 : 1.0)
                    .animation(pulse ? .easeInOut(duration: 1.5).repeatForever(autoreverses: true) : .default, value: pulse)
                
                Image(systemName: "exclamationmark.shield.fill")
                    .font(.system(size: 24))
                    .foregroundStyle(CRTheme.accentOrange)
            }
            
            .whileWindowVisible($pulse)
            VStack(alignment: .leading, spacing: 6) {
                Text(device.name)
                    .font(.system(size: 16, weight: .bold, design: .rounded))
                    .foregroundStyle(CRTheme.ink)
                
                HStack(spacing: 6) {
                    if device.pairingRequested {
                        Text("Wants to pair with this Mac")
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(CRTheme.inkSoft)
                    } else if device.outgoingPairingWaiting {
                        Text("Waiting for device to accept...")
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(CRTheme.inkSoft)
                    } else {
                        Text(device.pairingOutcomeText ?? "Ready to pair")
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(CRTheme.inkSoft)
                    }
                }
            }
            
            Spacer()
            
            // Actions & PIN
            HStack(spacing: 12) {
                if device.pairingRequested || device.outgoingPairingWaiting {
                    if let pin = device.pairingPin, !pin.isEmpty {
                        VStack(spacing: 2) {
                            Text("SECURITY PIN")
                                .font(.system(size: 9, weight: .bold, design: .rounded))
                                .tracking(0.5)
                                .foregroundStyle(CRTheme.inkSubtle)
                            Text(pin)
                                .font(.system(size: 20, weight: .heavy, design: .monospaced))
                                .foregroundStyle(CRTheme.ink)
                                .padding(.horizontal, 12)
                                .padding(.vertical, 6)
                                .background {
                                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                                        .fill(CRTheme.surfaceStrong)
                                        .overlay(
                                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                                .strokeBorder(CRTheme.stroke, lineWidth: 1)
                                        )
                                }
                        }
                        .padding(.trailing, 8)
                    }
                    
                    if device.pairingRequested {
                        Button {
                            store.respondToPairing(device, accepted: false)
                        } label: {
                            Image(systemName: "xmark")
                                .font(.system(size: 14, weight: .bold))
                                .foregroundStyle(CRTheme.inkSoft)
                                .frame(width: 36, height: 36)
                                .background(Circle().fill(CRTheme.surfaceStrong))
                        }
                        .buttonStyle(.plain)
                        .scaleEffect(isHovered ? 1.05 : 1.0)
                        
                        Button {
                            store.respondToPairing(device, accepted: true)
                        } label: {
                            Text("Accept")
                                .font(.system(size: 14, weight: .bold, design: .rounded))
                                .foregroundStyle(.white)
                                .padding(.horizontal, 16)
                                .frame(height: 36)
                                .background(CRTheme.accentGreen)
                                .clipShape(Capsule())
                        }
                        .buttonStyle(.plain)
                        .scaleEffect(isHovered ? 1.05 : 1.0)
                    } else {
                        Button("Cancel") { store.cancelPairingRequest(device) }
                            .buttonStyle(CRSecondaryButtonStyle())
                    }
                } else {
                    Button("Pair Device") { store.sendPairingRequest(device) }.buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.brandElectric))
                    Button("Remove") { store.forget(device) }.buttonStyle(CRSecondaryButtonStyle())
                }
            }
        }
        .padding(20)
        .background {
            RoundedRectangle(cornerRadius: CRTheme.radiusLarge, style: .continuous)
                .fill(CRTheme.surfaceElevated)
                .overlay(
                    RoundedRectangle(cornerRadius: CRTheme.radiusLarge, style: .continuous)
                        .strokeBorder(
                            isHovered ? CRTheme.accentOrange.opacity(0.5) : CRTheme.stroke,
                            lineWidth: isHovered ? 1.5 : 1.0
                        )
                )
                .shadow(color: isHovered ? CRTheme.accentOrange.opacity(0.15) : Color.black.opacity(0.05), radius: 12, y: 6)
        }
        .onHover { isHovered = $0 }
        .animation(.crSpring, value: isHovered)
    }
}

// MARK: - Battery Indicator Pill
struct BatteryIndicatorPill: View {
    let level: Int
    let charging: Bool

    private var iconName: String {
        if charging {
            return "battery.100.bolt"
        }
        if level <= 15 { return "battery.0" }
        if level <= 35 { return "battery.25" }
        if level <= 65 { return "battery.50" }
        if level <= 85 { return "battery.75" }
        return "battery.100"
    }

    private var tintColor: Color {
        if charging { return CRTheme.accentGreen }
        if level <= 20 { return Color.red }
        if level <= 50 { return CRTheme.accentOrange }
        return CRTheme.accentGreen
    }

    var body: some View {
        HStack(spacing: 3) {
            Image(systemName: iconName)
                .font(.system(size: 9.5, weight: .semibold))
            Text("\(level)%")
                .font(.system(size: 9.5, weight: .bold, design: .rounded))
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 2)
        .foregroundStyle(tintColor)
        .background(
            Capsule()
                .fill(tintColor.opacity(0.12))
        )
    }
}

// MARK: - Network Indicator Pill
struct NetworkIndicatorPill: View {
    let type: String

    private var iconName: String {
        switch type.lowercased() {
        case "wifi": return "wifi"
        case "cellular": return "antenna.radiowaves.left.and.right"
        default: return "network.slash"
        }
    }
    
    private var tintColor: Color {
        type.lowercased() == "offline" ? CRTheme.accentRed : CRTheme.accentBlue
    }
    
    var body: some View {
        HStack(spacing: 3) {
            Image(systemName: iconName)
                .font(.system(size: 9.5, weight: .semibold))
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 2)
        .foregroundStyle(tintColor)
        .background(
            Capsule()
                .fill(tintColor.opacity(0.12))
        )
    }
}

// MARK: - Magic Link Pairing Card
struct MagicLinkPairingCard: View {
    @ObservedObject var store: LinkAllStore
    @State private var hovered = false
    @State private var showingQR = false
    
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                ZStack {
                    Circle().fill(CRTheme.brandElectric.opacity(0.12)).frame(width: 44, height: 44)
                    Image(systemName: "qrcode.viewfinder").font(.system(size: 18, weight: .semibold)).foregroundStyle(CRTheme.brandElectric)
                }
                Spacer()
                Button {
                    showingQR = true
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "qrcode")
                        Text("Show QR")
                    }
                }
                .buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.brandElectric))
            }
            
            VStack(alignment: .leading, spacing: 4) {
                Text("Magic Link Pairing")
                    .font(.system(size: 15, weight: .bold, design: .rounded))
                    .foregroundStyle(CRTheme.ink)
                Text("Scan QR code with your phone camera or Link All mobile app to pair securely without typing IPs.")
                    .font(.system(size: 12.5))
                    .foregroundStyle(CRTheme.inkSoft)
                    .lineSpacing(2)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(18)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(CRTheme.surfaceStrong)
        .crCard(cornerRadius: CRTheme.radiusMedium, highlighted: hovered, accent: CRTheme.brandElectric)
        .scaleEffect(hovered ? 1.02 : 1.0)
        .onHover { hovered = $0 }
        .animation(.crSpring, value: hovered)
        .sheet(isPresented: $showingQR) {
            QRCodeSheetView(store: store)
        }
    }
}

// MARK: - QRCode Pairing Sheet View
struct QRCodePairingSheet: View {
    @ObservedObject var store: LinkAllStore
    @Environment(\.dismiss) var dismiss
    @State private var localIP: String = "127.0.0.1"

    var pairingURL: String {
        let name = store.settings?.deviceName ?? HostName()
        let port = store.settings?.port ?? 47823
        let fp = store.localFingerprint ?? ""
        return "linkall://pair?name=\(name.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? "")&ip=\(localIP)&port=\(port)&fingerprint=\(fp)"
    }

    var pinCode: String {
        let fp = store.localFingerprint ?? "123456"
        let digits = fp.filter { $0.isNumber }
        if digits.count >= 6 {
            return String(digits.prefix(6))
        }
        let sum = fp.utf8.reduce(0, { $0 + Int($1) })
        return String(format: "%06d", sum % 1000000)
    }

    var body: some View {
        VStack(spacing: 20) {
            HStack {
                Text("Magic Link Pair")
                    .font(.system(size: 16, weight: .bold))
                    .foregroundStyle(CRTheme.ink)
                Spacer()
                Button(action: { dismiss() }) {
                    Image(systemName: "xmark.circle.fill")
                        .font(.system(size: 18))
                        .foregroundStyle(CRTheme.inkSubtle)
                }
                .buttonStyle(PlainButtonStyle())
            }

            Text("Scan the QR code below from the Link All Android app to pair instantly, or use the 6-digit confirmation PIN.")
                .font(.system(size: 12))
                .foregroundStyle(CRTheme.inkSoft)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 10)

            if let qrImage = generateQRCode(from: pairingURL) {
                Image(nsImage: qrImage)
                    .interpolation(.none)
                    .resizable()
                    .scaledToFit()
                    .frame(width: 180, height: 180)
                    .padding(8)
                    .background(Color.white)
                    .cornerRadius(12)
                    .shadow(color: Color.black.opacity(0.1), radius: 8, x: 0, y: 4)
            } else {
                ProgressView()
                    .frame(width: 180, height: 180)
            }

            VStack(spacing: 4) {
                Text("PAIRING PIN")
                    .font(.system(size: 10, weight: .bold))
                    .foregroundStyle(CRTheme.inkSubtle)
                Text(pinCode)
                    .font(.system(size: 28, weight: .black, design: .monospaced))
                    .foregroundStyle(CRTheme.brandElectric)
                    .tracking(4)
            }
            .padding(.horizontal, 24)
            .padding(.vertical, 10)
            .background(CRTheme.surfaceElevated.opacity(0.6))
            .cornerRadius(12)
            .overlay {
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(CRTheme.stroke.opacity(0.42), lineWidth: 0.5)
            }

            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    Text("Device:")
                        .font(.system(size: 11, weight: .medium)).foregroundStyle(CRTheme.inkSoft)
                    Spacer()
                    Text(store.settings?.deviceName ?? HostName())
                        .font(.system(size: 11, weight: .semibold)).foregroundStyle(CRTheme.ink)
                }
                HStack {
                    Text("Network Address:")
                        .font(.system(size: 11, weight: .medium)).foregroundStyle(CRTheme.inkSoft)
                    Spacer()
                    Text("\(localIP):\(store.settings?.port ?? 47823)")
                        .font(.system(size: 11, weight: .semibold)).foregroundStyle(CRTheme.ink)
                }
            }
            .padding(.horizontal, 12)

            Button("Done") { dismiss() }
                .buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.brandElectric))
                .frame(width: 120)
        }
        .padding(24)
        .frame(width: 320)
        .background(CRTheme.canvasGradient)
        .onAppear {
            if let ip = getLocalIPAddress() {
                self.localIP = ip
            }
        }
        .onChange(of: store.connectedDevices.count) { _ in
            if !store.connectedDevices.isEmpty {
                dismiss()
            }
        }
    }

    private func generateQRCode(from string: String) -> NSImage? {
        let data = Data(string.utf8)
        let filter = CIFilter.qrCodeGenerator()
        filter.setValue(data, forKey: "inputMessage")
        filter.setValue("M", forKey: "inputCorrectionLevel")

        guard let outputImage = filter.outputImage else { return nil }
        
        let context = CIContext()
        guard let cgImage = context.createCGImage(outputImage, from: outputImage.extent) else { return nil }
        
        return NSImage(cgImage: cgImage, size: NSSize(width: 180, height: 180))
    }

    private func HostName() -> String {
        return Host.current().localizedName ?? "Mac"
    }
}

// MARK: - Robust IP Resolver
func getLocalIPAddress() -> String? {
    var address: String?
    var ifaddr: UnsafeMutablePointer<ifaddrs>?
    guard getifaddrs(&ifaddr) == 0 else { return nil }
    guard let firstAddr = ifaddr else { return nil }
    for ptr in sequence(first: firstAddr, next: { $0.pointee.ifa_next }) {
        let interface = ptr.pointee
        let addrFamily = interface.ifa_addr.pointee.sa_family
        if addrFamily == UInt8(AF_INET) {
            let name = String(cString: interface.ifa_name)
            if name.hasPrefix("lo") || name.hasPrefix("pdp_ip") { continue }
            var hostname = [CChar](repeating: 0, count: Int(NI_MAXHOST))
            getnameinfo(interface.ifa_addr, socklen_t(interface.ifa_addr.pointee.sa_len),
                        &hostname, socklen_t(hostname.count),
                        nil, socklen_t(0), NI_NUMERICHOST)
            let ip = String(cString: hostname)
            if !ip.hasPrefix("127.") && !ip.hasPrefix("169.254") {
                address = ip
                if name.hasPrefix("en") {
                    break
                }
            }
        }
    }
    freeifaddrs(ifaddr)
    return address
}

// MARK: - New Ecosystem UI Components

struct DeviceIdentityCard: View {
    let device: ManagedDevice
    var isOnline: Bool = true
    @State private var isHovered = false
    
    // Platform distinct identity
    private var platformBrand: Color {
        let name = device.name.lowercased()
        if name.contains("mac") { return CRTheme.ink } // Silver/Dark
        if name.contains("android") { return CRTheme.accentGreen } // Android green
        if name.contains("windows") { return CRTheme.accentBlue } // Windows blue
        return CRTheme.accentIndigo
    }
    
    var body: some View {
        HStack(spacing: 14) {
            ZStack(alignment: .bottomTrailing) {
                // Outer glow ring
                Circle()
                    .stroke(platformBrand.opacity(isOnline ? 0.3 : 0.0), lineWidth: 2)
                    .frame(width: 46, height: 46)
                    .overlay(
                        Circle()
                            .trim(from: 0.0, to: 0.8) // Mock 80% battery
                            .stroke(platformBrand, style: StrokeStyle(lineWidth: 2, lineCap: .round))
                            .rotationEffect(.degrees(-90))
                            .opacity(isOnline ? 1.0 : 0.0)
                    )
                
                DeviceAvatar(name: device.name, platform: nil, size: 38, color: platformBrand)
                
                if isOnline {
                    StatusDot(isOnline: true, size: 10)
                        .overlay(Circle().stroke(CRTheme.surfaceElevated, lineWidth: 2))
                        .offset(x: 2, y: 2)
                }
            }
            
            VStack(alignment: .leading, spacing: 3) {
                Text(device.name)
                    .font(.system(size: 14, weight: .bold, design: .rounded))
                    .foregroundStyle(isOnline ? CRTheme.ink : CRTheme.inkSoft)
                
                HStack(spacing: 6) {
                    OSIcon(device.os, size: 11)
                    
                    Text(isOnline ? "Active" : "Offline")
                        .font(.system(size: 11, weight: .semibold))
                }
                .foregroundStyle(isOnline ? platformBrand : CRTheme.inkSubtle)
            }
            .padding(.trailing, 10)
            
            if isOnline {
                // Live ping waves
                HStack(spacing: 2) {
                    ForEach(0..<3) { i in
                        Capsule()
                            .fill(platformBrand.opacity(0.4))
                            .frame(width: 2, height: isHovered ? CGFloat.random(in: 4...12) : 4)
                    }
                }
                .padding(.leading, 8)
                .animation(isHovered ? .easeInOut(duration: 0.5).repeatForever() : .default, value: isHovered)
            }
        }
        .padding(12)
        .background(
            RoundedRectangle(cornerRadius: 16)
                .fill(CRTheme.surfaceElevated.opacity(isHovered ? 1.0 : 0.6))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 16)
                .strokeBorder(isHovered ? platformBrand.opacity(0.3) : CRTheme.stroke.opacity(0.5), lineWidth: 1)
        )
        .opacity(isOnline ? 1.0 : 0.5)
        .shadow(color: isHovered ? platformBrand.opacity(0.15) : Color.clear, radius: 8, y: 4)
        .crHoverScale(scale: isOnline ? 1.04 : 1.0)
        .onHover { isHovered = $0 }
    }
}

struct PushClipboardFeaturedCard: View {
    let action: () -> Void
    @State private var isHovered = false
    @ObservedObject var store: LinkAllStore // Need store for avatars and preview
    
    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 0) {
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Push Clipboard")
                            .font(.system(size: 24, weight: .semibold, design: .rounded))
                            .foregroundStyle(CRTheme.ink)
                        Text("Sync copied text instantly")
                            .font(.system(size: 14, weight: .medium))
                            .foregroundStyle(CRTheme.inkSoft)
                    }
                    Spacer()
                    ZStack {
                        Circle().fill(CRTheme.accentBlue.opacity(0.15)).frame(width: 48, height: 48)
                        Image(systemName: "doc.on.clipboard.fill")
                            .font(.system(size: 22, weight: .medium))
                            .foregroundStyle(CRTheme.accentBlue)
                        
                        // Sync ripple
                        if isHovered {
                            Circle()
                                .stroke(CRTheme.brandElectric.opacity(0.5), lineWidth: 1)
                                .frame(width: 48, height: 48)
                                .scaleEffect(1.5)
                                .opacity(0.0)
                                .animation(isHovered ? .easeOut(duration: 1.0).repeatForever(autoreverses: false) : .default, value: isHovered)
                        }
                    }
                }
                
                Spacer(minLength: 24)
                
                VStack(alignment: .leading, spacing: 14) {
                    // Live Clipboard Preview
                    if let lastText = store.timeline.first(where: { $0.typeLabel == "Text" })?.fullText {
                        Text("\"\(lastText.prefix(40))...\"")
                            .font(.system(size: 13, weight: .medium, design: .monospaced))
                            .foregroundStyle(CRTheme.brandElectric)
                            .lineLimit(1)
                            .padding(.vertical, 8)
                            .padding(.horizontal, 12)
                            .background(CRTheme.brandElectric.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
                    } else {
                        Text("Clipboard ready...")
                            .font(.system(size: 13, weight: .medium, design: .monospaced))
                            .foregroundStyle(CRTheme.inkSubtle)
                            .padding(.vertical, 8)
                            .padding(.horizontal, 12)
                            .background(CRTheme.inkSubtle.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
                    }
                    
                    // Connected Targets & Live Status
                    HStack(alignment: .center) {
                        Text("Synced to:")
                            .font(.system(size: 12, weight: .semibold))
                            .foregroundStyle(CRTheme.inkSoft)
                        
                        if !store.connectedDevices.isEmpty {
                            HStack(spacing: -6) {
                                ForEach(Array(store.connectedDevices.prefix(3))) { d in
                                    DeviceAvatar(name: d.name, platform: nil, size: 20, color: CRTheme.accentBlue)
                                        .overlay(Circle().stroke(CRTheme.surfaceElevated, lineWidth: 2))
                                }
                            }
                        } else {
                            Text("No devices")
                                .font(.system(size: 12, weight: .medium))
                                .foregroundStyle(CRTheme.inkSubtle)
                        }
                        
                        Spacer()
                        
                        // Tiny waveform animation
                        HStack(spacing: 2) {
                            ForEach(0..<4) { i in
                                Capsule()
                                    .fill(CRTheme.brandElectric.opacity(0.6))
                                    .frame(width: 2, height: isHovered ? CGFloat.random(in: 4...12) : 4)
                            }
                        }
                        .animation(isHovered ? .easeInOut(duration: 0.3).repeatForever() : .default, value: isHovered)
                        
                        Text("Live now")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(CRTheme.accentBlue)
                    }
                }
            }
            .padding(24)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
            .crCard(cornerRadius: 24, highlighted: isHovered, accent: CRTheme.accentBlue)
            .crHoverScale(scale: 1.02)
        }
        .buttonStyle(.plain)
        .onHover { isHovered = $0 }
    }
}

struct SecondaryActionCard: View {
    let icon: String
    let title: String
    let color: Color
    let action: () -> Void
    @State private var isHovered = false
    
    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 12) {
                ZStack {
                    Circle().fill(color.opacity(0.15)).frame(width: 36, height: 36)
                    Image(systemName: icon)
                        .font(.system(size: 16, weight: .medium))
                        .foregroundStyle(color)
                }
                
                Text(title)
                    .font(.system(size: 15, weight: .semibold, design: .rounded))
                    .foregroundStyle(CRTheme.ink)
            }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .crCard(cornerRadius: 16, highlighted: isHovered, accent: color)
            .crHoverScale(scale: 1.03)
        }
        .buttonStyle(.plain)
        .onHover { isHovered = $0 }
    }
}

// MARK: - Legacy Action Cards

struct QuickActionCard: View {
    let icon: String
    let title: String
    let subtitle: String
    var isDragTarget: Bool = false
    let action: () -> Void
    
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 12) {
                ZStack {
                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                        .fill(CRTheme.brandElectric.opacity(isDragTarget ? 0.25 : 0.12))
                        .frame(width: 32, height: 32)
                    Image(systemName: icon)
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(CRTheme.brandElectric)
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(.system(size: 13, weight: .bold))
                        .foregroundStyle(Color.primary)
                    Text(subtitle)
                        .font(.system(size: 11))
                        .foregroundStyle(Color.secondary)
                        .lineLimit(1)
                }
                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
            .background(Color(NSColor.controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            .shadow(color: .black.opacity(0.05), radius: 2, y: 1)
            .overlay {
                if isDragTarget || hovered {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .strokeBorder(CRTheme.brandElectric.opacity(0.4), lineWidth: 1)
                }
            }
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .animation(.crFast, value: isDragTarget)
        .animation(.crFast, value: hovered)
    }
}

struct DeviceListRow: View {
    let device: ManagedDevice
    @ObservedObject var store: LinkAllStore
    @State private var hovered = false
    
    var body: some View {
        HStack(spacing: 16) {
            // Leading: Device type icon
            OSIcon(device.os, size: 18)
                .foregroundStyle(CRTheme.brandElectric)
                .frame(width: 24, alignment: .center)
            
            // Center: Device Name and status
            VStack(alignment: .leading, spacing: 2) {
                Text(device.name)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(Color.primary)
                HStack {
                    Text(device.lastSync?.relativeTimeString() ?? "Synced just now")
                        .font(.system(size: 11.5))
                        .foregroundStyle(Color.secondary)

                }
            }
            Spacer()
            
            // Trailing: Icon-only push button
            Button {
                store.sendCurrentClipboard(to: device)
            } label: {
                Image(systemName: "arrow.up.doc.fill")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(hovered ? CRTheme.brandElectric : Color.secondary)
                    .frame(width: 32, height: 32)
                    .background(Color(NSColor.controlBackgroundColor), in: Circle())
                    .shadow(color: .black.opacity(0.05), radius: 1, y: 1)
            }
            .buttonStyle(.plain)
            .onHover { hovered = $0 }
            .help("Push clipboard to \(device.name)")
        }
        .padding(.horizontal, 28)
        .padding(.vertical, 14)
    }
}

// MARK: - Continuity Orb

struct FloatingContinuityOrb: View {
    @State private var isAnimating = false
    @Environment(\.scenePhase) var scenePhase
    var activeCount: Int
    
    var body: some View {
        ZStack {
            // Core orb
            Circle()
                .fill(CRTheme.brandCyan.opacity(0.15))
                .frame(width: 60, height: 60)
                .blur(radius: 8)
                .scaleEffect(isAnimating ? 1.1 : 1.0)
                .animation(isAnimating ? .easeInOut(duration: 4.0).repeatForever(autoreverses: true) : .default, value: isAnimating)
            
            Circle()
                .stroke(CRTheme.brandCyan.opacity(0.4), lineWidth: 1)
                .frame(width: 60, height: 60)
            
            Image(systemName: "network")
                .font(.system(size: 24, weight: .light))
                .foregroundStyle(CRTheme.brandCyan)
            
            // Orbiting particles
            if activeCount > 0 {
                ForEach(0..<activeCount, id: \.self) { i in
                    Circle()
                        .fill(CRTheme.brandElectric)
                        .frame(width: 6, height: 6)
                        .offset(x: 40)
                        .rotationEffect(.degrees(Double(i) * (360.0 / Double(activeCount))))
                        .rotationEffect(.degrees(isAnimating ? 360 : 0))
                        .animation(isAnimating ? .linear(duration: 8).repeatForever(autoreverses: false) : .default, value: isAnimating)
                }
            }
        }
        .onAppear {
            if scenePhase == .active { isAnimating = true }
        }
        .onChange(of: scenePhase) { phase in
            isAnimating = phase == .active
        }
    }
}

// MARK: - QR Code Sheet

struct QRCodeSheetView: View {
    @ObservedObject var store: LinkAllStore
    @Environment(\.dismiss) var dismiss
    @State private var qrImage: NSImage?
    @State private var errorMessage: String?
    @State private var isLoading = true
    
    var body: some View {
        VStack(spacing: 24) {
            VStack(spacing: 8) {
                Text("Pair Device")
                    .font(.system(size: 20, weight: .bold))
                
                Text("Scan this QR code from the Link All Android app to pair securely.")
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
                    .font(.system(size: 13))
            }
            
            ZStack {
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .fill(CRTheme.surfaceElevated)
                    .shadow(color: .black.opacity(0.1), radius: 10, y: 5)
                    .frame(width: 240, height: 240)
                
                if isLoading {
                    ProgressView()
                } else if let img = qrImage {
                    Image(nsImage: img)
                        .interpolation(.none)
                        .resizable()
                        .scaledToFit()
                        .frame(width: 200, height: 200)
                        .padding(20)
                } else if let error = errorMessage {
                    Text(error)
                        .foregroundStyle(.red)
                        .multilineTextAlignment(.center)
                        .padding()
                }
            }
            
            Button("Done") {
                dismiss()
            }
            .buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.brandElectric))
            .frame(width: 120)
        }
        .padding(32)
        .frame(width: 360)
        .task {
            await loadQR()
        }
        .onChange(of: store.devices.count) { _ in
            for dev in store.devices {
                if dev.isConnected && dev.trustState != .trusted {
                    store.trust(dev)
                }
            }
            if !store.connectedDevices.isEmpty {
                dismiss()
            }
        }
    }
    
    private func loadQR() async {
        do {
            await store.refresh()
            let token = try await store.generateQrToken()
            let id = store.localDeviceId ?? ""
            let name = store.localDeviceName ?? Host.current().localizedName ?? "Mac"
            let ip = getLocalIPAddress() ?? "127.0.0.1"
            let port = "\(store.settings?.port ?? 47823)"
            let fp = store.localFingerprint ?? ""
            
            var components = URLComponents()
            components.scheme = "linkall"
            components.host = "pair"
            components.queryItems = [
                URLQueryItem(name: "id", value: id),
                URLQueryItem(name: "name", value: name),
                URLQueryItem(name: "token", value: token),
                URLQueryItem(name: "ip", value: ip),
                URLQueryItem(name: "port", value: port),
                URLQueryItem(name: "fingerprint", value: fp)
            ]
            
            guard let urlString = components.url?.absoluteString else {
                errorMessage = "Failed to construct pairing URL."
                isLoading = false
                return
            }
            
            let context = CIContext()
            let filter = CIFilter.qrCodeGenerator()
            filter.message = Data(urlString.utf8)
            filter.correctionLevel = "H"
            
            if let outputImage = filter.outputImage {
                let scaledImage = outputImage.transformed(by: CGAffineTransform(scaleX: 10, y: 10))
                if let cgImage = context.createCGImage(scaledImage, from: scaledImage.extent) {
                    qrImage = NSImage(cgImage: cgImage, size: .zero)
                } else {
                    errorMessage = "Failed to render QR Code."
                }
            } else {
                errorMessage = "Failed to generate QR Code."
            }
        } catch {
            errorMessage = error.localizedDescription
        }
        isLoading = false
    }
}
