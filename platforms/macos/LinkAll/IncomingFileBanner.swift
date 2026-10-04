// IncomingFileBanner.swift
import AppKit
import Combine
import SwiftUI

// MARK: - Window Manager

@MainActor
final class FileBannerWindowManager: NSObject {
    private let panel: FileBannerPanel
    private let hostingView: FileBannerHostingView<FileBannerContainerView>
    private var dismissTimer: Timer?

    override init() {
        self.panel = FileBannerPanel()
        // Provide an initial empty state
        self.hostingView = FileBannerHostingView(rootView: FileBannerContainerView(message: nil))
        super.init()
        panel.contentView = hostingView
        layoutPanel()
    }

    func show(title: String, body: String) {
        // Disabled per user request
    }


    func hide() {
        hostingView.rootView = FileBannerContainerView(message: nil)
        // Let SwiftUI animate out before hiding window
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in
            self?.panel.orderOut(nil)
        }
    }

    private func layoutPanel() {
        guard let screen = NSScreen.main else { return }
        let width: CGFloat = 360
        let height: CGFloat = 80
        // Top-center (beneath the notch if any)
        let frame = NSRect(
            x: screen.frame.midX - width / 2,
            y: screen.visibleFrame.maxY - height - 8,
            width: width,
            height: height
        )
        panel.setFrame(frame, display: false)
    }
}

// MARK: - Panel

private final class FileBannerHostingView<Content: View>: NSHostingView<Content> {
    override func hitTest(_ point: NSPoint) -> NSView? {
        // Just return nil to make it completely click-through, or super.hitTest if we want interaction.
        return super.hitTest(point)
    }
}

private final class FileBannerPanel: NSPanel {
    init() {
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 360, height: 80),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        
        level = .popUpMenu
        hasShadow = false
        isOpaque = false
        backgroundColor = .clear
        hidesOnDeactivate = false
        ignoresMouseEvents = true // Just an overlay
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .ignoresCycle]
    }
    
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

struct FileBannerMessage: Equatable {
    let title: String
    let body: String
}

// MARK: - SwiftUI Container

private struct FileBannerContainerView: View {
    let message: FileBannerMessage?

    var body: some View {
        Group {
            if let msg = message {
                FileBannerView(title: msg.title, bodyText: msg.body)
                    .transition(.asymmetric(
                        insertion: .move(edge: .top).combined(with: .opacity).combined(with: .scale(scale: 0.95)),
                        removal: .move(edge: .top).combined(with: .opacity).combined(with: .scale(scale: 0.9))
                    ))
            }
        }
        .frame(width: 360, height: 80, alignment: .top)
        .animation(.spring(response: 0.5, dampingFraction: 0.7, blendDuration: 0.1), value: message != nil)
    }
}

// MARK: - Banner View

private struct FileBannerView: View {
    let title: String
    let bodyText: String
    @Environment(\.colorScheme) var colorScheme

    var body: some View {
        HStack(spacing: 14) {
            // Icon
            ZStack {
                Circle()
                    .fill(
                        LinearGradient(colors: [Color(hex: 0x059669), Color(hex: 0x34D399)], startPoint: .topLeading, endPoint: .bottomTrailing)
                    )
                    .frame(width: 42, height: 42)
                    .overlay(Circle().strokeBorder(Color.white.opacity(0.3), lineWidth: 0.5))
                    .shadow(color: Color(hex: 0x059669).opacity(0.4), radius: 6, y: 3)
                
                Image(systemName: "arrow.down.doc.fill")
                    .font(.system(size: 16, weight: .bold))
                    .foregroundStyle(.white)
            }

            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.system(size: 14, weight: .bold, design: .rounded))
                    .foregroundStyle(Color.primary)
                    .lineLimit(1)
                
                Text(bodyText)
                    .font(.system(size: 12, weight: .medium, design: .rounded))
                    .foregroundStyle(Color.secondary)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .frame(width: 340, height: 64)
        .background(
            ZStack {
                CRHUDMaterial()
                    .clipShape(Capsule())
                
                Capsule()
                    .fill(Color.black.opacity(colorScheme == .dark ? 0.3 : 0.0))
                
                Capsule()
                    .strokeBorder(Color.white.opacity(0.2), lineWidth: 1)
            }
            .shadow(color: Color.black.opacity(0.3), radius: 20, x: 0, y: 10)
        )
    }
}
