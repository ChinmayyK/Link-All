import SwiftUI
import AppKit
import CoreImage.CIFilterBuiltins

// MARK: - Pairing panels

/// A device asks to pair: the code to compare, and Accept or Decline.
struct PairingRequestPanel: View {
    let device: ManagedDevice
    @ObservedObject var store: LinkAllStore

    var body: some View {
        Panel(highlighted: true) {
            VStack(alignment: .leading, spacing: 0) {
                Text("Pairing request")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(CRTheme.brandInk)
                Text(device.outgoingPairingWaiting ? "\(device.name) also wants to pair" : "\(device.name) wants to pair")
                    .font(.system(size: 20, weight: .bold))
                    .foregroundStyle(CRTheme.ink)
                    .lineLimit(2)
                    .padding(.top, 4)
                PinTiles(pin: device.pairingPin)
                    .padding(.top, 14)
                Text("Accept only if \(device.name) shows the same code.")
                    .font(.system(size: 12.5))
                    .foregroundStyle(CRTheme.inkSoft)
                    .padding(.top, 12)
                PairingCountdown(seconds: device.pairingExpiresIn)
                    .padding(.top, 3)
                HStack(spacing: 10) {
                    Button("Decline") { store.respondToPairing(device, accepted: false) }
                        .buttonStyle(.bordered)
                    Button("Accept") { store.respondToPairing(device, accepted: true) }
                        .buttonStyle(.borderedProminent)
                        .tint(CRTheme.brandElectric)
                }
                .controlSize(.large)
                .padding(.top, 16)
            }
            .padding(18)
        }
    }
}

/// This Mac asked to pair and waits: the code to compare there, and a way out.
struct OutgoingPairingPanel: View {
    let device: ManagedDevice
    @ObservedObject var store: LinkAllStore

    private var pin: String? { device.pairingPin.flatMap { $0.isEmpty ? nil : $0 } }

    var body: some View {
        Panel(highlighted: true) {
            VStack(alignment: .leading, spacing: 0) {
                Text("Waiting for approval")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(CRTheme.brandInk)
                Text("Accept on \(device.name)")
                    .font(.system(size: 20, weight: .bold))
                    .foregroundStyle(CRTheme.ink)
                    .lineLimit(2)
                    .padding(.top, 4)
                PinTiles(pin: pin)
                    .padding(.top, 14)
                Text(pin == nil
                     ? "Connecting to \(device.name)…"
                     : "\(device.name) shows a request with a code. Accept it there if it matches this one.")
                    .font(.system(size: 12.5))
                    .foregroundStyle(CRTheme.inkSoft)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.top, 12)
                PairingCountdown(seconds: device.pairingExpiresIn)
                    .padding(.top, 3)
                Button("Cancel request") { store.cancelPairingRequest(device) }
                    .buttonStyle(.bordered)
                    .controlSize(.large)
                    .padding(.top, 16)
            }
            .padding(18)
        }
    }
}

/// The pairing code, one digit to a tile so it's easy to compare.
struct PinTiles: View {
    let pin: String?

    var body: some View {
        if let pin, !pin.isEmpty {
            HStack(spacing: 6) {
                ForEach(Array(pin.enumerated()), id: \.offset) { _, digit in
                    Text(String(digit))
                        .font(.system(size: 22, weight: .bold, design: .monospaced))
                        .foregroundStyle(CRTheme.ink)
                        .frame(width: 34, height: 42)
                        .background(RoundedRectangle(cornerRadius: 9, style: .continuous).fill(CRTheme.surface))
                        .overlay(RoundedRectangle(cornerRadius: 9, style: .continuous).strokeBorder(CRTheme.stroke, lineWidth: 1))
                }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Code \(pin.map(String.init).joined(separator: " "))")
            .textSelection(.enabled)
        }
    }
}

/// "Expires in 42s", ticking here between the daemon's updates.
struct PairingCountdown: View {
    let seconds: Int?
    @State private var deadline: Date?

    var body: some View {
        Group {
            if let deadline {
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    let left = max(0, Int(deadline.timeIntervalSince(context.date).rounded()))
                    Text(left > 0 ? "Expires in \(left)s" : "Expired")
                }
            }
        }
        .font(.system(size: 12))
        .foregroundStyle(CRTheme.inkSubtle)
        .onAppear { deadline = Self.deadline(seconds) }
        .onChange(of: seconds) { deadline = Self.deadline($0) }
    }

    private static func deadline(_ seconds: Int?) -> Date? {
        seconds.map { Date().addingTimeInterval(TimeInterval($0)) }
    }
}

// MARK: - QR code

