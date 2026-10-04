import SwiftUI

struct RootContainerView: View {
    @ObservedObject var store: LinkAllStore
    @AppStorage("hasCompletedOnboarding") private var hasCompletedOnboarding = false

    var body: some View {
        Group {
            if hasCompletedOnboarding {
                CommandCenterRootView(store: store)
            } else {
                OnboardingView(store: store, onComplete: {
                    withAnimation(.crSpring) {
                        hasCompletedOnboarding = true
                    }
                })
            }
        }
        .frame(minWidth: 1200, minHeight: 760)
        .ignoresSafeArea(.all, edges: .top)
    }
}

struct OnboardingView: View {
    @ObservedObject var store: LinkAllStore
    @State private var selectedPeerId: String? = nil
    
    private var selectedPeer: PeerViewModel? {
        store.peers.first { $0.id == selectedPeerId }
    }
    
    private var currentStep: Int {
        guard let peer = selectedPeer else { return 0 }
        if !peer.trusted { return 1 }
        return 1
    }

    let onComplete: () -> Void

    var body: some View {
        ZStack {
            // Base Background
            CRFluidBackgroundView().ignoresSafeArea()
            
            // Massive subtle background radar glow
            if currentStep == 0 {
                BackgroundRadarGlow()
                    .transition(.opacity)
            }

            VStack(spacing: 0) {
                Spacer()

                ZStack {
                    if currentStep == 0 {
                        StepOneFindDevice(store: store, selectedPeerId: $selectedPeerId)
                            .transition(.asymmetric(insertion: .move(edge: .trailing).combined(with: .opacity), removal: .move(edge: .leading).combined(with: .opacity)))
                    } else if currentStep == 1 {
                        StepTwoVerify(store: store, selectedPeer: selectedPeer, onCancel: { selectedPeerId = nil })
                            .transition(.asymmetric(insertion: .move(edge: .trailing).combined(with: .opacity), removal: .move(edge: .leading).combined(with: .opacity)))
                    }
                }
                .animation(.crSpring, value: currentStep)
                
                Spacer()
                
                // Footer Navigation
                HStack {
                    if currentStep > 0 {
                        Button(action: {
                            withAnimation(.crSpring) { selectedPeerId = nil }
                        }) {
                            HStack(spacing: 6) {
                                Image(systemName: "chevron.left")
                                Text("Back")
                            }
                            .font(.system(size: 14, weight: .medium))
                            .foregroundStyle(CRTheme.inkSoft)
                            .padding(.horizontal, 16)
                            .padding(.vertical, 10)
                            .background(Capsule().fill(CRTheme.surfaceElevated.opacity(0.5)))
                            .overlay(Capsule().strokeBorder(CRTheme.strokeSoft, lineWidth: 1))
                        }
                        .buttonStyle(.plain)
                        .onHover { isHovered in if isHovered { NSCursor.pointingHand.push() } else { NSCursor.pop() } }
                    }
                    Spacer()
                }
                .padding(.horizontal, 60)
                .padding(.bottom, 60)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .onChange(of: selectedPeer?.trusted) { trusted in
            if trusted == true {
                // Add a tiny delay to let the user see the "Trusted" state before dismissing
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) {
                    onComplete()
                }
            }
        }
    }
}

// MARK: - Background Effects
private struct BackgroundRadarGlow: View {
    @State private var pulse = false
    
    var body: some View {
        ZStack {
            Circle()
                .fill(RadialGradient(colors: [CRTheme.brandElectric.opacity(0.15), .clear], center: .center, startRadius: 0, endRadius: 300))
                .frame(width: 800, height: 800)
                .scaleEffect(pulse ? 1.2 : 0.8)
                .opacity(pulse ? 0.5 : 1.0)
            
            Circle()
                .stroke(CRTheme.brandElectric.opacity(0.05), lineWidth: 1)
                .frame(width: pulse ? 1000 : 400, height: pulse ? 1000 : 400)
                .opacity(pulse ? 0 : 1)
        }
        .offset(y: 40)
        .onAppear {
            withAnimation(.easeOut(duration: 3.5).repeatForever(autoreverses: false)) {
                pulse = true
            }
        }
    }
}

// MARK: - Step 1: Find
private struct StepOneFindDevice: View {
    @ObservedObject var store: LinkAllStore
    @Binding var selectedPeerId: String?
    @State private var showingQR = false
    
