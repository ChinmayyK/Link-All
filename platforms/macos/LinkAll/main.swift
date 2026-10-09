import AppKit

// The app was Deskdrop (com.deskdrop.Deskdrop) before it became Link All.
// Its preferences live under the old bundle ID; copy them over once so an
// update keeps the user's choices.
private func carryOverDeskdropPreferences() {
    let defaults = UserDefaults.standard
    let marker = "linkAllCarriedOverDeskdropPrefs"
    guard !defaults.bool(forKey: marker) else { return }
    if let old = defaults.persistentDomain(forName: "com.deskdrop.Deskdrop") {
        for (key, value) in old where defaults.object(forKey: key) == nil {
            defaults.set(value, forKey: key)
        }
    }
    defaults.set(true, forKey: marker)
}

carryOverDeskdropPreferences()

MainActor.assumeIsolated {
    let app = NSApplication.shared
    let delegate = AppDelegate()
    app.delegate = delegate
    app.run()
}
