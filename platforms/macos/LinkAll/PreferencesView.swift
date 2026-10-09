// PreferencesView.swift — Link All macOS v4
// System Settings aesthetic: icon tabs, live dirty state, port validation, fingerprint display.

import SwiftUI
import AppKit

// MARK: - Root

struct PreferencesView: View {
    @ObservedObject var store: LinkAllStore

    @State private var tab              = SettingsTab.general
    @State private var copy             = LinkAllSettingsSnapshot.defaults
    @State private var patternDraft     = ""
    @State private var portString       = "47823"
    @State private var portIsInvalid    = false
    @State private var saveState        = SaveState.idle

    /// Unsaved means different from what the daemon has, so undoing a change
    /// by hand clears it too.
    private var isDirty: Bool { store.settings.map { copy != $0 } ?? false }

    var body: some View {
        VStack(spacing: 0) {
            // Header with horizontal tabs
            VStack(alignment: .leading, spacing: 20) {
                Text("Settings")
                    .font(.system(size: 28, weight: .bold))
                    .foregroundStyle(CRTheme.ink)
                    .padding(.horizontal, 40)
                    .padding(.top, 40)

                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(SettingsTab.allCases) { t in 
                            PrefsTabChip(tab: t, isSelected: tab == t) { tab = t } 
                        }
                    }
                    .padding(.horizontal, 40)
                    .padding(.bottom, 16)
                }
            }
            .background(CRTheme.surfaceElevated)

            Rectangle().fill(CRTheme.stroke).frame(height: 1)

            // Content
            ScrollView(.vertical, showsIndicators: false) {
                VStack(alignment: .leading, spacing: 0) {
                    pane
                        .padding(.horizontal, 40)
                        .padding(.top, 32)
                        .padding(.bottom, 28)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)

            PrefsFooter(
                isLoaded:      store.settings != nil,
                isDirty:       isDirty,
                portIsInvalid: portIsInvalid,
                saveState:     saveState,
                onRevert: {
                    if let s = store.settings { copy = s; portString = "\(s.port)" }
                    portIsInvalid = false
                },
                onSave: {
                    guard !portIsInvalid, saveState != .saving else { return }
                    saveState = .saving
                    Task {
                        let ok = await store.saveSettingsNow(copy)
                        if ok, let s = store.settings { copy = s }
                        saveState = ok ? .saved : .failed
                        if ok {
                            try? await Task.sleep(nanoseconds: 2_500_000_000)
                            if saveState == .saved { saveState = .idle }
                        }
                    }
                }
            )
        }
        .background(CRTheme.surface.ignoresSafeArea())
        // Fresh values each time the window opens, not whatever was last
        // cached; the controls show defaults only until this returns.
        .task {
            await store.loadSettings()
            if let s = store.settings { copy = s; portString = "\(s.port)" }
        }
        .onChange(of: copy) { _ in
            if saveState != .saving { saveState = .idle }
        }
    }

    @ViewBuilder private var pane: some View {
        switch tab {
        case .general:       GeneralPane(copy: $copy)
        case .sync:          SyncPane(copy: $copy, patternDraft: $patternDraft)
        case .notifications: NotificationsPane()
        case .network:       NetworkPane(store: store, copy: $copy, portString: $portString, portIsInvalid: $portIsInvalid)
                            .onChange(of: portString) { v in
                                if let p = UInt16(v), p > 1024 { copy.port = p; portIsInvalid = false }
                                else { portIsInvalid = true }
                            }
        case .security:      SecurityPane(copy: $copy, store: store)
        }
    }
}

// MARK: - Tab Bar

private enum SettingsTab: String, CaseIterable, Identifiable {
    case general, sync, notifications, network, security
    var id: String { rawValue }
    var label: String { rawValue.capitalized }
    var icon:  String {
        switch self {
        case .general:       return "switch.2"
        case .sync:          return "arrow.triangle.2.circlepath"
        case .notifications: return "bell.badge"
        case .network:       return "network"
        case .security:      return "lock.shield"
        }
    }
    var tint: Color {
        switch self {
        case .general:       return CRTheme.accentBlue
        case .sync:          return CRTheme.accentGreen
        case .notifications: return CRTheme.accentPurple
        case .network:       return CRTheme.accentIndigo
        case .security:      return CRTheme.accentOrange
        }
    }
}

