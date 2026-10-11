import AppKit
import Combine
import SwiftUI

@MainActor
final class LinkAllToastWindowManager: NSObject {
    private let store: LinkAllStore
    private let panel: ToastOverlayPanel
    private let hostingView: NSHostingView<ToastOverlayPanelView>
    private var cancellables = Set<AnyCancellable>()
    /// Height of the cards, as the view measures it. The panel is exactly
    /// this tall: a taller transparent panel sat above other apps' windows
    /// and swallowed their clicks.
    private var contentHeight: CGFloat = 0
    private var shrinkWork: DispatchWorkItem?

    init(store: LinkAllStore) {
        self.store = store
        self.panel = ToastOverlayPanel()
        self.hostingView = NSHostingView(rootView: ToastOverlayPanelView(store: store, onHeightChange: { _ in }))
        super.init()
        hostingView.rootView = ToastOverlayPanelView(store: store) { [weak self] height in
            self?.contentHeightChanged(height)
        }

        hostingView.translatesAutoresizingMaskIntoConstraints = false
        panel.contentView = hostingView

        NotificationCenter.default.addObserver(
            self,
            selector: #selector(layoutPanel),
            name: NSApplication.didChangeScreenParametersNotification,
            object: nil
        )
        UserDefaults.standard.addObserver(self, forKeyPath: "overlayPositionBottomRight", options: .new, context: nil)

        Publishers.CombineLatest(store.$toasts, store.$activeTransfers)
            .receive(on: RunLoop.main)
            .sink { [weak self] toasts, transfers in
                self?.handleUpdate(toasts: toasts, transfers: transfers)
            }
            .store(in: &cancellables)
    }

    deinit {
        NotificationCenter.default.removeObserver(self)
        UserDefaults.standard.removeObserver(self, forKeyPath: "overlayPositionBottomRight")
    }

    override func observeValue(forKeyPath keyPath: String?, of object: Any?, change: [NSKeyValueChangeKey : Any]?, context: UnsafeMutableRawPointer?) {
        if keyPath == "overlayPositionBottomRight" {
            DispatchQueue.main.async { self.layoutPanel() }
        } else {
            super.observeValue(forKeyPath: keyPath, of: object, change: change, context: context)
        }
    }

    private func handleUpdate(toasts: [ToastItem], transfers: [FileTransferState]) {
        layoutPanel()
        if toasts.isEmpty && transfers.isEmpty {
            panel.orderOut(nil)
        } else {
            panel.orderFrontRegardless()
        }
    }

    /// Grow at once so a new card is never clipped; shrink after the
    /// removal animation has played.
    private func contentHeightChanged(_ height: CGFloat) {
        shrinkWork?.cancel()
        if height >= contentHeight {
            contentHeight = height
            layoutPanel()
        } else {
            let work = DispatchWorkItem { [weak self] in
                self?.contentHeight = height
                self?.layoutPanel()
            }
            shrinkWork = work
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.35, execute: work)
        }
    }

    @objc private func layoutPanel() {
        guard let screen = activeScreen else { return }
        let visible = screen.visibleFrame
        let width: CGFloat = 360
        let height: CGFloat = max(1, min(visible.height - 24, contentHeight))
        let isBottomRight = UserDefaults.standard.bool(forKey: "overlayPositionBottomRight")
        
        let yPos: CGFloat = isBottomRight 
            ? visible.minY + 12 
            : visible.maxY - height - 12
            
        let frame = NSRect(
            x: visible.maxX - width - 16,
            y: yPos,
            width: width,
            height: height
        )
        panel.setFrame(frame, display: false)
    }

    private var activeScreen: NSScreen? {
        if let key = NSApp.keyWindow?.screen {
            return key
        }
        let mouse = NSEvent.mouseLocation
        return NSScreen.screens.first { NSMouseInRect(mouse, $0.frame, false) } ?? NSScreen.main
    }
}

private final class ToastOverlayPanel: NSPanel {
    init() {
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 400, height: 400),
            // Non-activating: clicking a card's button never takes focus
            // from the app being used.
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        becomesKeyOnlyIfNeeded = true

        level = .statusBar
        hasShadow = false
        isOpaque = false
        backgroundColor = .clear
        hidesOnDeactivate = false
        ignoresMouseEvents = false
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .ignoresCycle]
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

private struct ToastOverlayPanelView: View {
    @ObservedObject var store: LinkAllStore
    let onHeightChange: (CGFloat) -> Void

