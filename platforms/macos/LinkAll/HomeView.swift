import SwiftUI
import AppKit

/// Home: this Mac, then each device with what it plays and the things done most
/// with it, then the clipboard, what's moving and what happened lately.
/// A device's card opens its page, where everything else is.
struct HomeView: View {
    @ObservedObject var store: LinkAllStore
    @ObservedObject private var updates = UpdateChecker.shared
    @State private var browsing: ManagedDevice?
    @State private var choosingFiles = false
    @State private var sendTarget: ManagedDevice?

    var body: some View {
        let all = store.devices
        let linked = store.connectedDevices
        let others = otherDevices(all)
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: 0) {
                ThisMacHeader(store: store)
                attention(all)

                if linked.isEmpty && others.isEmpty {
                    HomeEmptyState(store: store)
                        .padding(.top, 24)
                } else {
                    if !linked.isEmpty {
                        SectionTitle(title: "Your devices")
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 300), spacing: 14)], alignment: .leading, spacing: 14) {
                            ForEach(linked) { device in
                                DeviceHomeCard(
                                    device: device,
                                    store: store,
                                    onSend: { sendTarget = device; choosingFiles = true },
                                    onBrowse: { browsing = device }
                                )
                            }
                        }
                    }
                    if !others.isEmpty {
                        SectionTitle(title: linked.isEmpty ? "Your devices" : "Other devices")
                        if linked.isEmpty && others.contains(where: { $0.trustState == .trusted }) {
                            Text("Paired devices reconnect on their own when they're on the same Wi-Fi or hotspot.")
                                .font(.system(size: 12.5))
                                .foregroundStyle(CRTheme.inkSoft)
                                .padding(.bottom, 10)
                        }
                        Panel {
                            ForEach(Array(others.enumerated()), id: \.element.id) { index, device in
                                if index > 0 { RowDivider(inset: 56) }
                                CompactDeviceRow(device: device, store: store)
                            }
                        }
                    }
                }

                HomeClipboardCard(store: store, linked: linked)
                transfers
                recent
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
        .fileImporter(isPresented: $choosingFiles, allowedContentTypes: [.item, .folder], allowsMultipleSelection: true) { result in
            if case .success(let urls) = result, let device = sendTarget {
                store.send(urls, to: device)
            }
            sendTarget = nil
        }
    }

    /// Paired devices that aren't linked, and nearby ones to pair: those asking
    /// first, then paired ones seen most recently, then the rest.
    private func otherDevices(_ all: [ManagedDevice]) -> [ManagedDevice] {
        let nearby = Set(store.pendingDevices.map(\.id))
        func rank(_ d: ManagedDevice) -> Int {
            if d.pairingRequested && d.trustState != .trusted { return 0 }
            return d.trustState == .trusted ? 1 : 2
        }
        return all
            .filter { !$0.isLinked && ($0.trustState == .trusted || nearby.contains($0.id)) }
            .sorted { a, b in
                if rank(a) != rank(b) { return rank(a) < rank(b) }
                let seenA = a.lastSeen ?? .distantPast, seenB = b.lastSeen ?? .distantPast
                if seenA != seenB { return seenA > seenB }
                return a.name.localizedCaseInsensitiveCompare(b.name) == .orderedAscending
            }
    }

    /// A pairing request first: someone is waiting on it. Then an update, and
    /// what stops sync right now.
    @ViewBuilder private func attention(_ all: [ManagedDevice]) -> some View {
        let asking = all.first { $0.pairingRequested && $0.trustState != .trusted }
        let waiting = all.first { $0.outgoingPairingWaiting }
        let issues = store.healthIssues
        if asking != nil || waiting != nil || updates.available != nil || !issues.isEmpty {
            VStack(alignment: .leading, spacing: 12) {
                if let asking {
                    PairingRequestPanel(device: asking, store: store)
                } else if let waiting {
                    OutgoingPairingPanel(device: waiting, store: store)
                }
                if let update = updates.available {
                    UpdateBanner(update: update)
                }
                if let issue = issues.first {
                    HealthBanner(issue: issue, store: store)
                    if issues.count > 1 {
                        Button("\(issues.count - 1) more to fix in Settings") {
                            store.selectedSection = .settings
                        }
                        .buttonStyle(LinkButtonStyle())
                    }
                }
            }
            .padding(.top, 22)
        }
    }

    @ViewBuilder private var transfers: some View {
        let moving = store.batchedTransfers
        if !moving.isEmpty {
            SectionTitle(title: "Transferring", meta: "\(moving.count)", seeAll: { store.selectedSection = .files })
            Panel {
                ForEach(Array(moving.prefix(3).enumerated()), id: \.element.id) { index, transfer in
                    if index > 0 { RowDivider(inset: 14) }
                    TransferProgressRow(transfer: transfer, store: store)
                }
            }
        }
    }

    @ViewBuilder private var recent: some View {
        let items = Array(store.timeline.filter { $0.typeLabel != "Connection" }.prefix(3))
        SectionTitle(title: "Recent", seeAll: items.isEmpty ? nil : { store.selectedSection = .activity })
        if items.isEmpty {
            EmptyNote(icon: "clock", text: "Copy something on either device, or send a file. It shows up here.")
        } else {
            Panel {
                ForEach(Array(items.enumerated()), id: \.element.id) { index, item in
                    if index > 0 { RowDivider() }
                    RecentRow(item: item, store: store)
                }
            }
        }
    }
}

