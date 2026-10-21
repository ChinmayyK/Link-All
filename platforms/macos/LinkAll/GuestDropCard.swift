import SwiftUI
import AppKit
import CoreImage.CIFilterBuiltins

/// The link, QR code and PIN for a Guest Drop session, counting down to its end.
struct GuestDropCard: View {
    let session: IpcGuestSession
    let onStop: () -> Void
    @State private var remaining: Int

    private let tick = Timer.publish(every: 1, on: .main, in: .common).autoconnect()

    init(session: IpcGuestSession, onStop: @escaping () -> Void) {
        self.session = session
        self.onStop = onStop
        _remaining = State(initialValue: session.expires_in_secs)
    }

    var body: some View {
        ModalCard(icon: "qrcode", title: "Receive from a guest",
                  subtitle: "They scan this, type the PIN and send a file. Nothing is paired or saved.", width: 460) {
            VStack(spacing: 14) {
                if let image = Self.qr(session.url) {
                    Image(nsImage: image)
                        .interpolation(.none)
                        .resizable()
                        .frame(width: 180, height: 180)
                        .padding(10)
                        .background(Color.white, in: RoundedRectangle(cornerRadius: 12))
                }
                Text(session.url)
                    .font(.system(size: 12, design: .monospaced))
                    .textSelection(.enabled)
                    .multilineTextAlignment(.center)
                HStack(spacing: 8) {
                    Text("PIN").font(.system(size: 12, weight: .semibold)).foregroundStyle(CRTheme.inkSoft)
                    Text(session.pin)
                        .font(.system(size: 28, weight: .bold, design: .monospaced))
                        .textSelection(.enabled)
                }
                Text(remaining > 0 ? "Works for \(remaining / 60):\(String(format: "%02d", remaining % 60)) more. Files go to Downloads › Link All › Guest." : "This link has ended.")
                    .font(.system(size: 11.5))
                    .foregroundStyle(CRTheme.inkSoft)
                Text("The page is not encrypted. Use it on a network you trust.")
                    .font(.system(size: 11))
                    .foregroundStyle(CRTheme.inkSubtle)
                HStack {
                    Button("Copy link") {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(session.url, forType: .string)
                    }
                    .buttonStyle(CRSecondaryButtonStyle())
                    Spacer()
                    Button("Stop", action: onStop)
                        .buttonStyle(CRSecondaryButtonStyle())
                        .keyboardShortcut(.cancelAction)
                }
            }
        }
        .onReceive(tick) { _ in if remaining > 0 { remaining -= 1 } }
    }

    static func qr(_ text: String) -> NSImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(text.utf8)
        filter.correctionLevel = "M"
        guard let output = filter.outputImage?.transformed(by: CGAffineTransform(scaleX: 8, y: 8)) else { return nil }
        let rep = NSCIImageRep(ciImage: output)
        let image = NSImage(size: rep.size)
        image.addRepresentation(rep)
        return image
    }
}
