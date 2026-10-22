import SwiftUI
import AppKit

/// The main window: sections and devices in the sidebar, the chosen one beside it.
/// Devices live on Home, and each opens its own page there.
struct MainWindowView: View {
    @ObservedObject var store: LinkAllStore
    @StateObject private var nowPlaying = NowPlayingModel()

    var body: some View {
        HStack(spacing: 0) {
            MainSidebar(store: store)
                .frame(width: 216)
            Divider()
            content
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .background(CRTheme.surface.ignoresSafeArea())
        .ignoresSafeArea(.all, edges: .top)
        .frame(minWidth: 900, idealWidth: 1200, minHeight: 600, idealHeight: 792)
        .environmentObject(nowPlaying)
        // Asks what devices play only while the window is on screen.
        .background(WindowVisibilityReader { visible in
            if visible { nowPlaying.start() } else { nowPlaying.stop() }
        })
        .sheet(isPresented: $store.showQrCodeSheet) {
            QRCodeSheetView(store: store)
        }
    }

    @ViewBuilder private var content: some View {
        Group {
            switch store.selectedSection {
            case .home:      HomeSection(store: store)
            case .clipboard: TimelineSectionView(store: store, density: .comfortable)
            case .files:     FilesView(store: store)
            case .share:     EdgeShareView(store: store)
            case .activity:  ActivityFeedView().environmentObject(store)
            case .settings:  PreferencesView(store: store)
            }
        }
        .id(store.selectedSection)
        .transition(.opacity)
        .animation(.crFast, value: store.selectedSection)
    }
}

/// Home, or one device's page over it. Home stays underneath, so Back returns
/// to where it was scrolled.
private struct HomeSection: View {
    @ObservedObject var store: LinkAllStore

    var body: some View {
        let device = store.openDeviceId.flatMap { id in store.devices.first { $0.id == id } }
        ZStack {
            HomeView(store: store)
                .opacity(device == nil ? 1 : 0)
                .disabled(device != nil)
                .accessibilityHidden(device != nil)
            if let device {
                DeviceDetailView(store: store, device: device)
                    .background(CRTheme.surface)
                    .transition(.move(edge: .trailing).combined(with: .opacity))
            }
        }
        .animation(.crSpring, value: device?.id)
        .onChange(of: store.peers) { _ in
            // Forgotten meanwhile: back to Home.
            if let id = store.openDeviceId, !store.peers.contains(where: { $0.id == id }) {
                store.openDeviceId = nil
            }
        }
    }
}

// MARK: - Sidebar

private struct MainSidebar: View {
    @ObservedObject var store: LinkAllStore

    /// Paired devices, connected ones first, and any asking to pair.
    private var devices: [ManagedDevice] {
        store.devices
            .filter { $0.trustState == .trusted || $0.pairingRequested || $0.outgoingPairingWaiting }
            .sorted { a, b in
                if a.isLinked != b.isLinked { return a.isLinked }
                return a.name.localizedCaseInsensitiveCompare(b.name) == .orderedAscending
            }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ScrollView(.vertical, showsIndicators: false) {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach([DashboardSection.home, .clipboard, .files, .share, .activity]) { section in
                        SidebarSectionRow(section: section, store: store)
                    }

                    let list = devices
                    if !list.isEmpty {
                        Text("Devices")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(CRTheme.inkSubtle)
                            .padding(.horizontal, 10)
                            .padding(.top, 20)
                            .padding(.bottom, 4)
                            .accessibilityAddTraits(.isHeader)
                        ForEach(list) { device in
                            SidebarDeviceRow(device: device, store: store)
                        }
                    }
                }
                .padding(.horizontal, 10)
                // Clears the window's close, minimise and zoom buttons.
                .padding(.top, 50)
                .padding(.bottom, 12)
            }
            Divider().padding(.horizontal, 10)
            SidebarSectionRow(section: .settings, store: store)
                .padding(10)
        }
        .background(CRTheme.surface)
    }
}

/// One section in the sidebar, with its keyboard shortcut.
private struct SidebarSectionRow: View {
    let section: DashboardSection
    @ObservedObject var store: LinkAllStore

    private var selected: Bool { store.selectedSection == section }

    /// Something waiting there: transfers in flight, clipboard items to apply.
    private var badge: Int {
        switch section {
        case .files: return store.batchedTransfers.count
        case .clipboard: return store.pendingClipboardCount
        default: return 0
        }
    }

    var body: some View {
        Button {
            withAnimation(.crFast) {
                store.selectedSection = section
                // Choosing Home again returns to Home from a device's page.
                if section == .home { store.openDeviceId = nil }
            }
        } label: {
            HStack(spacing: 9) {
                Image(systemName: selected ? section.selectedIcon : section.icon)
                    .font(.system(size: 13.5, weight: .medium))
                    .frame(width: 20)
                Text(section.title)
                    .font(.system(size: 13, weight: selected ? .semibold : .regular))
                Spacer(minLength: 4)
                if badge > 0 {
                    Text("\(min(badge, 99))")
                        .font(.system(size: 11, weight: .semibold).monospacedDigit())
                        .foregroundStyle(selected ? CRTheme.brandElectric : CRTheme.inkSoft)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1)
                        .background(Capsule().fill(selected ? Color.white : CRTheme.rowSelected))
                }
            }
        }
        .buttonStyle(SidebarRowStyle(selected: selected))
        .keyboardShortcut(section.shortcut, modifiers: .command)
        .accessibilityAddTraits(selected ? .isSelected : [])
        .accessibilityValue(badge > 0 ? "\(badge)" : "")
    }
}

/// A device in the sidebar: its OS, name and whether it's linked. Opens its page.
private struct SidebarDeviceRow: View {
    let device: ManagedDevice
    @ObservedObject var store: LinkAllStore

    private var selected: Bool { store.selectedSection == .home && store.openDeviceId == device.id }

    var body: some View {
        let status = DeviceStatus(device, store: store)
        Button {
            withAnimation(.crSpring) { store.openDevice(device.id) }
        } label: {
            HStack(spacing: 9) {
                OSIcon(device.os, size: 13)
                    .frame(width: 20)
                Text(device.name)
                    .font(.system(size: 13, weight: selected ? .semibold : .regular))
                    .lineLimit(1)
                    .truncationMode(.tail)
                Spacer(minLength: 4)
                StateDot(style: status.dot, size: 7)
                    .opacity(selected && status.dot == .idle ? 0.7 : 1)
            }
        }
        .buttonStyle(SidebarRowStyle(selected: selected))
        .help(status.text)
        .accessibilityLabel("\(device.name), \(status.text)")
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// The selected row is solid brand blue, as in a Mac sidebar; others fill faintly on hover.
private struct SidebarRowStyle: ButtonStyle {
    let selected: Bool

    func makeBody(configuration: Configuration) -> some View {
        RowBody(configuration: configuration, selected: selected)
    }

    private struct RowBody: View {
        let configuration: Configuration
        let selected: Bool
        @State private var hovered = false

        var body: some View {
            configuration.label
                .foregroundStyle(selected ? Color.white : CRTheme.ink)
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
                .frame(maxWidth: .infinity, minHeight: 30, alignment: .leading)
                .background(
                    RoundedRectangle(cornerRadius: 7, style: .continuous)
                        .fill(selected
                              ? CRTheme.brandElectric
                              : (configuration.isPressed ? CRTheme.rowSelected : (hovered ? CRTheme.rowHover : Color.clear)))
                )
                .contentShape(Rectangle())
                .onHover { hovered = $0 }
        }
    }
}