// MARK: - This Mac

/// Who this Mac is, whether it syncs, and where to reach it.
private struct ThisMacHeader: View {
    @ObservedObject var store: LinkAllStore

    private var status: (text: String, dot: StateDot.Style) {
        if let problem = store.serviceProblem {
            return (problem, store.serviceStarting ? .idle : .warn)
        }
        let syncOn = store.settings?.syncEnabled ?? true
        let parts = [syncOn ? "Sync on" : "Sync paused", getLocalIPAddress()].compactMap { $0 }
        return (parts.joined(separator: " · "), syncOn ? .live : .warn)
    }

    var body: some View {
        let status = status
        HStack(spacing: 14) {
            IconWell(systemName: "laptopcomputer", tone: .blue, size: 46)
            VStack(alignment: .leading, spacing: 3) {
                Text(store.localDeviceName ?? "This Mac")
                    .font(.system(size: 22, weight: .bold))
                    .foregroundStyle(CRTheme.ink)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .accessibilityAddTraits(.isHeader)
                HStack(spacing: 6) {
                    StateDot(style: status.dot)
                    Text(status.text)
                        .font(.system(size: 12.5))
                        .foregroundStyle(CRTheme.inkSoft)
                        .lineLimit(1)
                        .textSelection(.enabled)
                }
                .accessibilityElement(children: .combine)
            }
            Spacer(minLength: 12)
            HStack(spacing: 8) {
                if store.isRunning, let settings = store.settings {
                    Button(settings.syncEnabled ? "Pause sync" : "Resume sync") { store.toggleSync() }
                        .help(settings.syncEnabled ? "Stop sending and receiving until you resume" : "Sync with your devices again")
                }
                Button { store.openNotes() } label: {
                    Image(systemName: "note.text")
                }
                .help("Notes")
                .accessibilityLabel("Notes")
                Menu {
                    Button("Show pairing QR code") { store.showQrCodeSheet = true }
                    Button("Connect by IP address…") { store.promptConnectByAddress() }
                    Divider()
                    Button("Receive from a guest…") { store.receiveFromGuest() }
                    Button("Scan for devices") { store.scanForDevices() }
                } label: {
                    Image(systemName: "plus")
                }
                .menuStyle(.button)
                .fixedSize()
                .help("Add a device")
                .accessibilityLabel("Add a device")
            }
            .buttonStyle(.bordered)
            .controlSize(.large)
        }
    }
}

