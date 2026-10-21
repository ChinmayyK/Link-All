import SwiftUI
import AppKit

extension Notification.Name {
    /// The menu-bar panel's content changed height.
    static let linkAllPopoverContentChanged = Notification.Name("linkall.popoverContentChanged")
}

/// What the paired devices are playing, refreshed while the menu-bar panel or
/// the main window is on screen.
@MainActor
final class NowPlayingModel: ObservableObject {
    @Published var items: [IpcNowPlaying] = []
    /// Each device's cover, decoded once per new picture, not on every redraw.
    @Published private(set) var art: [String: NSImage] = [:]
    private var artSource: [String: String] = [:]
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

    func item(for deviceId: String) -> IpcNowPlaying? {
        items.first { $0.device_id == deviceId }
    }

    private func reload() {
        Task {
            let list = (try? await ipc.nowPlayingList()) ?? []
            guard list != items else { return }
            items = list
            updateArt(for: list)
        }
    }

    private func updateArt(for list: [IpcNowPlaying]) {
        var next = art
        var changed = false
        for item in list where artSource[item.device_id] != item.art_base64 {
            artSource[item.device_id] = item.art_base64
            next[item.device_id] = item.art_base64.flatMap { Data(base64Encoded: $0) }.flatMap { NSImage(data: $0) }
            changed = true
        }
        // A device that stopped playing loses its cover, so a new one decodes fresh.
        let playing = Set(list.map(\.device_id))
        for id in Array(artSource.keys) where !playing.contains(id) {
            artSource[id] = nil
            if next.removeValue(forKey: id) != nil { changed = true }
        }
        if changed { art = next }
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

// MARK: - In the main window

/// "1:23" for a track position in milliseconds.
private func trackClock(_ ms: Double) -> String {
    let s = max(0, Int(ms / 1000))
    return String(format: "%d:%02d", s / 60, s % 60)
}

/// The cover, or a note on a plain square when the device sent none.
struct NowPlayingCover: View {
    let art: NSImage?
    var size: CGFloat = 34

    var body: some View {
        Group {
            if let art {
                Image(nsImage: art).resizable().scaledToFill()
            } else {
                ZStack {
                    Rectangle().fill(CRTheme.rowSelected)
                    Image(systemName: "music.note")
                        .font(.system(size: size * 0.4))
                        .foregroundStyle(CRTheme.inkSoft)
                }
            }
        }
        .frame(width: size, height: size)
        .clipShape(RoundedRectangle(cornerRadius: size * 0.2, style: .continuous))
        .accessibilityHidden(true)
    }
}

/// A round media control with a hover fill.
private struct MediaButton: View {
    let symbol: String
    let label: String
    var size: CGFloat = 30
    var prominent = false
    let action: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: size * 0.42, weight: .semibold))
                .foregroundStyle(prominent ? Color.white : CRTheme.ink)
                .frame(width: size, height: size)
                .background(
                    Circle().fill(prominent
                                  ? AnyShapeStyle(CRTheme.brandElectric)
                                  : AnyShapeStyle(hovered ? CRTheme.rowSelected : Color.clear))
                )
                .overlay(Circle().fill(prominent && hovered ? Color.white.opacity(0.10) : .clear))
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .help(label)
        .accessibilityLabel(label)
    }
}

/// A one-line player for a device card on Home: cover, track and play/pause.
struct NowPlayingMini: View {
    let item: IpcNowPlaying
    let art: NSImage?
    let onCommand: (String, Int) -> Void

    private var detail: String {
        [item.artist.isEmpty ? item.album : item.artist, item.app]
            .filter { !$0.isEmpty }
            .joined(separator: " · ")
    }

    var body: some View {
        HStack(spacing: 10) {
            NowPlayingCover(art: art, size: 34)
            VStack(alignment: .leading, spacing: 1) {
                Text(item.title)
                    .font(.system(size: 12.5, weight: .semibold))
                    .foregroundStyle(CRTheme.ink)
                    .lineLimit(1)
                if !detail.isEmpty {
                    Text(detail)
                        .font(.system(size: 11))
                        .foregroundStyle(CRTheme.inkSoft)
                        .lineLimit(1)
                }
            }
            Spacer(minLength: 6)
            MediaButton(symbol: item.playing ? "pause.fill" : "play.fill",
                        label: item.playing ? "Pause" : "Play", size: 28) {
                onCommand("play_pause", 0)
            }
        }
        .padding(7)
        .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(CRTheme.surface))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(CRTheme.stroke.opacity(0.6), lineWidth: 1))
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Now playing: \(item.title)")
    }
}

/// The full player on a device's page: cover, track, a scrub bar and the controls.
struct NowPlayingPanel: View {
    let item: IpcNowPlaying
    let art: NSImage?
    let onCommand: (String, Int) -> Void
    @State private var scrubbing: Double?

    var body: some View {
        Panel {
            HStack(alignment: .top, spacing: 16) {
                NowPlayingCover(art: art, size: 76)
                VStack(alignment: .leading, spacing: 10) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(item.title)
                            .font(.system(size: 15, weight: .semibold))
                            .foregroundStyle(CRTheme.ink)
                            .lineLimit(1)
                        let byline = [item.artist, item.album].filter { !$0.isEmpty }.joined(separator: " — ")
                        if !byline.isEmpty {
                            Text(byline)
                                .font(.system(size: 12.5))
                                .foregroundStyle(CRTheme.inkSoft)
                                .lineLimit(1)
                        }
                        Text(item.app.isEmpty ? (item.playing ? "Playing" : "Paused") : item.app)
                            .font(.system(size: 11.5))
                            .foregroundStyle(CRTheme.inkSubtle)
                            .lineLimit(1)
                    }
                    if item.duration_ms > 0 {
                        VStack(spacing: 2) {
                            Slider(
                                value: Binding(
                                    get: { scrubbing ?? Double(min(item.position_ms, item.duration_ms)) },
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
                            .controlSize(.small)
                            .accessibilityLabel("Position")
                            HStack {
                                Text(trackClock(scrubbing ?? Double(item.position_ms)))
                                Spacer()
                                Text(trackClock(Double(item.duration_ms)))
                            }
                            .font(.system(size: 10.5).monospacedDigit())
                            .foregroundStyle(CRTheme.inkSubtle)
                        }
                    }
                    HStack(spacing: 14) {
                        MediaButton(symbol: "backward.fill", label: "Previous track") { onCommand("previous", 0) }
                        MediaButton(symbol: item.playing ? "pause.fill" : "play.fill",
                                    label: item.playing ? "Pause" : "Play", size: 36, prominent: true) {
                            onCommand("play_pause", 0)
                        }
                        MediaButton(symbol: "forward.fill", label: "Next track") { onCommand("next", 0) }
                    }
                }
            }
            .padding(16)
        }
    }
}