    var body: some View {
        VStack(spacing: 32) {
            
            VStack(spacing: 12) {
                // Header badge
                HStack(spacing: 6) {
                    Circle().fill(CRTheme.brandElectric).frame(width: 8, height: 8)
                    Text("DISCOVERY")
                        .font(.system(size: 11, weight: .bold, design: .rounded))
                        .foregroundStyle(CRTheme.inkSubtle)
                        .tracking(1)
                }
                .padding(.horizontal, 12).padding(.vertical, 6)
                .background(Capsule().fill(CRTheme.surfaceStrong))
                
                Text("Select your device")
                    .font(.system(size: 36, weight: .bold, design: .rounded))
                    .foregroundStyle(CRTheme.ink)
                
                Text("Make sure Link All is open on your phone or tablet.")
                    .font(.system(size: 16))
                    .foregroundStyle(CRTheme.inkSoft)
            }
            
            // Devices Area
            ZStack {
                if store.peers.isEmpty {
                    VStack(spacing: 24) {
                        RadarPulseView()
                        Text("Scanning local network...")
                            .foregroundStyle(CRTheme.inkSubtle)
                            .font(.system(size: 14, weight: .medium))
                        Button("Rescan") { store.scanForDevices() }
                            .buttonStyle(CRSecondaryButtonStyle())
                    }
                    .frame(height: 250)
                } else {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 20) {
                            ForEach(store.peers) { peer in
                                DiscoveryDeviceCard(peer: peer, isSelected: selectedPeerId == peer.id) {
                                    selectedPeerId = peer.id
                                    store.connectAndPair(deviceId: peer.id)
                                }
                            }
                        }
                        .padding(.horizontal, 40)
                        .padding(.vertical, 20)
                    }
                    .frame(height: 250)
                }
            }
            .frame(width: 600)
            
            // Manual fallback
            Button(action: { showingQR = true }) {
                HStack {
                    Image(systemName: "qrcode.viewfinder")
                    Text("Show QR Code")
                }
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(CRTheme.brandElectric)
                .padding(.horizontal, 20).padding(.vertical, 12)
                .background(Capsule().fill(CRTheme.brandElectric.opacity(0.1)))
                .overlay(Capsule().strokeBorder(CRTheme.brandElectric.opacity(0.2), lineWidth: 1))
            }
            .buttonStyle(.plain)
            .onHover { isHovered in if isHovered { NSCursor.pointingHand.push() } else { NSCursor.pop() } }
            .sheet(isPresented: $showingQR) { QRCodePairingSheet(store: store) }
        }
    }
}

private struct DiscoveryDeviceCard: View {
    let peer: PeerViewModel
    let isSelected: Bool
    let action: () -> Void
    @State private var hovered = false
    
    var body: some View {
        Button(action: action) {
            VStack(spacing: 16) {
                ZStack {
                    Circle()
                        .fill(isSelected ? CRTheme.brandElectric.opacity(0.15) : CRTheme.surfaceStrong)
                        .frame(width: 72, height: 72)
                    
                    OSIcon(platform: peer.platform, name: peer.displayName, size: 30)
                        .foregroundStyle(isSelected ? CRTheme.brandElectric : CRTheme.ink)
                }
                
                VStack(spacing: 4) {
                    Text(peer.displayName)
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(CRTheme.ink)
                        .lineLimit(1)
                    
                    Text("Tap to connect")
                        .font(.system(size: 12))
                        .foregroundStyle(isSelected ? CRTheme.brandElectric : CRTheme.inkSubtle)
                }
            }
            .frame(width: 160, height: 180)
            .background(CRTheme.surfaceElevated.opacity(0.85))
            .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 20, style: .continuous)
                    .strokeBorder(isSelected ? CRTheme.brandElectric : CRTheme.stroke, lineWidth: isSelected ? 2 : 1)
            )
            .shadow(color: isSelected ? CRTheme.brandElectric.opacity(0.3) : Color.black.opacity(hovered ? 0.08 : 0.04), radius: isSelected ? 16 : 8, y: isSelected ? 8 : 4)
            .scaleEffect(hovered || isSelected ? 1.02 : 1.0)
        }
        .buttonStyle(.plain)
        .onHover { isHovered in
            hovered = isHovered
            if isHovered { NSCursor.pointingHand.push() } else { NSCursor.pop() }
        }
        .animation(.crSpring, value: hovered)
        .animation(.crSpring, value: isSelected)
    }
}