/// Nothing paired or nearby yet: how to link the first device.
private struct HomeEmptyState: View {
    @ObservedObject var store: LinkAllStore

    var body: some View {
        Panel {
            VStack(alignment: .leading, spacing: 0) {
                IconWell(systemName: "laptopcomputer.and.iphone", tone: .blue, size: 44)
                Text("Link this Mac to your phone or PC")
                    .font(.system(size: 20, weight: .bold))
                    .foregroundStyle(CRTheme.ink)
                    .padding(.top, 14)
                Text("Open Link All on the other device, on the same Wi-Fi or hotspot. It shows up here, ready to pair. Nothing goes through a cloud.")
                    .font(.system(size: 13))
                    .foregroundStyle(CRTheme.inkSoft)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.top, 6)
                HStack(spacing: 10) {
                    Button("Show pairing QR code") { store.showQrCodeSheet = true }
                        .buttonStyle(.borderedProminent)
                        .tint(CRTheme.brandElectric)
                    Button("Connect by IP address…") { store.promptConnectByAddress() }
                        .buttonStyle(.bordered)
                }
                .controlSize(.large)
                .padding(.top, 18)
                // Someone new may not have Link All on their phone or PC yet.
                Link(destination: URL(string: "https://linkall.chinmaykudalkar.com/download")!) {
                    Label("Get Link All for your phone or PC", systemImage: "arrow.up.right")
                        .font(.system(size: 12.5, weight: .medium))
                }
                .padding(.top, 14)
            }
            .padding(22)
        }
    }
}

// MARK: - Devices

/// A linked device: who it is, what it plays, and three things done most with it.
/// The card opens the device's page; files dropped on it are sent to it.
private struct DeviceHomeCard: View {
    let device: ManagedDevice
    @ObservedObject var store: LinkAllStore
    @EnvironmentObject private var nowPlaying: NowPlayingModel
    let onSend: () -> Void
    let onBrowse: () -> Void
    @State private var hovered = false

    private var statusLine: String {
        let status = DeviceStatus(device, store: store)
        var parts = [status.text]
        if device.os != .unknown { parts.append(device.os.label) }
        if let battery = store.peerBatteries.first(where: { $0.deviceId == device.id }) {
            parts.append(battery.charging ? "\(battery.level)%, charging" : "\(battery.level)%")
        }
        return parts.joined(separator: " · ")
    }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 16, style: .continuous)
        VStack(alignment: .leading, spacing: 12) {
            Button(action: open) {
                HStack(spacing: 12) {
                    DeviceWell(os: device.os, tone: .blue, size: 42)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(device.name)
                            .font(.system(size: 15, weight: .semibold))
                            .foregroundStyle(CRTheme.ink)
                            .lineLimit(1)
                        HStack(spacing: 6) {
                            StateDot(style: DeviceStatus(device, store: store).dot)
                            Text(statusLine)
                                .font(.system(size: 12))
                                .foregroundStyle(CRTheme.inkSoft)
                                .lineLimit(1)
                        }
                    }
                    Spacer(minLength: 8)
                    Image(systemName: "chevron.right")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(hovered ? CRTheme.ink : CRTheme.inkSubtle)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("\(device.name), \(statusLine)")
            .accessibilityHint("Opens its page")

            if let item = nowPlaying.item(for: device.id) {
                NowPlayingMini(item: item, art: nowPlaying.art[device.id]) { action, value in
                    nowPlaying.send(item, action, value: value)
                }
            }

            HStack(spacing: 8) {
                Button(action: onSend) {
                    Label("Send", systemImage: "arrow.up.doc")
                }
                .buttonStyle(QuickActionStyle(primary: true))
                .help("Send files or folders to \(device.name)")

                Button { store.sendCurrentClipboard(to: device) } label: {
                    Label { Text("Clipboard") } icon: { Image(systemName: "doc.on.clipboard").foregroundStyle(CRTheme.brandInk) }
                }
                .buttonStyle(QuickActionStyle())
                .help("Send what you copied to \(device.name)")

                Button(action: onBrowse) {
                    Label { Text("Browse") } icon: { Image(systemName: "folder").foregroundStyle(CRTheme.brandInk) }
                }
                .buttonStyle(QuickActionStyle())
                .help("Browse the files on \(device.name)")
            }
        }
        .padding(14)
        .background(shape.fill(CRTheme.surfaceStrong))
        .overlay(shape.strokeBorder(hovered ? CRTheme.brandInk.opacity(0.45) : CRTheme.stroke.opacity(0.7), lineWidth: 1))
        .contentShape(shape)
        .onTapGesture(perform: open)
        .onHover { hovered = $0 }
        .animation(.crFast, value: hovered)
        .sendsDroppedFiles(to: device, store: store)
        .accessibilityElement(children: .contain)
    }

    private func open() {
        withAnimation(.crSpring) { store.openDevice(device.id) }
    }
}

