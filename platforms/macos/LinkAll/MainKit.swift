import SwiftUI
import AppKit
import UniformTypeIdentifiers

// The main window's building blocks, shared by Home, a device's page, Files and
// Settings. Plain surfaces split by hairlines; the brand blue only for actions
// and state; tinted icon wells to tell groups apart.

// MARK: - Tones

/// The tint of an icon well. Each maps to an existing CRTheme accent.
enum Tone {
    case blue, green, violet, amber, teal, rose, neutral

    var color: Color {
        switch self {
        case .blue:    return CRTheme.brandInk
        case .green:   return CRTheme.accentGreen
        case .violet:  return CRTheme.brandViolet
        case .amber:   return CRTheme.accentOrange
        case .teal:    return CRTheme.accentTeal
        case .rose:    return CRTheme.accentPink
        case .neutral: return CRTheme.inkSoft
        }
    }

    /// The same hue, faint, behind the icon.
    var soft: Color {
        switch self {
        case .neutral: return CRTheme.rowSelected
        case .blue:    return Color(light: CRTheme.brandElectric.opacity(0.10), dark: Color(hex: 0x5C95FF, opacity: 0.18))
        default:       return color.opacity(0.13)
        }
    }
}

// MARK: - Page structure

/// A page's title, what it is for, and anything that acts on the whole page.
struct PageHeader<Trailing: View>: View {
    let title: String
    var subtitle: String? = nil
    @ViewBuilder var trailing: () -> Trailing

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            VStack(alignment: .leading, spacing: 3) {
                Text(title)
                    .font(.system(size: 26, weight: .bold))
                    .foregroundStyle(CRTheme.ink)
                    .accessibilityAddTraits(.isHeader)
                if let subtitle {
                    Text(subtitle)
                        .font(.system(size: 13))
                        .foregroundStyle(CRTheme.inkSoft)
                        .lineLimit(2)
                }
            }
            Spacer(minLength: 12)
            trailing()
        }
    }
}

extension PageHeader where Trailing == EmptyView {
    init(title: String, subtitle: String? = nil) {
        self.init(title: title, subtitle: subtitle) { EmptyView() }
    }
}

/// A section on a page: its name, a count, and a way to see all of it.
struct SectionTitle: View {
    let title: String
    var meta: String? = nil
    var seeAll: (() -> Void)? = nil

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(title)
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(CRTheme.ink)
                .accessibilityAddTraits(.isHeader)
            if let meta {
                Text(meta)
                    .font(.system(size: 13))
                    .foregroundStyle(CRTheme.inkSoft)
            }
            Spacer(minLength: 8)
            if let seeAll {
                Button(action: seeAll) {
                    HStack(spacing: 3) {
                        Text("See all")
                        Image(systemName: "chevron.right").font(.system(size: 10, weight: .semibold))
                    }
                }
                .buttonStyle(LinkButtonStyle())
                .accessibilityLabel("See all \(title.lowercased())")
            }
        }
        .padding(.top, 28)
        .padding(.bottom, 10)
    }
}

/// A small heading over a group of tiles or rows.
struct GroupLabel: View {
    let text: String
    init(_ text: String) { self.text = text }

    var body: some View {
        Text(text.uppercased())
            .font(.system(size: 11, weight: .semibold))
            .tracking(0.8)
            .foregroundStyle(CRTheme.inkSubtle)
            .padding(.leading, 2)
            .padding(.top, 24)
            .padding(.bottom, 8)
            .accessibilityAddTraits(.isHeader)
    }
}

/// A grouped box of rows.
struct Panel<Content: View>: View {
    var highlighted = false
    @ViewBuilder var content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 0) { content() }
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(CRTheme.surfaceStrong))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(highlighted ? CRTheme.brandInk.opacity(0.7) : CRTheme.stroke.opacity(0.7),
                                  lineWidth: highlighted ? 1.5 : 1)
            )
    }
}

/// A plain note in place of an empty list.
struct EmptyNote: View {
    let icon: String
    let text: String

    var body: some View {
        Panel {
            HStack(spacing: 12) {
                Image(systemName: icon)
                    .font(.system(size: 15))
                    .foregroundStyle(CRTheme.inkSubtle)
                    .frame(width: 22)
                Text(text)
                    .font(.system(size: 13))
                    .foregroundStyle(CRTheme.inkSoft)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(16)
        }
    }
}

// MARK: - Icon wells

/// An SF Symbol on a soft, tinted square.
struct IconWell: View {
    let systemName: String
    var tone: Tone = .blue
    var size: CGFloat = 30

