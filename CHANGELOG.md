# Changelog

All notable changes to Link All are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning follows [Semantic Versioning](https://semver.org/).

---

## [Unreleased]

### Added
- **Now Playing.** What one device plays shows on the others with its title, artist, cover and controls. A computer's track (Spotify or Music on a Mac, any MPRIS player on Linux, any app using the system media controls on Windows) appears on the phone's lock screen and notification shade as a real player with a scrub bar; its buttons and the scrub bar control the computer (Remote control must be on at the computer). The phone's track (it needs notification access) appears in the Mac's menu-bar panel and at the top of Devices on Windows and Linux, with play/pause, skip and a scrub bar. `linkall-cli nowplaying` and `linkall-cli media <device-id> play_pause|next|previous|seek <s>` do the same.
- **Guest Drop.** Take a file from someone who isn't paired. A QR code, a link and a PIN (Mac: Window › Receive from Guest, ⇧⌘G; Windows and Linux: the ⋯ menu on Devices › Receive from a guest; or `linkall-cli guest`). The guest opens the page in any browser on the same network, types the PIN and sends files straight to Downloads › Link All › Guest at full network speed. It works for five minutes, five wrong PINs end it, nothing is paired or remembered, and the page is not encrypted, so use it on a network you trust. Windows shows the link and PIN without a QR code.
- **Insert from Phone.** Ask your phone for a photo or a scanned document and it comes straight back, onto the clipboard and pasted into the app you were using. Mac: Window › Insert Photo from Phone (⇧⌘I) or Scan Document with Phone (⇧⌘D); the Mac asks for Accessibility the first time to press ⌘V. Windows: a phone's ⋯ menu on Devices › Take photo / Scan document with phone. Linux: a phone's actions on Devices. The phone shows a notification to tap; a scanned document goes through Google's document scanner, which finds the page's edges, straightens it and cleans it (a phone without Google Play services uses its camera and a simple clean-up). `linkall-cli capture <device-id> [document]` does the same.
- **Notes.** Notes, links and snippets shared between your own devices, kept on each device and sent only to paired ones. On every app: the notes button at the top of Home opens it (also Window › Notes on a Mac). Type in the box at the bottom like a chat with yourself, or tap "Save what you copied". Swipe a note right to pin it and left to delete it (with Undo); tap it for Copy, Edit, Pin, Share and Delete. Links show their site and open on a tap, commands and code show as code, and the filters (All, Pinned, Links, Code) and search sort it. The editor has buttons for headings, bold, code and lists, and "Save to Notes" is in the share sheet. On a Mac the note icon in the menu-bar panel opens it too. Elsewhere: `linkall-cli scratch`. The newest edit wins; a device that was off catches up when it connects.
- **Android:** Verification codes. Turn on Settings › Continuity › Verification codes (it needs notification access) and the 4 to 8 digit code in a text from a bank, Google or GitHub pops up on your Mac, Windows PC or Linux computer with **Copy Code** and **Paste** buttons. Paste also presses Command/Ctrl-V in the app you're in (on a Mac it asks for Accessibility the first time). Only a message that says what the number is for counts, and an amount or a year in the same message is not picked.
- **Click to call.** Offer a number to your phone from a computer and it shows a notification; tap it and the dialer opens with the number filled in. Nothing is dialled on its own. On a Mac: Window › Call from Phone… (⇧⌘C), and `tel:` links once you choose Link All for them. On Windows: the ⋯ menu on Devices › Call from phone…, and `tel:` links once you choose Link All in Default apps. On Linux: the ⋯ menu on Devices › Call from Phone…, and `tel:` links. `linkall-cli call <device-id> <number>` does the same.
- **Walk-away lock.** On Mac, Windows and Linux, turn on "Lock when my phone leaves" and the computer locks when the paired phone that was connected has been gone for 30 seconds. A phone that sleeps or loses Wi-Fi counts as gone. When it comes back the display turns on so you can sign in (nothing is unlocked for you). Off by default.
- **Mouse and keyboard.** The phone's Remote sheet opens a full mouse for a paired computer: one finger moves the pointer and a tap clicks; tap then touch again and drag to drag, or switch on Hold left button; two fingers scroll with momentum (up, down and sideways), pinch zooms and a two-finger tap right-clicks; three fingers swipe to the overview, the desktop and between desktops, and tap to middle-click. A strip at the edge scrolls with one finger. Buttons for left, middle, right, back and forward, Shift/Ctrl/Alt/Win-or-Cmd that stay held, 23 shortcuts (copy, paste, undo, find, new tab, switch app …), Esc, Tab, arrows, Home, End, Page Up/Down, Delete, F1–F12, a text field, pointer speed, natural scrolling, and an Air mouse that points with the phone's gyroscope. Needs Remote control on at the computer.
- **Lock, screen off and ring a computer.** The Remote sheet can lock a computer, turn its screen off, and make it ring so you can find it. A computer rings only while Remote control is on.
- **Link All Drop.** Turn it on and any file put in the Link All Drop folder in your home folder is sent to your paired devices on its own. Off by default; on Mac, Windows and Linux.
- **Android:** Folder sync. Pick a folder on the phone (such as Camera) and a paired computer, and new files in it go to the computer by themselves whenever it is connected, into Downloads/Link All on the computer. It starts with what is new from now on, or sends what is already there if you choose. The phone remembers what each computer received, so nothing arrives twice, and a file that didn't arrive is sent again next time. Pause or remove a folder in Settings › Folder sync.
- **macOS, Windows, Linux, Android (direct download):** Link All tells you when a newer version exists. It asks GitHub's public release list once a day and shows a banner with a link to the release page (or the `brew upgrade --cask link-all` command for a Homebrew install). It never installs anything and sends nothing about you. Turn it off in Settings › Check for updates. The Google Play and Microsoft Store builds don't check, because the stores update them.
- **All:** Report a problem, in Diagnostics on macOS and Android and in Settings on Windows and Linux. It builds a report you can copy or save: the app and OS version, device states and the last log lines, with no clipboard text, file names, device names or full addresses (addresses keep only their last number, device IDs six characters). Nothing is uploaded. `linkall-cli report` prints the same text.
- **Android, macOS, Windows, Linux:** Phone Remote. Control a computer from the phone: play/pause, next and previous track, volume, mute and slide forward/back. It is off by default; turn it on per computer in Settings › Remote Control. The computer allows only those eight actions and nothing else.
- **Android, macOS, Windows, Linux:** Find my phone. Make a paired phone ring from your computer, at full volume even on silent, with a Stop button on the computer and "Found it" on the phone. It rings for up to a minute. It is off by default: turn it on in the phone's Settings › Continuity › Find my phone. If it is off, the computer says so. On the Mac it is in the menu-bar panel (Find Phone); on Windows and Linux it is in the phone's device menu; `linkall-cli find <uuid>` does the same.
- **All:** One device chooser everywhere. When several devices are connected, send, remote, browse and folder sync ask which one, ranked by the ones that can do the job, instead of picking one for you.
- **Android:** Settings › Setup lists everything that needs attention (notification access for calls, battery limits, notification permission, connection problems) with a fix button on each, instead of one banner at a time.
- **Android:** Actions now say what they did, including when they did nothing: "Sent to MacBook Pro", "Nothing to send: the clipboard is empty", "No device is connected".

### Changed
- **All:** A new layout built around your devices, the same on Android, Mac, Windows and Linux. Home leads with each connected device: its status, what it is playing, and the two or three things you do most with it. Click or tap one for its own page with everything for that device: send, browse its files, control it, play its music, make it ring, its details, and connecting or forgetting it. Transfers is now **Files**, with sending, a Browse list of your devices, what is moving and what arrived. Settings is a short list of categories.
- **Android:** The floating dock is Home, Files, Activity and Settings. Activity has a search box, and a received file opens from Activity or Files with a tap.
- **All:** Smoother and lighter: the apps ask the background service for less and less often when nothing on screen needs it (Now Playing covers are fetched once per track instead of with every update), and the phone no longer rewrites its status after every synced clip.
- **Windows:** Smaller install: 74 MB instead of 83 MB, and a 23 MB installer. `LinkAll.exe` is compiled for size (15.1 to 12.3 MB), and the Windows App SDK pieces Link All never loads (the Widgets API, WebView2 and the rest of the Windows AI runtime) are left out of both the installer and the Store package.

### Fixed
- **Android:** Sharing text with no device connected says so, instead of "Sent to your devices".
- **Android:** The activity menu no longer offers a Resend that did nothing.
- **Windows:** Forgetting a device asks first.
- **macOS:** The menu-bar icon no longer turns black in dark mode, and the red dot shows on it again.
- **macOS:** Dropping a file on the menu-bar icon with several devices asks which one instead of sending to the wrong one.
- **macOS, Windows:** Answer and Decline on the call banner follow the phone, and the banner says "Unknown caller" instead of showing the phone's name.
- **Windows:** A send with no window to ask in no longer goes to an arbitrary device; it asks you to open Link All and choose.
- **Linux:** The tray's clipboard send says it went to all devices.

## [2.0.2] - 2026-10-20

Phones can reach Windows PCs again, pairing works after a reinstall, and the Windows Store app is quieter, smaller and fully working.

### Changed
- **Windows (Microsoft Store):** The download is 30.5 MB instead of 49.9 MB. The package no longer carries the Windows App SDK's AI runtime, which Link All never used, or its string tables for languages other than English.
- **Windows:** A new call banner: a round avatar, "INCOMING CALL" (a running timer once you answer), the caller, the number and which phone is ringing, and round Decline and Answer buttons. The old one had window buttons drawn over it and cut off text.
- **Android:** Home lists your devices once, under Your devices, with the link state under the app's name. A device that isn't paired has a Pair button on its row.
- **macOS:** The audio menu during a call offers the earpiece and the speaker. Android lets only the phone's dialer choose a Bluetooth device for a call.

### Fixed
- **Windows:** Phones and other devices can connect to the PC again. Link All listened in a way that Windows limits to IPv6, so every connection to the PC's usual 192.168.x.x address was refused, and the link only came up when the PC dialled the phone. Pairing from the phone, and the phone reconnecting by itself, failed on every Windows PC.
- **Android:** What you copy on the phone reaches your computers by itself again. Since 1.4 the phone never watched its clipboard, so a copy only went across when sent by hand.
- **All:** A phone that reinstalled Link All can pair again and stays connected. Reinstalling gives the phone a new identity at the same address. A computer still reconnecting to the old one found the new phone there, closed the connection, and in doing so also ended the link the phone already had with it, every reconnect cycle. Now the computer keeps the connection as the device that answered and stops dialling any of the old one's addresses, so the pairing code no longer changes partway through a request.
- **All:** Pause stops a transfer within a few MB instead of letting about 64 MB more arrive.
- **All:** Unfinished downloads left behind when an app was closed mid-transfer are cleared from Downloads.
- **Windows:** The logo in the sidebar and in notifications follows dark mode.
- **Android:** Switching a call to the speaker or back to the earpiece from a computer works. The phone used to change audio settings that Android ignores for phone calls; it now presses the Speaker button on the call's notification.
- **Android:** If Link All's engine fails partway through starting, it is shut down instead of left running alongside the next attempt.
- **All:** A nearby device that hasn't been paired yet can still connect, and ask to pair, after it briefly drops off the network. Missing one discovery announcement used to mark it as disconnected by the user, which refused every automatic connection from it.
- **Windows (Microsoft Store):** Accept and Reject on a pairing notification now work. The Store package didn't register the notification activator, or include the runtime file notifications need, so the buttons did nothing.
- **Windows (Microsoft Store):** Windows Firewall rules for Link All are added at install. Without them, on a network marked Public (as many home Wi-Fi networks are), a dismissed firewall prompt blocked every phone from reaching the PC.
- **Windows:** No more technical notifications such as "network changed from [::]:47823 …", "connection to multiple endpoints failed after retries …" or "A device reported an issue". Only a folder that stopped sending is shown; the Devices page shows each device's state.
- **CLI:** `peers` shows each device's address and how many are connected, instead of "?:47823" and counting every known device as connected.

## [2.0.1] - 2026-10-19

A reliability release: devices stay linked, find each other again by themselves after any drop, and nothing you copy is lost while one is away.

### Added
- **Android (Google Play build):** Settings › Shared folders. Pick folders with the system folder picker and your computers can browse, preview, download, rename and delete files in them.
- **Linux:** A full Link All app, built with GTK 4 and libadwaita in Link All's own colours, light and dark. It has the same pages as the Mac and Windows apps: Devices (pair by code or QR, battery and storage, per-device actions), Clipboard history with search, pin and "Paste Here", Transfers with pause, resume, cancel and accept or decline, Browse Files with previews, download, rename and delete, Activity, and Settings. Search everything with Ctrl+K.
- **Linux:** A tray icon, desktop notifications for phone notifications, calls, received files and pairing requests, "Send with Link All" in Files, Dolphin and Nemo, start at login, and sending new screenshots automatically.
- **Linux:** `.deb` packages and a tarball with a no-root installer, for x86_64 and ARM64, plus a systemd user service for the background service.
- **Android:** When your phone pauses Link All in the background to save battery, Link All now says so: a guide with your phone's exact steps (it opens once after your first pairing), and a notification if devices dropped while the screen was off.
- **macOS, Windows, Linux:** A phone that pauses Link All in the background shows as "Paused by the phone", with what to do, instead of a plain "Offline".
- **macOS:** A banner when macOS blocks Link All from your local network, with a button to the setting.

### Fixed
- **All:** Paired devices now reconnect to each other by themselves after any drop. Pairing marked a device trusted but not one to reconnect to, so a computer waited for the phone (or for discovery) every time.
- **All:** A computer now reconnects to a phone that had connected to it; it used to save the phone's outgoing port and redial a dead address.
- **All:** Clipboard copied while a device was away (asleep, frozen or off Wi-Fi) now reaches it when it returns. If both devices copied while apart, the newer copy wins.
- **All:** A device that goes silent without closing the connection (it left Wi-Fi, or its OS froze it) is noticed within seconds of a send, and within about a minute otherwise, instead of looking connected for hours.
- **Android:** A phone with its screen off pings once a minute instead of every 5 seconds, which saves battery.
- **Android:** Link All stays linked with the screen off when allowed to run in the background, starts reliably at boot, and its background service always brings its engine back. Stopping Link All now keeps it stopped when notifications or calls arrive.
- **Android:** A second phone with the same name no longer replaces the first phone's pairing.
- **Android:** Renaming a phone file from a computer now works; the new name was never passed on.
- **Windows:** Browsing a device that refuses (such as the Google Play build) or doesn't answer now says why, instead of showing an empty folder that stayed empty after the cause was fixed.
- **Linux:** On systems without an XDG user-dirs config, received files go to ~/Downloads instead of the home folder, and paired devices can browse ~/Documents, ~/Pictures and the rest.
- **Windows:** An open Clipboard page now shows new items from your phone straight away.
- **Windows (Microsoft Store):** Data stays in the app's own storage, as the Store requires. "Send with Link All" in File Explorer's menu is only in the installer version.
- **macOS:** With several transfers going, the transfers pill says "To" when the Mac is sending, not "From".
- **Linux:** Network changes are still noticed after the computer wakes from suspend.

## [2.0.0] - 2026-10-11

Link All is continuity for every device you own, whatever it runs. Your Windows PC, Mac, Android phone and Linux machine share one clipboard, pass files and folders, show the phone's notifications and calls on the computer, and browse each other's files, straight over your own network with no cloud and no account.

It also includes everything listed under 1.4.0.

### Added
- **Android:** Browse, search and preview files on your Mac or Windows PC, and fetch any of them to the phone. Pick a device from Home › Get › Browse files or from a device's sheet.
- **macOS, Windows:** A "Let devices browse files" switch in General settings decides whether paired devices can see this computer's Downloads, Documents, Pictures, Videos and Music. It is off until you turn it on.
- **Android:** Home opens with a map of this phone and the devices it is linked to, solid for connected and dashed for offline. With more than four devices it becomes a row of chips.
- **Android:** Connected computers appear in the share sheet. The Link All notification can be swiped away, and you can pick a light or dark theme.
- **CLI:** `pause`, `resume` and `send-folder` commands.
- **Windows:** A Microsoft Store (MSIX) package. In it, "Start with Windows" uses the package's startup task, and pairings stay in the same data folder as the MSI install.
- **macOS:** The disk image opens with the usual install layout: drag Link All onto the Applications shortcut, or open it straight from the disk image to have it install itself.

### Changed
- **All platforms:** Activity reads as plain sentences ("Sent Launch-film-4K.mov to MacBook Pro · 640 MB") with sizes in KB, MB or GB, instead of bracketed device names and raw kilobytes.
- **Windows:** The Activity page shows the activity log (transfers, clips and connections) rather than repeating clipboard history. Clipboard shows each clip once with file sizes, Quick Access gives previews the whole row and fits three devices, and page content stays centred on wide windows.
- **macOS:** Browsing a computer shows its own folders (Downloads, Pictures, Other Folders) instead of phone places like WhatsApp and Camera Roll.
- **Android:** A Google Play build alongside the full build. It leaves out SMS code forwarding, answering calls and letting a computer browse the phone, which Play reserves for default SMS, dialer and file-manager apps.

### Removed
- **All platforms:** The phone camera stream to the computer (Continuity Camera), including the macOS virtual camera extension. Android no longer asks for camera access.
- **macOS:** The drop target on the screen edge. Drop files on the Link All menu bar icon instead.

### Fixed
- **Android (Google Play build):** Incoming calls show the caller's name, read from the phone app's call notification, instead of "Unknown Caller". A ring waits up to a second for the name so the computer shows it from the start. Computers say which phone is ringing when no name is available.
- **macOS:** The transfer pill no longer blocks clicks below it or takes focus from the app in use.
- **Android:** "Show my pairing code" follows dark mode.
- **macOS:** The event log says "Sent" for files this Mac sent, not "Received".
- **All platforms:** Copied links sync again when "Block sensitive text" is on. Its token check treated most links as secrets; now only a password in the address or a token-like parameter blocks one.
- **All platforms:** A phone with its screen off no longer drops off the computer after 30 seconds of quiet.
- **Security:** Pairing by QR code checks the computer's key; the Mac stops trusting devices while its QR code is open; unpaired devices can no longer send calls, warnings, sync-state or speed-test messages; a transfer can only be controlled by its own peer; Windows IPC pipes are owner-only.
- **Transfers:** Auto-accept for trusted devices is honoured, a paused transfer is not sent before the receiver accepts, and NTFS alternate data streams are kept out of received file names.
- **Android:** No crash after receiving a file or on pairing requests on Android 16. A received file is kept when saving to Downloads fails.
- **Windows:** Text from the daemon is decoded as UTF-8, so "·" and "…" no longer show as garbled characters, and each phone notification shows once.

## [1.4.0] - 2026-10-02
### Added
- **All platforms:** Send whole folders. A folder keeps its tree on the other device, a second copy lands in "Name (2)", and each side shows one progress row and one notification per folder. Accepting, declining or cancelling one file answers for the whole folder.
- **All platforms:** A health banner says what stops sync right now (no network, listener down, sync paused, devices not found, connection blocked) and the one thing to do about it.
- **All platforms:** Each device shows its operating system's icon.
- **Windows:** Clipboard images sync both ways, including Win+Shift+S screenshots. Images in history copy again when clicked.
- **Windows:** Phone notifications and incoming calls show as Windows notifications.
- **Windows:** The welcome guide opens on first run until a device is paired, and Settings can show it again. The installer adds a desktop shortcut.
- **Android:** Pin activity items to the top; pinned items are never trimmed or cleared and survive restarts.
- **Home screens:** Each app's home leads with this device's identity ("Visible as"), then Send (Files & folders, Clipboard, and Camera or Browse device), Transferring, Your devices and Recent.
- **Modals:** Android, macOS and Windows each use one custom modal style for sending, pairing, device actions and questions. Sending defaults to all connected devices.

### Changed
- **Older installs:** A device reinstalled or renamed no longer shows up as several paired devices; old copies of it are retired automatically.
- **Pairing:** A device on an older version is told it needs an update instead of failing silently.
- **Windows:** The system tray helper is now a native Rust exe (`platforms/windows/tray`, ~0.3 MB) instead of a WinForms app with its own self-contained .NET runtime (~110 MB). Same process name, pipes and menu.
- **Windows:** Only English language resources ship; WinUI's built-in control text shows in English on other display languages.
- **Windows:** The app targets .NET 10 and publishes with Native AOT: `LinkAll.exe` is native code and no .NET runtime ships with it. Daemon requests are built as `JsonObject`s and all JSON reading goes through a source-generated `LinkAllJsonContext`, so nothing relies on reflection. The unused `System.Drawing.Common` package is removed.
- **Windows:** The Rust engine DLL and tray helper link the C runtime statically, so they no longer need `vcruntime140.dll` (missing on some clean installs).
- **Build:** Windows releases ship as a per-user MSI and a portable zip of the whole app folder instead of a single-file exe. The MSI now installs every published file; before it listed only the app exe and its core DLL.

- **Android (battery):** No Wi-Fi lock is held while idle; only a transfer that is moving bytes takes one. The clipboard is polled at full rate only when it can be read (app in the foreground, an enabled accessibility service, or Android 9 and below), otherwise every 2 s instead of 2–5 times a second. Notification mirroring skips group summaries, media/progress/navigation updates and re-posts with unchanged text. Battery level is reported in 5% steps while the screen is off.
- **Core (battery):** LAN discovery sweeps (~250 TCP connects each) pause while the device sleeps, and the steady UDP discovery beacon slows from every 15 s to every 60 s.

### Fixed
- **Core:** Sending many files at once no longer loses about a third of them (a race accepted the same transfer twice).
- **Core:** Pausing a large transfer near the end and resuming it no longer fails, and resuming no longer rehashes the whole file.
- **macOS:** The transfer pill no longer stays on screen after a transfer finishes, and it counts a folder's files instead of the few in flight.
- **Android:** Folder progress keeps in step with the other device instead of sitting still during big files.
- **Android (Play build):** No longer asks for permissions Play restricts (all-files media, contacts, full-screen intents, battery exemption, background clipboard, accessibility); data shared with calls and notification mirroring is explained before it is turned on.
- **Android:** The benchmark module failed to configure (`missingDimensionStrategy` outside `defaultConfig`), which broke every Android build after the `full`/`play` flavors were added.
- **Windows:** The tray menu's "Rescan Network" now rescans. It used to post to a local HTTP port nothing listened on.
- **Build:** A plain `dotnet build` no longer writes a stale 113 MB copy of the tray helper into `publish\`.
- **Core:** Transfers interrupted by a reconnect now resume reliably; about half of them used to end with "missing chunks". Three races are closed: the old connection's teardown no longer pauses a transfer the new connection already resumed, a stale send loop can no longer skip chunks after a resume, and chunks still arriving on the old connection are dropped instead of being queued on a disk writer that is about to be torn down (which left the resumed copies skipped as duplicates).
- **Windows:** The taskbar progress bar uses source-generated COM interop; the old `[ComImport]` code is not supported under Native AOT and would have thrown on the first transfer. Value converters are `partial` for the same reason.
- **Windows:** The MSI now builds with WiX 5 (it had an invalid XML comment, an unsupported `Condition` attribute, and a `<Files>` path that produced an empty installer), and installing a rebuilt MSI of the same version replaces the installed app instead of registering a second Link All entry. The native `LinkAll.pdb` (~52 MB) no longer ships.

## [1.3.3] - 2026-09-05
### Fixed
- **Android:** Fix release build workflow by targeting `:app:assembleRelease` directly to avoid building the broken benchmark module.

## [1.3.2] - 2026-09-05
### Fixed
- **Android:** Added `-dontwarn` rules for the `benchmark` module's R8 pass (`androidx.profileinstaller.ProfileInstallReceiver`, `androidx.startup.Initializer`, `com.google.errorprone.annotations.MustBeClosed`) — optional classes referenced by `androidx.benchmark:benchmark-macro-junit4` that aren't on the classpath and aren't needed at runtime, which made `minifyReleaseWithR8` fail once shrinking was enabled there in 1.3.1.

## [1.3.1] - 2026-09-05
### Fixed
- **Android:** Release builds are now signed with the release keystore instead of the debug key; the build also fails fast if the keystore is configured but any of the store password, key alias, or key password is missing.
- **Android:** Enabled R8 shrinking in the `benchmark` module's release build type so it matches `app:release`, fixing a `checkTestedAppObfuscationRelease` failure in the release build.

## [1.3.0] - 2026-09-04
### Added
- **Core:** Known devices now display their trust fingerprint and first-seen date.

### Fixed
- **Core:** Closed a race in `PeerManager::replace_live_session` where two concurrent sessions for the same peer could each read the same stale state, both pass the tie-break check, and both insert — letting whichever insert landed last win the live slot regardless of which one the tie-break rule selected.
- **Core:** Closed a race where a stale failure report from a losing multi-address connect attempt could mark a peer `Failed` immediately after it had successfully connected.
- **Core:** Reconnect logging no longer floods during transient network interface flaps.

### Changed
- **Windows:** Diagnostic tracing consolidated into a single `TraceLog` writer instead of ad-hoc `File.AppendAllText` calls scattered across the UI layer; daemon calls that previously ran fire-and-forget now report failures through `DaemonActions`.
- **Build:** Every shippable artifact now carries a single shared version. The Rust crates inherit it from `[workspace.package]`, and `scripts/bump-version.sh --check` gates CI so the components cannot drift apart again.

### Removed
- Stale build artifacts (prebuilt `.zip`/`.dll`/`.exe`/`.pdb`) are no longer tracked in the repository, along with one-shot migration scripts and dated internal audit documents.

## [1.2.8] - 2026-08-24
### Fixed
- **Core:** Fixed a bug where untrusted discovery beacons (mDNS/UDP) were incorrectly saved as "remembered" peers, causing phantom devices to appear in the Known Devices list.

## [1.2.7] - 2026-08-24
### Fixed
- **Core:** Fix intermittent e2e test failure (test_tier4_scenario_device_reconnect_retry).

## [1.2.6] - 2026-08-24
### Fixed
- **Core:** Fix formatting errors that broke CI pipeline (rustfmt).

## [1.2.5] - 2026-08-24
### Added
- **Core:** Added camera stream request API to FFI.
- **Android:** Redesigned Ecosystem header with a sleek segmented control.
- **Android:** Upgraded "JUST COPIED" card styling and contrast.
- **Android:** Disabled mesh gradient in pure dark mode for a true black background.
- **Windows:** Comprehensive UI overhaul for the WinUI dashboard and dialogs.

## Pre-1.0 history

The entries below predate the 1.0 release and were never assigned version
headings. They accumulated under an `[Unreleased]` marker that went stale as
releases shipped around it. Kept for reference; `git log` is authoritative for
which release first contained a given change.

### Added (v4)
- **Android:** Added "Finish Onboarding" re-entry flow to empty dashboard.
- **Android:** Added OEM-specific battery restriction diagnostics (Xiaomi, Samsung) to help prevent background service termination.
- **macOS:** Implemented `ProcessInfo.beginActivity` to prevent App Nap from throttling the background daemon.
- **Windows:** Implemented `SetThreadExecutionState` to prevent Modern Standby sleep during active file transfers.
- **Core:** Received files are now saved directly to the root `Downloads` directory across all platforms.

### Added (v3)
- `ClipboardContent::is_empty()` — single guard method, eliminating per-call-site duplication (#16)
- `CompressionStats: Display` — consistent log format, usable in IPC responses (#12)
- `QualityProbe::degraded_from()` — detect link quality regressions between probe cycles with `quality_severity` ordering (#13)
- `PeerManager::sync_eligible_count()` — O(1) count of connected+trusted+sync-enabled peers (#14)
- TCP connect timeout (5 s) via `tokio::time::timeout` on all outbound connects (#1)
- TCP keepalive (`SO_KEEPALIVE`, idle 30 s / interval 5 s / 3 probes) on both accept and connect paths (#9)
- Unit tests: `network.rs` (framing, nonce XOR, nonce-echo verification, handshake integration), `discovery.rs` (address preference, version validation), `peer_manager.rs` (`connected_count`, `sync_eligible_count`), `probe.rs` (`degraded_from`, quality ordering), `compress.rs` (Display), `chunked.rs` (size cap rejection), `identity.rs` (fingerprint format), `protocol.rs` (is_empty)

### Fixed (v3)
- **Critical** — Handshake nonce verification was a stub; replay protection now enforced on the initiator side (#2)
- **Critical** — `set_nodelay(true)` was absent on the outbound (initiator) TCP path, adding ~40 ms Nagle delay (#3)
- **Critical** — `pairing.rs` `expire_stale()` silently dropped oneshot channels; now sends explicit `false` so waiters receive `Ok(false)` instead of a channel error (#5)
- **Critical** — `fuzz_sanity_test.rs` used `ExtensionFilter` before it existed; `bincode` added to `[dev-dependencies]` so integration tests compile without a separate crate override (#4)
- **Security** — mDNS version TXT record `v` was advertised but never validated on the browsing side; incompatible peers are now skipped at mDNS time (#6)
- **Security** — mDNS address selection used `HashSet::iter().next()` (arbitrary order); now prefers IPv4 → IPv6 global → IPv6 link-local to avoid silent connect failures from fe80:: addresses without a scope_id (#7)
- **Security** — `Reassembler::feed(Start)` had no total-payload size cap; malicious peers could announce `u64::MAX` bytes and tie up in-flight state indefinitely; capped at 512 MB / 8 192 chunks (#8)
- **Correctness** — `fingerprint_display()` doc comment showed wrong format (16 pairs of 2 chars) vs actual output (8 groups of 4 chars); corrected and test added (#10)
- **Correctness** — `crypto_vectors_test.rs` session key vector was a non-asserting stub; real HKDF-SHA256 vector computed and asserted (#11)
- **Correctness** — CLI unknown command printed bare `"Unknown command: foo"` with no quoting and wrong help hint; now prints `"Unknown command: 'foo'\n\nRun \`linkall-cli help\` to see all available commands."` (#15)

- Rust core engine (`linkall-core`) with:
  - X25519 ephemeral ECDH key exchange
  - HKDF-SHA256 session key derivation
  - ChaCha20-Poly1305 AEAD encryption with monotonic nonce counter
  - Replay attack protection
  - mDNS-SD service discovery
  - Framed TCP transport with sub-500 ms LAN propagation
  - TOFU (Trust On First Use) device trust model
  - PIN-based pairing as alternative to TOFU
  - Chunked transfer for payloads > 128 KB (streaming, pipeline-friendly)
  - Echo suppression (deduplication) with per-peer rate limiting
  - Clipboard history ring buffer (100 entries, persisted as NDJSON)
  - Per-peer metrics: latency p50/p95, throughput, session duration
  - Bandwidth throttle (token bucket, default 4 MB/s for large payloads)
  - Content filter chain: size limits, type allow-list, extension block-list,
    optional sensitive-text heuristics
  - Connection retry with exponential back-off and ±25 % jitter
  - Settings system with atomic JSON persistence and hot-patch API
  - Unix domain socket IPC server (CLI ↔ daemon)
  - C FFI exports for macOS / Windows platform wrappers
  - JNI bridge for Android

- **macOS** platform (`platforms/macos/`):
  - Menu-bar–only app (LSUIElement)
  - NSPasteboard watcher at 100 ms poll
  - TOFU sheet with fingerprint display
  - SwiftUI clipboard history popover (last 100 entries, searchable)
  - SwiftUI preferences panel with live settings editing
  - Hardened runtime + App Sandbox entitlements
  - Universal dylib (arm64 + x86_64) build script
  - Code-signing and optional DMG packaging

- **Windows** platform (`platforms/windows/`):
  - System-tray WinForms application
  - Win32 clipboard sequence-number watcher
  - P/Invoke bridge to Rust DLL
  - TOFU MessageBox with fingerprint
  - Clipboard history floating panel (searchable, re-push)
  - Preferences dialog with Registry persistence
  - Auto-start via `HKCU\...\Run`
  - Named-pipe IPC client (`DaemonClient`)
  - Daemon status poller (`DaemonPoller`)
  - WiX v4 MSI installer with firewall exception

- **Android** platform (`platforms/android/`):
  - Foreground service with `foregroundServiceType=dataSync`
  - JNI calls into liblinkall_core.so
  - Clipboard monitoring via `ClipboardManager`
  - PIN-based pairing full-screen activity
  - Settings activity
  - Auto-start `BootReceiver`
  - Notification channel (low-importance, persistent)

- **Linux** platform (`platforms/linux/`):
  - Headless daemon mode (arboard for X11 + Wayland)
  - `notify-send` desktop notifications
  - `.desktop` file for XDG autostart
  - systemd user service unit with security hardening

- **CLI** (`linkall-cli`):
  - `status`, `ping`, `push`, `peers`
  - `devices list`, `devices revoke`
  - `history`, `history --last N`, `history --search`, `history clear`
  - `settings get/set/reset`
  - `sync on/off`, `stop`
  - Live IPC when daemon running; offline file-read fallback

- **CI / CD** (`.github/workflows/`):
  - `ci.yml`: Rust fmt/clippy/test on Linux, macOS, Windows;
    Android cross-compile for arm64/armv7/x86_64;
    macOS Swift type-check; Windows dotnet build;
    `cargo audit` security scan; SBOM generation
  - `release.yml`: Produces universal macOS dylib, Windows zip + MSI,
    Android APK, Linux tarball; creates GitHub Release with all artifacts

- **Tests**:
  - Unit tests in every module (crypto, trust, dedup, chunked, settings,
    history, metrics, filter, retry, throttle, pairing, sim)
  - In-process `SimNetwork` harness for deterministic two-node tests
  - Criterion benchmarks: X25519 handshake, ChaCha20 encrypt/decrypt
    (1 KB – 4 MB), chunk reassembly, content hashing, dedup
  - Integration test: two real engine instances exchanging clipboard text

---

## Links

- [Security Policy](SECURITY.md)
- [Contributing](CONTRIBUTING.md)
- [GitHub](https://github.com/ChinmayyK/Link-All)