private struct PrefsTabChip: View {
    let tab: SettingsTab; let isSelected: Bool; let action: () -> Void
    @State private var hovered = false
    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                ZStack {
                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                        .fill(isSelected ? tab.tint : Color.clear)
                        .frame(width: 24, height: 24)
                    Image(systemName: isSelected ? tab.icon : tab.icon)
                        .font(.system(size: 13, weight: isSelected ? .bold : .medium))
                        .foregroundStyle(isSelected ? Color.white : CRTheme.inkSoft)
                }
                Text(tab.label)
                    .font(.system(size: 14, weight: isSelected ? .semibold : .medium))
                    .foregroundStyle(isSelected ? CRTheme.ink : CRTheme.inkSoft)
            }
            .padding(.horizontal, 14).padding(.vertical, 8)
            .background {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(isSelected ? CRTheme.surface : (hovered ? CRTheme.surfaceElevated : .clear))
                    .overlay {
                        if isSelected {
                            RoundedRectangle(cornerRadius: 10, style: .continuous)
                                .strokeBorder(Color.white.opacity(0.15), lineWidth: 1.0)
                        }
                    }
                    .shadow(color: .black.opacity(isSelected ? 0.08 : 0), radius: 6, y: 2)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain).scaleEffect(hovered && !isSelected ? 1.02 : 1.0).onHover { hovered = $0 }
        .animation(.crBounce, value: isSelected).animation(.crSpring, value: hovered)
    }
}

// MARK: - Footer

private enum SaveState { case idle, saving, saved, failed }

private struct PrefsFooter: View {
    let isLoaded:      Bool
    let isDirty:       Bool
    let portIsInvalid: Bool
    let saveState:     SaveState
    let onRevert:      () -> Void
    let onSave:        () -> Void

    private var canSave: Bool { isLoaded && isDirty && !portIsInvalid && saveState != .saving }

    var body: some View {
        VStack(spacing: 0) {
            CRDivider()
            HStack(spacing: 10) {
                // Status indicators
                Group {
                    if portIsInvalid {
                        HStack(spacing: 5) {
                            Image(systemName: "exclamationmark.triangle.fill")
                                .font(.system(size: 11)).foregroundStyle(CRTheme.accentRed)
                            Text("Port must be 1025–65535")
                                .font(.system(size: 12, weight: .medium)).foregroundStyle(CRTheme.accentRed)
                        }
                        .transition(.move(edge: .leading).combined(with: .opacity))
                    } else if !isLoaded {
                        footerNote("Loading settings…", color: CRTheme.inkSubtle)
                    } else if saveState == .saving {
                        HStack(spacing: 6) {
                            ProgressView().controlSize(.small)
                            Text("Saving…").font(.system(size: 12, weight: .medium)).foregroundStyle(CRTheme.inkSoft)
                        }
                    } else if saveState == .saved {
                        HStack(spacing: 5) {
                            Image(systemName: "checkmark.circle.fill")
                                .font(.system(size: 12)).foregroundStyle(CRTheme.accentGreen)
                            Text("Saved").font(.system(size: 12, weight: .medium)).foregroundStyle(CRTheme.accentGreen)
                        }
                        .transition(.opacity)
                    } else if saveState == .failed {
                        footerNote("Couldn't save. Is Link All running?", color: CRTheme.accentRed)
                    } else if isDirty {
                        HStack(spacing: 5) {
                            Circle().fill(CRTheme.accentYellow).frame(width: 5.5, height: 5.5)
                            Text("Unsaved changes")
                                .font(.system(size: 12, weight: .medium)).foregroundStyle(CRTheme.accentYellow)
                        }
                        .transition(.move(edge: .leading).combined(with: .opacity))
                    }
                }
                Spacer()
                Button("Revert", action: onRevert)
                    .buttonStyle(CRSecondaryButtonStyle())
                    .disabled(!isDirty || saveState == .saving)
                    .opacity(isDirty ? 1 : 0.45)
                Button(saveState == .saving ? "Saving…" : "Save Changes", action: onSave)
                    .buttonStyle(CRPrimaryButtonStyle())
                    .disabled(!canSave)
                    // The style draws the same blue either way; dim it so a
                    // disabled button doesn't look clickable.
                    .opacity(canSave ? 1 : 0.45)
                    .keyboardShortcut("s", modifiers: .command)
            }
            .padding(.horizontal, 24).padding(.vertical, 12)
        }
        .animation(.crFast, value: isDirty)
        .animation(.crFast, value: portIsInvalid)
        .animation(.crFast, value: saveState)
    }

