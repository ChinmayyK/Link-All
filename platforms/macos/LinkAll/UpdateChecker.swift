import Combine
import Foundation

/// Looks for a newer Link All on GitHub, at most once a day, and only tells the
/// user: it never installs anything. Failures are silent.
@MainActor
final class UpdateChecker: ObservableObject {
    static let shared = UpdateChecker()

    struct Update: Equatable {
        let version: String
        let url: URL
    }

    static let enabledKey = "updateCheckEnabled"
    private static let lastCheckKey = "updateLastCheck"
    private static let latestVersionKey = "updateLatestVersion"
    private static let latestURLKey = "updateLatestURL"
    private nonisolated static let latestEndpoint = URL(string: "https://api.github.com/repos/ChinmayyK/Link-All/releases/latest")!
    private static let every: TimeInterval = 24 * 60 * 60

    /// The update to offer, or nil.
    @Published private(set) var available: Update?

    private var timer: Timer?

    var isEnabled: Bool {
        get { UserDefaults.standard.object(forKey: Self.enabledKey) as? Bool ?? true }
        set {
            UserDefaults.standard.set(newValue, forKey: Self.enabledKey)
            if newValue { checkIfDue() } else { available = nil }
        }
    }

    /// Installed with `brew install --cask`, so the way to update is brew.
    var installedWithBrew: Bool {
        ["/opt/homebrew/Caskroom/link-all", "/usr/local/Caskroom/link-all"]
            .contains { FileManager.default.fileExists(atPath: $0) }
    }

    /// Checks now if a day has passed, then keeps checking while the app runs.
    func start() {
        checkIfDue()
        guard timer == nil else { return }
        timer = Timer.scheduledTimer(withTimeInterval: 6 * 60 * 60, repeats: true) { _ in
            Task { @MainActor in UpdateChecker.shared.checkIfDue() }
        }
    }

    func checkIfDue() {
        guard isEnabled else { return }
        let defaults = UserDefaults.standard
        let current = Self.installedVersion
        if let known = defaults.string(forKey: Self.latestVersionKey),
           let page = defaults.string(forKey: Self.latestURLKey).flatMap(URL.init(string:)),
           Self.isNewer(known, than: current) {
            available = Update(version: known, url: page)
        } else {
            available = nil
        }

        let last = defaults.object(forKey: Self.lastCheckKey) as? Date ?? .distantPast
        guard Date().timeIntervalSince(last) >= Self.every else { return }
        Task {
            guard let latest = await Self.fetchLatest() else { return }
            defaults.set(Date(), forKey: Self.lastCheckKey)
            defaults.set(latest.version, forKey: Self.latestVersionKey)
            defaults.set(latest.url.absoluteString, forKey: Self.latestURLKey)
            if isEnabled { available = Self.isNewer(latest.version, than: current) ? latest : nil }
        }
    }

    private static var installedVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""
    }

    private nonisolated static func fetchLatest() async -> Update? {
        var request = URLRequest(url: latestEndpoint, timeoutInterval: 8)
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        guard let (data, response) = try? await URLSession.shared.data(for: request),
              (response as? HTTPURLResponse)?.statusCode == 200 else { return nil }
        return parse(data)
    }

    nonisolated static func parse(_ data: Data) -> Update? {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              json["draft"] as? Bool != true, json["prerelease"] as? Bool != true,
              let tag = json["tag_name"] as? String,
              let page = json["html_url"] as? String, let url = URL(string: page) else { return nil }
        let version = tag.hasPrefix("v") ? String(tag.dropFirst()) : tag
        // Only ever open a page on the project's own repository.
        guard !version.isEmpty, page.hasPrefix("https://github.com/ChinmayyK/Link-All/") else { return nil }
        return Update(version: version, url: url)
    }

    /// True when `latest` is a higher major.minor.patch than `current`.
    nonisolated static func isNewer(_ latest: String, than current: String) -> Bool {
        func parts(_ v: String) -> [Int] {
            v.split(whereSeparator: { ".-+".contains($0) }).prefix(3).map { Int($0.prefix(while: \.isNumber)) ?? 0 }
        }
        let a = parts(latest), b = parts(current)
        for i in 0..<3 {
            let x = i < a.count ? a[i] : 0, y = i < b.count ? b[i] : 0
            if x != y { return x > y }
        }
        return false
    }
}
