import Foundation
import SystemExtensions
import os.log

class VirtualCameraInstaller: NSObject, OSSystemExtensionRequestDelegate, ObservableObject {
    static let shared = VirtualCameraInstaller()
    private let logger = OSLog(subsystem: "app.linkall.mac", category: "VirtualCameraInstaller")
    
    @Published var status: String = "Not Installed"
    
    func install() {
        let request = OSSystemExtensionRequest.activationRequest(forExtensionWithIdentifier: "app.linkall.mac.VirtualCamera", queue: .main)
        request.delegate = self
        OSSystemExtensionManager.shared.submitRequest(request)
        self.status = "Installing..."
    }
    
    func request(_ request: OSSystemExtensionRequest, actionForReplacingExtension existing: OSSystemExtensionProperties, withExtension ext: OSSystemExtensionProperties) -> OSSystemExtensionRequest.ReplacementAction {
        return .replace
    }
    
    func requestNeedsUserApproval(_ request: OSSystemExtensionRequest) {
        os_log("Extension needs user approval", log: logger, type: .info)
        self.status = "Needs Approval (Open Settings)"
    }
    
    func request(_ request: OSSystemExtensionRequest, didFinishWithResult result: OSSystemExtensionRequest.Result) {
        os_log("Extension installed successfully", log: logger, type: .info)
        self.status = "Installed Successfully"
    }
    
    func request(_ request: OSSystemExtensionRequest, didFailWithError error: Error) {
        os_log("Extension installation failed: %{public}@", log: logger, type: .error, error.localizedDescription)
        self.status = "Failed: \(error.localizedDescription)"
    }
}
