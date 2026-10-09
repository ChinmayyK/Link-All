import SwiftUI

struct StopwatchTextModifier: AnimatableModifier {
    var progress: Double

    var animatableData: Double {
        get { progress }
        set { progress = newValue }
    }

    func body(content: Content) -> some View {
        let textStr = String(format: "%.1f%%", progress * 100)
        return content.overlay(
            Text(textStr)
                .font(.system(size: 11, weight: .semibold, design: .monospaced))
                .foregroundColor(.secondary)
                .frame(width: 50, alignment: .trailing)
                .transaction { transaction in
                    transaction.animation = nil
                    transaction.disablesAnimations = true
                }
        )
    }
}

struct StopwatchProgressView: View {
    var progress: Double // Target progress (0.0 to 1.0)
    var tint: Color

    var body: some View {
        HStack(spacing: 8) {
            // Background track
            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(Color.primary.opacity(0.1))
                        .frame(height: 4)
                    
                    RoundedRectangle(cornerRadius: 2)
                        .fill(tint)
                        .frame(width: max(0, geo.size.width * CGFloat(progress)), height: 4)
                }
                .frame(maxHeight: .infinity)
            }
            .frame(height: 12)
            
            // Stopwatch formatted text
            Color.clear
                .frame(width: 50, height: 12)
                .modifier(StopwatchTextModifier(progress: progress))
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Transfer Progress")
        .accessibilityValue("\(Int(max(0, min(progress, 1)) * 100))%")
    }
}

struct StopwatchTransferTextModifier: AnimatableModifier {
    var bytesReceived: Double
    var totalBytes: Int64
    var speedBps: Int64?
    var etaSecs: Int64?
    var isDashboard: Bool

    var animatableData: Double {
        get { bytesReceived }
        set { bytesReceived = newValue }
    }

    func body(content: Content) -> some View {
        let mbTotal = Double(totalBytes) / 1_048_576.0
        let mbRecv = bytesReceived / 1_048_576.0
        
        let sizeStr: String
        if mbTotal >= 1.0 { 
            sizeStr = String(format: "%.2f / %.2f MB", mbRecv, mbTotal) 
        } else {
            let kbTotal = Double(totalBytes) / 1_024.0
            let kbRecv = bytesReceived / 1_024.0
            if kbTotal >= 1.0 { 
                sizeStr = String(format: "%.2f / %.2f KB", kbRecv, kbTotal) 
            } else {
                sizeStr = String(format: "%.0f / %lld B", bytesReceived, totalBytes)
            }
        }
        var dashboardText = sizeStr
        if isDashboard {
            if let speed = speedBps {
                let mbps = Double(speed) / 1_048_576.0
                if mbps >= 1.0 { dashboardText += String(format: " • %.1f MB/s", mbps) }
                else { dashboardText += String(format: " • %.0f KB/s", Double(speed) / 1024.0) }
            }
            if let eta = etaSecs, eta > 0 {
                dashboardText += " • \(eta)s remaining"
            }
        }
        
        return content.overlay(
            Group {
                if isDashboard {
                    Text(dashboardText)
                        .font(.system(size: 11, design: .rounded))
                        .foregroundStyle(CRTheme.inkSoft)
                        .frame(maxWidth: .infinity, alignment: .leading)
                } else {
                    Text(sizeStr)
                        .font(.system(size: 11, design: .monospaced))
                        .foregroundStyle(CRTheme.inkSoft)
                        .frame(maxWidth: .infinity, alignment: .trailing)
                }
            }
            .transaction { transaction in
                transaction.animation = nil
                transaction.disablesAnimations = true
            }
        )
    }
}

struct StopwatchSizeText: View {
    var bytesReceived: Double
    var totalBytes: Int64
    var speedBps: Int64? = nil
    var etaSecs: Int64? = nil
    var isDashboard: Bool = false

    var body: some View {
        let mbTotal = Double(totalBytes) / 1_048_576.0
        let mbRecv = bytesReceived / 1_048_576.0
        
        let sizeStr: String
        if mbTotal >= 1.0 { 
            sizeStr = String(format: "%.2f / %.2f MB", mbRecv, mbTotal) 
        } else {
            let kbTotal = Double(totalBytes) / 1_024.0
            let kbRecv = bytesReceived / 1_024.0
            if kbTotal >= 1.0 { 
                sizeStr = String(format: "%.2f / %.2f KB", kbRecv, kbTotal) 
            } else {
                sizeStr = String(format: "%.0f / %lld B", bytesReceived, totalBytes)
            }
        }
        
        if isDashboard {
            var text = sizeStr
            if let speed = speedBps {
                let mbps = Double(speed) / 1_048_576.0
                if mbps >= 1.0 { text += String(format: " • %.1f MB/s", mbps) }
                else { text += String(format: " • %.0f KB/s", Double(speed) / 1024.0) }
            }
            if let eta = etaSecs, eta > 0 {
                text += " • \(eta)s remaining"
            }
            return AnyView(
                Text(text)
                    .font(.system(size: 11, design: .rounded))
                    .hidden()
                    .modifier(StopwatchTransferTextModifier(
                        bytesReceived: bytesReceived,
                        totalBytes: totalBytes,
                        speedBps: speedBps,
                        etaSecs: etaSecs,
                        isDashboard: isDashboard
                    ))
            )
        } else {
            return AnyView(
                Text(sizeStr)
                    .font(.system(size: 11, design: .monospaced))
                    .hidden()
                    .modifier(StopwatchTransferTextModifier(
                        bytesReceived: bytesReceived,
                        totalBytes: totalBytes,
                        speedBps: speedBps,
                        etaSecs: etaSecs,
                        isDashboard: isDashboard
                    ))
            )
        }
    }
}
