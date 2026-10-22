import Cocoa
import CoreGraphics

// Shares this Mac's keyboard and mouse with the computer next to it.
//
// The pointer is watched until it is pushed against a screen edge that has a
// computer linked to it. Then the daemon is asked to hand the keyboard and
// mouse over; until the pointer comes back, or Ctrl+Option+Esc is pressed,
// this Mac's mouse and keyboard are swallowed by an event tap and sent to that
// computer in small batches.
//
// Needs the Accessibility permission (an event tap that can swallow input
// does), which is asked for the first time sharing is turned on.

// ── IPC ───────────────────────────────────────────────────────────────────────

extension LinkAllIPCClient {
    /// Asks `target` to take this Mac's keyboard and mouse. `edge` is 0 left,
    /// 1 right, 2 top, 3 bottom.
    func edgeBegin(target: String, edge: Int, pos: Int) async throws {
        let raw = try await send(cmd: ["cmd": "edge_begin", "target": target, "edge": edge, "pos": pos])
        try Self.throwIfError(raw)
    }

    func edgeInput(_ batch: EdgeBatch) async throws {
        let raw = try await send(cmd: [
            "cmd": "edge_input",
            "dx": batch.dx, "dy": batch.dy,
            "scroll_x": batch.scrollX, "scroll_y": batch.scrollY,
            "buttons": batch.buttons.map { [$0.0, $0.1] as [Any] },
            "keys": batch.keys.map { [$0.0, $0.1] as [Any] },
        ])
        try Self.throwIfError(raw)
    }

    func edgeEnd() async throws {
        _ = try await send(cmd: ["cmd": "edge_end"])
    }

    /// What happened to the hand-off since the last ask: the pointer coming
    /// back (and where along the edge), or why it ended.
    func edgeStatus() async throws -> (returnedPos: Int?, endedReason: String?) {
        let raw = try await send(cmd: ["cmd": "edge_status"])
        guard let obj = try JSONSerialization.jsonObject(with: raw) as? [String: Any],
              let data = obj["data"] as? [String: Any] else { return (nil, nil) }
        return (data["returned_pos"] as? Int, data["ended_reason"] as? String)
    }

    private static func throwIfError(_ raw: Data) throws {
        guard let obj = try JSONSerialization.jsonObject(with: raw) as? [String: Any],
              (obj["status"] as? String) == "error" else { return }
        throw LinkAllIPCError.serverError((obj["message"] as? String) ?? "Something went wrong")
    }
}

/// Input gathered since the last send, in the order it happened.
struct EdgeBatch {
    var dx = 0, dy = 0, scrollX = 0, scrollY = 0
    var buttons: [(Int, Bool)] = []
    var keys: [(Int, Bool)] = []
    var isEmpty: Bool { dx == 0 && dy == 0 && scrollX == 0 && scrollY == 0 && buttons.isEmpty && keys.isEmpty }
}

// ── The service ───────────────────────────────────────────────────────────────

final class EdgeControlService {
    static let shared = EdgeControlService()

    static let enabledKey = "edgeControlEnabled"
    static func linkKey(_ edge: Int) -> String { "edgeLink\(edge)" }
    static let edgeNames = ["left", "right", "above", "below"]

    private enum Mode { case idle, starting, remote }

    private var mode = Mode.idle
    private var tap: CFMachPort?
    private var runLoopSource: CFRunLoopSource?
    private var timer: Timer?

    private var hits = [0, 0, 0, 0]
    private var since = [Date](repeating: .distantPast, count: 4)
    private var cooldownUntil = Date.distantPast
    private var lastEdge = 1

    private var batch = EdgeBatch()
    private var sending = false
    private var lastSend = Date.distantPast
    private var lastStatus = Date.distantPast
    private var failures = 0

    /// Why the last hand-off ended, for the app to show once.
    private(set) var lastNote: String?
    func takeNote() -> String? { defer { lastNote = nil }; return lastNote }