    var body: some View {
        Image(systemName: systemName)
            .font(.system(size: size * 0.46, weight: .semibold))
            .foregroundStyle(tone.color)
            .frame(width: size, height: size)
            .background(RoundedRectangle(cornerRadius: size * 0.28, style: .continuous).fill(tone.soft))
            .accessibilityHidden(true)
    }
}

/// A device's OS logo on a soft, tinted square.
struct DeviceWell: View {
    let os: DeviceOS
    var tone: Tone = .blue
    var size: CGFloat = 30

    var body: some View {
        OSIcon(os, size: size * 0.5)
            .foregroundStyle(tone.color)
            .frame(width: size, height: size)
            .background(RoundedRectangle(cornerRadius: size * 0.28, style: .continuous).fill(tone.soft))
            .accessibilityHidden(true)
    }
}

/// A small status dot: filled when live, a ring when idle.
struct StateDot: View {
    enum Style { case live, warn, accent, idle }
    let style: Style
    var size: CGFloat = 7

    var body: some View {
        Group {
            switch style {
            case .live:   Circle().fill(CRTheme.accentGreen)
            case .warn:   Circle().fill(CRTheme.accentOrange)
            case .accent: Circle().fill(CRTheme.brandInk)
            case .idle:   Circle().strokeBorder(CRTheme.inkSubtle, lineWidth: 1.5)
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}

// MARK: - Device status

/// How a device is doing, in a few words, shared by its card, its row and its page.
struct DeviceStatus {
    let text: String
    let dot: StateDot.Style

    @MainActor
    init(_ device: ManagedDevice, store: LinkAllStore) {
        let pin = device.pairingPin.flatMap { $0.isEmpty ? nil : $0 }
        if let test = store.activeSpeedTests.first(where: { $0.id == device.id }) {
            self.init(text: "Speed test · \(test.phase) · \(test.speedMbpsString)", dot: .accent)
        } else if device.pairingRequested && device.trustState != .trusted {
            self.init(text: pin.map { "Wants to pair · code \($0)" } ?? "Wants to pair", dot: .accent)
        } else if device.outgoingPairingWaiting {
            self.init(text: pin.map { "Approve on \(device.name) · code \($0)" } ?? "Waiting for \(device.name)", dot: .warn)
        } else if device.isLinked {
            self.init(text: "Connected", dot: .live)
        } else if let outcome = device.pairingOutcomeText {
            self.init(text: outcome, dot: .warn)
        } else if device.connectionState == .connecting {
            self.init(text: "Connecting…", dot: .idle)
        } else if device.trustState != .trusted {
            self.init(text: "Not paired", dot: .idle)
        } else if device.pausedByPhone {
            self.init(text: "Paused by the phone", dot: .warn)
        } else if device.explicitDisconnect {
            self.init(text: "Disconnected", dot: .idle)
        } else if let seen = device.lastSeen {
            self.init(text: "Seen \(seen.relativeTimeString())", dot: .idle)
        } else {
            self.init(text: "Offline", dot: .idle)
        }
    }

    private init(text: String, dot: StateDot.Style) {
        self.text = text
        self.dot = dot
    }
}

// MARK: - Tiles

/// One action in a tile grid.
struct TileSpec: Identifiable {
    var id: String { label }
    let icon: String
    let label: String
    var tone: Tone = .blue
    var enabled = true
    /// Said on hover; what the tile does when the label alone isn't enough.
    var help: String? = nil
    let action: () -> Void
}

/// Actions as wide tiles, as many across as fit.
struct TileGrid: View {
    let tiles: [TileSpec]

    var body: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 172), spacing: 10)], alignment: .leading, spacing: 10) {
            ForEach(tiles) { ActionTile(spec: $0) }
        }
    }
}

struct ActionTile: View {
    let spec: TileSpec

    var body: some View {
        Button(action: spec.action) {
            HStack(spacing: 11) {
                IconWell(systemName: spec.icon, tone: spec.enabled ? spec.tone : .neutral, size: 30)
                Text(spec.label)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(CRTheme.ink)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 11)
            .frame(maxWidth: .infinity, minHeight: 52, alignment: .leading)
        }
        .buttonStyle(CardButtonStyle(radius: 12))
        .disabled(!spec.enabled)
        .opacity(spec.enabled ? 1 : 0.5)
        .help(spec.help ?? spec.label)
    }
}

// MARK: - Rows

/// A row inside a Panel: icon well, title, detail, then a control or a chevron.
struct PanelRow<Trailing: View>: View {
    let icon: String
    var tone: Tone = .neutral
    let title: String
    var detail: String? = nil
    var destructive = false
    var action: (() -> Void)? = nil
    @ViewBuilder var trailing: () -> Trailing