    var body: some View {
        VStack(alignment: .center, spacing: 8) {
            // Dynamic Island Transfers (Highest Priority)
            // One card per folder, not per file in flight.
            if store.batchedTransfers.count > 1 {
                GroupedDynamicIslandTransferCard(transfers: store.batchedTransfers, store: store)
                    .transition(.asymmetric(
                        insertion: .move(edge: .top).combined(with: .opacity).combined(with: .scale(scale: 0.85)),
                        removal: .opacity.combined(with: .scale(scale: 0.95))
                    ))
                    .zIndex(1000)
            } else {
                ForEach(store.batchedTransfers) { transfer in
                    DynamicIslandTransferCard(transfer: transfer, store: store)
                        .transition(.asymmetric(
                            insertion: .move(edge: .top).combined(with: .opacity).combined(with: .scale(scale: 0.85)),
                            removal: .opacity.combined(with: .scale(scale: 0.95))
                        ))
                        .zIndex(Double(transfer.id.hashValue))
                }
            }
            
            // Standard Toasts
            ForEach(Array(store.toasts.suffix(3).reversed())) { toast in
                ToastOverlayCard(
                    toast: toast,
                    onDismiss: { store.dismissToast(id: toast.id) }
                )
                .transition(.asymmetric(
                    insertion: .move(edge: .top).combined(with: .opacity).combined(with: .scale(scale: 0.85)),
                    removal: .opacity.combined(with: .scale(scale: 0.95))
                ))
                .zIndex(Double(toast.id.hashValue))
            }
        }
        .padding(.top, 4)
        .frame(maxWidth: .infinity, alignment: .top)
        .padding(10)
        .fixedSize(horizontal: false, vertical: true)
        .background(GeometryReader { geo in
            Color.clear
                .onAppear { onHeightChange(geo.size.height) }
                .onChange(of: geo.size.height) { onHeightChange($0) }
        })
        .frame(maxHeight: .infinity, alignment: .top)
        .animation(.spring(response: 0.3, dampingFraction: 0.8), value: store.toasts.map(\.id.uuidString) + store.batchedTransfers.map(\.id))
    }
}

// MARK: - Dynamic Island Card

private struct DynamicIslandTransferCard: View {
    let transfer: FileTransferState
    @ObservedObject var store: LinkAllStore

    var progressColor: Color {
        switch transfer.status {
        case .paused: return CRTheme.stroke
        case .failed: return CRTheme.accentRed
        default: return CRTheme.brandElectric
        }
    }

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            // Left Icon
            Image(systemName: transfer.isDirectory ? "folder.fill" : "arrow.down.doc.fill")
                .font(.system(size: 16, weight: .semibold, design: .rounded))
                .foregroundStyle(progressColor)
                .frame(width: 20)

            // Content Column
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(transfer.fileName)
                        .font(.system(size: 13, weight: .bold, design: .rounded))
                        .foregroundStyle(Color.primary)
                        .lineLimit(1)
                    
