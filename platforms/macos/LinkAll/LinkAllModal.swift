import AppKit
import SwiftUI
import UniformTypeIdentifiers

// One modal for the whole app: a card in Link All's own surface, type and
// icon wells over a dimmed screen, instead of NSAlert. It lives in its own
// floating panel, so the menu bar, drop targets and the dashboard can all
// show it whether or not the dashboard window is open.

@MainActor
final class LinkAllModal {
    static let shared = LinkAllModal()
    private var panel: NSPanel?

    private final class KeyPanel: NSPanel {
        var onEscape: (() -> Void)?
        override var canBecomeKey: Bool { true }
        override var canBecomeMain: Bool { false }
        override func cancelOperation(_ sender: Any?) { onEscape?() }
    }

    /// Shows `content` centered on the active screen. The content gets a
    /// `dismiss` closure; Escape and clicks on the backdrop dismiss too.
    func present<Content: View>(@ViewBuilder _ content: @escaping (_ dismiss: @escaping () -> Void) -> Content) {
        dismiss()
        guard let screen = NSScreen.main ?? NSScreen.screens.first else { return }
        let panel = KeyPanel(
            contentRect: screen.frame,
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.level = .modalPanel
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.onEscape = { [weak self] in self?.dismiss() }
        let close: () -> Void = { [weak self] in self?.dismiss() }
        panel.contentView = NSHostingView(rootView: ModalBackdrop(onDismiss: close) { content(close) })
        panel.setFrame(screen.frame, display: true)
        NSApp.activate(ignoringOtherApps: true)
        panel.makeKeyAndOrderFront(nil)
        self.panel = panel
    }

    func dismiss() {
        panel?.orderOut(nil)
        panel = nil
    }
}

private struct ModalBackdrop<Content: View>: View {
    let onDismiss: () -> Void
    @ViewBuilder let content: () -> Content
    @State private var shown = false

    var body: some View {
        ZStack {
            Color.black.opacity(shown ? 0.35 : 0)
                .ignoresSafeArea()
                .contentShape(Rectangle())
                .onTapGesture(perform: onDismiss)
            content()
                .scaleEffect(shown ? 1 : 0.96)
                .opacity(shown ? 1 : 0)
        }
        .onAppear { withAnimation(.spring(response: 0.32, dampingFraction: 0.86)) { shown = true } }
    }
}

// MARK: - Card

/// The modal card: icon well, title and subtitle, then content.
struct ModalCard<Content: View>: View {
    let icon: String
    var tint: Color = CRTheme.brandElectric
    let title: String
    var subtitle: String? = nil
    var width: CGFloat = 440
    @ViewBuilder let content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(spacing: 14) {
                ZStack {
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .fill(tint.opacity(0.14))
                        .frame(width: 44, height: 44)
                    Image(systemName: icon)
                        .font(.system(size: 19, weight: .semibold))
                        .foregroundStyle(tint)
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(.system(size: 17, weight: .bold, design: .rounded))
                        .foregroundStyle(CRTheme.ink)
                        .lineLimit(2)
                    if let subtitle {
                        Text(subtitle)
                            .font(.system(size: 12.5))
                            .foregroundStyle(CRTheme.inkSoft)
                            .lineLimit(2)
                    }
                }
                Spacer(minLength: 0)
            }
            content()
        }
        .padding(22)
        .frame(width: width, alignment: .leading)
        .background {
            RoundedRectangle(cornerRadius: 22, style: .continuous)
                .fill(CRTheme.surfaceElevated)
                .overlay(RoundedRectangle(cornerRadius: 22, style: .continuous).strokeBorder(CRTheme.stroke, lineWidth: 1))
                .shadow(color: .black.opacity(0.28), radius: 40, y: 18)
        }
        // Clicks inside the card must not reach the backdrop.
        .onTapGesture {}
    }
}

/// A large choice inside a modal: icon well, title, detail, chevron.
struct ModalOption: View {
    let icon: String
    let title: String
    var detail: String? = nil
    var danger = false
    let action: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 12) {
                ZStack {
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .fill((danger ? CRTheme.accentRed : CRTheme.brandElectric).opacity(0.12))
                        .frame(width: 36, height: 36)
                    Image(systemName: icon)
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(danger ? CRTheme.accentRed : CRTheme.brandElectric)
                }
                VStack(alignment: .leading, spacing: 1) {
                    Text(title)
                        .font(.system(size: 13.5, weight: .semibold))
                        .foregroundStyle(danger ? CRTheme.accentRed : CRTheme.ink)
                    if let detail {
                        Text(detail)
                            .font(.system(size: 12))
                            .foregroundStyle(CRTheme.inkSoft)
                            .lineLimit(2)
                    }
                }
                Spacer(minLength: 4)
                if !danger {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(CRTheme.inkSubtle)
                }
            }
            .padding(12)
            .background(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .fill(hovered ? CRTheme.rowHover : CRTheme.surfaceStrong)
            )
            .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(CRTheme.stroke.opacity(0.7), lineWidth: 1))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
    }
}