    private var enabled: Bool { UserDefaults.standard.bool(forKey: Self.enabledKey) }
    private func link(_ edge: Int) -> String? {
        let id = UserDefaults.standard.string(forKey: Self.linkKey(edge)) ?? ""
        return id.isEmpty ? nil : id
    }

    /// Starts or stops watching to match the settings. Call at launch and
    /// whenever one of them changes.
    func reload() {
        let wanted = enabled && (0..<4).contains { link($0) != nil }
        if wanted { installTap() } else { endRemote(tellDaemon: true, note: nil); removeTap() }
    }

    // ── Event tap ────────────────────────────────────────────────────────────

    private func installTap() {
        guard tap == nil else { return }
        // An event tap that can swallow input needs Accessibility access.
        let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
        guard AXIsProcessTrustedWithOptions(options) else {
            lastNote = "Allow Link All in System Settings > Privacy & Security > Accessibility to share the keyboard and mouse."
            return
        }
        let types: [CGEventType] = [
            .mouseMoved, .leftMouseDragged, .rightMouseDragged, .otherMouseDragged,
            .leftMouseDown, .leftMouseUp, .rightMouseDown, .rightMouseUp, .otherMouseDown, .otherMouseUp,
            .scrollWheel, .keyDown, .keyUp, .flagsChanged,
        ]
        let mask = types.reduce(CGEventMask(0)) { $0 | (1 << $1.rawValue) }
        let callback: CGEventTapCallBack = { _, type, event, refcon in
            guard let refcon else { return Unmanaged.passUnretained(event) }
            let me = Unmanaged<EdgeControlService>.fromOpaque(refcon).takeUnretainedValue()
            return me.handle(type: type, event: event) ? nil : Unmanaged.passUnretained(event)
        }
        guard let tap = CGEvent.tapCreate(
            tap: .cgSessionEventTap, place: .headInsertEventTap, options: .defaultTap,
            eventsOfInterest: mask, callback: callback,
            userInfo: Unmanaged.passUnretained(self).toOpaque()
        ) else {
            lastNote = "Link All couldn't watch the keyboard and mouse. Check its Accessibility permission."
            return
        }
        self.tap = tap
        runLoopSource = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
        CFRunLoopAddSource(CFRunLoopGetMain(), runLoopSource, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
        timer = Timer.scheduledTimer(withTimeInterval: 0.008, repeats: true) { [weak self] _ in self?.tick() }
    }

    private func removeTap() {
        timer?.invalidate(); timer = nil
        if let tap { CGEvent.tapEnable(tap: tap, enable: false) }
        if let runLoopSource { CFRunLoopRemoveSource(CFRunLoopGetMain(), runLoopSource, .commonModes) }
        runLoopSource = nil
        tap = nil
    }

    /// True to swallow the event.
    private func handle(type: CGEventType, event: CGEvent) -> Bool {
        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
            if let tap { CGEvent.tapEnable(tap: tap, enable: true) }
            return false
        }
        switch mode {
        case .idle:
            if type == .mouseMoved { checkEdges(event.location) }
            return false
        case .starting:
            return false
        case .remote:
            capture(type: type, event: event)
            return true
        }
    }

    // ── Starting ─────────────────────────────────────────────────────────────

    private func checkEdges(_ at: CGPoint) {
        guard Date() >= cooldownUntil else { return }
        let bounds = Self.desktopBounds()
        for e in 0..<4 {
            guard link(e) != nil else { hits[e] = 0; continue }
            let atEdge: Bool
            switch e {
            case 0: atEdge = at.x <= bounds.minX
            case 1: atEdge = at.x >= bounds.maxX - 1
            case 2: atEdge = at.y <= bounds.minY
            default: atEdge = at.y >= bounds.maxY - 1
            }
            if !atEdge { hits[e] = 0; continue }
            if hits[e] == 0 { since[e] = Date() }
            hits[e] += 1
            // Dragging a window to the edge is not a push.
            if hits[e] >= 4, Date().timeIntervalSince(since[e]) >= 0.12, NSEvent.pressedMouseButtons == 0 {
                hits[e] = 0
                let along = e <= 1
                    ? (at.y - bounds.minY) / max(bounds.height - 1, 1)
                    : (at.x - bounds.minX) / max(bounds.width - 1, 1)
                begin(edge: e, pos: Int((min(max(along, 0), 1) * 65535).rounded()))
                return
            }
        }
    }