                    Text("• \(transfer.exactPercentString)")
                        .font(.system(size: 11, weight: .bold, design: .default))
                        .foregroundStyle(progressColor)
                        .lineLimit(1)
                }
                
                if case .queued = transfer.status {
                    Text("Queued... (\(transfer.fromDeviceName))")
                        .font(.system(size: 12, weight: .medium, design: .default))
                        .foregroundStyle(Color.primary.opacity(0.7))
                        .lineLimit(1)
                } else if transfer.isDirectory {
                    Text("\(transfer.itemLabel) · \(transfer.fromDeviceName)")
                        .font(.system(size: 12, weight: .medium, design: .default))
                        .foregroundStyle(Color.primary.opacity(0.7))
                        .lineLimit(1)
                } else {
                    Text(transfer.directionLabel)
                        .font(.system(size: 12, weight: .medium, design: .default))
                        .foregroundStyle(Color.primary.opacity(0.7))
                        .lineLimit(1)
                }

                if case .incoming = transfer.status {
                    // Waiting state
                } else if case .queued = transfer.status {
                    // Waiting in queue
                } else if case .failed = transfer.status {
                    // Failed state
                } else {
                    GeometryReader { geo in
                        ZStack(alignment: .leading) {
                            Capsule()
                                .fill(Color.primary.opacity(0.1))
                                .frame(height: 4)
                            
                            Capsule()
                                .fill(progressColor)
                                .frame(width: max(0, geo.size.width * CGFloat(transfer.exactRatio)), height: 4)
                                .animation(.linear(duration: 0.5), value: transfer.exactRatio)
                        }
                    }
                    .frame(height: 4)
                    .padding(.top, 4)
                }
            }

            Spacer(minLength: 8)

            // Dynamic Action Button based on status
            if case .incoming = transfer.status {
                HStack(spacing: 4) {
                    Button(action: { store.acceptFileTransfer(transfer) }) {
                        Image(systemName: "checkmark.circle.fill")
                            .font(.system(size: 16, weight: .medium))
                            .foregroundStyle(CRTheme.accentGreen)
                    }
                    .buttonStyle(.plain)
                    
                    Button(action: { store.rejectFileTransfer(transfer) }) {
                        Image(systemName: "xmark.circle.fill")
                            .font(.system(size: 16, weight: .medium))
                            .foregroundStyle(CRTheme.accentRed)
                    }
                    .buttonStyle(.plain)
                }
            } else if transfer.isDirectory {
                // A folder pauses file by file in the engine; offer Cancel.
                Button(action: { store.cancelFileTransfer(transfer) }) {
                    Image(systemName: "xmark.circle.fill")
                        .font(.system(size: 14, weight: .medium))
                        .foregroundStyle(Color.primary.opacity(0.4))
                }
                .buttonStyle(.plain)
            } else if case .transferring = transfer.status {
                Button(action: { store.pauseFileTransfer(transfer) }) {
                    Image(systemName: "pause.circle.fill")
                        .font(.system(size: 14, weight: .medium))
                        .foregroundStyle(Color.primary.opacity(0.4))
                }
                .buttonStyle(.plain)
            } else if case .paused = transfer.status {
                Button(action: { store.resumeFileTransfer(transfer) }) {
                    Image(systemName: "play.circle.fill")
                        .font(.system(size: 14, weight: .medium))
                        .foregroundStyle(Color.primary.opacity(0.4))
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .frame(minWidth: 260, maxWidth: 360, alignment: .leading)
        .background {
            Capsule(style: .continuous)
                .fill(CRTheme.surface.opacity(0.85))
                .overlay(
                    Capsule(style: .continuous)
                        .strokeBorder(Color.primary.opacity(0.1), lineWidth: 0.5)
                )
                .shadow(color: Color.black.opacity(0.2), radius: 20, x: 0, y: 10)
        }
    }
}

// MARK: - Grouped Dynamic Island Card

private struct GroupedDynamicIslandTransferCard: View {
    let transfers: [FileTransferState]
    @ObservedObject var store: LinkAllStore

    var totalBytesReceived: Int64 {
        transfers.reduce(0) { $0 + $1.bytesReceived }
    }
    
    var totalBytesExpected: Int64 {
        transfers.reduce(0) { $0 + $1.totalBytes }
    }
    
    var exactRatio: Double {
        // Folders report their own progress; bytes of their files in
        // flight say nothing about the whole folder.
        if transfers.contains(where: { $0.isDirectory }) {
            return transfers.reduce(0) { $0 + $1.exactRatio } / Double(max(1, transfers.count))
        }
        let expected = totalBytesExpected
        if expected > 0 {
            return min(1.0, max(0.0, Double(totalBytesReceived) / Double(expected)))
        }
        // Fallback to average percent if totalBytes are missing
        let totalPercent = transfers.reduce(0) { $0 + $1.percent }
        return Double(totalPercent) / Double(transfers.count * 100)
    }

    var exactPercentString: String {
        String(format: "%.1f%%", exactRatio * 100.0)
    }

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            // Left Icon
            Image(systemName: "square.stack.3d.down.right.fill")
                .font(.system(size: 16, weight: .semibold, design: .rounded))
                .foregroundStyle(CRTheme.brandElectric)
                .frame(width: 20)

            // Content Column
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text("\(transfers.count) transfers")
                        .font(.system(size: 13, weight: .bold, design: .rounded))
                        .foregroundStyle(Color.primary)
                        .lineLimit(1)
                    
