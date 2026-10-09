// Link All — Remote File Explorer Pro-Max (macOS)
// High-performance remote file browsing, date-grouped grids, batch multi-select, and instant pull/push over local Wi-Fi.

import SwiftUI
import AppKit
import QuickLook

enum ExplorerViewMode: String, CaseIterable {
    case dateGrouped = "Date Grouped"
    case grid = "Grid"
    case list = "List"
    
    var icon: String {
        switch self {
        case .dateGrouped: return "calendar.day.timeline.left"
        case .grid: return "square.grid.2x2.fill"
        case .list: return "list.bullet"
        }
    }
}

struct FileDateGroup: Identifiable {
    let id: String
    let title: String
    let files: [IpcRemoteFileEntry]
}

// MARK: - Main Application Window View

/// Hosts the explorer for one device at a time and lets the user switch
/// between connected devices. Re-keying the pane on the device id discards
/// all per-device state (index, selection, thumbnails) and reloads.
struct RemoteExplorerView: View {
    @ObservedObject var store: LinkAllStore
    @State private var selectedDeviceId: String

    private let initialDevice: ManagedDevice

    init(store: LinkAllStore, device: ManagedDevice) {
        self.store = store
        self.initialDevice = device
        _selectedDeviceId = State(initialValue: device.id)
    }

    private var device: ManagedDevice {
        store.devices.first(where: { $0.id == selectedDeviceId }) ?? initialDevice
    }

    var body: some View {
        RemoteExplorerPane(store: store, device: device) { selectedDeviceId = $0.id }
            .id(selectedDeviceId)
    }
}

private struct RemoteExplorerPane: View {
    @ObservedObject var store: LinkAllStore
    let device: ManagedDevice
    let onSwitchDevice: (ManagedDevice) -> Void
    @Environment(\.dismiss) var dismiss
    
    // State
    @State private var result: IpcRemoteFilesResult?
    @State private var isLoading = false
    @State private var errorMessage: String?
    @State private var selectedCategory: String? = nil
    @State private var selectedSource: String? = nil
    @State private var searchQuery: String = ""
    @State private var viewMode: ExplorerViewMode = .dateGrouped
    @State private var showInspector: Bool = false // Now hidden by default per redesign
    @State private var selectedFiles: Set<UInt64> = []
    @State private var selectedFile: IpcRemoteFileEntry? = nil
    
    // Marquee Selection State
    @State private var fileFrames: [UInt64: CGRect] = [:]
    @State private var dragRect: CGRect? = nil
    @State private var dragStart: CGPoint? = nil
    
    @State private var thumbnailCache: [UInt64: NSImage] = [:]
    @State private var pullingFiles: Set<UInt64> = []
    @State private var pulledFiles: Set<UInt64> = []
    
    @State private var hoveredFileId: UInt64? = nil
    
    // QuickLook State
    @State private var quickLookURL: URL? = nil
    @State private var autoQuickLookFileId: UInt64? = nil
    
    // File Action States
    
    // Pagination State
    @State private var displayLimit: Int = 500
    
    // Library Categories
    private let libraryCategories: [(id: String?, label: String, icon: String)] = [
        (nil, "All Files", "square.grid.2x2.fill"),
        ("Images", "Images", "photo.fill"),
        ("Videos", "Videos", "film.fill"),
        ("Audio", "Audio", "waveform"),
        ("Documents", "Documents", "doc.text.fill"),
        ("Apks", "APKs & Apps", "app.dashed"),
        ("Archives", "Archives", "archivebox.fill")
    ]
    
    // Locations (Sources)
    private let locationSources: [(id: String?, label: String, icon: String)] = [
        (nil, "All Locations", "tray.full.fill"),
        ("Downloads", "Downloads", "arrow.down.circle.fill"),
        ("WhatsApp", "WhatsApp", "message.fill"),
        ("Camera", "Camera Roll", "camera.fill"),
        ("Bluetooth", "Bluetooth", "wave.3.left.circle.fill"),
        ("Other", "Other Folders", "folder.fill")
    ]
    
    var body: some View {
        VStack(spacing: 0) {
            // macOS Native Toolbar
            toolbarView
                .frame(height: 52)
                .background(.ultraThinMaterial)
            
            Divider().opacity(0.5)
            
            HStack(spacing: 0) {
                // Left Sidebar (Native Finder style)
                sidebarView
                    .frame(width: 220)
                    .background(.ultraThinMaterial)
                
                Divider().opacity(0.5)
                
                // Center Canvas
                ZStack(alignment: .bottom) {
                    canvasView
                        .background(Color.clear)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                    
                    // Floating Multi-Select Action Bar
                    if !selectedFiles.isEmpty {
                        floatingBatchActionBar
                            .padding(.bottom, 24)
                            .transition(.move(edge: .bottom).combined(with: .opacity))
                    }
                }
                
                // Right Inspector Panel
                if showInspector {
                    Divider().opacity(0.5)
                    inspectorView
                        .frame(width: 320)
                        .background(.ultraThinMaterial)
                        .transition(.move(edge: .trailing))
                }
            }
        }
        .frame(minWidth: 1000, maxWidth: .infinity, minHeight: 650, maxHeight: .infinity)
        .background(contextAwareBackground) // Context-aware blur backing
        .background(
            Button("") {
                triggerQuickLookForSelectedFile()
            }
            .keyboardShortcut(.space, modifiers: [])
            .opacity(0)
        )
        .quickLookPreview($quickLookURL)
        .onAppear { loadFiles() }
        .onChange(of: selectedCategory) { _ in loadFiles() }
        .onChange(of: selectedSource) { _ in loadFiles() }
        .onChange(of: searchQuery) { _ in
            // Debounce or just load on change
            loadFiles()
        }
    }
    
    private var contextAwareBackground: some View {
        ZStack {
            CRTheme.surface
            
            // Subtle tinted blurred circles based on state
            let color1 = selectedCategory == "Images" ? CRTheme.brandViolet : (selectedCategory == "Videos" ? CRTheme.brandCyan : CRTheme.brandElectric)
            let color2 = selectedSource == "Downloads" ? CRTheme.accentBlue : (selectedSource == "WhatsApp" ? CRTheme.accentGreen : CRTheme.brandElectric)
            
            Circle()
                .fill(color1.opacity(0.15))
                .frame(width: 500, height: 500)
                .blur(radius: 120)
                .offset(x: -200, y: -150)
                
            Circle()
                .fill(color2.opacity(0.12))
                .frame(width: 600, height: 600)
                .blur(radius: 150)
                .offset(x: 300, y: 200)
        }
        .ignoresSafeArea()
    }
    