    private func begin(edge: Int, pos: Int) {
        guard let target = link(edge), mode == .idle else { return }
        mode = .starting
        Task { @MainActor in
            do {
                try await LinkAllIPCClient.shared.edgeBegin(target: target, edge: edge, pos: pos)
                // Sharing may have been turned off, or this edge relinked,
                // while the other computer answered: with no tap there would
                // be no way to bring the hidden pointer back.
                guard self.mode == .starting, self.tap != nil, self.link(edge) == target else {
                    if self.mode == .starting { self.mode = .idle }
                    try? await LinkAllIPCClient.shared.edgeEnd()
                    return
                }
                self.startRemote(edge: edge)
            } catch {
                self.cooldownUntil = Date().addingTimeInterval(3)
                self.mode = .idle
                // A retried request can fail while the first one went through;
                // ending is a no-op when nothing was started.
                try? await LinkAllIPCClient.shared.edgeEnd()
            }
        }
    }

    private func startRemote(edge: Int) {
        lastEdge = edge
        batch = EdgeBatch()
        failures = 0
        sending = false
        lastStatus = Date()
        // The pointer stays where it is while the mouse's deltas keep coming.
        CGAssociateMouseAndMouseCursorPosition(0)
        CGDisplayHideCursor(CGMainDisplayID())
        mode = .remote
    }

    // ── While the other computer is driven ───────────────────────────────────

    private func capture(type: CGEventType, event: CGEvent) {
        switch type {
        case .mouseMoved, .leftMouseDragged, .rightMouseDragged, .otherMouseDragged:
            batch.dx += Int(event.getIntegerValueField(.mouseEventDeltaX))
            batch.dy += Int(event.getIntegerValueField(.mouseEventDeltaY))
        case .leftMouseDown: batch.buttons.append((0, true))
        case .leftMouseUp: batch.buttons.append((0, false))
        case .rightMouseDown: batch.buttons.append((1, true))
        case .rightMouseUp: batch.buttons.append((1, false))
        case .otherMouseDown, .otherMouseUp:
            // 2 is the middle button, 3 and 4 the side ones, as on the wire.
            let n = Int(event.getIntegerValueField(.mouseEventButtonNumber))
            batch.buttons.append((min(max(n, 2), 4), type == .otherMouseDown))
        case .scrollWheel:
            batch.scrollY += Int(event.getIntegerValueField(.scrollWheelEventPointDeltaAxis1))
            batch.scrollX -= Int(event.getIntegerValueField(.scrollWheelEventPointDeltaAxis2))
        case .keyDown, .keyUp:
            let code = Int(event.getIntegerValueField(.keyboardEventKeycode))
            let down = type == .keyDown
            // Ctrl+Option+Esc: the way back that needs nothing from the network.
            if down, code == 53, event.flags.contains(.maskControl), event.flags.contains(.maskAlternate) {
                DispatchQueue.main.async { self.endRemote(tellDaemon: true, note: "You pressed Ctrl+Option+Esc.") }
                return
            }
            // The other computer repeats a held key on its own.
            if down, event.getIntegerValueField(.keyboardEventAutorepeat) != 0 { return }
            if let sc = Self.scancode(forMacKey: code) { batch.keys.append((sc, down)) }
        case .flagsChanged:
            let code = Int(event.getIntegerValueField(.keyboardEventKeycode))
            guard let sc = Self.scancode(forMacKey: code) else { return }
            // Caps Lock reports each press once, as a lock change: send a tap.
            if code == 57 {
                batch.keys.append((sc, true))
                batch.keys.append((sc, false))
                return
            }
            // Left and right share a flag, so read the key's own device bit:
            // it says whether this key is down now, even if it was already
            // held when the hand-off began.
            guard let bit = Self.deviceFlag(forMacKey: code),
                  let flag = Self.modifierFlag(forMacKey: code) else { return }
            // Synthetic events (remappers, virtual keyboards) may carry only
            // the shared flag: when neither side of this modifier has its
            // device bit, the shared flag decides.
            let raw = event.flags.rawValue
            let sides = Self.deviceFlags(sharing: flag)
            let down = raw & sides != 0 ? raw & bit != 0 : event.flags.contains(flag)
            batch.keys.append((sc, down))
        default:
            break
        }
    }