    private func footerNote(_ text: String, color: Color) -> some View {
        Text(text).font(.system(size: 12, weight: .medium)).foregroundStyle(color)
    }
}

// MARK: - Notifications Pane

// The same rows as the other panes. This tab used to embed a macOS Form,
// which collapses to nothing inside the settings ScrollView.
private struct NotificationsPane: View {
    @AppStorage("showTransferNotifications") private var showTransferNotifications = true
    @AppStorage("keepVisibleDuringTransfer") private var keepVisibleDuringTransfer = false
    @AppStorage("playCompletionSound") private var playCompletionSound = true
    @AppStorage("minFileSizeThresholdMB") private var minFileSizeThresholdMB = 0.0

    var body: some View {
        PrefsSection(title: "Transfers", icon: "arrow.up.arrow.down.circle.fill", tint: CRTheme.accentPurple) {
            PrefsRow(icon: "bell.fill", label: "Transfer notifications",
                     description: "Show progress and completion for files you send and receive.") {
                Toggle("", isOn: $showTransferNotifications).labelsHidden()
            }
            PrefsDivider()
            PrefsRow(icon: "pin.fill", label: "Keep visible while sending",
                     description: "Leave the progress notification on screen until the transfer ends.") {
                Toggle("", isOn: $keepVisibleDuringTransfer).labelsHidden()
            }
            .disabled(!showTransferNotifications)
            PrefsDivider()
            PrefsRow(icon: "speaker.wave.2.fill", label: "Completion sound",
                     description: "Play a sound when a transfer finishes.") {
                Toggle("", isOn: $playCompletionSound).labelsHidden()
            }
            .disabled(!showTransferNotifications)
            PrefsDivider()
            PrefsRow(icon: "line.3.horizontal.decrease.circle", label: "Only for files larger than",
                     description: "Smaller transfers finish without a notification. 0 shows all of them.") {
                HStack(spacing: 6) {
                    TextField("0", value: $minFileSizeThresholdMB, format: .number)
                        .crInput()
                        .multilineTextAlignment(.trailing)
                        .frame(width: 70)
                    Text("MB").font(.system(size: 13)).foregroundStyle(CRTheme.inkSoft)
                }
            }
            .disabled(!showTransferNotifications)
        }
    }
}

// MARK: - General Pane

private struct GeneralPane: View {
    @Binding var copy: LinkAllSettingsSnapshot
    @AppStorage("cr_app_theme") private var appTheme: String = "system"
    @AppStorage("mirrorAndroidNotifications") private var mirrorAndroidNotifications: Bool = true
    @AppStorage("autoForwardMacScreenshots") private var autoForwardMacScreenshots: Bool = false
    @AppStorage("magicClipboardEnabled") private var magicClipboardEnabled: Bool = false

