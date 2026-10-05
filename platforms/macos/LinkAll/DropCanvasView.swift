import SwiftUI
import UniformTypeIdentifiers

struct DropCanvasView: View {
    @ObservedObject var store: LinkAllStore
    @State private var isTargeted = false
    @State private var pulse = false

    private var connectedDevices: [ManagedDevice] {
        store.devices.filter { $0.isConnected }
    }

    var body: some View {
        ZStack {
            // Base layer: Native macOS Frosted Glass
            CRHUDMaterial()
                .ignoresSafeArea()
            
            // Pulse glow when targeted
            if isTargeted {
                LinearGradient(
                    colors: [CRTheme.brandElectric.opacity(0.25), CRTheme.brandViolet.opacity(0.20), CRTheme.brandCyan.opacity(0.20)],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
                .blur(radius: 35)
                .ignoresSafeArea()
            }

            VStack(spacing: 12) {
                // Header Status Bar
                HStack {
                    HStack(spacing: 6) {
                        Circle()
                            .fill(store.connectedCount > 0 ? CRTheme.accentGreen : CRTheme.accentOrange)
                            .frame(width: 6.5, height: 6.5)
                            .shadow(color: (store.connectedCount > 0 ? CRTheme.accentGreen : CRTheme.accentOrange).opacity(0.6), radius: 3)
                        Text(store.connectedCount > 0 ? "\(store.connectedCount) connected" : "No devices connected")
                            .font(.system(size: 11, weight: .semibold, design: .rounded))
                            .foregroundStyle(CRTheme.inkSoft)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 4)
                    .background(Capsule().fill(CRTheme.surfaceStrong.opacity(0.6)))
                    .overlay(Capsule().strokeBorder(CRTheme.strokeSoft, lineWidth: 0.5))
                    
                    Spacer()
                    
                    Button(action: {
                        NotificationCenter.default.post(name: .init("closeDropCanvas"), object: nil)
                    }) {
                        Image(systemName: "xmark")
                            .font(.system(size: 10, weight: .bold))
                            .foregroundStyle(CRTheme.inkSubtle)
                            .frame(width: 22, height: 22)
                            .background(Circle().fill(CRTheme.surfaceStrong.opacity(0.5)))
                            .overlay(Circle().strokeBorder(CRTheme.strokeSoft, lineWidth: 0.5))
                    }
                    .buttonStyle(.plain)
                }
                .padding(.horizontal, 16)
                .padding(.top, 14)

                // Drop Area Card
                ZStack {
                    // Background shape
                    ZStack {
                        RoundedRectangle(cornerRadius: 16, style: .continuous)
                            .fill(CRTheme.surfaceElevated.opacity(isTargeted ? 0.7 : 0.45))
                            .overlay(
                                RoundedRectangle(cornerRadius: 16, style: .continuous)
                                    .strokeBorder(
                                        isTargeted ? CRTheme.brandElectric : CRTheme.stroke,
                                        lineWidth: isTargeted ? 2 : 1
                                    )
                                    .shadow(color: isTargeted ? CRTheme.brandElectric.opacity(0.5) : .clear, radius: 12, x: 0, y: 0)
                            )
                        
                        if isTargeted {
                            // Animated radar rings
                            RoundedRectangle(cornerRadius: 16, style: .continuous)
                                .stroke(CRTheme.brandElectric, lineWidth: 2)
                                .scaleEffect(pulse ? 1.05 : 1.0)
                                .opacity(pulse ? 0 : 0.85)
                            
                            RoundedRectangle(cornerRadius: 16, style: .continuous)
                                .stroke(CRTheme.brandCyan, lineWidth: 1.5)
                                .scaleEffect(pulse ? 1.09 : 1.0)
                                .opacity(pulse ? 0 : 0.45)
                        }
                    }

                    VStack(spacing: 10) {
                        ZStack {
                            Circle()
                                .fill(isTargeted ? CRTheme.brandElectric.opacity(0.20) : CRTheme.strokeSoft)
                                .frame(width: 54, height: 54)
                                .overlay(
                                    Circle()
                                        .strokeBorder(
                                            isTargeted ? CRTheme.brandElectric : CRTheme.stroke,
                                            style: StrokeStyle(lineWidth: isTargeted ? 2 : 1.5, dash: isTargeted ? [] : [4, 3])
                                        )
                                )
                            
                            Image(systemName: isTargeted ? "arrow.down.app.fill" : "arrow.down.doc.fill")
                                .font(.system(size: 22, weight: .medium))
                                .foregroundStyle(isTargeted ? CRTheme.brandElectric : CRTheme.inkSoft)
                                .offset(y: isTargeted ? 2 : -2)
                                .shadow(color: isTargeted ? CRTheme.brandElectric.opacity(0.5) : .clear, radius: 8)
                        }
                        .scaleEffect(isTargeted ? 1.12 : 1.0)
                        .animation(.spring(response: 0.3, dampingFraction: 0.65), value: isTargeted)

                        VStack(spacing: 4) {
                            Text(isTargeted ? "Release to send" : "Drop files to send")
                                .font(.system(size: 15, weight: .bold, design: .rounded))
                                .foregroundStyle(isTargeted ? CRTheme.brandElectric : CRTheme.ink)
                                .contentTransition(.interpolate)

                            // Says what the drop does: one device gets it
                            // straight away, several bring up the picker.
                            Text(connectedDevices.isEmpty ? "Connect a device first"
                                 : connectedDevices.count == 1 ? "Sends to \(connectedDevices[0].name)"
                                 : "Choose a device after you drop")
                                .font(.system(size: 11.5, weight: .medium))
                                .foregroundStyle(isTargeted ? CRTheme.ink : CRTheme.inkSubtle)
                                .lineLimit(1)
                                .truncationMode(.middle)
                                .contentTransition(.interpolate)
                        }
                    }
                }
                .frame(height: 140)
                .padding(.horizontal, 16)
                .padding(.bottom, 16)
            }
        }
        .frame(width: 320, height: 216)
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .onDrop(of: [.fileURL], delegate: CanvasDropDelegate(store: store, isTargeted: $isTargeted))
        .onChange(of: isTargeted) { targeted in
            if targeted {
                withAnimation(.easeInOut(duration: 1.0).repeatForever(autoreverses: true)) {
                    pulse = true
                }
            } else {
                withAnimation(.easeOut(duration: 0.3)) {
                    pulse = false
                }
            }
        }
    }
}

struct CanvasDropDelegate: DropDelegate {
    let store: LinkAllStore
    @Binding var isTargeted: Bool

