import Foundation

@MainActor
final class DaemonManager {
    static let shared = DaemonManager()
    
    private var daemonProcess: Process?

    private init() {}

    func startDaemonIfNeeded() {
        if isDaemonSocketPresent() {
            ensureDaemonResponsive(forceRestartOnFailure: true)
            return
        }
        launchDaemonProcess()
    }

    private func launchDaemonProcess() {
        cleanupDaemonSocketIfNeeded()

        let candidates = [
            Bundle.main.resourceURL?.appendingPathComponent("linkall-daemon"),
            Bundle.main.executableURL?.deletingLastPathComponent().appendingPathComponent("linkall-daemon"),
            URL(fileURLWithPath: "/usr/local/bin/linkall-daemon"),
            URL(fileURLWithPath: "/opt/homebrew/bin/linkall-daemon")
        ].compactMap { $0 }

        guard let daemonURL = candidates.first(where: {
            FileManager.default.isExecutableFile(atPath: $0.path)
        }) else {
            NSLog("Link All: linkall-daemon not found in bundle or PATH candidates")
            return
        }

        let process = Process()
        process.executableURL = daemonURL
        process.environment = ProcessInfo.processInfo.environment.merging([
            "LINKALL_LOG": "info"
        ]) { current, _ in current }

        do {
            try process.run()
            daemonProcess = process
            NSLog("Link All: started daemon at \(daemonURL.path)")
        } catch {
            NSLog("Link All: failed to start daemon: \(error.localizedDescription)")
        }
    }

    func ensureDaemonResponsiveFromStore() {
        ensureDaemonResponsive(forceRestartOnFailure: true)
    }

    private func ensureDaemonResponsive(forceRestartOnFailure: Bool) {
        Task {
            let responded = await withTaskGroup(of: Bool.self) { group in
                group.addTask {
                    do {
                        try await LinkAllIPCClient.shared.ping()
                        return true
                    } catch {
                        return false
                    }
                }
                group.addTask {
                    try? await Task.sleep(nanoseconds: 1_000_000_000)
                    return false
                }
                let result = await group.next() ?? false
                group.cancelAll()
                return result
            }
            if !responded && forceRestartOnFailure {
                self.daemonProcess?.terminate()
                self.daemonProcess = nil
                self.cleanupDaemonSocketIfNeeded()
                self.launchDaemonProcess()
            }
        }
    }

    private func candidateSocketPaths() -> [String] {
        var paths: [String] = []
        if let runtime = ProcessInfo.processInfo.environment["XDG_RUNTIME_DIR"] {
            paths.append("\(runtime)/linkall.sock")
        }
        paths.append("/tmp/linkall-\(getuid())/linkall.sock")
        if let home = ProcessInfo.processInfo.environment["HOME"] {
            paths.append("\(home)/.linkall.sock")
        }
        return paths
    }

    private func isDaemonSocketPresent() -> Bool {
        for path in candidateSocketPaths() {
            if FileManager.default.fileExists(atPath: path) {
                return true
            }
        }
        return false
    }

    private func cleanupDaemonSocketIfNeeded() {
        for path in candidateSocketPaths() {
            if FileManager.default.fileExists(atPath: path) {
                try? FileManager.default.removeItem(atPath: path)
            }
        }
    }
    
    func terminate() {
        daemonProcess?.terminate()
        daemonProcess = nil
    }
}
