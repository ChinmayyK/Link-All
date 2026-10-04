import AppKit
import SwiftUI
import Combine

class CameraStreamState: ObservableObject {
    @Published var image: NSImage? = nil
    @Published var isWaiting: Bool = true
    @Published var pulse: Bool = false
    @Published var isPinned: Bool = false
}

struct CameraPreviewView: View {
    @ObservedObject var state: CameraStreamState
    var onClose: () -> Void
    
    @State private var showControls = false
    @State private var hideControlsTask: Task<Void, Never>? = nil
    @State private var isHoveringClose = false
    @State private var isHoveringPin = false
    
    var body: some View {
        ZStack {
            // Background
            VisualEffectView(material: .hudWindow, blendingMode: .behindWindow)
                .edgesIgnoringSafeArea(.all)
            
            // Completely edge-to-edge camera feed using CoreAnimation for perfect aspect-fill
            if let img = state.image {
                CameraFeedView(image: img)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .ignoresSafeArea()
                    .transition(.opacity.combined(with: .scale(scale: 0.98)))
            }
            
            // Refined waiting state
            if state.isWaiting {
                VisualEffectView(material: .hudWindow, blendingMode: .withinWindow)
                    .edgesIgnoringSafeArea(.all)
                    .transition(.opacity)
                
                VStack(spacing: 24) {
                    ZStack {
                        Circle()
                            .fill(Color.white.opacity(0.1))
                            .frame(width: 72, height: 72)
                            .blur(radius: state.pulse ? 20 : 0)
                            .animation(Animation.easeInOut(duration: 1.5).repeatForever(autoreverses: true), value: state.pulse)
                        
                        Circle()
                            .stroke(Color.white.opacity(0.2), lineWidth: 3)
                            .frame(width: 64, height: 64)
                        
                        Circle()
                            .trim(from: 0, to: 0.7)
                            .stroke(Color.white, style: StrokeStyle(lineWidth: 3, lineCap: .round))
                            .frame(width: 64, height: 64)
                            .rotationEffect(Angle(degrees: state.pulse ? 360 : 0))
                            .animation(Animation.linear(duration: 1.2).repeatForever(autoreverses: false), value: state.pulse)
                        
                        Image(systemName: "camera.fill")
                            .font(.system(size: 20))
                            .foregroundColor(.white.opacity(0.8))
                    }
                    .onAppear { state.pulse = true }
                    
                    Text("Connecting to Camera...")
                        .font(.system(size: 15, weight: .medium, design: .rounded))
                        .foregroundColor(Color.white.opacity(0.9))
                }
                .transition(.opacity)
            }
            
            // Floating control HUD
            VStack {
                Spacer()
                
                HStack(spacing: 16) {
                    // Pin Button
                    Button(action: {
                        state.isPinned.toggle()
                    }) {
                        Image(systemName: state.isPinned ? "pin.fill" : "pin")
                            .font(.system(size: 16, weight: .semibold))
                            .foregroundColor(state.isPinned ? .white : .white.opacity(0.8))
                            .frame(width: 44, height: 44)
                            .background(
                                Circle()
                                    .fill(state.isPinned ? Color.blue.opacity(0.8) : Color.white.opacity(0.1))
                            )
                            .overlay(Circle().stroke(Color.white.opacity(0.2), lineWidth: 0.5))
                            .scaleEffect(isHoveringPin ? 1.05 : 1.0)
                            .animation(.spring(response: 0.3, dampingFraction: 0.6), value: isHoveringPin)
                    }
                    .buttonStyle(PlainButtonStyle())
                    .onHover { hovering in isHoveringPin = hovering }
                    .help("Keep Window on Top")
                    
                    // Stop Streaming Button
                    Button(action: onClose) {
                        HStack(spacing: 8) {
                            Image(systemName: "xmark.circle.fill")
                                .font(.system(size: 16, weight: .bold))
                            Text("Stop")
                                .font(.system(size: 14, weight: .bold, design: .rounded))
                        }
                        .padding(.horizontal, 20)
                        .padding(.vertical, 12)
                        .background(
                            isHoveringClose
                            ? Color.red.opacity(0.9)
                            : Color.red.opacity(0.7)
                        )
                        .overlay(
                            Capsule().stroke(Color.white.opacity(0.2), lineWidth: 1)
                        )
                        .foregroundColor(.white)
                        .clipShape(Capsule())
                        .shadow(color: isHoveringClose ? Color.red.opacity(0.4) : Color.black.opacity(0.3), radius: 8, x: 0, y: 4)
                        .scaleEffect(isHoveringClose ? 1.05 : 1.0)
                        .animation(.spring(response: 0.3, dampingFraction: 0.6), value: isHoveringClose)
                    }
                    .buttonStyle(PlainButtonStyle())
                    .onHover { hovering in isHoveringClose = hovering }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                .background(
                    VisualEffectView(material: .hudWindow, blendingMode: .withinWindow)
                        .clipShape(Capsule())
                        .shadow(color: .black.opacity(0.2), radius: 15, x: 0, y: 10)
                )
                .overlay(Capsule().stroke(Color.white.opacity(0.15), lineWidth: 1))
                .padding(.bottom, 24)
                .opacity(showControls || state.isWaiting ? 1.0 : 0.0)
                .scaleEffect(showControls || state.isWaiting ? 1.0 : 0.95)
                .animation(.spring(response: 0.4, dampingFraction: 0.7), value: showControls)
                .animation(.spring(response: 0.4, dampingFraction: 0.7), value: state.isWaiting)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .ignoresSafeArea()
        .onHover { hovering in
            withAnimation {
                showControls = hovering
            }
            if hovering {
                hideControlsTask?.cancel()
                hideControlsTask = Task {
                    try? await Task.sleep(nanoseconds: 3_000_000_000)
                    guard !Task.isCancelled else { return }
                    withAnimation { showControls = false }
                }
            }
        }
        .animation(.easeInOut(duration: 0.4), value: state.isWaiting)
    }
}

struct VisualEffectView: NSViewRepresentable {
    let material: NSVisualEffectView.Material
    let blendingMode: NSVisualEffectView.BlendingMode
    
    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = material
        view.blendingMode = blendingMode
        view.state = .active
        return view
    }
    
    func updateNSView(_ nsView: NSVisualEffectView, context: Context) {
        nsView.material = material
        nsView.blendingMode = blendingMode
    }
}

// A purely layer-hosted view that guarantees absolute perfect aspect-fill
// using Core Animation directly, completely bypassing any AppKit layout quirks.
class AspectFillImageView: NSView {
    private let imageLayer = CALayer()
    
    var image: NSImage? {
        didSet {
            updateImage()
        }
    }
    
    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        setup()
    }
    
    required init?(coder: NSCoder) {
        super.init(coder: coder)
        setup()
    }
    
    private func setup() {
        self.wantsLayer = true
        
        // Use a pure Core Animation layer for exact aspect fill
        imageLayer.contentsGravity = .resizeAspectFill
        imageLayer.masksToBounds = true
        self.layer?.addSublayer(imageLayer)
        
        setContentHuggingPriority(.defaultLow, for: .horizontal)
        setContentHuggingPriority(.defaultLow, for: .vertical)
        setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        setContentCompressionResistancePriority(.defaultLow, for: .vertical)
    }
    
    override func layout() {
        super.layout()
        // Ensure the layer perfectly tracks the view bounds
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        if let bounds = self.layer?.bounds {
            imageLayer.frame = bounds
        }
        CATransaction.commit()
    }
    
    private func updateImage() {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        if let image = image {
            let windowScale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2.0
            imageLayer.contentsScale = windowScale
            
            // Extract the raw CGImage from the NSImage to avoid AppKit scaling interference
            var rect = CGRect(x: 0, y: 0, width: image.size.width, height: image.size.height)
            if let cgImage = image.cgImage(forProposedRect: &rect, context: nil, hints: nil) {
                imageLayer.contents = cgImage
            } else {
                imageLayer.contents = nil
            }
        } else {
            imageLayer.contents = nil
        }
        CATransaction.commit()
    }
}

struct CameraFeedView: NSViewRepresentable {
    var image: NSImage?
    
