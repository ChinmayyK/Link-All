import SwiftUI
import UniformTypeIdentifiers
import AppKit

@MainActor
class EdgeDropWindowManager: NSObject {
    static let shared = EdgeDropWindowManager()
    
    private var edgePanel: NSPanel?
    private var store: LinkAllStore?
    private var isExpanded = false
    
    private let restingWidth: CGFloat = 16
    private let restingHeight: CGFloat = 200
    private let expandedWidth: CGFloat = 280
    private let expandedHeight: CGFloat = 240
    
    func setup(with store: LinkAllStore) {
        self.store = store
        ensurePanel()
        updatePosition(expanded: false, animated: false)
    }
    
    private func ensurePanel() {
        if edgePanel != nil { return }
        guard let store = store else { return }
        
        let panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: restingWidth, height: restingHeight),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = false
        panel.level = .floating
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        
        let hostView = EdgeDropHostingView(store: store, manager: self)
        panel.contentView = hostView
        edgePanel = panel
        panel.orderFrontRegardless()
    }
    
    func updatePosition(expanded: Bool, animated: Bool) {
        guard let panel = edgePanel, let screen = NSScreen.main else { return }
        self.isExpanded = expanded
        panel.hasShadow = expanded
        
        let visibleFrame = screen.visibleFrame
        let width = expanded ? expandedWidth : restingWidth
        let height = expanded ? expandedHeight : restingHeight
        let x = visibleFrame.minX
        let y = visibleFrame.midY - (height / 2)
        
        let newFrame = NSRect(x: x, y: y, width: width, height: height)
        if animated {
            NSAnimationContext.runAnimationGroup { context in
                context.duration = 0.25
                context.timingFunction = CAMediaTimingFunction(name: .easeOut)
                panel.animator().setFrame(newFrame, display: true)
            }
        } else {
            panel.setFrame(newFrame, display: true)
        }
    }
    
    func handleDragEntered() {
        guard !isExpanded else { return }
        NSHapticFeedbackManager.defaultPerformer.perform(.alignment, performanceTime: .default)
        updatePosition(expanded: true, animated: true)
    }
    
    func handleDragExited() {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [weak self] in
            guard let self = self, let panel = self.edgePanel else { return }
            let mouseLoc = NSEvent.mouseLocation
            if panel.frame.contains(mouseLoc) { return }
            self.updatePosition(expanded: false, animated: true)
        }
    }
    
    func handleDrop(urls: [URL]) {
        guard let store = store, !urls.isEmpty else {
            updatePosition(expanded: false, animated: true)
            return
        }
        
        NSHapticFeedbackManager.defaultPerformer.perform(.generic, performanceTime: .default)
        NSSound(named: "Glass")?.play()
        
        NotificationCenter.default.post(name: NSNotification.Name("linkallTriggerParticles"), object: nil)
        
        // Defer past the drag session: choosing a target may show a modal prompt.
        DispatchQueue.main.async { [weak self] in
            store.sendFilesChoosingTarget(urls: urls) { [weak self] sent in
                guard sent else {
                    self?.updatePosition(expanded: false, animated: true)
                    return
                }
                store.showToast(
                    title: "Instant Portal Transfer (\(urls.count) file\(urls.count == 1 ? "" : "s"))",
                    body: urls.map(\.lastPathComponent).joined(separator: ", "),
                    tint: CRTheme.brandElectric,
                    systemImage: "arrow.right.to.line.compact",
                    ttl: 3.5
                )
                DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) { [weak self] in
                    self?.updatePosition(expanded: false, animated: true)
                }
            }
        }
    }
}

@MainActor
class EdgeDropHostingView: NSView {
    let store: LinkAllStore
    weak var manager: EdgeDropWindowManager?
    private var hostingController: NSHostingController<EdgeDropSwiftUIView>?
    private var isTargetedBinding = Binding<Bool>(
        get: { false },
        set: { _ in }
    )
    private var isTargeted = false {
        didSet {
            updateSwiftUIView()
        }
    }
    