    var body: some View {
        PrefsSection(title: "Identity", icon: "person.crop.circle.fill", tint: CRTheme.accentBlue) {
            PrefsRow(icon: "tag.fill", label: "Device name",
                     description: "How this Mac appears to other Link All peers.") {
                TextField("MacBook Pro", text: $copy.deviceName).crInput().frame(maxWidth: 220)
            }
            PrefsDivider()
            PrefsRow(icon: "power", label: "Start on login",
                     description: "Launch Link All automatically at login.") {
                Toggle("", isOn: $copy.startOnLogin).labelsHidden()
            }
            PrefsDivider()
            PrefsRow(icon: "sparkles", label: "Onboarding",
                     description: "View the welcome tour again.") {
                Button("Replay Tour") {
                    UserDefaults.standard.set(false, forKey: "hasCompletedOnboarding")
                }
                .buttonStyle(CRSecondaryButtonStyle())
            }
        }

        PrefsSection(title: "Ambient Continuity", icon: "star.fill", tint: CRTheme.accentYellow) {
            PrefsRow(icon: "camera.viewfinder", label: "Screenshot Sync",
                     description: "Instantly sends Mac screenshots to connected Android devices.") {
                Toggle("", isOn: $autoForwardMacScreenshots).labelsHidden()
            }
            PrefsDivider()
            PrefsRow(icon: "doc.on.clipboard.fill", label: "Magic Clipboard",
                     description: "Silently auto-sync Mac clipboard to connected devices in the background.") {
                Toggle("", isOn: $magicClipboardEnabled).labelsHidden()
            }
        }

        PrefsSection(title: "Notifications", icon: "app.badge.fill", tint: CRTheme.accentOrange) {
            PrefsRow(icon: "iphone.radiowaves.left.and.right", label: "Mirror Android Notifications",
                     description: "Receive notifications from connected Android devices.") {
                Toggle("", isOn: $mirrorAndroidNotifications).labelsHidden()
            }
        }
        
        PrefsSection(title: "Appearance", icon: "circle.lefthalf.filled", tint: CRTheme.accentIndigo) {
            PrefsRow(icon: "sun.max.fill", label: "Light",
                     description: "Classic clean look with light backgrounds.") {
                if appTheme == "light" {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(CRTheme.accentBlue)
                }
            }
            .contentShape(Rectangle())
            .onTapGesture { applyTheme("light") }
            PrefsDivider()
            PrefsRow(icon: "moon.fill", label: "True Black",
                     description: "Deep black — ideal for dark environments.") {
                if appTheme == "dark" {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(CRTheme.accentBlue)
                }
            }
            .contentShape(Rectangle())
            .onTapGesture { applyTheme("dark") }
            PrefsDivider()
            PrefsRow(icon: "circle.righthalf.filled", label: "System Default",
                     description: "Follow macOS system appearance setting.") {
                if appTheme == "system" {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(CRTheme.accentBlue)
                }
            }
            .contentShape(Rectangle())
            .onTapGesture { applyTheme("system") }
        }

        PrefsSection(title: "Notifications", icon: "bell.badge.fill", tint: CRTheme.accentPurple) {
            PrefsRow(icon: "bell.fill", label: "Show receive notification",
                     description: "Display a banner when a peer pushes content to this Mac.") {
                Toggle("", isOn: $copy.showReceiveNotification).labelsHidden()
            }
        }

        PrefsSection(title: "History", icon: "clock.fill", tint: CRTheme.accentIndigo) {
            PrefsRow(icon: "list.number", label: "Max timeline items",
                     description: "Oldest entries are dropped when the limit is reached.") {
                Stepper("\(copy.historyLimit) items",
                        value: $copy.historyLimit, in: 10...500, step: 10)
                    .frame(maxWidth: 190)
            }
            PrefsDivider()
            PrefsRow(icon: "doc.text", label: "Max text size per item",
                     description: "Clipboard text larger than this won't be stored locally.") {
                Stepper("\(copy.maxHistoryTextBytes / 1024) KB",
                        value: Binding(
                            get: { copy.maxHistoryTextBytes / 1024 },
                            set: { copy.maxHistoryTextBytes = $0 * 1024 }
                        ), in: 4...512, step: 4)
                    .frame(maxWidth: 190)
            }
        }
    }

    private func applyTheme(_ theme: String) {
        appTheme = theme
        switch theme {
        case "light":  NSApp.appearance = NSAppearance(named: .aqua)
        case "dark":   NSApp.appearance = NSAppearance(named: .darkAqua)
        default:       NSApp.appearance = nil  // follows system
        }
    }
}

// MARK: - Sync Pane