// MARK: - Step 2: Verify
private struct StepTwoVerify: View {
    @ObservedObject var store: LinkAllStore
    var selectedPeer: PeerViewModel?
    var onCancel: () -> Void
    
    @State private var hasTimedOut = false
    
    var body: some View {
        VStack(spacing: 32) {
            
            VStack(spacing: 12) {
                // Header badge
                HStack(spacing: 6) {
                    Circle().fill(CRTheme.brandViolet).frame(width: 8, height: 8)
                    Text("VERIFICATION")
                        .font(.system(size: 11, weight: .bold, design: .rounded))
                        .foregroundStyle(CRTheme.inkSubtle)
                        .tracking(1)
                }
                .padding(.horizontal, 12).padding(.vertical, 6)
                .background(Capsule().fill(CRTheme.surfaceStrong))
                
                Text("Secure Pairing")
                    .font(.system(size: 36, weight: .bold, design: .rounded))
                    .foregroundStyle(CRTheme.ink)
                
                if let peer = selectedPeer {
                    Text("Verify this code matches what is shown on **\(peer.displayName)**.")
                        .font(.system(size: 16))
                        .foregroundStyle(CRTheme.inkSoft)
                }
            }
            
            if let peer = selectedPeer {
                if peer.trusted {
                    // Success state
                    VStack(spacing: 24) {
                        Image(systemName: "checkmark.shield.fill")
                            .font(.system(size: 56))
                            .foregroundStyle(CRTheme.accentGreen)
                        
                        Text("Device Trusted!")
                            .font(.system(size: 20, weight: .bold))
                            .foregroundStyle(CRTheme.ink)
                    }
                    .frame(height: 200)
                    .transition(.scale.combined(with: .opacity))
                }
                else if let pin = peer.pairingPin {
                    VStack(spacing: 40) {
                        // Glass PIN Display
                        Text(pin)
                            .font(.system(size: 48, weight: .black, design: .monospaced))
                            .tracking(12)
                            .foregroundStyle(CRTheme.ink)
                            .padding(.horizontal, 48)
                            .padding(.vertical, 24)
                            .background(
                                CRHUDMaterial()
                                    .clipShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
                                    .overlay(RoundedRectangle(cornerRadius: 24, style: .continuous).strokeBorder(CRTheme.strokeSoft, lineWidth: 1))
                                    .shadow(color: Color.black.opacity(0.1), radius: 20, y: 10)
                            )
                        
                        if peer.pairingRequested {
                            HStack(spacing: 20) {
                                Button(action: {
                                    store.respondToPairing(ManagedDevice(peer: peer), accepted: false)
                                    onCancel()
                                }) {
                                    Text("Decline")
                                        .font(.system(size: 15, weight: .semibold))
                                        .foregroundStyle(CRTheme.ink)
                                        .frame(width: 140, height: 48)
                                        .background(Capsule().fill(CRTheme.surfaceStrong))
                                        .overlay(Capsule().strokeBorder(CRTheme.stroke, lineWidth: 1))
                                }
                                .buttonStyle(.plain)
                                .onHover { isHovered in if isHovered { NSCursor.pointingHand.push() } else { NSCursor.pop() } }
                                
                                Button(action: {
                                    store.respondToPairing(ManagedDevice(peer: peer), accepted: true)
                                }) {
                                    Text("Trust Device")
                                        .font(.system(size: 15, weight: .bold))
                                        .foregroundStyle(.white)
                                        .frame(width: 140, height: 48)
                                        .background(Capsule().fill(CRTheme.brandElectric))
                                        .shadow(color: CRTheme.brandElectric.opacity(0.4), radius: 8, y: 4)
                                }
                                .buttonStyle(.plain)
                                .onHover { isHovered in if isHovered { NSCursor.pointingHand.push() } else { NSCursor.pop() } }
                            }
                        } else {
                            ProgressView()
                                .scaleEffect(0.8)
                                .padding(.top, 10)
                        }
                    }
                } else if hasTimedOut {
                    VStack(spacing: 24) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .font(.system(size: 40))
                            .foregroundStyle(CRTheme.accentRed)
                        
                        Text("Connection timed out.")
                            .foregroundStyle(CRTheme.ink)
                        
                        Text("Make sure both devices are on the same Wi-Fi network and Link All is running.")
                            .font(.system(size: 12, weight: .regular))
                            .foregroundStyle(CRTheme.inkSoft)
                            .multilineTextAlignment(.center)
                        
                        Button("Try Again") { onCancel() }
                            .buttonStyle(CRSecondaryButtonStyle())
                    }
                    .frame(height: 200)
                } else {
                    VStack(spacing: 24) {
                        ProgressView()
                            .scaleEffect(1.2)
                        Text("Connecting to \(peer.displayName)...")
                            .foregroundStyle(CRTheme.inkSubtle)
                    }
                    .frame(height: 200)
                }
            } else {
                Text("No device selected.")
                    .frame(height: 200)
            }
        }
        .task(id: selectedPeer?.id) {
            hasTimedOut = false
            guard selectedPeer != nil else { return }
            do {
                try await Task.sleep(nanoseconds: 10_000_000_000)
                if !Task.isCancelled {
                    if selectedPeer?.pairingPin == nil && selectedPeer?.trusted != true {
                        hasTimedOut = true
                    }
                }
            } catch {}
        }
        .onChange(of: selectedPeer?.pairingPin) { _ in
            hasTimedOut = false
        }
    }
}

