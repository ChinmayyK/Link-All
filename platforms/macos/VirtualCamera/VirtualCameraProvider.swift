import Foundation
import CoreMediaIO
import os.log

fileprivate let logger = OSLog(subsystem: "app.linkall.VirtualCamera", category: "Provider")

class ProviderSource: NSObject, CMIOExtensionProviderSource {
    private(set) var provider: CMIOExtensionProvider!
    private var deviceSource: DeviceSource!
    
    init(clientQueue: DispatchQueue?) {
        super.init()
        provider = CMIOExtensionProvider(source: self, clientQueue: clientQueue)
        deviceSource = DeviceSource(localizedName: "Link All Camera")
        
        do {
            try provider.addDevice(deviceSource.device)
        } catch {
            os_log("Failed to add device: %{public}@", log: logger, type: .error, error.localizedDescription)
        }
    }
    
    func connect(to client: CMIOExtensionClient) throws {
        // Accept all clients
    }
    
    func disconnect(from client: CMIOExtensionClient) {
        // Handle disconnection if needed
    }
    
    var availableProperties: Set<CMIOExtensionProperty> {
        return [.providerManufacturer]
    }
    
    func providerProperties(forProperties properties: Set<CMIOExtensionProperty>) throws -> CMIOExtensionProviderProperties {
        let providerProperties = CMIOExtensionProviderProperties(dictionary: [:])
        if properties.contains(.providerManufacturer) {
            providerProperties.setPropertyState(CMIOExtensionPropertyState(value: "Link All" as NSString), forProperty: .providerManufacturer)
        }
        return providerProperties
    }
    
    func setProviderProperties(_ providerProperties: CMIOExtensionProviderProperties) throws {
        // Read-only properties
    }
}

class DeviceSource: NSObject, CMIOExtensionDeviceSource {
    private(set) var device: CMIOExtensionDevice!
    private var streamSource: StreamSource!
    
    init(localizedName: String) {
        super.init()
        let deviceID = UUID()
        device = CMIOExtensionDevice(localizedName: localizedName, deviceID: deviceID, legacyDeviceID: nil, source: self)
        
        let videoDimensions = CMVideoDimensions(width: 1920, height: 1080)
        streamSource = StreamSource(localizedName: "Link All Video Stream", videoDimensions: videoDimensions)
        
        do {
            try device.addStream(streamSource.stream)
        } catch {
            os_log("Failed to add stream: %{public}@", log: logger, type: .error, error.localizedDescription)
        }
    }
    
    var availableProperties: Set<CMIOExtensionProperty> {
        return [.deviceTransportType, .deviceModel]
    }
    
    func deviceProperties(forProperties properties: Set<CMIOExtensionProperty>) throws -> CMIOExtensionDeviceProperties {
        let deviceProperties = CMIOExtensionDeviceProperties(dictionary: [:])
        if properties.contains(.deviceTransportType) {
            deviceProperties.setPropertyState(CMIOExtensionPropertyState(value: 0x76697274 as NSNumber), forProperty: .deviceTransportType)
        }
        if properties.contains(.deviceModel) {
            deviceProperties.setPropertyState(CMIOExtensionPropertyState(value: "Link All Virtual Camera" as NSString), forProperty: .deviceModel)
        }
        return deviceProperties
    }
    
    func setDeviceProperties(_ deviceProperties: CMIOExtensionDeviceProperties) throws {
        // Read-only properties
    }
}
