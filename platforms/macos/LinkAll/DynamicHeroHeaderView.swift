import SwiftUI

struct DynamicHeroHeaderView: View {
    @ObservedObject var store: LinkAllStore
    
    private var targetDevice: ManagedDevice? {
        if let id = store.selectedPendingDevice?.id, let fresh = store.pendingDevices.first(where: { $0.id == id }) {
            return fresh
        }
        return store.pendingDevices.first(where: { $0.pairingRequested || $0.outgoingPairingWaiting })
    }
    
    var body: some View {
        Group {
            if let targetDevice = targetDevice {
                pairingHijackView(for: targetDevice)
            } else {
                standardHeroView
            }
        }
        .padding(.horizontal, 40)
        .animation(.crSpring, value: targetDevice?.id)
    }
    
    // This Mac's identity and link state. Peer names live in the sidebar
    // and the device panel, not up here.
    private var standardHeroView: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text("Visible as")
                    .font(.system(size: 15, weight: .regular))
                    .foregroundStyle(CRTheme.inkSoft)
                Text(store.localDeviceName ?? Host.current().localizedName ?? "This Mac")
                    .font(.system(size: 26, weight: .bold, design: .rounded))
                    .foregroundStyle(CRTheme.ink)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }

            HStack(spacing: 8) {
                if let lead = store.connectedDevices.first {
                    Circle().fill(CRTheme.accentGreen).frame(width: 8, height: 8)
                    Text("Connected")
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(CRTheme.accentGreen)
                    let synced = store.connectedDevices.compactMap(\.lastSync).max()
                    let meta = [lead.ip, synced.map { "synced \($0.relativeTimeString())" }]
                        .compactMap { $0 }
                        .joined(separator: "  ·  ")
                    if !meta.isEmpty {
                        Text(meta)
                            .font(.system(size: 13, weight: .regular, design: .monospaced))
                            .foregroundStyle(CRTheme.inkSoft)
                            .lineLimit(1)
                            .truncationMode(.tail)
                    }
                } else {
                    Circle().strokeBorder(CRTheme.inkSubtle, lineWidth: 1.5).frame(width: 8, height: 8)
                    Text("Not connected")
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(CRTheme.inkSoft)
                }
            }
        }
        .transition(.asymmetric(
            insertion: .move(edge: .leading).combined(with: .opacity),
            removal: .move(edge: .trailing).combined(with: .opacity)
        ))
    }
    
    @ViewBuilder
    private func pairingHijackView(for device: ManagedDevice) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            if device.pairingRequested {
                Text("\(device.name) wants to pair.")
                    .font(.system(size: 32, weight: .bold, design: .rounded))
                    .foregroundStyle(CRTheme.ink)
                
                if let pin = device.pairingPin, !pin.isEmpty {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Verify this PIN matches your device:")
                            .font(.system(size: 14))
                            .foregroundStyle(CRTheme.inkSoft)
                        Text(pin)
                            .font(.system(size: 36, weight: .bold, design: .monospaced))
                            .foregroundStyle(CRTheme.ink)
                            .tracking(4)
                            .padding(.horizontal, 24)
                            .padding(.vertical, 12)
                            .background(CRTheme.surfaceStrong)
                            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    }
                } else {
                    Text("This device wants to connect to your Mac.")
                        .font(.system(size: 15))
                        .foregroundStyle(CRTheme.inkSoft)
                }
                
                HStack(spacing: 12) {
                    Button("Decline") { store.respondToPairing(device, accepted: false) }
                        .buttonStyle(CRSecondaryButtonStyle())
                    Button("Accept") { store.respondToPairing(device, accepted: true) }
                        .buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.accentGreen))
                }
            } else if device.outgoingPairingWaiting {
                Text("Waiting for \(device.name)...")
                    .font(.system(size: 32, weight: .bold, design: .rounded))
                    .foregroundStyle(CRTheme.ink)
                
                if let pin = device.pairingPin {
                    Text("Verify code: \(pin)")
                        .font(.system(size: 18, weight: .medium, design: .monospaced))
                        .foregroundStyle(CRTheme.brandElectric)
                }

                HStack(spacing: 8) {
                    ProgressView().scaleEffect(0.6)
                    Text("Accept the request on your device.")
                        .font(.system(size: 15))
                        .foregroundStyle(CRTheme.inkSoft)
                    
                    Button("Cancel") { store.cancelPairingRequest(device) }
                        .buttonStyle(CRSecondaryButtonStyle())
                        .padding(.leading, 12)
                }
            } else {
                Text("\(device.name) discovered nearby.")
                    .font(.system(size: 32, weight: .bold, design: .rounded))
                    .foregroundStyle(CRTheme.ink)
                Text("Connected over Local Wi-Fi.")
                    .font(.system(size: 15))
                    .foregroundStyle(CRTheme.inkSoft)
                Button("Pair with Device") { store.sendPairingRequest(device) }
                    .buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.brandElectric))
            }
        }
        .transition(.asymmetric(
            insertion: .move(edge: .trailing).combined(with: .opacity),
            removal: .move(edge: .leading).combined(with: .opacity)
        ))
    }
}