/// A code another device scans to pair with this Mac without typing an address.
struct QRCodeSheetView: View {
    @ObservedObject var store: LinkAllStore
    @Environment(\.dismiss) var dismiss
    @State private var qrImage: NSImage?
    @State private var errorMessage: String?
    @State private var isLoading = true

    var body: some View {
        VStack(spacing: 24) {
            VStack(spacing: 8) {
                Text("Pair a device")
                    .font(.system(size: 20, weight: .bold))

                Text("Scan this code with Link All on your phone.")
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
                    .font(.system(size: 13))
            }

            ZStack {
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .fill(CRTheme.surfaceElevated)
                    .shadow(color: .black.opacity(0.1), radius: 10, y: 5)
                    .frame(width: 240, height: 240)

                if isLoading {
                    ProgressView()
                } else if let img = qrImage {
                    Image(nsImage: img)
                        .interpolation(.none)
                        .resizable()
                        .scaledToFit()
                        .frame(width: 200, height: 200)
                        .padding(20)
                        .accessibilityLabel("Pairing code for this Mac")
                } else if let error = errorMessage {
                    Text(error)
                        .foregroundStyle(.red)
                        .multilineTextAlignment(.center)
                        .padding()
                }
            }

            Button("Done") {
                dismiss()
            }
            .buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.brandElectric))
            .keyboardShortcut(.defaultAction)
            .frame(width: 120)
        }
        .padding(32)
        .frame(width: 360)
        .task {
            await loadQR()
        }
        .onChange(of: store.connectedDevices.count) { _ in
            if !store.connectedDevices.isEmpty {
                dismiss()
            }
        }
    }

    private func loadQR() async {
        do {
            await store.refresh()
            let token = try await store.generateQrToken()
            let id = store.localDeviceId ?? ""
            let name = store.localDeviceName ?? Host.current().localizedName ?? "Mac"
            let ip = getLocalIPAddress() ?? "127.0.0.1"
            let port = "\(store.settings?.port ?? 47823)"
            let fp = store.localFingerprint ?? ""

            var components = URLComponents()
            components.scheme = "linkall"
            components.host = "pair"
            components.queryItems = [
                URLQueryItem(name: "id", value: id),
                URLQueryItem(name: "name", value: name),
                URLQueryItem(name: "token", value: token),
                URLQueryItem(name: "ip", value: ip),
                URLQueryItem(name: "port", value: port),
                URLQueryItem(name: "fingerprint", value: fp)
            ]

            guard let urlString = components.url?.absoluteString else {
                errorMessage = "Failed to construct pairing URL."
                isLoading = false
                return
            }

            let context = CIContext()
            let filter = CIFilter.qrCodeGenerator()
            filter.message = Data(urlString.utf8)
            filter.correctionLevel = "H"

            if let outputImage = filter.outputImage {
                let scaledImage = outputImage.transformed(by: CGAffineTransform(scaleX: 10, y: 10))
                if let cgImage = context.createCGImage(scaledImage, from: scaledImage.extent) {
                    qrImage = NSImage(cgImage: cgImage, size: .zero)
                } else {
                    errorMessage = "Failed to render QR Code."
                }
            } else {
                errorMessage = "Failed to generate QR Code."
            }
        } catch {
            errorMessage = error.localizedDescription
        }
        isLoading = false
    }
}

// MARK: - This Mac's address

/// This Mac's IPv4 address on the local network, preferring Wi-Fi or Ethernet.
func getLocalIPAddress() -> String? {
    var address: String?
    var ifaddr: UnsafeMutablePointer<ifaddrs>?
    guard getifaddrs(&ifaddr) == 0 else { return nil }
    guard let firstAddr = ifaddr else { return nil }
    for ptr in sequence(first: firstAddr, next: { $0.pointee.ifa_next }) {
        let interface = ptr.pointee
        let addrFamily = interface.ifa_addr.pointee.sa_family
        if addrFamily == UInt8(AF_INET) {
            let name = String(cString: interface.ifa_name)
            if name.hasPrefix("lo") || name.hasPrefix("pdp_ip") { continue }
            var hostname = [CChar](repeating: 0, count: Int(NI_MAXHOST))
            getnameinfo(interface.ifa_addr, socklen_t(interface.ifa_addr.pointee.sa_len),
                        &hostname, socklen_t(hostname.count),
                        nil, socklen_t(0), NI_NUMERICHOST)
            let ip = String(cString: hostname)
            if !ip.hasPrefix("127.") && !ip.hasPrefix("169.254") {
                address = ip
                if name.hasPrefix("en") {
                    break
                }
            }
        }
    }
    freeifaddrs(ifaddr)
    return address
}
