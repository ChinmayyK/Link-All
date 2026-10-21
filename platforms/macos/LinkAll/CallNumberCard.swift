import SwiftUI

/// Asks for a phone number to call from a paired phone.
struct CallNumberCard: View {
    let onCall: (String) -> Void
    let onCancel: () -> Void
    @State private var number = ""

    var body: some View {
        ModalCard(icon: "phone.fill", title: "Call from your phone",
                  subtitle: "Your phone shows a notification; tap it to open the dialer", width: 420) {
            VStack(spacing: 14) {
                TextField("+1 234 567 8900", text: $number)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit(send)
                HStack {
                    Button("Cancel", action: onCancel)
                        .buttonStyle(CRSecondaryButtonStyle())
                        .keyboardShortcut(.cancelAction)
                    Spacer()
                    Button("Send to phone", action: send)
                        .buttonStyle(CRSecondaryButtonStyle())
                        .keyboardShortcut(.defaultAction)
                        .disabled(number.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
        }
    }

    private func send() {
        let typed = number.trimmingCharacters(in: .whitespaces)
        if !typed.isEmpty { onCall(typed) }
    }
}