private struct SyncPane: View {
    @Binding var copy: LinkAllSettingsSnapshot
    @Binding var patternDraft: String

    var body: some View {
        PrefsSection(title: "Sync Control", icon: "arrow.triangle.2.circlepath", tint: CRTheme.accentGreen) {
            PrefsRow(icon: "power.circle.fill", label: "Enable sync",
                     description: "Toggle clipboard sync for all devices at once.") {
                Toggle("", isOn: $copy.syncEnabled).labelsHidden()
            }
            PrefsDivider()
            PrefsRow(icon: "gearshape.2.fill", label: "Sync mode",
                     description: "Auto syncs immediately. Manual requires an explicit trigger.") {
                Picker("", selection: $copy.syncMode) {
                    Text("Auto").tag(SyncModeModel.auto)
                    Text("Manual").tag(SyncModeModel.manual)
                    Text("Receive only").tag(SyncModeModel.receive)
                }
                .pickerStyle(.segmented).labelsHidden().frame(maxWidth: 220)
            }
        }

        PrefsSection(title: "Content Types", icon: "doc.on.doc.fill", tint: CRTheme.accentBlue) {
            PrefsRow(icon: "doc.text.fill",  label: "Text")   { Toggle("", isOn: $copy.syncText).labelsHidden() }
            PrefsDivider()
            PrefsRow(icon: "photo.fill",     label: "Images") { Toggle("", isOn: $copy.syncImages).labelsHidden() }
            PrefsDivider()
            PrefsRow(icon: "folder.fill",    label: "Files")  { Toggle("", isOn: $copy.syncFiles).labelsHidden() }
            PrefsDivider()
            PrefsRow(icon: "arrow.up.arrow.down.circle.fill", label: "Max payload",
                     description: "Largest item that will be synced to peers.") {
                Stepper("\(Int(copy.maxPayloadBytes / 1024 / 1024)) MB",
                        value: Binding(
                            get: { Int(copy.maxPayloadBytes / 1024 / 1024) },
                            set: { copy.maxPayloadBytes = UInt64($0) * 1024 * 1024 }
                        ), in: 1...512)
                    .frame(maxWidth: 180)
            }
        }

        PrefsSection(title: "Ignore Patterns", icon: "eye.slash.fill", tint: CRTheme.accentRed) {
            VStack(alignment: .leading, spacing: 11) {
                Text("Items matching these substrings won't be synced. Useful for passwords, OTPs, and API keys.")
                    .font(.system(size: 12)).foregroundStyle(CRTheme.inkSoft).lineSpacing(2)
                    .fixedSize(horizontal: false, vertical: true)

                HStack(spacing: 8) {
                    TextField("e.g. password, Bearer, sk-", text: $patternDraft)
                        .crInput()
                    Button("Add") {
                        let t = patternDraft.trimmingCharacters(in: .whitespacesAndNewlines)
                        guard !t.isEmpty else { return }
                        copy.ignorePatterns.append(t)
                        patternDraft = ""
                    }
                    .buttonStyle(CRPrimaryButtonStyle())
                    .disabled(patternDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }

                if !copy.ignorePatterns.isEmpty {
                    VStack(spacing: 3) {
                        ForEach(copy.ignorePatterns, id: \.self) { pattern in
                            HStack(spacing: 9) {
                                Button {
                                    copy.ignorePatterns.removeAll { $0 == pattern }
                                } label: {
                                    Image(systemName: "xmark.circle.fill")
                                        .font(.system(size: 14))
                                        .foregroundStyle(CRTheme.accentRed.opacity(0.60))
                                }
                                .buttonStyle(.plain)
                                Text(pattern)
                                    .font(.system(size: 12, design: .monospaced)).foregroundStyle(CRTheme.ink)
                                Spacer()
                                CRTag(text: "substring", tint: CRTheme.inkSubtle)
                            }
                            .padding(.horizontal, 10).padding(.vertical, 7)
                            .background {
                                RoundedRectangle(cornerRadius: 7, style: .continuous)
                                    .fill(CRTheme.surface)
                                    .overlay {
                                        RoundedRectangle(cornerRadius: 7, style: .continuous)
                                            .strokeBorder(CRTheme.stroke.opacity(0.40), lineWidth: 0.5)
                                    }
                            }
                        }
                    }
                }
            }
            .padding(.horizontal, 14).padding(.vertical, 13)
        }
    }
}

// MARK: - Network Pane

private struct NetworkPane: View {
    @ObservedObject var store: LinkAllStore
    @Binding var copy: LinkAllSettingsSnapshot
    @Binding var portString: String
    @Binding var portIsInvalid: Bool

