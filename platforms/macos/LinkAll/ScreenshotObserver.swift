import Foundation
import AppKit

final class ScreenshotObserver {
    private var stream: FSEventStreamRef?
    private let observerStartDate = Date()
    private var sentScreenshotPaths = Set<String>()
    private let onScreenshot: (URL) -> Void
    
    init(onScreenshot: @escaping (URL) -> Void) {
        self.onScreenshot = onScreenshot
    }
    
    func start() {
        guard stream == nil else { return }
        
        // Determine screenshot directory (defaults to Desktop)
        var screenshotDir = FileManager.default.urls(for: .desktopDirectory, in: .userDomainMask).first!.path
        if let customLocation = UserDefaults(suiteName: "com.apple.screencapture")?.string(forKey: "location") {
            screenshotDir = (customLocation as NSString).expandingTildeInPath
        }
        
        let pathsToWatch = [screenshotDir] as CFArray
        
        var context = FSEventStreamContext(
            version: 0,
            info: UnsafeMutableRawPointer(Unmanaged.passUnretained(self).toOpaque()),
            retain: nil,
            release: nil,
            copyDescription: nil
        )
        
        let flags = UInt32(kFSEventStreamCreateFlagUseCFTypes | kFSEventStreamCreateFlagFileEvents)
        
        stream = FSEventStreamCreate(
            kCFAllocatorDefault,
            fsEventsCallback,
            &context,
            pathsToWatch,
            FSEventStreamEventId(kFSEventStreamEventIdSinceNow),
            1.0, // latency
            flags
        )
        
        if let stream = stream {
            FSEventStreamSetDispatchQueue(stream, DispatchQueue.main)
            FSEventStreamStart(stream)
        }
    }
    
    func stop() {
        if let stream = stream {
            FSEventStreamStop(stream)
            FSEventStreamInvalidate(stream)
            FSEventStreamRelease(stream)
            self.stream = nil
        }
    }
    
    fileprivate func handleFileEvent(path: String, flags: FSEventStreamEventFlags) {
        // We only care about newly created/modified files
        guard (flags & UInt32(kFSEventStreamEventFlagItemCreated | kFSEventStreamEventFlagItemModified)) != 0 else { return }
        
        // Screen capture files typically contain "Screen Shot" or "Screenshot"
        let lowerPath = path.lowercased()
        guard lowerPath.contains("screenshot") || lowerPath.contains("screen shot") else { return }
        
        let url = URL(fileURLWithPath: path)
        
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: path),
              let creationDate = attributes[.creationDate] as? Date else { return }
        
        if creationDate > observerStartDate, !sentScreenshotPaths.contains(path) {
            sentScreenshotPaths.insert(path)
            onScreenshot(url)
        }
    }
}

private func fsEventsCallback(
    streamRef: ConstFSEventStreamRef,
    clientCallBackInfo: UnsafeMutableRawPointer?,
    numEvents: Int,
    eventPaths: UnsafeMutableRawPointer,
    eventFlags: UnsafePointer<FSEventStreamEventFlags>,
    eventIds: UnsafePointer<FSEventStreamEventId>
) {
    guard let info = clientCallBackInfo else { return }
    let observer = Unmanaged<ScreenshotObserver>.fromOpaque(info).takeUnretainedValue()
    
    let paths = Unmanaged<CFArray>.fromOpaque(eventPaths).takeUnretainedValue() as! [String]
    for i in 0..<numEvents {
        observer.handleFileEvent(path: paths[i], flags: eventFlags[i])
    }
}
