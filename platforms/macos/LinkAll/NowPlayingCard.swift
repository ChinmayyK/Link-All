import SwiftUI
import AppKit

extension Notification.Name {
    /// The menu-bar panel's content changed height.
    static let linkAllPopoverContentChanged = Notification.Name("linkall.popoverContentChanged")
}

/// What the paired devices are playing, refreshed while the menu-bar panel is open.
@MainActor
final class NowPlayingModel: ObservableObject {
    @Published var items: [IpcNowPlaying] = []
    private var timer: Timer?
    private let ipc = LinkAllIPCClient.shared

    func start() {
        reload()
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 1.5, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.reload() }
        }
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }

    private func reload() {
        Task {
            let list = (try? await ipc.nowPlayingList()) ?? []
            if list != items { items = list }
        }
    }

    func send(_ item: IpcNowPlaying, _ action: String, value: Int = 0) {
        Task {
            await ipc.mediaCommand(target: item.device_id, action: action, value: value)
            reload()
        }
    }
}

/// A track playing on another device: cover, title, artist, a scrub bar and controls.
struct NowPlayingCard: View {
    let item: IpcNowPlaying
    let onCommand: (String, Int) -> Void
    @State private var scrubbing: Double?

    private var art: NSImage? {
        item.art_base64.flatMap { Data(base64Encoded: $0) }.flatMap { NSImage(data: $0) }
    }

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Group {
                if let art {
                    Image(nsImage: art).resizable().scaledToFill()
                } else {
                    ZStack {
                        Rectangle().fill(CRTheme.surfaceStrong)
                        Image(systemName: "music.note").foregroundStyle(CRTheme.inkSoft)
                    }
                }
            }
            .frame(width: 56, height: 56)
            .clipShape(RoundedRectangle(cornerRadius: 10))

            VStack(alignment: .leading, spacing: 6) {
                VStack(alignment: .leading, spacing: 1) {
                    Text(item.title).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                    Text(item.artist.isEmpty ? item.album : item.artist)
                        .font(.system(size: 11.5)).foregroundStyle(CRTheme.inkSoft).lineLimit(1)
                    Text("\(item.app.isEmpty ? "Playing" : item.app) · on \(item.device_name)")
                        .font(.system(size: 10.5)).foregroundStyle(CRTheme.inkSubtle).lineLimit(1)
                }
                if item.duration_ms > 0 {
                    Slider(
                        value: Binding(
                            get: { scrubbing ?? Double(item.position_ms) },
                            set: { scrubbing = $0 }
                        ),
                        in: 0...Double(item.duration_ms),
                        onEditingChanged: { editing in
                            if !editing, let to = scrubbing {
                                onCommand("seek", Int(to))
                                scrubbing = nil
                            }
                        }
                    )
                    .controlSize(.mini)
                }
                HStack(spacing: 18) {
                    control("backward.fill") { onCommand("previous", 0) }
                    control(item.playing ? "pause.fill" : "play.fill", size: 16) { onCommand("play_pause", 0) }
                    control("forward.fill") { onCommand("next", 0) }
                    Spacer()
                    Text("\(clock(scrubbing ?? Double(item.position_ms))) / \(clock(Double(item.duration_ms)))")
                        .font(.system(size: 10.5, design: .monospaced)).foregroundStyle(CRTheme.inkSubtle)
                }
            }
        }
        .padding(12)
        .background(CRTheme.surfaceStrong, in: RoundedRectangle(cornerRadius: 14))
    }

    private func control(_ symbol: String, size: CGFloat = 13, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol).font(.system(size: size)).foregroundStyle(CRTheme.ink)
        }
        .buttonStyle(.plain)
    }

    private func clock(_ ms: Double) -> String {
        let s = Int(ms / 1000)
        return String(format: "%d:%02d", s / 60, s % 60)
    }
}