    init(store: LinkAllStore, manager: EdgeDropWindowManager) {
        self.store = store
        self.manager = manager
        super.init(frame: .zero)
        registerForDraggedTypes([
            .fileURL,
            .init(rawValue: "com.apple.pasteboard.promised-file-url"),
            .init(rawValue: "com.apple.NSFilePromiseItemMetaData")
        ])
        setupHosting()
    }
    
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }
    
    private func setupHosting() {
        let binding = Binding<Bool>(
            get: { [weak self] in self?.isTargeted ?? false },
            set: { [weak self] val in self?.isTargeted = val }
        )
        let swiftUIView = EdgeDropSwiftUIView(store: store, isTargeted: binding)
        let hc = NSHostingController(rootView: swiftUIView)
        hc.view.autoresizingMask = [.width, .height]
        hc.view.frame = bounds
        addSubview(hc.view)
        hostingController = hc
    }
    
    private func updateSwiftUIView() {
        let binding = Binding<Bool>(
            get: { [weak self] in self?.isTargeted ?? false },
            set: { [weak self] val in self?.isTargeted = val }
        )
        hostingController?.rootView = EdgeDropSwiftUIView(store: store, isTargeted: binding)
    }
    
    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        guard store.connectedCount > 0 else { return [] }
        isTargeted = true
        manager?.handleDragEntered()
        return .copy
    }
    
    override func draggingExited(_ sender: NSDraggingInfo?) {
        isTargeted = false
        manager?.handleDragExited()
    }
    
    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        isTargeted = false
        guard let pboard = sender.draggingPasteboard.propertyList(forType: .fileURL) as? String else {
            if let urls = sender.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: nil) as? [URL], !urls.isEmpty {
                manager?.handleDrop(urls: urls)
                return true
            }
            manager?.handleDragExited()
            return false
        }
        if let url = URL(string: pboard) {
            manager?.handleDrop(urls: [url])
            return true
        }
        if let urls = sender.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: nil) as? [URL], !urls.isEmpty {
            manager?.handleDrop(urls: urls)
            return true
        }
        manager?.handleDragExited()
        return false
    }
}

struct EdgeDropSwiftUIView: View {
    @ObservedObject var store: LinkAllStore
    @Binding var isTargeted: Bool
    @State private var pulse = false
    @State private var triggerBurst = false
    
    private var connectedDevices: [ManagedDevice] {
        store.devices.filter { $0.isConnected }
    }
    
    var body: some View {
        ZStack {
            if store.connectedCount > 0 {
                if isTargeted {
                    // Expanded Portal Card
                    ZStack {
                        CRHUDMaterial()
                            .ignoresSafeArea()
                        
                        LinearGradient(
                            colors: [CRTheme.brandElectric.opacity(0.30), CRTheme.brandCyan.opacity(0.20)],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        )
                        .blur(radius: 20)
                        
                        RoundedRectangle(cornerRadius: 16, style: .continuous)
                            .strokeBorder(CRTheme.brandElectric, lineWidth: 2)
                            .shadow(color: CRTheme.brandElectric.opacity(0.6), radius: 12)
                        
                        // Animated radar rings
                        RoundedRectangle(cornerRadius: 16, style: .continuous)
                            .stroke(CRTheme.brandCyan, lineWidth: 1.5)
                            .scaleEffect(pulse ? 1.05 : 1.0)
                            .opacity(pulse ? 0 : 0.8)
                        
                        VStack(spacing: 14) {
                            ZStack {
                                Circle()
                                    .fill(CRTheme.brandElectric.opacity(0.25))
                                    .frame(width: 56, height: 56)
                                    .overlay(
                                        Circle()
                                            .strokeBorder(CRTheme.brandElectric, lineWidth: 2)
                                    )
                                Image(systemName: "arrow.right.to.line.compact")
                                    .font(.system(size: 24, weight: .bold))
                                    .foregroundStyle(CRTheme.brandElectric)
                            }
                            .scaleEffect(pulse ? 1.08 : 1.0)
                            
                            VStack(spacing: 4) {
                                Text("Edge Portal Drop ✨")
                                    .font(.system(size: 15, weight: .bold, design: .rounded))
                                    .foregroundStyle(CRTheme.ink)
                                
                                Text(connectedDevices.isEmpty ? "Sending to active mesh" : "Instant transfer to \(connectedDevices.map(\.name).joined(separator: ", "))")
                                    .font(.system(size: 11.5, weight: .medium))
                                    .foregroundStyle(CRTheme.inkSubtle)
                                    .multilineTextAlignment(.center)
                                    .lineLimit(2)
                                    .padding(.horizontal, 12)
                            }
                        }
                    }
                    .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
                    .padding(8)
                    .onAppear {
                        withAnimation(.easeInOut(duration: 0.8).repeatForever(autoreverses: true)) {
                            pulse = true
                        }
                    }
                    
                    ParticleEffectView(isTriggered: triggerBurst)
                        .allowsHitTesting(false)
                        .onReceive(NotificationCenter.default.publisher(for: NSNotification.Name("linkallTriggerParticles"))) { _ in
                            triggerBurst = true
                            DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) { triggerBurst = false }
                        }
                } else {
                    // Resting Edge Sliver
                    HStack {
                        Capsule()
                            .fill(
                                LinearGradient(
                                    colors: [CRTheme.brandElectric, CRTheme.brandCyan],
                                    startPoint: .top,
                                    endPoint: .bottom
                                )
                            )
                            .frame(width: 5, height: 90)
                            .shadow(color: CRTheme.brandElectric.opacity(0.6), radius: 6)
                        Spacer()
                    }
                    .padding(.leading, 2)
                }
            }
        }
    }
}