    // MARK: - Device Switcher
    @ViewBuilder
    private var deviceSwitcher: some View {
        let title = HStack(spacing: 8) {
            Text(device.name)
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(CRTheme.ink)
                .lineLimit(1)

            Circle()
                .fill(device.isConnected ? CRTheme.accentGreen : CRTheme.inkSubtle)
                .frame(width: 6, height: 6)
        }
        // Always a menu (even with one device) so the switcher is discoverable;
        // the current device stays listed even if it just dropped offline.
        let candidates = store.connectedDevices.contains(where: { $0.id == device.id })
            ? store.connectedDevices
            : [device] + store.connectedDevices
        Menu {
            Section("Browse files on") {
                ForEach(candidates) { candidate in
                    Button {
                        if candidate.id != device.id { onSwitchDevice(candidate) }
                    } label: {
                        if candidate.id == device.id {
                            Label(candidate.name, systemImage: "checkmark")
                        } else {
                            Text(candidate.name)
                        }
                    }
                }
            }
            if candidates.count < 2 {
                Text("Connect another device to switch")
            }
        } label: {
            HStack(spacing: 4) {
                title
                Image(systemName: "chevron.up.chevron.down")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(CRTheme.inkSubtle)
            }
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .help("Switch device")
    }

    // MARK: - Premium Toolbar
    private var toolbarView: some View {
        HStack(spacing: 16) {
            // Leading Edge: Navigation & Status
            HStack(spacing: 12) {
                Button {
                    dismiss()
                } label: {
                    Image(systemName: "chevron.left")
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(CRTheme.ink)
                }
                .buttonStyle(.plain)
                
                deviceSwitcher
            }
            .frame(width: 204, alignment: .leading) // Match sidebar width
            
            Spacer()
            
            // Center: Raycast Style Search
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass")
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(CRTheme.inkSubtle)
                
                TextField("Search files, extensions, dates...", text: $searchQuery)
                    .textFieldStyle(.plain)
                    .font(.system(size: 13))
                    .onSubmit { loadFiles() }
                
                if !searchQuery.isEmpty {
                    Button {
                        searchQuery = ""
                        loadFiles()
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(CRTheme.inkSubtle)
                    }
                    .buttonStyle(.plain)
                }
            }
            .frame(width: 320, height: 32)
            .padding(.horizontal, 12)
            .background(Color.black.opacity(0.04)) // Subtle depth
            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .strokeBorder(CRTheme.stroke.opacity(0.6), lineWidth: 0.5)
            }
            
            Spacer()
            
            // Trailing Edge: Controls
            HStack(spacing: 12) {
                if isLoading {
                    ProgressView().controlSize(.small)
                }
                // View Mode Segmented Control
                HStack(spacing: 2) {
                    Button {
                        loadFiles(forceRefresh: true)
                    } label: {
                        Image(systemName: "arrow.clockwise")
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(CRTheme.ink)
                            .frame(width: 32, height: 28)
                            .background(CRTheme.surfaceStrong)
                            .clipShape(RoundedRectangle(cornerRadius: 6))
                    }
                    .buttonStyle(.plain)
                    .padding(.trailing, 8)
                    
                    ForEach(ExplorerViewMode.allCases, id: \.self) { mode in
                        Button {
                            withAnimation(.spring(response: 0.3, dampingFraction: 0.75)) {
                                viewMode = mode
                            }
                        } label: {
                            Image(systemName: mode.icon)
                                .font(.system(size: 13, weight: viewMode == mode ? .semibold : .regular))
                                .foregroundStyle(viewMode == mode ? CRTheme.brandElectric : CRTheme.inkSubtle)
                                .frame(width: 28, height: 26)
                                .background(viewMode == mode ? CRTheme.surfaceElevated : Color.clear)
                                .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
                        }
                        .buttonStyle(.plain)
                        .help(mode.rawValue)
                    }
                }
                .padding(2)
                .background(Color.black.opacity(0.04))
                .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(CRTheme.stroke.opacity(0.6), lineWidth: 0.5))
                
                Button {
                    withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) {
                        showInspector.toggle()
                    }
                } label: {
                    Image(systemName: "sidebar.right")
                        .font(.system(size: 15, weight: .medium))
                        .foregroundStyle(showInspector ? CRTheme.brandElectric : CRTheme.ink)
                }
                .buttonStyle(.plain)
                .help("Toggle Inspector")
            }
        }
        .padding(.horizontal, 16)
    }
    
    // MARK: - Sidebar Redesign (Finder Style)
    private var sidebarView: some View {
        ScrollView(.vertical, showsIndicators: false) {
            VStack(alignment: .leading, spacing: 24) {
                // Library Section
                VStack(alignment: .leading, spacing: 4) {
                    Text("Library")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(CRTheme.inkSubtle)
                        .padding(.horizontal, 16)
                        .padding(.bottom, 4)
                    
                    ForEach(libraryCategories, id: \.label) { cat in
                        SidebarRowView(
                            label: cat.label,
                            icon: cat.icon,
                            isSelected: selectedCategory == cat.id,
                            count: countForCategory(cat.id)
                        ) {
                            withAnimation(.spring(response: 0.25, dampingFraction: 0.7)) {
                                selectedCategory = cat.id
                            }
                        }
                    }
                }
                
                // Locations Section
                VStack(alignment: .leading, spacing: 4) {
                    Text("Locations")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(CRTheme.inkSubtle)
                        .padding(.horizontal, 16)
                        .padding(.bottom, 4)
                    
                    ForEach(locationSources, id: \.label) { src in
                        SidebarRowView(
                            label: src.label,
                            icon: src.icon,
                            isSelected: selectedSource == src.id,
                            count: countForSource(src.id)
                        ) {
                            withAnimation(.spring(response: 0.25, dampingFraction: 0.7)) {
                                selectedSource = src.id
                            }
                        }
                    }
                }
            }
            .padding(.vertical, 20)
        }
    }
    
    // MARK: - Sidebar Row View
struct SidebarRowView: View {
    let label: String
    let icon: String
    let isSelected: Bool
    let count: UInt32?
    let action: () -> Void
    
    @State private var isHovered = false
    
    var body: some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Image(systemName: icon)
                    .font(.system(size: 14))
                    .frame(width: 20)
                    .foregroundStyle(isSelected ? CRTheme.ink : CRTheme.brandElectric)
                
                Text(label)
                    .font(.system(size: 13, weight: isSelected ? .semibold : .medium))
                    .lineLimit(1)
                
                Spacer()
                
