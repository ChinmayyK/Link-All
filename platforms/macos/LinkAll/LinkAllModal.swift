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

/// "Files & folders": where to send (all devices unless one is picked),
/// then what. With `urls` already chosen (a drop, the Finder service) it
/// only asks where; otherwise it takes a drop or opens the picker, which
/// accepts files and folders together.
struct SendModalCard: View {
    let devices: [ManagedDevice]
    let urls: [URL]?
    let onSend: (_ urls: [URL], _ target: ManagedDevice?) -> Void
    let onCancel: () -> Void
    @State private var targetId: String? = nil
    @State private var dropHover = false

    private var target: ManagedDevice? { devices.first { $0.id == targetId } }

    var body: some View {
        ModalCard(
            icon: "paperplane.fill",
            title: urls.map { "Send \($0.count) item\($0.count == 1 ? "" : "s")" } ?? "Send files & folders",
            subtitle: devices.count == 1 ? "To \(devices[0].name)" : "To all your devices, or pick one",
            width: 480
        ) {
            if devices.count > 1 {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        chip(nil, icon: "rectangle.stack.fill", label: "All devices")
                        ForEach(devices) { d in chip(d.id, icon: nil, label: d.name, os: d.os) }
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
                ModalButtons(confirm: "Send", onCancel: onCancel, onConfirm: { onSend(urls, target) })
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

    @ViewBuilder
    private func chip(_ id: String?, icon: String?, label: String, os: DeviceOS? = nil) -> some View {
        let selected = targetId == id
        Button { targetId = id } label: {
            HStack(spacing: 6) {
                if let icon { Image(systemName: icon).font(.system(size: 11, weight: .semibold)) }
                else if let os { OSIcon(os, size: 12) }
                Text(label).font(.system(size: 12.5, weight: .medium)).lineLimit(1)
            }
            .foregroundStyle(selected ? Color.white : CRTheme.ink)
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
            .background(Capsule().fill(selected ? CRTheme.brandElectric : CRTheme.surfaceStrong))
            .overlay(Capsule().strokeBorder(selected ? CRTheme.brandElectric : CRTheme.stroke, lineWidth: 1))
        }
        .buttonStyle(.plain)
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