                    Text("• \(exactPercentString)")
                        .font(.system(size: 11, weight: .bold, design: .default))
                        .foregroundStyle(CRTheme.brandElectric)
                        .lineLimit(1)
                }
                
                Text("From \(transfers.first?.fromDeviceName ?? "Multiple Devices")")
                    .font(.system(size: 12, weight: .medium, design: .default))
                    .foregroundStyle(Color.primary.opacity(0.7))
                    .lineLimit(1)

                GeometryReader { geo in
                    ZStack(alignment: .leading) {
                        Capsule()
                            .fill(Color.primary.opacity(0.1))
                            .frame(height: 4)
                        
                        Capsule()
                            .fill(CRTheme.brandElectric)
                            .frame(width: max(0, geo.size.width * CGFloat(exactRatio)), height: 4)
                            .animation(.linear(duration: 0.5), value: exactRatio)
                    }
                }
                .frame(height: 4)
                .padding(.top, 4)
            }

            Spacer(minLength: 8)

            // Cancel all button
            Button(action: {
                // These are under way: cancel, not decline.
                for t in transfers {
                    store.cancelFileTransfer(t)
                }
            }) {
                Image(systemName: "xmark.circle.fill")
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(Color.primary.opacity(0.4))
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .frame(minWidth: 260, maxWidth: 360, alignment: .leading)
        .background {
            Capsule(style: .continuous)
                .fill(CRTheme.surface.opacity(0.85))
                .overlay(
                    Capsule(style: .continuous)
                        .strokeBorder(Color.primary.opacity(0.1), lineWidth: 0.5)
                )
                .shadow(color: Color.black.opacity(0.2), radius: 20, x: 0, y: 10)
        }
    }
}

private struct ToastOverlayCard: View {
    let toast: ToastItem
    let onDismiss: () -> Void

    @Environment(\.colorScheme) var colorScheme
    @State private var hovered = false

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            // Left Icon
            Image(systemName: toast.systemImage)
                .font(.system(size: 16, weight: .semibold, design: .rounded))
                .foregroundStyle(toast.tint)
                .frame(width: 20)

            // Content Column
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(toast.title)
                        .font(.system(size: 13, weight: .bold, design: .rounded))
                        .foregroundStyle(Color.primary)
                        .lineLimit(1)
                    
                    if let detail = toast.detail, !detail.isEmpty {
                        Text("• " + detail)
                            .font(.system(size: 11, weight: .medium, design: .default))
                            .foregroundStyle(Color.primary.opacity(0.5))
                            .lineLimit(1)
                    }
                }
                
                if !toast.body.isEmpty {
                    Text(toast.body)
                        .font(.system(size: 12, weight: .medium, design: .default))
                        .foregroundStyle(Color.primary.opacity(0.7))
                        .lineLimit(2)
                }

                if let progress = toast.progress {
                    CRProgressBar(value: progress, tint: toast.tint, height: 4)
                        .padding(.top, 4)
                }

                if toast.primaryAction != nil || toast.secondaryAction != nil {
                    HStack(spacing: 8) {
                        if let secondary = toast.secondaryAction {
                            ToastOverlayButton(action: secondary)
                        }
                        if let primary = toast.primaryAction {
                            ToastOverlayButton(action: primary)
                        }
                    }
                    .padding(.top, 6)
                }
            }

            Spacer(minLength: 8)

            // Close Button
            if hovered {
                Button(action: onDismiss) {
                    Image(systemName: "xmark.circle.fill")
                        .font(.system(size: 14, weight: .medium))
                        .foregroundStyle(Color.primary.opacity(0.4))
                }
                .buttonStyle(.plain)
                .transition(.opacity)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .frame(minWidth: 260, maxWidth: 360, alignment: .leading)
        .background {
            Capsule(style: .continuous)
                .fill(CRTheme.surface.opacity(0.85))
                .overlay(
                    Capsule(style: .continuous)
                        .strokeBorder(Color.primary.opacity(0.1), lineWidth: 0.5)
                )
                .shadow(color: Color.black.opacity(0.2), radius: 20, x: 0, y: 10)
        }
        .scaleEffect(hovered ? 1.01 : 1.0)
        .onHover { isHovering in withAnimation(.easeOut(duration: 0.15)) { hovered = isHovering } }
    }
}

private struct ToastOverlayButton: View {
    let action: ToastAction
    @State private var isHovered = false
    @Environment(\.colorScheme) var colorScheme

    var body: some View {
        Button(action: action.handler) {
            Text(action.title)
                .font(.system(size: 11, weight: .semibold, design: .rounded))
                .foregroundStyle(action.role == .secondary ? Color.primary.opacity(0.8) : (colorScheme == .dark ? Color.black : Color.white))
                .padding(.horizontal, 14)
                .padding(.vertical, 6)
                .background {
                    Capsule()
                        .fill(
                            action.role == .secondary
                            ? Color.primary.opacity(isHovered ? 0.2 : 0.1)
                            : toastTintOrAccent()
                        )
                }
        }
        .buttonStyle(.plain)
        .scaleEffect(isHovered ? 1.05 : 1.0)
        .onHover { isHovering in withAnimation(.easeOut(duration: 0.15)) { isHovered = isHovering } }
    }
    
    private func toastTintOrAccent() -> Color {
        if action.role == .destructive { return .red }
        return colorScheme == .dark ? .white : .black
    }
}
