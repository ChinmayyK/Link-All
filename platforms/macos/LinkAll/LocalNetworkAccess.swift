import Foundation
import Network

/// Whether macOS lets Link All use the local network.
///
/// macOS asks once per app; if the answer was "Don't Allow" (or the app was
/// replaced and the prompt not answered yet), every connection Link All
/// starts to a device on the network fails with "No route to host", which
/// looks the same as a device that left the network. A Bonjour browse tells
/// the two apart: it waits with a policy-denied error only when the
/// permission is off, and turns ready once it is granted.
final class LocalNetworkAccess {
    private var browser: NWBrowser?
    private let onChange: (Bool) -> Void

    /// `onChange(true)` while access is denied, `false` once it works.
    init(onChange: @escaping (Bool) -> Void) {
        self.onChange = onChange
    }

    func start() {
        guard browser == nil else { return }
        let browser = NWBrowser(for: .bonjour(type: "_deskdrop._tcp", domain: nil), using: .tcp)
        browser.stateUpdateHandler = { [weak self] state in
            switch state {
            case .waiting(let error), .failed(let error):
                // kDNSServiceErr_PolicyDenied
                if case .dns(let code) = error, code == -65570 { self?.report(true) }
            case .ready:
                self?.report(false)
            default:
                break
            }
        }
        browser.browseResultsChangedHandler = { _, _ in }
        browser.start(queue: .main)
        self.browser = browser
    }

    private func report(_ denied: Bool) {
        DispatchQueue.main.async { self.onChange(denied) }
    }
}
