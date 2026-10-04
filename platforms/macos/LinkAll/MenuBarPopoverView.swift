import SwiftUI

enum MenuBarPopoverAction {
    case dashboard
    case quickAccess
    case commandPalette
    case pushClipboard
    case sendFile
    case scan
    case connectByIP
    case diagnostics
    case quit
}

struct MenuBarPopoverView: View {
    @ObservedObject var store: LinkAllStore
    var onAction: (MenuBarPopoverAction) -> Void
    @Environment(\.colorScheme) var scheme

    var body: some View {
        VStack(spacing: 0) {
            // Header Hero
            VStack(spacing: 12) {
                HStack(spacing: 12) {
                    ZStack {
                        Circle()
                            .fill(store.connectedCount > 0 ? CRTheme.accentGreen.opacity(0.15) : Color.primary.opacity(0.05))
                            .frame(width: 40, height: 40)
                        
                        Image(systemName: store.connectedCount > 0 ? "link.badge.plus" : "wifi.slash")
                            .font(.system(size: 16, weight: .semibold))
                            .foregroundStyle(store.connectedCount > 0 ? CRTheme.accentGreen : Color.secondary)
                    }
                    
                    VStack(alignment: .leading, spacing: 2) {
                        Text(store.statusLine)
                            .font(.system(size: 14, weight: .bold, design: .rounded))
                            .foregroundStyle(Color.primary)
                            .lineLimit(1)
                        
                        if let status = store.dashboardStatus, let sync = status.lastSyncAt {
                            Text("Last sync: \(sync.relativeTimeString())")
                                .font(.system(size: 11, weight: .medium))
                                .foregroundStyle(Color.secondary)
                        } else {
                            Text(store.connectedCount > 0 ? "Mesh Active" : "No devices connected")
                                .font(.system(size: 11, weight: .medium))
                                .foregroundStyle(Color.secondary)
                        }
                    }
                    
                    Spacer()
                    
                    Button(action: { onAction(.diagnostics) }) {
                        Image(systemName: "info.circle")
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(Color.secondary.opacity(0.7))
                            .frame(width: 28, height: 28)
                            .background(Color.primary.opacity(0.05), in: Circle())
                    }
                    .buttonStyle(.plain)
                    .crHoverScale()

                    Button(action: { onAction(.quit) }) {
                        Image(systemName: "power")
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(Color.secondary.opacity(0.7))
                            .frame(width: 28, height: 28)
                            .background(Color.primary.opacity(0.05), in: Circle())
                    }
                    .buttonStyle(.plain)
                    .crHoverScale()
                }
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 16)
            
            Divider().opacity(0.5)
            
            // Actions Grid
            LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 10) {
                PopoverActionButton(
                    title: "Dashboard",
                    icon: "square.grid.2x2.fill",
                    tint: CRTheme.brandElectric,
                    action: { onAction(.dashboard) }
                )
                PopoverActionButton(
                    title: "Quick Access",
                    icon: "clock.arrow.circlepath",
                    tint: CRTheme.brandViolet,
                    action: { onAction(.quickAccess) }
                )
                PopoverActionButton(
                    title: "Files & Folders",
                    icon: "paperplane.fill",
                    tint: CRTheme.brandCyan,
                    action: { onAction(.sendFile) }
                )
                PopoverActionButton(
                    title: "Push Clipboard",
                    icon: "doc.on.clipboard.fill",
                    tint: CRTheme.brandPink,
                    action: { onAction(.pushClipboard) }
                )
            }
            .padding(16)
            
            // Secondary Actions
            HStack(spacing: 0) {
                Button(action: { onAction(.commandPalette) }) {
                    Text("Command Palette ⌘K")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(Color.secondary)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 10)
                }
                .buttonStyle(.plain)
                
                Divider().frame(height: 14)
                
                Button(action: { onAction(.scan) }) {
                    Text("Scan Network")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(Color.secondary)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 10)
                }
                .buttonStyle(.plain)

                Divider().frame(height: 14)

                // mDNS is blocked on networks with client isolation; this is the way in there.
                Button(action: { onAction(.connectByIP) }) {
                    Text("Connect by IP")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(Color.secondary)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 10)
                }
                .buttonStyle(.plain)
            }
            .background(Color.primary.opacity(0.02))
            .overlay(Rectangle().frame(height: 1).opacity(0.05), alignment: .top)
        }
        .frame(width: 320)
        .background(CRVisualEffect(material: .menu))
        .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .strokeBorder(Color.primary.opacity(0.1), lineWidth: 0.5)
        )
    }
}

private struct PopoverActionButton: View {
    let title: String
    let icon: String
    let tint: Color
    let action: () -> Void
    
    @State private var isHovered = false
    
    var body: some View {
        Button(action: {
            NSHapticFeedbackManager.defaultPerformer.perform(.generic, performanceTime: .default)
            action()
        }) {
            VStack(spacing: 8) {
                if #available(macOS 14.0, *) {
                    Image(systemName: icon)
                        .font(.system(size: 18, weight: .medium))
                        .foregroundStyle(tint)
                        .symbolEffect(.bounce, value: isHovered)
                        .shadow(color: tint.opacity(isHovered ? 0.4 : 0), radius: 4, y: 2)
                } else {
                    Image(systemName: icon)
                        .font(.system(size: 18, weight: .medium))
                        .foregroundStyle(tint)
                        .shadow(color: tint.opacity(isHovered ? 0.4 : 0), radius: 4, y: 2)
                }
                
                Text(title)
                    .font(.system(size: 12, weight: .semibold, design: .rounded))
                    .foregroundStyle(Color.primary.opacity(isHovered ? 1 : 0.8))
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 14)
            .background {
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .fill(Color.primary.opacity(isHovered ? 0.08 : 0.04))
                    .overlay(
                        RoundedRectangle(cornerRadius: 12, style: .continuous)
                            .strokeBorder(Color.primary.opacity(0.05), lineWidth: 1)
                    )
            }
        }
        .buttonStyle(.plain)
        .scaleEffect(isHovered ? 1.02 : 1.0)
        .onHover { isHovering in withAnimation(.crFast) { isHovered = isHovering } }
    }
}
