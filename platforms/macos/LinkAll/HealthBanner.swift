import SwiftUI

/// What stops sync right now (from the daemon's health list), and the one
/// thing to do about it.
struct HealthBanner: View {
    let issue: IpcHealthIssue
    @ObservedObject var store: LinkAllStore

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(CRTheme.accentOrange)
                .font(.system(size: 15))
            VStack(alignment: .leading, spacing: 3) {
                Text(issue.title)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(CRTheme.ink)
                Text(issue.detail)
                    .font(.system(size: 12))
                    .foregroundStyle(CRTheme.inkSoft)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 8)
            if let label = actionLabel {
                Button(label, action: act)
                    .buttonStyle(.bordered)
                    .controlSize(.small)
            }
        }
        .padding(14)
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(CRTheme.accentOrange.opacity(0.10))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .strokeBorder(CRTheme.accentOrange.opacity(0.35), lineWidth: 1)
        )
    }

    private var actionLabel: String? {
        switch issue.kind {
        case "no_network": return "Network Settings"
        case "sync_paused": return "Resume Sync"
        case "devices_not_found", "connection_blocked": return "Search Again"
        default: return nil
        }
    }

    private func act() {
        switch issue.kind {
        case "no_network":
            if let url = URL(string: "x-apple.systempreferences:com.apple.Network-Settings.extension") {
                NSWorkspace.shared.open(url)
            }
        case "sync_paused":
            if store.settings?.syncEnabled == false { store.toggleSync() }
        case "devices_not_found", "connection_blocked":
            store.scanForDevices()
        default:
            break
        }
    }
}
