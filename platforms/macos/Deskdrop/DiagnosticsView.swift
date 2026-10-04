import SwiftUI

struct DiagnosticsView: View {
    @ObservedObject var store: DeskdropStore
    @Environment(\.dismiss) var dismiss

    var body: some View {
        VStack(spacing: 0) {
            // Header
            HStack {
                Text("Diagnostics")
                    .font(.system(size: 16, weight: .semibold, design: .rounded))
                Spacer()
                Button(action: { dismiss() }) {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(Color.secondary)
                        .font(.system(size: 18))
                }
                .buttonStyle(.plain)
            }
            .padding()
            .background(Color(NSColor.windowBackgroundColor).opacity(0.8))
            
            Divider()
            
            ScrollView {
                VStack(spacing: 16) {
                    DiagnosticItem(
                        icon: "bolt.fill",
                        title: "Daemon Status",
                        status: store.isRunning ? "Running" : "Stopped",
                        isOk: store.isRunning,
                        suggestion: store.isRunning ? nil : "Restart the app or check Activity Monitor for deskdrop-daemon.",
                        actionLabel: store.isRunning ? nil : "Restart Connection",
                        onAction: { store.reconnect() }
                    )
                    
                    DiagnosticItem(
                        icon: "network",
                        title: "Local Network",
                        status: store.connectedCount > 0 ? "Connected to \(store.connectedCount) peers" : "Looking for peers",
                        isOk: store.connectedCount > 0,
                        suggestion: store.connectedCount > 0 ? nil : "Ensure devices are on the same Wi-Fi network and no firewall is blocking Link All.",
                        actionLabel: store.connectedCount > 0 ? nil : "Scan Again",
                        onAction: { store.rescanNetwork() }
                    )
                    
                    DiagnosticItem(
                        icon: "doc.on.clipboard",
                        title: "Clipboard Sync",
                        status: store.clipboardPolicy.autoApply ? "Enabled" : "Paused",
                        isOk: store.clipboardPolicy.autoApply,
                        suggestion: store.clipboardPolicy.autoApply ? nil : "Enable auto-apply in Settings for instant paste.",
                        actionLabel: store.clipboardPolicy.autoApply ? nil : "Enable",
                        onAction: { store.enableAutoApply() }
                    )
                    
                    if let status = store.status {
                        DiagnosticItem(
                            icon: "cpu",
                            title: "Daemon Version",
                            status: status.daemonVersion ?? "Unknown",
                            isOk: status.daemonVersion != nil,
                            suggestion: nil,
                            actionLabel: nil,
                            onAction: nil
                        )
                    }
                }
                .padding()
            }
        }
        .frame(width: 400, height: 350)
        .background(CRHUDMaterial())
    }
}

private struct DiagnosticItem: View {
    let icon: String
    let title: String
    let status: String
    let isOk: Bool
    let suggestion: String?
    let actionLabel: String?
    let onAction: (() -> Void)?
    
    var body: some View {
        HStack(alignment: .top, spacing: 16) {
            Image(systemName: icon)
                .font(.system(size: 20))
                .foregroundStyle(isOk ? CRTheme.accentGreen : CRTheme.accentOrange)
                .frame(width: 24)
            
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text(title)
                        .font(.system(size: 14, weight: .medium))
                        .foregroundStyle(Color.primary)
                    Spacer()
                    Text(status)
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(isOk ? CRTheme.accentGreen : CRTheme.accentOrange)
                }
                
                if let suggestion = suggestion {
                    Text(suggestion)
                        .font(.system(size: 12))
                        .foregroundStyle(Color.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                
                if let actionLabel = actionLabel, let onAction = onAction {
                    Button(actionLabel) {
                        onAction()
                    }
                    .buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.brandElectric))
                    .padding(.top, 8)
                }
            }
        }
        .padding()
        .background(Color(NSColor.controlBackgroundColor).opacity(0.5))
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
    }
}