    var body: some View {
        PrefsSection(title: "Direct Connection", icon: "network", tint: CRTheme.brandElectric) {
            VStack(alignment: .leading, spacing: 12) {
                Text("Connect directly to a peer on a non-local network or custom VLAN using an IP address and port.")
                    .font(.system(size: 12.5))
                    .foregroundStyle(CRTheme.inkSoft)
                    .lineSpacing(2)
                    .fixedSize(horizontal: false, vertical: true)
                
                HStack(spacing: 10) {
                    TextField("IP Address:Port (e.g. 192.168.1.50:47823)", text: $store.manualConnectAddress)
                        .crInput()
                        .onSubmit { store.connectManual() }
                    
                    Button("Connect") {
                        store.connectManual()
                    }
                    .buttonStyle(CRPrimaryButtonStyle(tint: CRTheme.brandElectric))
                    .disabled(store.manualConnectAddress.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 14)
        }

        PrefsSection(title: "Listener", icon: "antenna.radiowaves.left.and.right", tint: CRTheme.accentIndigo) {
            PrefsRow(icon: "number.circle.fill", label: "Port",
                     description: "TCP port the daemon binds to. Changes take effect on restart.") {
                TextField("47823", text: $portString)
                    .crInput(invalid: portIsInvalid)
                    .frame(width: 90)
            }
        }

        PrefsSection(title: "Clipboard Polling", icon: "timer", tint: CRTheme.accentBlue) {
            PrefsRow(icon: "clock.arrow.circlepath", label: "Poll interval",
                     description: "How often the daemon checks for new clipboard content.") {
                Stepper("\(copy.clipboardPollMs) ms",
                        value: Binding(
                            get: { Int(copy.clipboardPollMs) },
                            set: { copy.clipboardPollMs = UInt64($0) }
                        ), in: 50...2000, step: 25)
                    .frame(maxWidth: 190)
            }
        }

        PrefsSection(title: "Smart Sync", icon: "sparkles", tint: CRTheme.accentGreen) {
            PrefsRow(icon: "rectangle.2.swap", label: "Duplicate window",
                     description: "Content seen within this window won't be re-broadcast.") {
                Stepper("\(copy.smartSyncDuplicateWindowMs) ms",
                        value: Binding(
                            get: { Int(copy.smartSyncDuplicateWindowMs) },
                            set: { copy.smartSyncDuplicateWindowMs = UInt64($0) }
                        ), in: 100...10000, step: 100)
                    .frame(maxWidth: 205)
            }
            PrefsDivider()
            PrefsRow(icon: "clock.badge.checkmark", label: "Debounce window",
                     description: "Delay after a copy event before broadcasting.") {
                Stepper("\(copy.smartSyncDebounceMs) ms",
                        value: Binding(
                            get: { Int(copy.smartSyncDebounceMs) },
                            set: { copy.smartSyncDebounceMs = UInt64($0) }
                        ), in: 25...2000, step: 25)
                    .frame(maxWidth: 205)
            }
        }

        PrefsSection(title: "Rate Limiting", icon: "gauge.with.dots.needle.67percent", tint: CRTheme.accentOrange) {
            PrefsRow(icon: "arrow.up.circle.fill", label: "Max pushes / sec") {
                Stepper("\(copy.maxPushesPerSec)", value: $copy.maxPushesPerSec, in: 1...60)
                    .frame(maxWidth: 165)
            }
            PrefsDivider()
            PrefsRow(icon: "bolt.circle.fill", label: "Burst allowance",
                     description: "Extra pushes allowed before the sustained limit applies.") {
                Stepper("\(copy.rateLimitBurst)", value: $copy.rateLimitBurst, in: 1...20)
                    .frame(maxWidth: 165)
            }
        }
    }
}

// MARK: - Security Pane

private struct SecurityPane: View {
    @Binding var copy: LinkAllSettingsSnapshot
    @ObservedObject var store: LinkAllStore

    var body: some View {
        PrefsSection(title: "Trust", icon: "checkmark.shield", tint: CRTheme.accentGreen) {
            PrefsRow(icon: "person.badge.clock.fill", label: "Require trust confirmation",
                     description: "Show a prompt for every new device before allowing sync (TOFU model).") {
                Toggle("", isOn: $copy.requireTofuConfirmation).labelsHidden()
            }
        }

        PrefsSection(title: "Content Filtering", icon: "eye.slash.circle.fill", tint: CRTheme.accentOrange) {
            PrefsRow(icon: "key.slash.fill", label: "Block likely secrets",
                     description: "Skip sync for content matching common secret patterns.") {
                Toggle("", isOn: $copy.blockSensitiveText).labelsHidden()
            }
        }

        // This device's identity — name + cryptographic fingerprint for peer verification.
        if let s = store.settings {
            PrefsSection(title: "This Device", icon: "checkmark.seal.fill", tint: CRTheme.accentBlue) {
                PrefsRow(icon: "person.crop.circle.fill", label: "Name") {
                    Text(s.deviceName.isEmpty ? "Unnamed" : s.deviceName)
                        .font(.system(size: 13)).foregroundStyle(CRTheme.inkSoft)
                }
                if let fp = store.localFingerprint, !fp.isEmpty {
                    PrefsDivider()
                    PrefsRow(icon: "key.fill", label: "Fingerprint") {
                        HStack(spacing: 8) {
                            Text(String(fp.prefix(16)) + "...")
                                .font(.system(size: 11, design: .monospaced))
                                .foregroundStyle(CRTheme.inkSoft)
                            Button("Copy") {
                                NSPasteboard.general.clearContents()
                                NSPasteboard.general.setString(fp, forType: .string)
                            }
                            .buttonStyle(CRSecondaryButtonStyle())
                        }
                    }
                }
            }
        }

        // Security model info card
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                ZStack {
                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                        .fill(CRTheme.accentGreen.opacity(0.10)).frame(width: 22, height: 22)
                    Image(systemName: "checkmark.shield.fill")
                        .font(.system(size: 10.5, weight: .semibold)).foregroundStyle(CRTheme.accentGreen)
                }
                Text("Security Architecture")
                    .font(.system(size: 13, weight: .semibold)).foregroundStyle(CRTheme.ink)
            }

            VStack(alignment: .leading, spacing: 8) {
                SecInfoRow(icon: "lock.fill",        tint: CRTheme.accentBlue,
                           text: "Traffic is encrypted end-to-end with the Noise protocol.")
                SecInfoRow(icon: "person.2.slash",   tint: CRTheme.accentGreen,
                           text: "No relay servers. Peers connect directly on your local network.")
                SecInfoRow(icon: "checkmark.shield", tint: CRTheme.accentIndigo,
                           text: "TOFU trust model — each device fingerprint is verified on first connection.")
            }
        }
        .padding(14)
        .background {
            RoundedRectangle(cornerRadius: 11, style: .continuous)
                .fill(CRTheme.surface)
                .overlay {
                    RoundedRectangle(cornerRadius: 11, style: .continuous)
                        .strokeBorder(CRTheme.stroke.opacity(0.40), lineWidth: 0.5)
                }
        }
        .padding(.bottom, 24)
    }
}

private struct SecInfoRow: View {
    let icon: String; let tint: Color; let text: String
    var body: some View {
        HStack(alignment: .top, spacing: 9) {
            ZStack {
                Circle().fill(tint.opacity(0.09)).frame(width: 22, height: 22)
                Image(systemName: icon).font(.system(size: 10, weight: .semibold)).foregroundStyle(tint)
            }
            Text(text).font(.system(size: 12)).foregroundStyle(CRTheme.inkSoft)
                .fixedSize(horizontal: false, vertical: true).lineSpacing(1.5)
            Spacer()
        }
    }
}

// MARK: - Shared Section + Row + Divider

private struct PrefsSection<Content: View>: View {
    let title: String; let icon: String; let tint: Color
    @ViewBuilder var content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                ZStack {
                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                        .fill(tint.opacity(0.12)).frame(width: 24, height: 24)
                    Image(systemName: icon).font(.system(size: 11, weight: .bold))
                        .foregroundStyle(tint).symbolRenderingMode(.hierarchical)
                }
                Text(title.uppercased())
                    .font(.system(size: 11, weight: .bold)).tracking(1.0).foregroundStyle(CRTheme.inkSubtle)
            }
            .padding(.leading, 4)

