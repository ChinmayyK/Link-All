import SwiftUI
import AppKit

/// One device and everything this Mac can do with it, on one page: send to
/// it, play its music, browse its files, use its phone features, make it
/// ring, and look after the link. Opened from its card on Home.
struct DeviceDetailView: View {
    @ObservedObject var store: LinkAllStore
    let device: ManagedDevice
    @EnvironmentObject private var nowPlaying: NowPlayingModel
    @State private var browsing: ManagedDevice?
    @State private var choosingFiles = false
    @State private var confirmForget = false
    @State private var renaming = false
    @State private var newName = ""

    var body: some View {
        let status = DeviceStatus(device, store: store)
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: 0) {
                Button { store.openDeviceId = nil } label: {
                    Label("Home", systemImage: "chevron.left")
                }
                .buttonStyle(LinkButtonStyle())
                .keyboardShortcut("[", modifiers: .command)
                .help("Back to Home (Esc or ⌘[)")

                // Who it is and how it's linked.
                HStack(spacing: 16) {
                    DeviceWell(os: device.os, tone: device.isLinked ? .blue : .neutral, size: 64)
                    VStack(alignment: .leading, spacing: 4) {
                        Text(device.name)
                            .font(.system(size: 26, weight: .bold))
                            .foregroundStyle(CRTheme.ink)
                            .lineLimit(2)
                            .accessibilityAddTraits(.isHeader)
                        HStack(spacing: 6) {
                            StateDot(style: status.dot)
                            Text([status.text, device.os == .unknown ? nil : device.os.label]
                                .compactMap { $0 }.joined(separator: " · "))
                                .font(.system(size: 13))
                                .foregroundStyle(CRTheme.inkSoft)
                        }
                    }
                }
                .padding(.top, 16)

                statePanel
                if device.isLinked { actions }
                about
                if device.trustState == .trusted { manage }
            }
            .padding(.horizontal, 32)
            .padding(.top, 40)
            .padding(.bottom, 36)
            .frame(maxWidth: 940, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .sheet(item: $browsing) { device in
            RemoteExplorerView(store: store, device: device)
        }
        // Esc goes back to Home, as ⌘[ does.
        .onExitCommand { store.openDeviceId = nil }
        .fileImporter(isPresented: $choosingFiles, allowedContentTypes: [.item, .folder], allowsMultipleSelection: true) { result in
            if case .success(let urls) = result { store.send(urls, to: device) }
        }
        .confirmationDialog("Forget \(device.name)?", isPresented: $confirmForget) {
            Button("Forget", role: .destructive) {
                store.forget(device)
                store.openDeviceId = nil
            }
        } message: {
            Text("This Mac stops trusting it. To use it again, pair again and compare the code.")
        }
        .alert("Rename \(device.name)", isPresented: $renaming) {
            TextField("Name", text: $newName)
            Button("Rename") {
                let name = newName.trimmingCharacters(in: .whitespacesAndNewlines)
                if !name.isEmpty { store.rename(device, to: name) }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Shown on this Mac only.")
        }
    }

    /// Pairing, not paired yet, or offline: what to do first.
    @ViewBuilder private var statePanel: some View {
        if device.pairingRequested && device.trustState != .trusted {
            PairingRequestPanel(device: device, store: store).padding(.top, 24)
        } else if device.outgoingPairingWaiting {
            OutgoingPairingPanel(device: device, store: store).padding(.top, 24)
        } else if device.trustState != .trusted {
            notice(title: "Not paired yet",
                   body: "Pair to share your clipboard and files with \(device.name). Both devices show a code to compare first.",
                   button: "Pair with \(device.name)") { store.sendPairingRequest(device) }
        } else if !device.isConnected {
            notice(title: "\(device.name) isn't connected",
                   body: "It reconnects on its own when both are on the same Wi-Fi or hotspot and Link All is open on it.",
                   button: "Connect now") { store.connect(device) }
        }
    }

    private func notice(title: String, body: String, button: String, action: @escaping () -> Void) -> some View {
        Panel {
            VStack(alignment: .leading, spacing: 6) {
                Text(title).font(.system(size: 14, weight: .semibold)).foregroundStyle(CRTheme.ink)
                Text(body).font(.system(size: 13)).foregroundStyle(CRTheme.inkSoft)
                Button(button, action: action)
                    .buttonStyle(QuickActionStyle(primary: true))
                    .padding(.top, 8)
            }
            .padding(16)
        }
        .padding(.top, 24)
    }

    @ViewBuilder private var actions: some View {
        if let item = nowPlaying.item(for: device.id) {
            NowPlayingPanel(item: item, art: nowPlaying.art[device.id]) { action, value in
                nowPlaying.send(item, action, value: value)
            }
            .padding(.top, 24)
        }

        GroupLabel("Share")
        TileGrid(tiles: [
            TileSpec(icon: "arrow.up.doc", label: "Send files or folders") { choosingFiles = true },
            TileSpec(icon: "doc.on.clipboard", label: "Send clipboard",
                     help: "Send what you copied to \(device.name)") { store.sendCurrentClipboard(to: device) },
        ])

        GroupLabel("Files")
        TileGrid(tiles: [
            TileSpec(icon: "folder", label: "Browse files", tone: .green) { browsing = device },
        ])

        if device.isPhone {
            GroupLabel("Phone")
            TileGrid(tiles: [
                TileSpec(icon: "phone", label: "Call a number", tone: .violet) { store.callFromPhone(on: device) },
                TileSpec(icon: "doc.viewfinder", label: "Scan a document", tone: .violet) {
                    store.captureFromPhone(mode: "document", on: device)
                },
                TileSpec(icon: "camera", label: "Take a photo", tone: .violet) {
                    store.captureFromPhone(mode: "photo", on: device)
                },
            ])

            GroupLabel("Find")
            TileGrid(tiles: [
                TileSpec(icon: "bell.and.waves.left.and.right", label: "Ring", tone: .amber,
                         help: "Rings \(device.name), even on silent") { store.ring(device) },
                TileSpec(icon: "bell.slash", label: "Stop ringing", tone: .amber) { store.stopRinging(device) },
            ])
        }
    }

    @ViewBuilder private var about: some View {
        if device.trustState == .trusted || device.isConnected {
            GroupLabel("About this device")
            Panel {
                InfoRow(label: "Status", value: DeviceStatus(device, store: store).text)
                if let platform = device.platform {
                    RowDivider(inset: 14)
                    InfoRow(label: "System", value: platform)
                }
                if let version = device.appVersion {
                    RowDivider(inset: 14)
                    InfoRow(label: "Link All", value: version)
                }
                if let battery = store.peerBatteries.first(where: { $0.deviceId == device.id }) {
                    RowDivider(inset: 14)
                    InfoRow(label: "Battery", value: battery.charging ? "\(battery.level)%, charging" : "\(battery.level)%")
                }
                if let ip = device.ip {
                    RowDivider(inset: 14)
                    InfoRow(label: "Address", value: ip, monospaced: true)
                }
                if let sync = device.lastSync {
                    RowDivider(inset: 14)
                    InfoRow(label: "Last synced", value: sync.relativeTimeString())
                }
                if device.isLinked {
                    RowDivider(inset: 14)
                    let test = store.activeSpeedTests.first { $0.id == device.id }
                    PanelRow(icon: "speedometer", tone: .teal, title: "Test connection speed",
                             detail: test.map { "\($0.phase) · \($0.speedMbpsString)" } ?? "Takes about ten seconds",
                             action: { store.startSpeedTest(deviceId: device.id) })
                }
            }
        }
    }

    private var manage: some View {
        VStack(alignment: .leading, spacing: 0) {
            GroupLabel("Manage")
            Panel {
                PanelRow(icon: "pencil", title: "Rename", detail: "Shown on this Mac only",
                         action: { newName = device.name; renaming = true })
                RowDivider()
                if device.isConnected {
                    PanelRow(icon: "link.badge.plus", title: "Disconnect",
                             detail: "Reconnect any time from this page", action: { store.disconnect(device) })
                } else {
                    PanelRow(icon: "link", title: "Connect", action: { store.connect(device) })
                }
                RowDivider()
                PanelRow(icon: "trash", title: "Forget \(device.name)", detail: "Unpairs it here. Pair again to use it",
                         destructive: true, action: { confirmForget = true })
            }
        }
    }
}
