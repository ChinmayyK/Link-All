import AppKit
import SwiftUI

/// A newer Link All exists. Opens its release page, or shows the brew command
/// when Link All was installed with brew. Never installs anything.
struct UpdateBanner: View {
    let update: UpdateChecker.Update
    @ObservedObject var checker: UpdateChecker = .shared

    private let brewCommand = "brew upgrade --cask link-all"

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "arrow.down.circle.fill")
                .foregroundStyle(CRTheme.accentBlue)
                .font(.system(size: 15))
            VStack(alignment: .leading, spacing: 3) {
                Text("Link All \(update.version) is out")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(CRTheme.ink)
                Text(checker.installedWithBrew
                     ? "Update with Homebrew: \(brewCommand)"
                     : "Download it from the release page to update.")
                    .font(.system(size: 12))
                    .foregroundStyle(CRTheme.inkSoft)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 8)
            Button(checker.installedWithBrew ? "Copy Command" : "See What's New") {
                if checker.installedWithBrew {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(brewCommand, forType: .string)
                } else {
                    NSWorkspace.shared.open(update.url)
                }
            }
            .buttonStyle(.bordered)
            .controlSize(.small)
        }
        .padding(14)
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(CRTheme.accentBlue.opacity(0.10))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .strokeBorder(CRTheme.accentBlue.opacity(0.35), lineWidth: 1)
        )
    }
}