/// A device that isn't linked right now, or one nearby to pair: one line, the
/// one thing to do with it, and its page on a click.
private struct CompactDeviceRow: View {
    let device: ManagedDevice
    @ObservedObject var store: LinkAllStore
    @State private var hovered = false

    var body: some View {
        let status = DeviceStatus(device, store: store)
        let paired = device.trustState == .trusted
        let asksUs = device.pairingRequested && !paired
        let connecting = device.connectionState == .connecting
        HStack(spacing: 12) {
            Button {
                withAnimation(.crSpring) { store.openDevice(device.id) }
            } label: {
                HStack(spacing: 12) {
                    DeviceWell(os: device.os, tone: paired ? .neutral : .blue, size: 30)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(device.name)
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(CRTheme.ink)
                            .lineLimit(1)
                        HStack(spacing: 6) {
                            StateDot(style: status.dot, size: 6)
                            Text(status.text)
                                .font(.system(size: 11.5))
                                .foregroundStyle(CRTheme.inkSoft)
                                .lineLimit(1)
                        }
                    }
                    Spacer(minLength: 8)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("\(device.name), \(status.text)")
            .accessibilityHint("Opens its page")

            Group {
                if asksUs {
                    Button("Accept") { store.respondToPairing(device, accepted: true) }
                        .buttonStyle(.borderedProminent)
                        .tint(CRTheme.brandElectric)
                        .help("Accept if \(device.name) shows the same code")
                } else if device.outgoingPairingWaiting {
                    EmptyView()
                } else if connecting {
                    ProgressView().controlSize(.small)
                } else if !paired {
                    Button("Pair") { store.sendPairingRequest(device) }
                        .buttonStyle(.borderedProminent)
                        .tint(CRTheme.brandElectric)
                } else {
                    Button("Connect") { store.connect(device) }
                        .buttonStyle(.bordered)
                }
            }
            .controlSize(.small)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .background(hovered ? CRTheme.rowHover : Color.clear)
        .onHover { hovered = $0 }
    }
}

// MARK: - Clipboard

/// Something to move between this Mac and a device: an item that arrived and
/// waits, or what was copied here. What it is, not what it says: copied text
/// can be a password or a code.
private struct HomeClipboardCard: View {
    @ObservedObject var store: LinkAllStore
    let linked: [ManagedDevice]

    var body: some View {
        let pending = store.activityFeed.filter(\.isApplicable)
        if let first = pending.first {
            SectionTitle(title: "Clipboard")
            Panel {
                row(title: "Copied on \(first.device_name)", detail: detail(for: first, more: pending.count - 1)) {
                    Button("Copy") { Task { await store.applyClipboard(entry: first) } }
                        .buttonStyle(.borderedProminent)
                        .tint(CRTheme.brandElectric)
                        .help("Put it on this Mac's clipboard")
                }
            }
        } else if let copied = store.quickSendContext, !linked.isEmpty {
            SectionTitle(title: "Clipboard")
            Panel {
                row(title: "Copied on this Mac",
                    detail: "\(copied.text.count) characters · \(copied.timestamp.relativeTimeString())") {
                    sendControl
                }
            }
        }
    }

    private func detail(for entry: IpcActivityEntry, more: Int) -> String {
        let count = entry.text_preview?.count ?? 0
        // The feed carries a 400-character preview, not the whole text.
        let size = count >= 400 ? "Over 400 characters" : "\(count) characters"
        let when = Date(timeIntervalSince1970: Double(entry.timestamp_ms) / 1000).relativeTimeString()
        return [size, when, more > 0 ? "\(more) more waiting" : nil].compactMap { $0 }.joined(separator: " · ")
    }

    @ViewBuilder private var sendControl: some View {
        if linked.count == 1, let only = linked.first {
            Button("Send to \(only.name)") { store.sendCurrentClipboard(to: only) }
                .buttonStyle(.borderedProminent)
                .tint(CRTheme.brandElectric)
        } else {
            Menu("Send") {
                Button("All devices") { store.sendCurrentClipboard(to: nil) }
                Divider()
                ForEach(linked) { device in
                    Button(device.name) { store.sendCurrentClipboard(to: device) }
                }
            }
            .menuStyle(.button)
            .buttonStyle(.bordered)
            .fixedSize()
        }
    }

    private func row<Trailing: View>(title: String, detail: String, @ViewBuilder trailing: () -> Trailing) -> some View {
        HStack(spacing: 12) {
            IconWell(systemName: "doc.on.clipboard", tone: .blue, size: 30)
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(CRTheme.ink)
                    .lineLimit(1)
                Text(detail)
                    .font(.system(size: 11.5))
                    .foregroundStyle(CRTheme.inkSoft)
                    .lineLimit(1)
            }
            Spacer(minLength: 10)
            trailing()
                .controlSize(.small)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 11)
    }
}

// MARK: - Recent

/// One thing that happened: what, from where and when; Copy or Show in Finder on hover.
private struct RecentRow: View {
    let item: TimelineItem
    @ObservedObject var store: LinkAllStore
    @State private var hovered = false