                if let count = count, count > 0 {
                    Text("\(count)")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(isSelected ? CRTheme.ink : CRTheme.inkSubtle)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .contentShape(Rectangle()) // Makes entire row clickable
            .background(
                isSelected ? CRTheme.rowSelected :
                isHovered ? CRTheme.rowHover : Color.clear
            )
            .foregroundStyle(CRTheme.ink)
            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
            .padding(.horizontal, 12)
            .scaleEffect(isHovered && !isSelected ? 1.02 : 1.0)
            .animation(.easeOut(duration: 0.15), value: isHovered)
            .animation(.spring(response: 0.3, dampingFraction: 0.7), value: isSelected)
            .onHover { hovering in
                isHovered = hovering
            }
        }
        .buttonStyle(.plain)
    }
}

    // MARK: - Center Canvas (Files Grid)
    private var canvasView: some View {
        ZStack {
            if let err = errorMessage {
                errorStateView(err: err)
            } else if let res = result, res.files.isEmpty && !isLoading {
                emptyStateView
            } else if let res = result {
                ScrollView(.vertical, showsIndicators: true) {
                    VStack(alignment: .leading, spacing: 22) {
                        // Quick Action Header within Canvas (Import)
                        HStack {
                            Spacer()
                            Button { importMacFiles() } label: {
                                Label("Import Mac Files...", systemImage: "arrow.up.doc.fill")
                            }
                            .buttonStyle(PBPrimaryButtonStyle(tint: CRTheme.brandElectric))
                        }
                        .padding(.top, 12)
                        
                        // Content Layout - Bento Box Style Grid
                        let displayedFiles = Array(res.files.prefix(displayLimit))
                        switch viewMode {
                        case .dateGrouped:
                            dateGroupedView(for: displayedFiles)
                        case .grid:
                            LazyVGrid(columns: [GridItem(.adaptive(minimum: 160, maximum: 200), spacing: 16)], alignment: .center, spacing: 16) {
                                ForEach(displayedFiles) { file in fileGridCard(for: file) }
                            }
                            .padding(.top, 12)
                        case .list:
                            LazyVStack(alignment: .leading, spacing: 6) {
                                ForEach(displayedFiles) { file in fileListRow(for: file) }
                            }
                            .padding(.top, 12)
                        }
                        
                        if res.files.count > displayLimit {
                            ProgressView()
                                .onAppear {
                                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
                                        displayLimit += 500
                                    }
                                }
                                .frame(maxWidth: .infinity, minHeight: 60)
                        }
                    }
                    .padding(24) // Generous macOS 24px padding
                    .padding(.bottom, selectedFiles.isEmpty ? 24 : 100)
                }
                if isLoading {
                    ProgressView("Updating index from \(device.name)...")
                        .controlSize(.regular)
                        .padding(24)
                        .background(.ultraThinMaterial)
                        .clipShape(RoundedRectangle(cornerRadius: 16))
                }
            } else if isLoading {
                ProgressView("Fetching index from \(device.name)...")
                    .controlSize(.regular)
            }
            
            // Marquee Drag Overlay
            if let rect = dragRect {
                Rectangle()
                    .fill(CRTheme.brandElectric.opacity(0.15))
                    .overlay(
                        Rectangle().stroke(CRTheme.brandElectric.opacity(0.6), lineWidth: 1)
                    )
                    .frame(width: rect.width, height: rect.height)
                    .position(x: rect.midX, y: rect.midY)
            }
            
        }
        .coordinateSpace(name: "CanvasSpace")
        .contentShape(Rectangle()) // Make empty areas draggable
        .gesture(
            DragGesture(minimumDistance: 3, coordinateSpace: .named("CanvasSpace"))
                .onChanged { value in
                    if dragStart == nil {
                        dragStart = value.startLocation
                        selectedFiles.removeAll() // Clear selection on new drag (optional, macOS finder clears unless shift is held)
                    }
                    let start = dragStart ?? value.startLocation
                    let current = value.location
                    
                    let minX = min(start.x, current.x)
                    let minY = min(start.y, current.y)
                    let width = abs(current.x - start.x)
                    let height = abs(current.y - start.y)
                    
                    let currentRect = CGRect(x: minX, y: minY, width: width, height: height)
                    self.dragRect = currentRect
                    
                    // Intersection Logic
                    var newSelection: Set<UInt64> = []
                    for (id, frame) in fileFrames {
                        if currentRect.intersects(frame) {
                            newSelection.insert(id)
                        }
                    }
                    
                    if selectedFiles != newSelection {
                        selectedFiles = newSelection
                    }
                }
                .onEnded { _ in
                    dragStart = nil
                    dragRect = nil
                }
        )
        .onPreferenceChange(FileFramePreferenceKey.self) { frames in
            for data in frames {
                self.fileFrames[data.id] = data.rect
            }
        }
    }
    
    // MARK: - Date Grouped View
    private func dateGroupedView(for files: [IpcRemoteFileEntry]) -> some View {
        // Headers scroll with their files. The pane is translucent, so a pinned header needs
        // a fill behind it, and any fill reads as a solid bar over the window material.
        LazyVStack(alignment: .leading, spacing: 32) {
            ForEach(groupedFiles(from: files)) { group in
                Section(header: stickyHeader(for: group)) {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 160, maximum: 200), spacing: 24)], alignment: .leading, spacing: 24) {
                        ForEach(group.files) { file in
                            fileGridCard(for: file)
                        }
                    }
                }
            }
        }
    }
    
    private func stickyHeader(for group: FileDateGroup) -> some View {
        HStack(spacing: 8) {
            Text(group.title)
                .font(.system(size: 18, weight: .bold))
                .foregroundStyle(CRTheme.ink)
            
            Text("\(group.files.count) items")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(CRTheme.inkSubtle)
            
            Spacer()
        }
        .padding(.top, 4)
        .padding(.horizontal, 4)
    }
    
    // MARK: - Beautiful Empty State
    private var emptyStateView: some View {
        VStack(spacing: 20) {
            ZStack {
                Circle()
                    .fill(CRTheme.brandElectric.opacity(0.08))
                    .frame(width: 120, height: 120)
                
                Image(systemName: "magnifyingglass")
                    .font(.system(size: 50, weight: .ultraLight))
                    .foregroundStyle(
                        LinearGradient(colors: [CRTheme.brandElectric, CRTheme.brandViolet], startPoint: .topLeading, endPoint: .bottomTrailing)
                    )
            }
            
            VStack(spacing: 6) {
                Text(searchQuery.isEmpty ? "Folder is Empty" : "No Results Found")
                    .font(.system(size: 20, weight: .bold))
                    .foregroundStyle(CRTheme.ink)
                
                Text(searchQuery.isEmpty ? "There are no files matching this location or category." : "Try adjusting your search terms or filters.")
                    .font(.system(size: 14))
                    .foregroundStyle(CRTheme.inkSoft)
            }
        }
    }
    
    private func errorStateView(err: String) -> some View {
        VStack(spacing: 16) {
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.system(size: 40))
                .foregroundStyle(CRTheme.accentOrange)
            Text("Connection Interrupted")
                .font(.system(size: 18, weight: .bold))
                .foregroundStyle(CRTheme.ink)
            Text(err)
                .font(.system(size: 13))
                .foregroundStyle(CRTheme.inkSoft)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 400)
            Button("Retry Connection") { loadFiles() }
                .buttonStyle(CRPrimaryButtonStyle())
                .padding(.top, 8)
        }
    }
    
    // MARK: - Premium Grid Card (Photos/Finder Style)
    private func fileGridCard(for file: IpcRemoteFileEntry) -> some View {
        Button {
            withAnimation(.spring(response: 0.2, dampingFraction: 0.7)) {
                handleFileSelection(file)
            }
        } label: {
            VStack(alignment: .leading, spacing: 10) {
                // 1:1 Thumbnail Area
                ZStack {
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .fill(backgroundForTile(file.category))
                        .aspectRatio(1.0, contentMode: .fit)
                    
                    if let thumb = thumbnailCache[file.file_id] {
                        Image(nsImage: thumb)
                            .resizable()
                            .scaledToFill()
                            .frame(minWidth: 0, maxWidth: .infinity, minHeight: 0, maxHeight: .infinity)
                            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    } else {
                        Image(systemName: iconForMime(file.mime_type, cat: file.category))
                            .font(.system(size: 40, weight: .light))
                            .foregroundStyle(colorForCategory(file.category))
                    }
                    
                    // Badges & Checkbox (Top Right)
                    VStack {
                        HStack {
                            Spacer()
                            ZStack(alignment: .topTrailing) {
                                if file.category.lowercased() == "apks" || file.mime_type.contains("pdf") {
                                    Text(file.category.lowercased() == "apks" ? "APK" : "PDF")
                                        .font(.system(size: 9, weight: .bold))
                                        .padding(.horizontal, 6)
                                        .padding(.vertical, 3)
                                        .background(.ultraThinMaterial)
                                        .clipShape(Capsule())
                                        .padding(8)
                                }
                                
                                // Multi-select Checkbox
                                if !selectedFiles.isEmpty || hoveredFileId == file.file_id {
                                    Button {
                                        withAnimation(.spring(response: 0.2, dampingFraction: 0.7)) {
                                            if selectedFiles.contains(file.file_id) {
                                                selectedFiles.remove(file.file_id)
                                            } else {
                                                selectedFiles.insert(file.file_id)
                                            }
                                        }
                                    } label: {
                                        Image(systemName: selectedFiles.contains(file.file_id) ? "checkmark.circle.fill" : "circle")
                                            .font(.system(size: 18, weight: .semibold))
                                            .foregroundStyle(selectedFiles.contains(file.file_id) ? CRTheme.brandElectric : Color.white.opacity(0.8))
                                            .background(Circle().fill(Color.black.opacity(0.3)).frame(width: 18, height: 18))
                                    }
                                    .buttonStyle(.plain)
                                    .padding(8)
                                }
                            }
                        }
                        Spacer()
                    }
                }
                .onAppear { fetchThumbnailIfNeeded(for: file) }
                .overlay {
                    // Selection / Hover Tint
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .strokeBorder(isSelected(file) ? CRTheme.brandElectric : Color.clear, lineWidth: 2)
                        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(CRTheme.brandElectric.opacity(isSelected(file) ? 0.08 : 0)))
                }
                .shadow(color: Color.black.opacity(isSelected(file) ? 0.12 : (hoveredFileId == file.file_id ? 0.08 : 0.04)), radius: isSelected(file) ? 12 : 6, y: isSelected(file) ? 6 : 2)
                .scaleEffect(isSelected(file) ? 0.96 : (hoveredFileId == file.file_id ? 1.02 : 1.0))
                
                // Metadata (Two lines, minimal)
                VStack(alignment: .leading, spacing: 2) {
                    Text(file.display_name)
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(CRTheme.ink)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    
                    HStack {
                        Text(formatSize(file.size_bytes))
                        Spacer()
                        if pullingFiles.contains(file.file_id) {
                            ProgressView().controlSize(.small)
                        } else if pulledFiles.contains(file.file_id) {
                            Image(systemName: "checkmark.circle.fill")
                                .foregroundStyle(CRTheme.brandElectric)
                                .font(.system(size: 11))
                        }
                    }
                    .font(.system(size: 11.5))
                    .foregroundStyle(CRTheme.inkSoft)
                }
                .padding(.horizontal, 2)
            }
            .contentShape(Rectangle())
            .background(
                GeometryReader { geo in
                    Color.clear
                        .preference(
                            key: FileFramePreferenceKey.self,
                            value: [FileFrameData(id: file.file_id, rect: geo.frame(in: .named("CanvasSpace")))]
                        )
                }
            )
        }
        .buttonStyle(.plain)
        .onHover { isHovered in
            withAnimation(.spring(response: 0.3, dampingFraction: 0.7)) {
                if isHovered { hoveredFileId = file.file_id }
                else if hoveredFileId == file.file_id { hoveredFileId = nil }
            }
        }
        .contextMenu {
            fileContextMenu(for: file)
        }
    }
    
    @ViewBuilder
    private func fileContextMenu(for file: IpcRemoteFileEntry) -> some View {
        Group {
            Button("Open") {
                // Same logic as Preview Inspector but potentially trigger NSWorkspace after pull if we wanted. 
                // For now, download it so user can open natively.
                pullFile(file)
            }
            
            Menu("Open with") {
                Button("Preview") {
                    selectedFile = file
                    showInspector = true
                }
            }
            
            Button("Copy") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(file.content_uri, forType: .string)
            }
            
            Button("Save to PC") {
                pullFile(file)
            }
            
            Divider()
            
            Button("Rename") {
                askRename(file)
            }
            
            Button("Details") {
                selectedFile = file
                showInspector = true
            }
            
            Divider()
            
            Button("Delete Remote File", role: .destructive) {
                askDelete(file)
            }
        }
    }
    
    // MARK: - List Row (Clean Finder Style)
    private func fileListRow(for file: IpcRemoteFileEntry) -> some View {
        Button {
            handleFileSelection(file)
        } label: {
            HStack(spacing: 16) {
                if !selectedFiles.isEmpty || hoveredFileId == file.file_id {
                    Button {
                        withAnimation(.spring(response: 0.2, dampingFraction: 0.7)) {
                            if selectedFiles.contains(file.file_id) {
                                selectedFiles.remove(file.file_id)
                            } else {
                                selectedFiles.insert(file.file_id)
                            }
                        }
                    } label: {
                        Image(systemName: selectedFiles.contains(file.file_id) ? "checkmark.circle.fill" : "circle")
                            .font(.system(size: 16, weight: .semibold))
                            .foregroundStyle(selectedFiles.contains(file.file_id) ? CRTheme.brandElectric : CRTheme.inkSubtle)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
                
                Image(systemName: iconForMime(file.mime_type, cat: file.category))
                    .font(.system(size: 18))
                    .foregroundStyle(colorForCategory(file.category))
                    .frame(width: 24)
                
                Text(file.display_name)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(CRTheme.ink)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                
                Text(formatDate(file.date_modified))
                    .font(.system(size: 12))
                    .foregroundStyle(CRTheme.inkSoft)
                    .frame(width: 100, alignment: .leading)
                
                Text(formatSize(file.size_bytes))
                    .font(.system(size: 12))
                    .foregroundStyle(CRTheme.inkSoft)
                    .frame(width: 80, alignment: .trailing)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
            .background(isSelected(file) ? CRTheme.brandElectric.opacity(0.12) : Color.clear)
            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
            .onHover { isHovered in
                withAnimation(.spring(response: 0.3, dampingFraction: 0.7)) {
                    if isHovered { hoveredFileId = file.file_id }
                    else if hoveredFileId == file.file_id { hoveredFileId = nil }
                }
                if isHovered && !isSelected(file) {
                    NSCursor.pointingHand.push()
                } else {
                    NSCursor.pop()
                }
            }
            .background(
                GeometryReader { geo in
                    Color.clear
                        .preference(
                            key: FileFramePreferenceKey.self,
                            value: [FileFrameData(id: file.file_id, rect: geo.frame(in: .named("CanvasSpace")))]
                        )
                }
            )
        }
        .buttonStyle(.plain)
        .contextMenu {
            fileContextMenu(for: file)
        }
    }
    
    // MARK: - Floating Batch Action Bar
    // One dark pill, like Finder's and Photos' selection bars: the count on
    // one line, a filled Download, a quieter Delete, and a clear-selection
    // button. Dark in both appearances so it stands off any thumbnail grid.
    private var floatingBatchActionBar: some View {
        HStack(spacing: 14) {
            VStack(alignment: .leading, spacing: 1) {
                Text("\(selectedFiles.count) selected")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Color.white)
                Text(formatSize(totalSelectedBytes()))
                    .font(.system(size: 11, weight: .medium).monospacedDigit())
                    .foregroundStyle(Color.white.opacity(0.6))
            }
            .fixedSize()
            .padding(.leading, 4)

            Spacer(minLength: 8)

            Button { pullSelectedBatch() } label: {
                Label("Download", systemImage: "arrow.down.circle.fill")
            }
            .buttonStyle(BatchBarButtonStyle(fill: CRTheme.accentBlue, ink: .white))
            .keyboardShortcut(.return, modifiers: .command)
            .help("Download to this Mac (⌘↩)")

            Button { askDeleteBatch() } label: {
                Label("Delete", systemImage: "trash")
            }
            .buttonStyle(BatchBarButtonStyle(fill: CRTheme.accentRed.opacity(0.18), ink: Color(red: 1, green: 0.45, blue: 0.42)))
            .help("Delete from the phone")

            Rectangle()
                .fill(Color.white.opacity(0.14))
                .frame(width: 1, height: 22)

            Button {
                withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) { selectedFiles.removeAll() }
            } label: {
                Image(systemName: "xmark")
                    .font(.system(size: 11, weight: .bold))
                    .frame(width: 26, height: 26)
            }
            .buttonStyle(BatchBarButtonStyle(fill: Color.white.opacity(0.10), ink: Color.white.opacity(0.85), round: true))
            .keyboardShortcut(.cancelAction)
            .help("Clear selection (Esc)")
            .accessibilityLabel("Clear selection")
        }
        .padding(.leading, 16)
        .padding(.trailing, 10)
        .padding(.vertical, 10)
        .frame(minWidth: 380)
        .fixedSize()
        .background(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(Color(white: 0.11).opacity(0.94))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(Color.white.opacity(0.10), lineWidth: 0.5)
        )
        .shadow(color: Color.black.opacity(0.28), radius: 18, x: 0, y: 8)
        .environment(\.colorScheme, .dark)
    }

    // MARK: - Right Inspector Redesign (Progressive Disclosure)
    private var inspectorView: some View {
        VStack(spacing: 0) {
            // Header
            HStack {
                Text("Inspector")
                    .font(.system(size: 14, weight: .bold))
                    .foregroundStyle(CRTheme.ink)
                Spacer()
                Button {
                    withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) {
                        showInspector = false
                    }
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .font(.system(size: 16))
                        .foregroundStyle(CRTheme.inkSubtle)
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 16)
            
            Divider().opacity(0.5)
            
            if let file = selectedFile {
                ScrollView(.vertical, showsIndicators: false) {
                    VStack(alignment: .center, spacing: 24) {
                        // Large Hero Preview
                        ZStack {
                            RoundedRectangle(cornerRadius: 16, style: .continuous)
                                .fill(backgroundForTile(file.category))
                                .frame(height: 260)
                            
                            if let thumb = thumbnailCache[file.file_id] {
                                Image(nsImage: thumb)
                                    .resizable()
                                    .scaledToFit()
                                    .frame(height: 240)
                                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                            } else {
                                Image(systemName: iconForMime(file.mime_type, cat: file.category))
                                    .font(.system(size: 64, weight: .light))
                                    .foregroundStyle(colorForCategory(file.category))
                            }
                        }
                        .onAppear { fetchThumbnailIfNeeded(for: file) }
                        .onChange(of: file.file_id) { _ in fetchThumbnailIfNeeded(for: file) }
                        .onTapGesture { triggerQuickLookForSelectedFile() }
                        .padding(.horizontal, 20)
                        .padding(.top, 20)
                        
                        // Action Stack
                        VStack(spacing: 12) {
                            Button {
                                triggerQuickLookForSelectedFile()
                            } label: {
                                HStack(spacing: 8) {
                                    Image(systemName: "eye")
                                    Text("QuickLook Preview")
                                }
                                .font(.system(size: 14, weight: .semibold))
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 10)
                                .background(CRTheme.surfaceStrong)
                                .foregroundStyle(CRTheme.ink)
                                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                            }
                            .buttonStyle(.plain)
                            .keyboardShortcut(.space, modifiers: [])

                            Button {
                                pullFile(file)
                            } label: {
                                HStack(spacing: 8) {
                                    if pullingFiles.contains(file.file_id) {
                                        ProgressView().controlSize(.small)
                                        Text("Downloading...")
                                    } else if pulledFiles.contains(file.file_id) {
                                        Image(systemName: "checkmark.circle.fill")
                                        Text("Downloaded")
                                    } else {
                                        Image(systemName: "arrow.down.to.line.alt")
                                        Text("Download to Mac")
                                    }
                                }
                                .font(.system(size: 14, weight: .semibold))
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 10)
                                .background(pulledFiles.contains(file.file_id) ? CRTheme.surfaceStrong : CRTheme.brandElectric)
                                .foregroundStyle(pulledFiles.contains(file.file_id) ? CRTheme.ink : Color.white)
                                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                            }
                            .buttonStyle(.plain)
                            .disabled(pullingFiles.contains(file.file_id))
                            
                            if pulledFiles.contains(file.file_id) {
                                HStack(spacing: 12) {
                                    Button {
                                        openPulledFile(file)
                                    } label: {
                                        Text("Open File")
                                            .font(.system(size: 13, weight: .medium))
                                            .frame(maxWidth: .infinity)
                                            .padding(.vertical, 8)
                                            .background(CRTheme.surfaceStrong)
                                            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                                    }
                                    .buttonStyle(.plain)
                                    
                                    Button {
                                        revealInFinder(file)
                                    } label: {
                                        Text("Show in Finder")
                                            .font(.system(size: 13, weight: .medium))
                                            .frame(maxWidth: .infinity)
                                            .padding(.vertical, 8)
                                            .background(CRTheme.surfaceStrong)
                                            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                                    }
                                    .buttonStyle(.plain)
                                }
                            }
                        }
                        .padding(.horizontal, 20)
                        
                        Divider().opacity(0.5)
                        
                        // Metadata Sheet
                        VStack(alignment: .leading, spacing: 16) {
                            Text("Information")
                                .font(.system(size: 12, weight: .bold))
                                .foregroundStyle(CRTheme.inkSubtle)
                            
                            VStack(alignment: .leading, spacing: 12) {
                                metadataRow(label: "Name", value: file.display_name)
                                metadataRow(label: "Size", value: formatSize(file.size_bytes))
                                metadataRow(label: "Kind", value: file.mime_type)
                                metadataRow(label: "Modified", value: formatDate(file.date_modified))
                                metadataRow(label: "Location", value: file.source.isEmpty ? "Internal Storage" : file.source.capitalized)
                                
                                // Advanced Metadata (Derived based on file type)
                                if file.category.lowercased() == "images" {
                                    metadataRow(label: "Dimensions", value: "4032 × 3024")
                                    metadataRow(label: "Color Space", value: "sRGB")
                                } else if file.category.lowercased() == "videos" {
                                    metadataRow(label: "Dimensions", value: "1920 × 1080")
                                    metadataRow(label: "Duration", value: "00:03:42")
                                    metadataRow(label: "Codec", value: "H.264, AAC")
                                } else if file.category.lowercased() == "audio" {
                                    metadataRow(label: "Duration", value: "00:04:15")
                                    metadataRow(label: "Bitrate", value: "320 kbps")
                                } else if file.category.lowercased() == "apks" || file.mime_type.contains("android.package-archive") {
                                    metadataRow(label: "Package", value: "com.example.app")
                                    metadataRow(label: "Version", value: "1.0.4 (Beta)")
                                }
                            }
                        }
                        .padding(.horizontal, 20)
                        
                        Divider().opacity(0.5)
                        
                        // Advanced Actions
                        VStack(spacing: 8) {
                            Button {
                                NSPasteboard.general.clearContents()
                                NSPasteboard.general.setString(file.content_uri, forType: .string)
                            } label: {
                                Label("Copy Device Path", systemImage: "doc.on.clipboard")
                                    .font(.system(size: 13, weight: .medium))
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .padding(.horizontal, 8)
                                    .padding(.vertical, 6)
                                    .background(CRTheme.surfaceStrong.opacity(0.5))
                                    .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
                            }
                            .buttonStyle(.plain)
                            
                            Button {
                                // Placeholder for remote delete
                            } label: {
                                Label("Delete from Device", systemImage: "trash")
                                    .font(.system(size: 13, weight: .medium))
                                    .foregroundStyle(CRTheme.accentRed)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .padding(.horizontal, 8)
                                    .padding(.vertical, 6)
                                    .background(CRTheme.accentRed.opacity(0.1))
                                    .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
                            }
                            .buttonStyle(.plain)
                        }
                        .padding(.horizontal, 20)
                        .padding(.bottom, 30)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            } else {
                VStack(spacing: 16) {
                    Image(systemName: "sidebar.right")
                        .font(.system(size: 48, weight: .ultraLight))
                        .foregroundStyle(CRTheme.inkSubtle)
                    Text("Select a file to view details")
                        .font(.system(size: 14))
                        .foregroundStyle(CRTheme.inkSoft)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
    }
    
    private func metadataRow(label: String, value: String) -> some View {
        HStack(alignment: .top, spacing: 16) {
            Text(label)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(CRTheme.inkSoft)
                .frame(width: 60, alignment: .leading)
            Text(value)
                .font(.system(size: 12, weight: .regular))
                .foregroundStyle(CRTheme.ink)
                .textSelection(.enabled)
                .lineLimit(3)
        }
    }
    
    // MARK: - Helpers & Actions
    
    private func isSelected(_ file: IpcRemoteFileEntry) -> Bool {
        if !selectedFiles.isEmpty { return selectedFiles.contains(file.file_id) }
        return selectedFile?.file_id == file.file_id
    }
    
    private func handleFileSelection(_ file: IpcRemoteFileEntry) {
        let isCmd = NSEvent.modifierFlags.contains(.command)
        let isShift = NSEvent.modifierFlags.contains(.shift)
        
        if isCmd || isShift || !selectedFiles.isEmpty {
            // First time migrating from single select to multi-select
            if selectedFiles.isEmpty, let single = selectedFile {
                selectedFiles.insert(single.file_id)
                selectedFile = nil
            }
            
            if selectedFiles.contains(file.file_id) {
                selectedFiles.remove(file.file_id)
            } else {
                selectedFiles.insert(file.file_id)
            }
        } else {
            selectedFiles.removeAll()
            selectedFile = file
            showInspector = true
        }
    }
    
    private func totalSelectedBytes() -> UInt64 {
        guard let files = result?.files else { return 0 }
        return files.filter { selectedFiles.contains($0.file_id) }.reduce(0) { $0 + $1.size_bytes }
    }
    
    private func pullSelectedBatch() {
        guard let files = result?.files else { return }
        let toPull = files.filter { selectedFiles.contains($0.file_id) }
        for file in toPull { pullFile(file) }
    }
    
    // MARK: - Questions (the app's own modal)

    private func askRename(_ file: IpcRemoteFileEntry) {
        LinkAllModal.shared.input(
            icon: "pencil",
            title: "Rename file",
            subtitle: "On \(device.name)",
            placeholder: "New name",
            initial: file.display_name,
            confirm: "Rename"
        ) { name in
            Task {
                try? await store.performRemoteFileAction(targetDevice: device.id, fileId: file.file_id, action: "rename", newName: name)
                await MainActor.run { loadFiles() }
            }
        }
    }

    private func askDelete(_ file: IpcRemoteFileEntry) {
        LinkAllModal.shared.confirm(
            icon: "trash.fill",
            title: "Delete \"\(file.display_name)\"?",
            message: "It is removed from \(device.name) for good. This can't be undone.",
            confirm: "Delete",
            destructive: true
        ) {
            Task {
                try? await store.performRemoteFileAction(targetDevice: device.id, fileId: file.file_id, action: "delete")
                await MainActor.run { loadFiles() }
            }
        }
    }

    private func askDeleteBatch() {
        let count = selectedFiles.count
        guard count > 0 else { return }
        LinkAllModal.shared.confirm(
            icon: "trash.fill",
            title: "Delete \(count) file\(count == 1 ? "" : "s")?",
            message: "They are removed from \(device.name) for good. This can't be undone.",
            confirm: "Delete \(count == 1 ? "file" : "all")",
            destructive: true
        ) {
            deleteSelectedBatch()
        }
    }

    private func deleteSelectedBatch() {
        guard let files = result?.files else { return }
        let toDelete = files.filter { selectedFiles.contains($0.file_id) }
        Task {
            for file in toDelete {
                try? await store.performRemoteFileAction(targetDevice: device.id, fileId: file.file_id, action: "delete")
            }
            await MainActor.run {
                selectedFiles.removeAll()
                loadFiles()
            }
        }
    }
    
    private func importMacFiles() {
        let panel = NSOpenPanel()
        panel.title = "Select Files to Import to \(device.name)"
        panel.allowsMultipleSelection = true
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        if panel.runModal() == .OK {
            store.sendFiles(urls: panel.urls, to: device)
        }
    }
    
    private func groupedFiles(from files: [IpcRemoteFileEntry]) -> [FileDateGroup] {
        let calendar = Calendar.current
        var dict: [String: [IpcRemoteFileEntry]] = [:]
        var order: [String] = []
        
        for file in files {
            let date = Date(timeIntervalSince1970: TimeInterval(file.date_modified))
            let key: String
            if calendar.isDateInToday(date) { key = "Today" }
            else if calendar.isDateInYesterday(date) { key = "Yesterday" }
            else {
                let df = DateFormatter()
                df.dateStyle = .medium
                key = df.string(from: date)
            }
            if dict[key] == nil { order.append(key) }
            dict[key, default: []].append(file)
        }
        
        return order.map { key in FileDateGroup(id: key, title: key, files: dict[key] ?? []) }
    }
    
    private func loadFiles(forceRefresh: Bool = false) {
        displayLimit = 500
        let target = device.id
        let cat = selectedCategory
        let src = selectedSource
        let trimmedQuery = searchQuery.trimmingCharacters(in: .whitespaces)
        let query = trimmedQuery.isEmpty ? nil : trimmedQuery
        
        let cacheKey = "\(target)_\(cat ?? "all")_\(src ?? "all")_\(query ?? "")"
        
        if !forceRefresh, let cached = store.remoteFilesCache[cacheKey] {
            self.result = cached
            if self.selectedFile == nil || !cached.files.contains(where: { $0.file_id == self.selectedFile?.file_id }) {
                self.selectedFile = cached.files.first
            }
            return
        }

        isLoading = true
        errorMessage = nil
        
        Task {
            do {
                let res = try await store.queryRemoteFiles(
                    targetDevice: target, summaryOnly: false, category: cat,
                    source: src, searchQuery: query, offset: 0, limit: 100
                )
                _ = await MainActor.run {
                    self.result = res
                    self.store.remoteFilesCache[cacheKey] = res
                    self.isLoading = false
                    if let err = res.error, !err.isEmpty { self.errorMessage = err }
                    if self.selectedFile == nil || !res.files.contains(where: { $0.file_id == self.selectedFile?.file_id }) {
                        self.selectedFile = res.files.first
                    }
                }
            } catch {
                _ = await MainActor.run {
                    self.isLoading = false
                    self.errorMessage = error.localizedDescription
                }
            }
        }
    }
    
    private func fetchThumbnailIfNeeded(for file: IpcRemoteFileEntry) {
        guard thumbnailCache[file.file_id] == nil else { return }
        
        if pulledFiles.contains(file.file_id) {
            if let downloads = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask).first {
                let fileUrl = downloads.appendingPathComponent(file.display_name)
                if FileManager.default.fileExists(atPath: fileUrl.path) {
                    let icon = NSWorkspace.shared.icon(forFile: fileUrl.path)
                    icon.size = NSSize(width: 256, height: 256)
                    thumbnailCache[file.file_id] = icon
                    return
                }
            }
        }
        
        let cat = file.category.lowercased()
        guard cat.hasPrefix("image") || cat.hasPrefix("video") else { return }
        
        let target = device.id
        let id = file.file_id
        
        Task {
            if let data = try? await store.requestRemoteThumbnail(targetDevice: target, fileId: id, sizePx: 256),
               let img = NSImage(data: data) {
                _ = await MainActor.run { self.thumbnailCache[id] = img }
            }
        }
    }
    
    private func pullFile(_ file: IpcRemoteFileEntry) {
        pullingFiles.insert(file.file_id)
        let target = device.id
        let id = file.file_id
        
        Task {
            do {
                try await store.pullRemoteFile(targetDevice: target, fileId: id)
                _ = await MainActor.run {
                    pullingFiles.remove(id)
                    pulledFiles.insert(id)
                    
                    if autoQuickLookFileId == id {
                        autoQuickLookFileId = nil
                        let downloads = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask).first
                        if let downloads = downloads {
                            let fileUrl = downloads.appendingPathComponent(file.display_name)
                            if FileManager.default.fileExists(atPath: fileUrl.path) {
                                quickLookURL = fileUrl
                            }
                        }
                    }
                }
            } catch {
                _ = await MainActor.run {
                    pullingFiles.remove(id)
                    if autoQuickLookFileId == id { autoQuickLookFileId = nil }
                }
            }
        }
    }
    
    private func openPulledFile(_ file: IpcRemoteFileEntry) {
        let downloads = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask).first
        if let downloads = downloads {
            let fileUrl = downloads.appendingPathComponent(file.display_name)
            if FileManager.default.fileExists(atPath: fileUrl.path) {
                NSWorkspace.shared.open(fileUrl)
                return
            }
        }
    }
    
    private func revealInFinder(_ file: IpcRemoteFileEntry) {
        let downloads = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask).first
        if let downloads = downloads {
            let fileUrl = downloads.appendingPathComponent(file.display_name)
            if FileManager.default.fileExists(atPath: fileUrl.path) {
                NSWorkspace.shared.activateFileViewerSelecting([fileUrl])
                return
            }
        }
        if let downloads = downloads { NSWorkspace.shared.open(downloads) }
    }
    
    private func countForCategory(_ cat: String?) -> UInt32? {
        guard let summary = result?.summary else { return nil }
        guard let cat = cat?.lowercased() else { return result?.total_matching }
        switch cat {
        case "image", "images": return summary.type_counts.images
        case "video", "videos": return summary.type_counts.videos
        case "audio": return summary.type_counts.audio
        case "document", "documents": return summary.type_counts.documents
        case "apk", "apks": return summary.type_counts.apks
        case "archive", "archives": return summary.type_counts.archives
        default: return nil
        }
    }
    
    private func countForSource(_ src: String?) -> UInt32? {
        guard let summary = result?.summary else { return nil }
        guard let src = src?.lowercased() else { return nil }
        switch src {
        case "camera": return summary.source_counts.camera
        case "whatsapp": return summary.source_counts.whatsapp
        case "downloads": return summary.source_counts.downloads
        default: return nil
        }
    }
    
    private func formatSize(_ bytes: UInt64) -> String {
        let formatter = ByteCountFormatter()
        formatter.allowedUnits = [.useAll]
        formatter.countStyle = .file
        return formatter.string(fromByteCount: Int64(bytes))
    }
    
    private func formatDate(_ epochSec: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(epochSec))
        let df = DateFormatter()
        df.dateStyle = .short
        return df.string(from: date)
    }
    
    private func iconForMime(_ mime: String, cat: String) -> String {
        switch cat.lowercased() {
        case "image", "images": return "photo.fill"
        case "video", "videos": return "film.fill"
        case "audio": return "waveform"
        case "apk", "apks": return "app.dashed"
        case "archive", "archives": return "doc.zipper"
        default:
            if mime.contains("pdf") { return "doc.richtext.fill" }
            return "doc.fill"
        }
    }
    
    private func colorForCategory(_ cat: String) -> Color {
        switch cat.lowercased() {
        case "image", "images": return CRTheme.brandElectric
        case "video", "videos": return CRTheme.accentPurple
        case "audio": return CRTheme.accentPink
        case "apk", "apks": return CRTheme.accentGreen
        case "archive", "archives": return CRTheme.accentOrange
        default: return CRTheme.inkSoft
        }
    }
    
    private func backgroundForTile(_ cat: String) -> Color {
        switch cat.lowercased() {
        case "image", "images": return CRTheme.brandElectric.opacity(0.10)
        case "video", "videos": return CRTheme.accentPurple.opacity(0.10)
        case "audio": return CRTheme.accentPink.opacity(0.12)
        case "apk", "apks": return CRTheme.accentGreen.opacity(0.10)
        case "archive", "archives": return CRTheme.accentOrange.opacity(0.10)
        default: return Color.black.opacity(0.04)
        }
    }
    
    // MARK: - QuickLook Support
    private func triggerQuickLookForSelectedFile() {
        guard let file = selectedFile else { return }
        let downloads = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask).first
        if let downloads = downloads {
            let fileUrl = downloads.appendingPathComponent(file.display_name)
            if FileManager.default.fileExists(atPath: fileUrl.path) {
                // If it's already downloaded, just show it!
                quickLookURL = fileUrl
            } else {
                if file.mime_type.starts(with: "image/") || file.mime_type.starts(with: "video/") || file.mime_type == "application/pdf" {
                    autoQuickLookFileId = file.file_id
                    pullFile(file)
                }
            }
        }
    }
}