/// The two answers at the bottom of a modal.
struct ModalButtons: View {
    var cancel: String? = "Cancel"
    let confirm: String
    var destructive = false
    var confirmDisabled = false
    let onCancel: () -> Void
    let onConfirm: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Spacer()
            if let cancel {
                Button(cancel, action: onCancel)
                    .buttonStyle(CRSecondaryButtonStyle())
                    .keyboardShortcut(.cancelAction)
            }
            Button(confirm, action: onConfirm)
                .buttonStyle(CRPrimaryButtonStyle(tint: destructive ? CRTheme.accentRed : CRTheme.brandElectric))
                .keyboardShortcut(.defaultAction)
                .disabled(confirmDisabled)
                .opacity(confirmDisabled ? 0.5 : 1)
        }
    }
}

// MARK: - Ready-made modals

extension LinkAllModal {
    /// Explains something and asks to go ahead.
    func confirm(
        icon: String,
        tint: Color = CRTheme.brandElectric,
        title: String,
        message: String,
        confirm: String,
        cancel: String? = "Cancel",
        destructive: Bool = false,
        onConfirm: @escaping () -> Void,
        onCancel: @escaping () -> Void = {}
    ) {
        present { dismiss in
            ModalCard(icon: icon, tint: destructive ? CRTheme.accentRed : tint, title: title) {
                Text(message)
                    .font(.system(size: 13))
                    .foregroundStyle(CRTheme.inkSoft)
                    .fixedSize(horizontal: false, vertical: true)
                ModalButtons(
                    cancel: cancel, confirm: confirm, destructive: destructive,
                    onCancel: { dismiss(); onCancel() },
                    onConfirm: { dismiss(); onConfirm() }
                )
            }
        }
    }

    /// Asks for one line of text.
    func input(
        icon: String,
        title: String,
        subtitle: String? = nil,
        placeholder: String,
        initial: String = "",
        confirm: String,
        onConfirm: @escaping (String) -> Void
    ) {
        present { dismiss in
            ModalInputCard(
                icon: icon, title: title, subtitle: subtitle, placeholder: placeholder,
                initial: initial, confirm: confirm,
                onCancel: dismiss,
                onConfirm: { value in dismiss(); onConfirm(value) }
            )
        }
    }
}

private struct ModalInputCard: View {
    let icon: String
    let title: String
    let subtitle: String?
    let placeholder: String
    let initial: String
    let confirm: String
    let onCancel: () -> Void
    let onConfirm: (String) -> Void
    @State private var text = ""
    @FocusState private var focused: Bool

    var body: some View {
        ModalCard(icon: icon, title: title, subtitle: subtitle) {
            TextField(placeholder, text: $text)
                .textFieldStyle(.plain)
                .font(.system(size: 14))
                .padding(.horizontal, 12)
                .padding(.vertical, 10)
                .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(CRTheme.surfaceStrong))
                .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(focused ? CRTheme.brandElectric : CRTheme.stroke, lineWidth: 1))
                .focused($focused)
                .onSubmit(submit)
            ModalButtons(
                confirm: confirm,
                confirmDisabled: trimmed.isEmpty,
                onCancel: onCancel,
                onConfirm: submit
            )
        }
        .onAppear {
            text = initial
            DispatchQueue.main.async { focused = true }
        }
    }

    private var trimmed: String { text.trimmingCharacters(in: .whitespacesAndNewlines) }
    private func submit() { if !trimmed.isEmpty { onConfirm(trimmed) } }
}

// MARK: - Send

/// "Files & folders": where to send, then what. The core ranks the devices; the
/// one used last is already selected, so the usual case is one click on a button
/// that names the device. Devices that can't be used right now are listed below,
/// greyed, with the reason. With `urls` already chosen (a drop, the Finder
/// service) it only asks where; otherwise it takes a drop or opens the picker,
/// which accepts files and folders together.
struct SendModalCard: View {
    let devices: [ManagedDevice]
    let choices: IpcDeviceChoices
    let urls: [URL]?
    let onSend: (_ urls: [URL], _ target: ManagedDevice?) -> Void
    let onCancel: () -> Void
    /// The selected device's id, `allId` for every device.
    @State private var selection: String
    @State private var dropHover = false
    private static let allId = "*all*"