    var body: some View {
        if let action {
            Button(action: action) { content(showsChevron: Trailing.self == EmptyView.self && !destructive) }
                .buttonStyle(RowButtonStyle())
        } else {
            content(showsChevron: false)
        }
    }

    private func content(showsChevron: Bool) -> some View {
        HStack(spacing: 12) {
            IconWell(systemName: icon, tone: destructive ? .rose : tone, size: 28)
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(destructive ? CRTheme.accentRed : CRTheme.ink)
                if let detail {
                    Text(detail)
                        .font(.system(size: 11.5))
                        .foregroundStyle(CRTheme.inkSoft)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 10)
            trailing()
            if showsChevron {
                Image(systemName: "chevron.right")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(CRTheme.inkSubtle)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
    }
}

extension PanelRow where Trailing == EmptyView {
    init(icon: String, tone: Tone = .neutral, title: String, detail: String? = nil,
         destructive: Bool = false, action: (() -> Void)? = nil) {
        self.init(icon: icon, tone: tone, title: title, detail: detail,
                  destructive: destructive, action: action) { EmptyView() }
    }
}

/// A label and its value, for facts about a device or this Mac.
struct InfoRow: View {
    let label: String
    let value: String
    var monospaced = false

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 16) {
            Text(label)
                .font(.system(size: 13))
                .foregroundStyle(CRTheme.ink)
            Spacer(minLength: 12)
            Text(value)
                .font(monospaced ? .system(size: 12, design: .monospaced) : .system(size: 13))
                .foregroundStyle(CRTheme.inkSoft)
                .lineLimit(1)
                .truncationMode(.middle)
                .textSelection(.enabled)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .accessibilityElement(children: .combine)
    }
}

/// The hairline between rows in a Panel.
struct RowDivider: View {
    var inset: CGFloat = 54
    var body: some View { CRDivider(inset: inset) }
}

// MARK: - Buttons

/// A card or tile: a raised fill that lightens on hover and dips when pressed.
struct CardButtonStyle: ButtonStyle {
    var radius: CGFloat = 14

    func makeBody(configuration: Configuration) -> some View {
        CardBody(configuration: configuration, radius: radius)
    }

    private struct CardBody: View {
        let configuration: Configuration
        let radius: CGFloat
        @State private var hovered = false
        @Environment(\.isEnabled) private var enabled

        var body: some View {
            let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
            configuration.label
                .background(
                    ZStack {
                        shape.fill(CRTheme.surfaceStrong)
                        shape.fill(configuration.isPressed ? CRTheme.rowSelected : (hovered && enabled ? CRTheme.rowHover : Color.clear))
                    }
                )
                .overlay(shape.strokeBorder(hovered && enabled ? CRTheme.brandInk.opacity(0.45) : CRTheme.stroke.opacity(0.7), lineWidth: 1))
                .contentShape(shape)
                .scaleEffect(configuration.isPressed ? 0.985 : 1)
                .animation(.crFast, value: hovered)
                .animation(.crFast, value: configuration.isPressed)
                .onHover { hovered = $0 }
        }
    }
}

/// A row in a list: a faint fill on hover, a firmer one while pressed.
struct RowButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        RowBody(configuration: configuration)
    }

    private struct RowBody: View {
        let configuration: Configuration
        @State private var hovered = false
        @Environment(\.isEnabled) private var enabled

        var body: some View {
            configuration.label
                .background(configuration.isPressed ? CRTheme.rowSelected : (hovered && enabled ? CRTheme.rowHover : .clear))
                .animation(.crFast, value: hovered)
                .onHover { hovered = $0 }
        }
    }
}

/// Text that acts like a link: the brand blue, dimmer while pressed.
struct LinkButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        LinkBody(configuration: configuration)
    }

    private struct LinkBody: View {
        let configuration: Configuration
        @State private var hovered = false

        var body: some View {
            configuration.label
                .font(.system(size: 12.5, weight: .medium))
                .foregroundStyle(CRTheme.brandInk)
                .opacity(configuration.isPressed ? 0.6 : (hovered ? 0.85 : 1))
                .padding(.horizontal, 6)
                .padding(.vertical, 3)
                .background(RoundedRectangle(cornerRadius: 6, style: .continuous).fill(hovered ? CRTheme.rowHover : .clear))
                .contentShape(Rectangle())
                .onHover { hovered = $0 }
        }
    }
}

/// A square icon-only button, as in a toolbar. Give it `.help` and an accessibility label.
struct IconButtonStyle: ButtonStyle {
    var size: CGFloat = 30