    private var tone: Tone {
        switch item.typeLabel {
        case "Image": return .violet
        case "File", "Folder": return .green
        default: return .blue
        }
    }

    var body: some View {
        HStack(spacing: 12) {
            IconWell(systemName: item.iconName, tone: tone, size: 28)
            VStack(alignment: .leading, spacing: 2) {
                Text(item.title)
                    .font(.system(size: 13))
                    .foregroundStyle(CRTheme.ink)
                    .lineLimit(1)
                Text("\(item.sourceDevice) · \(item.timestamp.relativeTimeString())")
                    .font(.system(size: 11.5))
                    .foregroundStyle(CRTheme.inkSoft)
                    .lineLimit(1)
            }
            Spacer(minLength: 10)
            if hovered {
                if item.fullText != nil {
                    Button("Copy") { store.copyTimelineItem(item) }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                } else if let path = item.filePath, !path.isEmpty {
                    Button("Show in Finder") {
                        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                }
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .frame(minHeight: 46)
        .background(hovered ? CRTheme.rowHover : Color.clear)
        .contentShape(Rectangle())
        .onHover { hovered = $0 }
        .contextMenu {
            if item.fullText != nil {
                Button("Copy") { store.copyTimelineItem(item) }
                Menu("Send to") {
                    Button("All devices") { store.sendTimelineItem(item, to: nil) }
                    ForEach(store.connectedDevices) { device in
                        Button(device.name) { store.sendTimelineItem(item, to: device) }
                    }
                }
            }
            if let path = item.filePath, !path.isEmpty {
                Button("Show in Finder") {
                    NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
                }
            }
        }
    }
}