    /// Runs every 8 ms: sends what has gathered, and asks how it is going.
    private func tick() {
        guard mode == .remote else { return }
        if !sending, !batch.isEmpty, Date().timeIntervalSince(lastSend) >= 0.008 {
            let out = batch
            batch = EdgeBatch()
            sending = true
            lastSend = Date()
            Task { @MainActor in
                do { try await LinkAllIPCClient.shared.edgeInput(out); self.failures = 0 }
                catch { self.failures += 1 }
                self.sending = false
                if self.failures >= 5, self.mode == .remote {
                    // Input fails as soon as the pointer comes back or the other
                    // side ends it; the status says which, before the next poll.
                    let s = try? await LinkAllIPCClient.shared.edgeStatus()
                    if let pos = s?.returnedPos {
                        self.endRemote(tellDaemon: false, note: nil, returnedPos: pos)
                    } else if let why = s?.endedReason {
                        self.endRemote(tellDaemon: false, note: why)
                    } else {
                        self.endRemote(tellDaemon: true, note: "The other computer stopped answering.")
                    }
                }
            }
        }
        if Date().timeIntervalSince(lastStatus) >= 0.1 {
            lastStatus = Date()
            Task { @MainActor in
                guard let s = try? await LinkAllIPCClient.shared.edgeStatus(), self.mode == .remote else { return }
                if let pos = s.returnedPos {
                    self.endRemote(tellDaemon: false, note: nil, returnedPos: pos)
                } else if let why = s.endedReason {
                    self.endRemote(tellDaemon: false, note: why)
                }
            }
        }
    }

    private func endRemote(tellDaemon: Bool, note: String?, returnedPos: Int = 32768) {
        guard mode == .remote else { return }
        mode = .idle
        CGAssociateMouseAndMouseCursorPosition(1)
        CGDisplayShowCursor(CGMainDisplayID())
        placeAtEdge(pos: returnedPos)
        cooldownUntil = Date().addingTimeInterval(0.8)
        batch = EdgeBatch()
        if let note { lastNote = note }
        if tellDaemon { Task { try? await LinkAllIPCClient.shared.edgeEnd() } }
    }

    /// Just inside the edge it left by, `pos` of the way along it.
    private func placeAtEdge(pos: Int) {
        let b = Self.desktopBounds()
        let f = CGFloat(pos) / 65535
        let inset: CGFloat = 6
        let p: CGPoint
        switch lastEdge {
        case 0: p = CGPoint(x: b.minX + inset, y: b.minY + f * (b.height - 1))
        case 1: p = CGPoint(x: b.maxX - 1 - inset, y: b.minY + f * (b.height - 1))
        case 2: p = CGPoint(x: b.minX + f * (b.width - 1), y: b.minY + inset)
        default: p = CGPoint(x: b.minX + f * (b.width - 1), y: b.maxY - 1 - inset)
        }
        CGWarpMouseCursorPosition(p)
    }

    /// All displays together, in the top-left-origin coordinates events use.
    private static func desktopBounds() -> CGRect {
        var count: UInt32 = 0
        CGGetActiveDisplayList(0, nil, &count)
        var ids = [CGDirectDisplayID](repeating: 0, count: Int(count))
        CGGetActiveDisplayList(count, &ids, &count)
        return ids.prefix(Int(count)).reduce(CGRect.null) { $0.union(CGDisplayBounds($1)) }
    }