    func dropEntered(info: DropInfo) {
        withAnimation(.crFast) { isTargeted = true }
    }

    func dropExited(info: DropInfo) {
        withAnimation(.crFast) { isTargeted = false }
        // Do NOT close the popover on accidental drag exit over the window border;
        // let the user either drop or explicitly dismiss using Esc / X button.
    }

    func performDrop(info: DropInfo) -> Bool {
        isTargeted = false
        NotificationCenter.default.post(name: .init("closeDropCanvas"), object: nil)
        NSHapticFeedbackManager.defaultPerformer.perform(.generic, performanceTime: .default)
        
        let providers = info.itemProviders(for: [.fileURL])
        let group = DispatchGroup()
        let lock = NSLock()
        var urls: [URL] = []
        
        for provider in providers {
            group.enter()
            provider.loadItem(forTypeIdentifier: UTType.fileURL.identifier, options: nil) { (item, error) in
                lock.lock()
                defer { lock.unlock() }
                if let data = item as? Data,
                   let url = URL(dataRepresentation: data, relativeTo: nil) {
                    urls.append(url)
                } else if let url = item as? URL {
                    urls.append(url)
                }
                group.leave()
            }
        }
        
        group.notify(queue: .main) {
            store.sendFilesChoosingTarget(urls: urls) { sent in
                guard sent else { return }
                store.showToast(
                    title: "Sending \(urls.count) item\(urls.count == 1 ? "" : "s")",
                    body: urls.map(\.lastPathComponent).joined(separator: ", "),
                    tint: CRTheme.brandElectric,
                    systemImage: "arrow.up.doc.fill",
                    ttl: 3.5
                )
            }
        }
        return true
    }
}