private struct RadarPulseView: View {
    @State private var isPulsing = false
    
    var body: some View {
        ZStack {
            Circle()
                .fill(CRTheme.brandElectric.opacity(0.1))
                .frame(width: 100, height: 100)
                .scaleEffect(isPulsing ? 2.0 : 0.5)
                .opacity(isPulsing ? 0 : 1)
                .animation(.spring(response: 2.0, dampingFraction: 0.8).repeatForever(autoreverses: false).delay(0.4), value: isPulsing)
            
            Circle()
                .fill(CRTheme.brandElectric.opacity(0.15))
                .frame(width: 70, height: 70)
                .scaleEffect(isPulsing ? 1.6 : 0.8)
                .opacity(isPulsing ? 0 : 1)
                .animation(.spring(response: 2.0, dampingFraction: 0.8).repeatForever(autoreverses: false).delay(0.2), value: isPulsing)
            
            ZStack {
                Circle().fill(CRTheme.surfaceElevated).frame(width: 64, height: 64)
                    .shadow(color: Color.black.opacity(0.1), radius: 8, y: 4)
                
                if #available(macOS 14.0, *) {
                    Image(systemName: "antenna.radiowaves.left.and.right")
                        .font(.system(size: 24, weight: .semibold))
                        .foregroundStyle(CRTheme.brandElectric)
                        .symbolEffect(.variableColor.cumulative, options: .repeating)
                } else {
                    Image(systemName: "antenna.radiowaves.left.and.right")
                        .font(.system(size: 24, weight: .semibold))
                        .foregroundStyle(CRTheme.brandElectric)
                }
            }
        }
        .onAppear {
            isPulsing = true
        }
    }
}

