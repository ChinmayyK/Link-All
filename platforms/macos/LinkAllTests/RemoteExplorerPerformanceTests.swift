import XCTest
import SwiftUI
@testable import LinkAll

final class RemoteExplorerPerformanceTests: XCTestCase {
    
    var store: LinkAllStore!
    var device: ManagedDevice!
    
    override func setUpWithError() throws {
        store = LinkAllStore()
        
        let peer = PeerViewModel(
            id: "test-device-123",
            displayName: "Test Android Device",
            platform: "android",
            trusted: true,
            remembered: true,
            connected: true,
            connectionStatus: "connected",
            syncEnabled: true,
            remoteSyncEnabled: true,
            autoConnect: true,
            lastError: nil,
            pairingRequested: false,
            outgoingPairingWaiting: false,
            pairingPin: nil,
            explicitDisconnect: false,
            lastSeen: Date(),
            lastDiscoveryAt: Date(),
            lastSync: Date(),
            ip: "192.168.1.5"
        )
        device = ManagedDevice(peer: peer)
    }
    
    override func tearDownWithError() throws {
        store = nil
        device = nil
    }
    
    private func generateMockFiles(count: Int) -> [IpcRemoteFileEntry] {
        let now = UInt64(Date().timeIntervalSince1970)
        return (0..<count).map { i in
            IpcRemoteFileEntry(
                file_id: UInt64(i),
                display_name: "MockFile_\(i).txt",
                size_bytes: 1024 * 1024 * UInt64((i % 100) + 1),
                mime_type: "text/plain",
                date_modified: now - UInt64((i % 10) * 86400),
                category: "Documents",
                source: "Downloads",
                content_uri: "content://mock/file/\(i)"
            )
        }
    }
    
    private func setupStoreWithMockFiles(count: Int) {
        let mockFiles = generateMockFiles(count: count)
        let summary = IpcRemoteFilesSummary(
            type_counts: IpcRemoteFileCategoryCounts(images: 0, videos: 0, audio: 0, documents: UInt32(count), apks: 0, archives: 0),
            source_counts: IpcRemoteFileSourceCounts(whatsapp: 0, downloads: UInt32(count), camera: 0)
        )
        let result = IpcRemoteFilesResult(summary: summary, files: mockFiles, total_matching: UInt32(count), error: nil)
        
        let cacheKey = "\(device.id)_all_all_"
        store.remoteFilesCache[cacheKey] = result
    }
    
    func testPerformanceWith10KFiles() {
        setupStoreWithMockFiles(count: 10_000)
        let view = RemoteExplorerView(store: store, device: device)
        
        measure(metrics: [XCTMemoryMetric(), XCTCPUMetric()]) {
            let hostingController = NSHostingController(rootView: view)
            hostingController.view.setFrameSize(NSSize(width: 1000, height: 800))
            hostingController.view.layoutSubtreeIfNeeded()
        }
    }
    
    func testPerformanceWith50KFiles() {
        setupStoreWithMockFiles(count: 50_000)
        let view = RemoteExplorerView(store: store, device: device)
        
        measure(metrics: [XCTMemoryMetric(), XCTCPUMetric()]) {
            let hostingController = NSHostingController(rootView: view)
            hostingController.view.setFrameSize(NSSize(width: 1000, height: 800))
            hostingController.view.layoutSubtreeIfNeeded()
        }
    }
    
    func testPerformanceWith100KFiles() {
        setupStoreWithMockFiles(count: 100_000)
        let view = RemoteExplorerView(store: store, device: device)
        
        measure(metrics: [XCTMemoryMetric(), XCTCPUMetric()]) {
            let hostingController = NSHostingController(rootView: view)
            hostingController.view.setFrameSize(NSSize(width: 1000, height: 800))
            hostingController.view.layoutSubtreeIfNeeded()
        }
    }
}