            VStack(spacing: 0) { content() }
                .background(.regularMaterial)
                .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
                .shadow(color: .black.opacity(0.08), radius: 14, y: 6)
                .overlay {
                    RoundedRectangle(cornerRadius: 18, style: .continuous)
                        .strokeBorder(Color.white.opacity(0.2), lineWidth: 1.0)
                }
        }
        .padding(.bottom, 32)
    }
}

private struct PrefsRow<Control: View>: View {
    var icon: String? = nil
    let label: String
    var description: String? = nil
    @ViewBuilder var control: () -> Control

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            HStack(alignment: .center, spacing: 12) {
                if let icon {
                    Image(systemName: icon).font(.system(size: 14, weight: .medium))
                        .foregroundStyle(CRTheme.inkSubtle).frame(width: 20)
                        .symbolRenderingMode(.hierarchical)
                }
                VStack(alignment: .leading, spacing: 3) {
                    Text(label).font(.system(size: 14, weight: .semibold)).foregroundStyle(CRTheme.ink)
                    if let d = description {
                        Text(d).font(.system(size: 12)).foregroundStyle(CRTheme.inkSoft)
                            .fixedSize(horizontal: false, vertical: true).lineSpacing(1.5)
                    }
                }
            }
            Spacer(minLength: 16)
            control()
        }
        .padding(.horizontal, 18).padding(.vertical, 14)
    }
}

