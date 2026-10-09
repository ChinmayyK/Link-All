// ClipboardWatcher.swift
// Efficient NSPasteboard watcher using change-count diffing.
// Replaces the naive Timer approach with a dedicated polling thread
// that idles at 50 ms on change activity, 200 ms when idle.

import AppKit
import Foundation

/// Observes the system pasteboard and calls `onChange` whenever the contents
/// change.  Runs on a dedicated background thread — never blocks the main queue.
///
/// - `suppressCount`: incremented by the caller before it sets the clipboard
///   programmatically; the watcher skips that many change events.
final class ClipboardWatcher {

    // ── Configuration ─────────────────────────────────────────────────────────

    /// Poll interval when the clipboard has changed recently (ms).
    private let activeIntervalMs: Int = 50
    /// Poll interval when nothing is changing (ms).
    private let idleIntervalMs: Int = 200
    /// Idle after this many consecutive no-change polls.
    private let idleAfterCount: Int = 10

    // ── State ─────────────────────────────────────────────────────────────────

    private var lastChangeCount: Int = NSPasteboard.general.changeCount
    private var consecutiveNoChange: Int = 0
    private var thread: Thread?
    private var running = false

    /// Incremented by the caller before setting the clipboard programmatically.
    /// The watcher skips that many change events to prevent echo.
    private var _lastAppliedChangeCount: Int = 0

    func setLastAppliedChangeCount(_ count: Int) {
        lock.lock()
        defer { lock.unlock() }
        _lastAppliedChangeCount = max(_lastAppliedChangeCount, count)
    }

    private let lock = NSLock()

    // ── Callbacks ─────────────────────────────────────────────────────────────

    var onTextChange:  ((String) -> Void)?
    var onImageChange: ((Data, String) -> Void)?  // (data, mimeType)
    var onFileChange:  (([URL]) -> Void)?

    // ── Lifecycle ─────────────────────────────────────────────────────────────

    func start() {
        guard !running else { return }
        running = true
        let t = Thread { [weak self] in self?.watchLoop() }
        t.name = "app.linkall.ClipboardWatcher"
        t.qualityOfService = .background
        t.start()
        thread = t
    }

    func stop() {
        running = false
    }

    // ── Watch loop ────────────────────────────────────────────────────────────

    private func watchLoop() {
        while running {
            let pb = NSPasteboard.general
            let currentCount = pb.changeCount

            if currentCount != lastChangeCount {
                lastChangeCount = currentCount
                consecutiveNoChange = 0

                lock.lock()
                let applied = _lastAppliedChangeCount
                lock.unlock()

                if currentCount <= applied {
                    sleep(intervalMs: activeIntervalMs)
                    continue
                }

                dispatchContents(from: pb)
                sleep(intervalMs: activeIntervalMs)
            } else {
                consecutiveNoChange += 1
                let interval = consecutiveNoChange >= idleAfterCount
                    ? idleIntervalMs
                    : activeIntervalMs
                sleep(intervalMs: interval)
            }
        }
    }

    private func dispatchContents(from pb: NSPasteboard) {
        // Ordered by priority: try richest format first.

        // 1. Plain text (most common clipboard content).
        if let text = pb.string(forType: .string), !text.isEmpty {
            let captured = text
            DispatchQueue.main.async { [weak self] in
                self?.onTextChange?(captured)
            }
            return
        }

        // 2. PNG image.
        if let pngData = pb.data(forType: .png) {
            let captured = pngData
            DispatchQueue.main.async { [weak self] in
                self?.onImageChange?(captured, "image/png")
            }
            return
        }

        // 3. TIFF → convert to PNG.
        if let tiffData = pb.data(forType: .tiff),
           let bitmapRep = NSBitmapImageRep(data: tiffData),
           let pngData = bitmapRep.representation(using: .png, properties: [:]) {
            let captured = pngData
            DispatchQueue.main.async { [weak self] in
                self?.onImageChange?(captured, "image/png")
            }
            return
        }

        // 4. Any other pasteboard-native image format macOS can decode
        // (including HEIC/HEIF/AVIF-backed NSImage objects) is normalized to
        // PNG so Android and other receivers get a broadly compatible payload.
        if let images = pb.readObjects(forClasses: [NSImage.self], options: nil) as? [NSImage],
           let image = images.first,
           let tiffData = image.tiffRepresentation,
           let bitmapRep = NSBitmapImageRep(data: tiffData),
           let pngData = bitmapRep.representation(using: .png, properties: [:]) {
            let captured = pngData
            DispatchQueue.main.async { [weak self] in
                self?.onImageChange?(captured, "image/png")
            }
            return
        }

        // 5. File URLs.
        let classes: [AnyClass] = [NSURL.self]
        let options: [NSPasteboard.ReadingOptionKey: Any] = [
            .urlReadingFileURLsOnly: true
        ]
        if let urls = pb.readObjects(forClasses: classes, options: options) as? [URL],
           !urls.isEmpty {
            let captured = urls
            DispatchQueue.main.async { [weak self] in
                self?.onFileChange?(captured)
            }
        }
    }

    private func sleep(intervalMs: Int) {
        Thread.sleep(forTimeInterval: Double(intervalMs) / 1000.0)
    }
}

// ── Clipboard setter (with suppress) ─────────────────────────────────────────

/// Sets clipboard content while preventing echo back through the watcher.
final class ClipboardSetter {

    private weak var watcher: ClipboardWatcher?

    init(watcher: ClipboardWatcher) {
        self.watcher = watcher
    }

    func setText(_ text: String) {
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(text, forType: .string)
        watcher?.setLastAppliedChangeCount(pb.changeCount)
    }

    func setImage(_ data: Data, mimeType: String) {
        let pb = NSPasteboard.general
        pb.clearContents()
        let type: NSPasteboard.PasteboardType = mimeType.contains("png") ? .png : .tiff
        pb.setData(data, forType: type)
        watcher?.setLastAppliedChangeCount(pb.changeCount)
    }

    func setFileURL(_ url: URL) {
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.writeObjects([url as NSURL])
        watcher?.setLastAppliedChangeCount(pb.changeCount)
    }
}