    init(devices: [ManagedDevice], choices: IpcDeviceChoices, urls: [URL]?,
         onSend: @escaping (_ urls: [URL], _ target: ManagedDevice?) -> Void, onCancel: @escaping () -> Void) {
        self.devices = devices
        self.choices = choices
        self.urls = urls
        self.onSend = onSend
        self.onCancel = onCancel
        let usable = choices.usable
        _selection = State(initialValue: (usable.first { $0.last_used } ?? usable.first)?.id ?? Self.allId)
    }

    private var target: ManagedDevice? { selection == Self.allId ? nil : devices.first { $0.id == selection } }
    private var sendLabel: String {
        if selection == Self.allId { return "Send to all" }
        return "Send to \(choices.usable.first { $0.id == selection }?.name ?? "device")"
    }

    var body: some View {
        ModalCard(
            icon: "paperplane.fill",
            title: urls.map { "Send \($0.count) item\($0.count == 1 ? "" : "s")" } ?? "Send files & folders",
            subtitle: choices.usable.count == 1 ? "Only \(choices.usable[0].name) is connected" : "\(choices.usable.count) devices are connected",
            width: 480
        ) {
            VStack(spacing: 8) {
                if choices.offers_all {
                    row(id: Self.allId, icon: nil, symbol: "rectangle.stack.fill", title: "All devices",
                        detail: "\(choices.usable.count) connected", enabled: true)
                }
                ForEach(choices.usable) { d in
                    row(id: d.id, icon: osFor(d), symbol: nil, title: d.name,
                        detail: [d.platform, d.last_used ? "used last" : nil].compactMap { $0 }.joined(separator: " · "), enabled: true)
                }
                if !choices.unusable.isEmpty {
                    Text("Not available")
                        .font(.system(size: 11.5, weight: .semibold))
                        .foregroundStyle(CRTheme.inkSoft)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.top, 8)
                    ForEach(choices.unusable) { d in
                        row(id: d.id, icon: osFor(d), symbol: nil, title: d.name, detail: d.reason ?? "", enabled: false)
                    }
                }
            }

            if let urls {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(urls.prefix(4), id: \.self) { url in
                        HStack(spacing: 8) {
                            Image(systemName: isFolder(url) ? "folder.fill" : "doc.fill")
                                .foregroundStyle(CRTheme.brandElectric)
                            Text(url.lastPathComponent).font(.system(size: 13)).lineLimit(1).truncationMode(.middle)
                        }
                    }
                    if urls.count > 4 {
                        Text("and \(urls.count - 4) more").font(.system(size: 12)).foregroundStyle(CRTheme.inkSoft)
                    }
                }
                .padding(12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(CRTheme.surfaceStrong))
                ModalButtons(confirm: sendLabel, onCancel: onCancel, onConfirm: { onSend(urls, target) })
            } else {
                VStack(spacing: 10) {
                    Image(systemName: "tray.and.arrow.down.fill")
                        .font(.system(size: 24, weight: .semibold))
                        .foregroundStyle(dropHover ? CRTheme.brandElectric : CRTheme.inkSubtle)
                    Text("Drop files or folders here")
                        .font(.system(size: 13.5, weight: .semibold))
                        .foregroundStyle(CRTheme.ink)
                    Button("Choose files or folders…", action: choose)
                        .buttonStyle(CRPrimaryButtonStyle())
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 26)
                .background(
                    RoundedRectangle(cornerRadius: 16, style: .continuous)
                        .fill(dropHover ? CRTheme.brandElectric.opacity(0.08) : CRTheme.surfaceStrong)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 16, style: .continuous)
                        .strokeBorder(dropHover ? CRTheme.brandElectric : CRTheme.stroke, style: StrokeStyle(lineWidth: 1.5, dash: [6, 5]))
                )
                .onDrop(of: [.fileURL], isTargeted: $dropHover) { providers in
                    loadURLs(providers) { dropped in if !dropped.isEmpty { onSend(dropped, target) } }
                    return true
                }
                HStack {
                    Spacer()
                    Button("Cancel", action: onCancel).buttonStyle(CRSecondaryButtonStyle()).keyboardShortcut(.cancelAction)
                }
            }
        }
    }

    private func osFor(_ d: IpcDeviceChoice) -> DeviceOS {
        devices.first { $0.id == d.id }?.os ?? DeviceOS(platform: d.platform, name: d.name)
    }

    /// A device row: logo, name, detail and a selection mark. Greyed and inert when not `enabled`.
    @ViewBuilder
    private func row(id: String, icon: DeviceOS?, symbol: String?, title: String, detail: String, enabled: Bool) -> some View {
        let selected = enabled && selection == id
        Button { if enabled { selection = id } } label: {
            HStack(spacing: 12) {
                ZStack {
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .fill(selected ? CRTheme.surface : CRTheme.brandElectric.opacity(0.10))
                        .frame(width: 36, height: 36)
                    if let icon { OSIcon(icon, size: 16) }
                    else if let symbol { Image(systemName: symbol).font(.system(size: 14, weight: .semibold)) }
                }
                .foregroundStyle(selected ? CRTheme.brandElectric : CRTheme.inkSoft)
                VStack(alignment: .leading, spacing: 1) {
                    Text(title).font(.system(size: 13.5, weight: .semibold)).lineLimit(1)
                    if !detail.isEmpty {
                        Text(detail).font(.system(size: 11.5)).foregroundStyle(CRTheme.inkSoft).lineLimit(1)
                    }
                }
                Spacer(minLength: 8)
                if enabled {
                    Image(systemName: selected ? "checkmark.circle.fill" : "circle")
                        .font(.system(size: 16))
                        .foregroundStyle(selected ? CRTheme.brandElectric : CRTheme.inkSubtle)
                }
            }
            .foregroundStyle(CRTheme.ink)
            .padding(.horizontal, 12).padding(.vertical, 9)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(selected ? CRTheme.brandElectric.opacity(0.10) : CRTheme.surfaceStrong))
            .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(selected ? CRTheme.brandElectric : CRTheme.stroke, lineWidth: selected ? 2 : 1))
            .opacity(enabled ? 1 : 0.55)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(enabled ? title : "\(title), not available: \(detail)")
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private func choose() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true
        panel.canChooseFiles = true
        panel.canChooseDirectories = true
        panel.prompt = "Send"
        panel.level = .modalPanel + 1
        if panel.runModal() == .OK, !panel.urls.isEmpty { onSend(panel.urls, target) }
    }

    private func isFolder(_ url: URL) -> Bool {
        (try? url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) ?? false
    }

    private func loadURLs(_ providers: [NSItemProvider], done: @escaping ([URL]) -> Void) {
        var urls: [URL] = []
        let group = DispatchGroup()
        for p in providers {
            group.enter()
            _ = p.loadObject(ofClass: URL.self) { url, _ in
                if let url { DispatchQueue.main.async { urls.append(url) } }
                group.leave()
            }
        }
        group.notify(queue: .main) { done(urls) }
    }
}