private struct PrefsDivider: View {
    var body: some View { Divider().padding(.leading, 50) }
}

// MARK: - Settings Snapshot default

extension LinkAllSettingsSnapshot {
    static var defaults: LinkAllSettingsSnapshot {
        LinkAllSettingsSnapshot(
            port: 47823, deviceName: "", syncEnabled: true,
            syncText: true, syncImages: true, syncFiles: true,
            syncMode: .auto, maxPayloadBytes: 64 * 1024 * 1024,
            historyLimit: 50, maxHistoryTextBytes: 64 * 1024,
            showReceiveNotification: true, requireTofuConfirmation: true,
            blockedDeviceIds: [], blockSensitiveText: true,
            ignorePatterns: [], clipboardPollMs: 100,
            maxPushesPerSec: 10, rateLimitBurst: 3,
            smartSyncDuplicateWindowMs: 1500, smartSyncDebounceMs: 150,
            startOnLogin: false
        )
    }
}

// MARK: - Fingerprint formatting

/// Groups a hex fingerprint into readable pairs: "AB:CD:EF:12:34..."
private func formattedFingerprint(_ raw: String) -> String {
    let clean = raw.replacingOccurrences(of: ":", with: "")
        .uppercased()
        .filter { $0.isHexDigit }
    var pairs: [String] = []
    var i = clean.startIndex
    while i < clean.endIndex {
        let j = clean.index(i, offsetBy: 2, limitedBy: clean.endIndex) ?? clean.endIndex
        pairs.append(String(clean[i..<j]))
        i = j
    }
    // Group into blocks of 8 pairs per line for readability
    return stride(from: 0, to: pairs.count, by: 8)
        .map { pairs[$0..<min($0 + 8, pairs.count)].joined(separator: ":") }
        .joined(separator: "\n")
}
