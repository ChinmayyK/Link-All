import Cocoa

class ShareViewController: NSViewController {
    
    override func loadView() {
        self.view = NSView(frame: NSRect(x: 0, y: 0, width: 200, height: 50))
        let label = NSTextField(labelWithString: "Sending via Link All...")
        label.frame = self.view.bounds
        label.alignment = .center
        self.view.addSubview(label)
        
        guard let context = self.extensionContext else { return }
        guard let item = context.inputItems.first as? NSExtensionItem else {
            context.completeRequest(returningItems: nil, completionHandler: nil)
            return
        }
        guard let attachments = item.attachments else {
            context.completeRequest(returningItems: nil, completionHandler: nil)
            return
        }
        
        var sharedItems: [String] = []
        let group = DispatchGroup()
        
        for attachment in attachments {
            if attachment.hasItemConformingToTypeIdentifier("public.file-url") {
                group.enter()
                attachment.loadItem(forTypeIdentifier: "public.file-url", options: nil) { (data, error) in
                    if let url = data as? URL {
                        sharedItems.append("file://" + url.path)
                    }
                    group.leave()
                }
            } else if attachment.hasItemConformingToTypeIdentifier("public.text") {
                group.enter()
                attachment.loadItem(forTypeIdentifier: "public.text", options: nil) { (data, error) in
                    if let text = data as? String {
                        sharedItems.append("text://" + text)
                    }
                    group.leave()
                }
            }
        }
        
        group.notify(queue: .main) {
            if !sharedItems.isEmpty {
                // Determine the App Group identifier, usually TeamID.app.linkall or group.app.linkall
                // Here we fallback to standard UserDefaults if App Group fails
                let defaults = UserDefaults(suiteName: "group.app.linkall") ?? UserDefaults.standard
                var queue = defaults.stringArray(forKey: "SharedItemsQueue") ?? []
                queue.append(contentsOf: sharedItems)
                defaults.set(queue, forKey: "SharedItemsQueue")
                defaults.synchronize()
                
                // Trigger Darwin Notification so the main app picks it up
                let center = CFNotificationCenterGetDarwinNotifyCenter()
                let notificationName = CFStringCreateWithCString(nil, "app.linkall.sharedItemReceived", CFStringBuiltInEncodings.UTF8.rawValue)
                CFNotificationCenterPostNotification(center, notificationName, nil, nil, true)
            }
            
            // Allow the UI a moment to show "Sending..."
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
                context.completeRequest(returningItems: nil, completionHandler: nil)
            }
        }
    }
}