    func makeBody(configuration: Configuration) -> some View {
        IconBody(configuration: configuration, size: size)
    }

    private struct IconBody: View {
        let configuration: Configuration
        let size: CGFloat
        @State private var hovered = false
        @Environment(\.isEnabled) private var enabled

        var body: some View {
            configuration.label
                .font(.system(size: size * 0.46, weight: .medium))
                .foregroundStyle(hovered && enabled ? CRTheme.ink : CRTheme.inkSoft)
                .frame(width: size, height: size)
                .background(
                    RoundedRectangle(cornerRadius: size * 0.27, style: .continuous)
                        .fill(configuration.isPressed ? CRTheme.rowSelected : (hovered && enabled ? CRTheme.rowHover : .clear))
                )
                .contentShape(Rectangle())
                .opacity(enabled ? 1 : 0.4)
                .animation(.crFast, value: hovered)
                .onHover { hovered = $0 }
        }
    }
}

/// A quick action on a device card: icon and label on a soft fill, the first one solid.
struct QuickActionStyle: ButtonStyle {
    var primary = false

    func makeBody(configuration: Configuration) -> some View {
        QuickBody(configuration: configuration, primary: primary)
    }

    private struct QuickBody: View {
        let configuration: Configuration
        let primary: Bool
        @State private var hovered = false

        var body: some View {
            let shape = RoundedRectangle(cornerRadius: 8, style: .continuous)
            configuration.label
                .font(.system(size: 12.5, weight: .medium))
                .foregroundStyle(primary ? Color.white : CRTheme.ink)
                .frame(maxWidth: .infinity, minHeight: 30)
                .background(shape.fill(primary ? AnyShapeStyle(CRTheme.brandElectric) : AnyShapeStyle(CRTheme.rowSelected)))
                .overlay(shape.fill(configuration.isPressed ? Color.black.opacity(0.12) : (hovered ? Color.white.opacity(primary ? 0.10 : 0.04) : .clear)))
                .contentShape(shape)
                .animation(.crFast, value: hovered)
                .onHover { hovered = $0 }
        }
    }
}

// MARK: - File drops

/// The file URLs in a drop, delivered on the main queue.
func loadDroppedFileURLs(_ providers: [NSItemProvider], done: @escaping ([URL]) -> Void) {
    var urls: [URL] = []
    let group = DispatchGroup()
    for provider in providers where provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) {
        group.enter()
        _ = provider.loadObject(ofClass: URL.self) { url, _ in
            DispatchQueue.main.async {
                if let url, url.isFileURL { urls.append(url) }
                group.leave()
            }
        }
    }
    group.notify(queue: .main) { done(urls) }
}

extension View {
    /// Files dropped here go to `device`, with an outline while a drag hovers.
    func sendsDroppedFiles(to device: ManagedDevice?, store: LinkAllStore, radius: CGFloat = 16) -> some View {
        modifier(FileDropModifier(device: device, store: store, radius: radius))
    }
}

private struct FileDropModifier: ViewModifier {
    let device: ManagedDevice?
    @ObservedObject var store: LinkAllStore
    let radius: CGFloat
    @State private var targeted = false

    func body(content: Content) -> some View {
        if let device {
            content
                .overlay {
                    if targeted {
                        RoundedRectangle(cornerRadius: radius, style: .continuous)
                            .strokeBorder(CRTheme.brandInk, style: StrokeStyle(lineWidth: 2, dash: [6, 4]))
                            .background(RoundedRectangle(cornerRadius: radius, style: .continuous).fill(CRTheme.brandElectric.opacity(0.06)))
                            .overlay(alignment: .bottom) {
                                Text("Drop to send to \(device.name)")
                                    .font(.system(size: 12, weight: .semibold))
                                    .foregroundStyle(.white)
                                    .padding(.horizontal, 12).padding(.vertical, 6)
                                    .background(Capsule().fill(CRTheme.brandElectric))
                                    .padding(.bottom, 10)
                            }
                            .allowsHitTesting(false)
                    }
                }
                .onDrop(of: [.fileURL], isTargeted: $targeted) { providers in
                    loadDroppedFileURLs(providers) { store.send($0, to: device) }
                    return true
                }
        } else {
            content
        }
    }
}

// MARK: - Formatting

/// "12.4 MB", "820 KB": a size in the units people read.
func byteCountLabel(_ bytes: Int64) -> String {
    ByteCountFormatter.string(fromByteCount: max(0, bytes), countStyle: .file)
}

/// This Mac's Link All version, with its build.
var appVersionLabel: String {
    let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""
    let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String
    return [version, build.map { "(\($0))" }].compactMap { $0 }.joined(separator: " ")
}