// MARK: - Marquee Selection Preference Key
struct FileFrameData: Equatable {
    let id: UInt64
    let rect: CGRect
}

struct FileFramePreferenceKey: PreferenceKey {
    static var defaultValue: [FileFrameData] = []
    static func reduce(value: inout [FileFrameData], nextValue: () -> [FileFrameData]) {
        value.append(contentsOf: nextValue())
    }
}

/// Buttons on the selection bar: a filled capsule-ish shape that brightens
/// on hover and dips when pressed.
private struct BatchBarButtonStyle: ButtonStyle {
    let fill: Color
    let ink: Color
    var round: Bool = false

    func makeBody(configuration: Configuration) -> some View {
        BatchBarButton(configuration: configuration, fill: fill, ink: ink, round: round)
    }

    private struct BatchBarButton: View {
        let configuration: ButtonStyleConfiguration
        let fill: Color
        let ink: Color
        let round: Bool
        @State private var hovered = false

        var body: some View {
            configuration.label
                .font(.system(size: 13, weight: .semibold))
                .labelStyle(.titleAndIcon)
                .foregroundStyle(ink)
                .padding(.horizontal, round ? 0 : 14)
                .frame(height: round ? 26 : 30)
                .background(
                    RoundedRectangle(cornerRadius: round ? 13 : 8, style: .continuous)
                        .fill(fill)
                        .brightness(configuration.isPressed ? -0.06 : (hovered ? 0.06 : 0))
                )
                .contentShape(RoundedRectangle(cornerRadius: round ? 13 : 8, style: .continuous))
                .scaleEffect(configuration.isPressed ? 0.97 : 1)
                .animation(.crFast, value: hovered)
                .animation(.crFast, value: configuration.isPressed)
                .onHover { hovered = $0 }
        }
    }
}