    // ── Key maps ─────────────────────────────────────────────────────────────

    /// The PS/2 set 1 scan code the other end expects for a Mac key. Command
    /// acts as Control, Option as Alt and Control as the Windows key, so
    /// Cmd+C on this keyboard copies over there.
    static func scancode(forMacKey code: Int) -> Int? { macToScancode[code] }

    /// The device-dependent flag bit (NX_DEVICE*KEYMASK) for one modifier key.
    static func deviceFlag(forMacKey code: Int) -> UInt64? {
        switch code {
        case 59: return 0x0000_0001 // left Control
        case 56: return 0x0000_0002 // left Shift
        case 60: return 0x0000_0004 // right Shift
        case 55: return 0x0000_0008 // left Command
        case 54: return 0x0000_0010 // right Command
        case 58: return 0x0000_0020 // left Option
        case 61: return 0x0000_0040 // right Option
        case 62: return 0x0000_2000 // right Control
        default: return nil
        }
    }

    /// The device bits of both keys (left and right) behind one shared flag.
    static func deviceFlags(sharing flag: CGEventFlags) -> UInt64 {
        switch flag {
        case .maskShift: return 0x0000_0006
        case .maskControl: return 0x0000_2001
        case .maskAlternate: return 0x0000_0060
        case .maskCommand: return 0x0000_0018
        default: return 0
        }
    }

    /// The shared flag a Mac modifier key sets while held.
    static func modifierFlag(forMacKey code: Int) -> CGEventFlags? {
        switch code {
        case 56, 60: return .maskShift
        case 59, 62: return .maskControl
        case 58, 61: return .maskAlternate
        case 55, 54: return .maskCommand
        default: return nil
        }
    }

    private static let macToScancode: [Int: Int] = [
        53: 0x01, 18: 0x02, 19: 0x03, 20: 0x04, 21: 0x05, 23: 0x06, 22: 0x07, 26: 0x08, 28: 0x09, 25: 0x0A,
        29: 0x0B, 27: 0x0C, 24: 0x0D, 51: 0x0E, 48: 0x0F, 12: 0x10, 13: 0x11, 14: 0x12, 15: 0x13, 17: 0x14,
        16: 0x15, 32: 0x16, 34: 0x17, 31: 0x18, 35: 0x19, 33: 0x1A, 30: 0x1B, 36: 0x1C, 55: 0x1D, 0: 0x1E,
        1: 0x1F, 2: 0x20, 3: 0x21, 5: 0x22, 4: 0x23, 38: 0x24, 40: 0x25, 37: 0x26, 41: 0x27, 39: 0x28,
        50: 0x29, 56: 0x2A, 42: 0x2B, 6: 0x2C, 7: 0x2D, 8: 0x2E, 9: 0x2F, 11: 0x30, 45: 0x31, 46: 0x32,
        43: 0x33, 47: 0x34, 44: 0x35, 60: 0x36, 67: 0x37, 58: 0x38, 49: 0x39, 57: 0x3A, 122: 0x3B, 120: 0x3C,
        99: 0x3D, 118: 0x3E, 96: 0x3F, 97: 0x40, 98: 0x41, 100: 0x42, 101: 0x43, 109: 0x44, 71: 0x45,
        89: 0x47, 91: 0x48, 92: 0x49, 78: 0x4A, 86: 0x4B, 87: 0x4C, 88: 0x4D, 69: 0x4E, 83: 0x4F, 84: 0x50,
        85: 0x51, 82: 0x52, 65: 0x53, 103: 0x57, 111: 0x58,
        76: 0xE01C, 54: 0xE01D, 75: 0xE035, 61: 0xE038, 115: 0xE047, 126: 0xE048, 116: 0xE049, 123: 0xE04B,
        124: 0xE04D, 119: 0xE04F, 125: 0xE050, 121: 0xE051, 114: 0xE052, 117: 0xE053, 59: 0xE05B, 62: 0xE05C,
    ]
}