/// Which phone to ring, when more than one can be asked. One tap rings it.
struct FindPhoneCard: View {
    let choices: IpcDeviceChoices
    let onPick: (IpcDeviceChoice) -> Void
    let onCancel: () -> Void

    var body: some View {
        ModalCard(icon: "bell.and.waves.left.and.right.fill", title: "Ring which phone?",
                  subtitle: "It rings even on silent until you tap Found it", width: 440) {
            VStack(spacing: 8) {
                ForEach(choices.usable) { d in row(d, enabled: true) }
                if !choices.unusable.isEmpty {
                    Text("Not available")
                        .font(.system(size: 11.5, weight: .semibold))
                        .foregroundStyle(CRTheme.inkSoft)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.top, 8)
                    ForEach(choices.unusable) { d in row(d, enabled: false) }
                }
            }
            HStack {
                Spacer()
                Button("Cancel", action: onCancel).buttonStyle(CRSecondaryButtonStyle()).keyboardShortcut(.cancelAction)
            }
        }
    }

    @ViewBuilder
    private func row(_ d: IpcDeviceChoice, enabled: Bool) -> some View {
        Button { if enabled { onPick(d) } } label: {
            HStack(spacing: 12) {
                OSIcon(DeviceOS(platform: d.platform, name: d.name), size: 16)
                    .frame(width: 36, height: 36)
                    .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(CRTheme.brandElectric.opacity(0.10)))
                VStack(alignment: .leading, spacing: 1) {
                    Text(d.name).font(.system(size: 13.5, weight: .semibold)).lineLimit(1)
                    Text(enabled ? "Ring" : (d.reason ?? ""))
                        .font(.system(size: 11.5)).foregroundStyle(CRTheme.inkSoft).lineLimit(1)
                }
                Spacer(minLength: 8)
            }
            .foregroundStyle(CRTheme.ink)
            .padding(.horizontal, 12).padding(.vertical, 9)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(CRTheme.surfaceStrong))
            .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(CRTheme.stroke, lineWidth: 1))
            .opacity(enabled ? 1 : 0.5)
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
    }
}

/// Shown while a phone rings; closing it asks the phone to stop.
struct RingingCard: View {
    let name: String
    let onStop: () -> Void

    var body: some View {
        ModalCard(icon: "bell.fill", title: "\(name) is ringing",
                  subtitle: "It rings at full volume for up to a minute", width: 400) {
            ModalButtons(cancel: nil, confirm: "Stop ringing", onCancel: onStop, onConfirm: onStop)
        }
    }
}
