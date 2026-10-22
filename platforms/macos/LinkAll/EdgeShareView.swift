import SwiftUI

// Keyboard & Mouse: use this Mac's keyboard and mouse on the computer next to
// it. Whether it is on, where each computer sits, what is happening now, and
// whether each linked computer is ready.

struct EdgeOverview {
    var hosting = false
    var controlled = false
    var peer: String?
    var refusedTarget: String?
    var refusedReason: String?
}

extension LinkAllIPCClient {
    /// Where the hand-off stands, without clearing what the service itself acts on.
    func edgeOverview() async -> EdgeOverview? {
        guard let raw = try? await send(cmd: ["cmd": "edge_status", "peek": true]),
              let obj = try? JSONSerialization.jsonObject(with: raw) as? [String: Any],
              let data = obj["data"] as? [String: Any] else { return nil }
        return EdgeOverview(
            hosting: data["hosting"] as? Bool ?? false,
            controlled: data["controlled"] as? Bool ?? false,
            peer: data["peer"] as? String,
            refusedTarget: data["refused_target"] as? String,
            refusedReason: data["refused_reason"] as? String
        )
    }
}

struct EdgeShareView: View {
    @ObservedObject var store: LinkAllStore

    @AppStorage(EdgeControlService.enabledKey) private var enabled = false
    @AppStorage("edgeLink0") private var link0 = ""
    @AppStorage("edgeLink1") private var link1 = ""
    @AppStorage("edgeLink2") private var link2 = ""
    @AppStorage("edgeLink3") private var link3 = ""
    @State private var overview = EdgeOverview()

    private let tick = Timer.publish(every: 1, on: .main, in: .common).autoconnect()

    private var links: [Binding<String>] { [$link0, $link1, $link2, $link3] }
    private var linkValues: [String] { [link0, link1, link2, link3] }

    /// Paired computers: a phone can't take a keyboard and mouse.
    private var computers: [PeerViewModel] {
        store.peers.filter { $0.trusted && ($0.platform ?? "").lowercased() != "android" }
    }

    private func name(of id: String?) -> String {
        guard let id else { return "another computer" }
        return store.peers.first { $0.id == id }?.displayName ?? "another computer"
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                PageHeader(title: "Keyboard & Mouse",
                           subtitle: "Use this Mac's keyboard and mouse on the computer next to it")
                    .padding(.bottom, 18)

                statusPanel
                    .padding(.bottom, 14)

                Panel {
                    PanelRow(icon: "keyboard", title: "Share this keyboard and mouse",
                             detail: "Push the pointer against a screen edge for about a quarter of a second to move to the computer on that side. Needs the Accessibility permission.") {
                        Toggle("", isOn: $enabled).labelsHidden()
                    }
                }

                SectionTitle(title: "Where each computer sits")
                Panel {
                    ForEach(0..<4, id: \.self) { edge in
                        if edge > 0 { RowDivider() }
                        PanelRow(icon: ["arrow.left", "arrow.right", "arrow.up", "arrow.down"][edge],
                                 title: "On the \(EdgeControlService.edgeNames[edge])") {
                            Picker("", selection: links[edge]) {
                                Text("Nobody").tag("")
                                ForEach(computers) { Text($0.displayName).tag($0.id) }
                            }
                            .labelsHidden()
                            .frame(maxWidth: 200)
                        }
                    }
                }

                if enabled, linkValues.contains(where: { !$0.isEmpty }) {
                    SectionTitle(title: "Ready to share?")
                    Panel {
                        let linked = (0..<4).filter { !linkValues[$0].isEmpty }
                        ForEach(linked, id: \.self) { edge in
                            if edge != linked.first { RowDivider() }
                            readinessRow(edge: edge)
                        }
                    }
                }

                Text("Press Ctrl+Option+Esc at any time to bring the keyboard and mouse back to this Mac. Command acts as Control on the other computer, so shortcuts like Cmd+C work there.")
                    .font(.system(size: 12))
                    .foregroundStyle(CRTheme.inkSoft)
                    .padding(.top, 18)
            }
            .padding(.horizontal, 24)
            .padding(.vertical, 20)
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .onChange(of: enabled) { _ in EdgeControlService.shared.reload() }
        .onChange(of: link0) { _ in EdgeControlService.shared.reload() }
        .onChange(of: link1) { _ in EdgeControlService.shared.reload() }
        .onChange(of: link2) { _ in EdgeControlService.shared.reload() }
        .onChange(of: link3) { _ in EdgeControlService.shared.reload() }
        .onReceive(tick) { _ in
            Task { @MainActor in
                if let o = await LinkAllIPCClient.shared.edgeOverview() { overview = o }
            }
        }
    }

    private var statusPanel: some View {
        Panel {
            HStack(spacing: 12) {
                StateDot(style: overview.hosting || overview.controlled ? .accent : (enabled ? .live : .idle),
                         size: 9)
                VStack(alignment: .leading, spacing: 2) {
                    Text(statusTitle)
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(CRTheme.ink)
                    Text(statusDetail)
                        .font(.system(size: 12))
                        .foregroundStyle(CRTheme.inkSoft)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 10)
                if overview.hosting {
                    Button("Take It Back") { Task { try? await LinkAllIPCClient.shared.edgeEnd() } }
                }
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)
        }
    }

    private var statusTitle: String {
        if overview.hosting { return "Controlling \(name(of: overview.peer))" }
        if overview.controlled { return "Being controlled by \(name(of: overview.peer))" }
        return enabled ? "Ready" : "Off"
    }

    private var statusDetail: String {
        if overview.hosting {
            return "This Mac's keyboard and mouse are on that computer. Push the pointer back past its edge, or press Ctrl+Option+Esc."
        }
        if overview.controlled { return "That computer's keyboard and mouse are driving this Mac." }
        return enabled
            ? "Push the pointer against a screen edge that has a computer next to it."
            : "Turn on sharing to move between computers with one keyboard and mouse."
    }

    private func readinessRow(edge: Int) -> some View {
        let id = linkValues[edge]
        let peer = store.peers.first { $0.id == id }
        let verdict: String
        var ok = false
        if peer == nil {
            verdict = "Not paired any more. Choose another computer."
        } else if peer?.trusted != true {
            verdict = "Not paired yet."
        } else if peer?.connected != true {
            verdict = "Not connected right now. Open Link All on it, on the same Wi-Fi."
        } else if overview.refusedTarget == id, let why = overview.refusedReason, !why.isEmpty {
            verdict = why
        } else {
            verdict = "Connected. If it does not take over, turn on Remote control in Link All on that computer."
            ok = true
        }
        return PanelRow(icon: ok ? "checkmark.circle" : "exclamationmark.triangle",
                        tone: ok ? .green : .amber,
                        title: "\(peer?.displayName ?? "A computer") (\(EdgeControlService.edgeNames[edge]))",
                        detail: verdict)
    }
}