    func makeNSView(context: Context) -> AspectFillImageView {
        return AspectFillImageView(frame: .zero)
    }
    
    func updateNSView(_ nsView: AspectFillImageView, context: Context) {
        nsView.image = image
    }
}

class CameraPreviewWindowController: NSWindowController, NSWindowDelegate {
    
    static let shared = CameraPreviewWindowController()
    
    private var streamState = CameraStreamState()
    private var cancellables = Set<AnyCancellable>()
    
    init() {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 800, height: 600),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.title = "Continuity Camera"
        window.isReleasedWhenClosed = false
        window.center()
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.isOpaque = false
        window.backgroundColor = .clear
        
        // Keep the zoom button available so the user can enter/exit full screen manually if needed
        window.standardWindowButton(.closeButton)?.isHidden = true
        window.standardWindowButton(.miniaturizeButton)?.isHidden = true
        window.standardWindowButton(.zoomButton)?.isHidden = false
        
        // Ensure the window is allowed to enter native full screen
        window.collectionBehavior = [.fullScreenPrimary]
        
        // Make the entire window draggable
        window.isMovableByWindowBackground = true
        
        super.init(window: window)
        window.delegate = self
        
        let rootView = CameraPreviewView(state: streamState, onClose: { [weak self] in
            self?.close()
        })
        
        window.contentView = NSHostingView(rootView: rootView)
        
        // Observe pinning state to adjust window level
        streamState.$isPinned
            .receive(on: DispatchQueue.main)
            .sink { [weak self] isPinned in
                self?.window?.level = isPinned ? .floating : .normal
            }
            .store(in: &cancellables)
    }
    
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }
    
    override func showWindow(_ sender: Any?) {
        // Reset state on show
        streamState.isWaiting = true
        streamState.image = nil
        super.showWindow(sender)
        
        // Automatically enter native full-screen mode for a bezel-less experience
        if let window = self.window, !window.styleMask.contains(.fullScreen) {
            window.toggleFullScreen(nil)
        }
    }
    
    func windowWillClose(_ notification: Notification) {
        streamState.isWaiting = true
        streamState.image = nil
        // Let the engine know to drop the connection
        NotificationCenter.default.post(name: .linkallCameraWindowClosed, object: nil)
    }
    
    func updateFrame(data: Data) {
        DispatchQueue.main.async {
            if let image = NSImage(data: data) {
                if self.streamState.isWaiting {
                    withAnimation(.easeInOut(duration: 0.4)) {
                        self.streamState.isWaiting = false
                    }
                }
                self.streamState.image = image
            }
        }
    }
}

extension Notification.Name {
    static let linkallCameraWindowClosed = Notification.Name("linkallCameraWindowClosed")
}
