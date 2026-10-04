# Android Remote File Browser Integration Plan

This document outlines the architectural plan for integrating a "Remote File Browser" into Link All, allowing the desktop clients to seamlessly browse and download files/photos from a connected Android device over the local network without relying on USB cables, ADB, or cloud services.

## 1. Overview & Architecture

Link All currently operates on a "Push" model (e.g., pushing clipboard data or files to a peer). To enable remote browsing, we will introduce a **Request/Response (Pull) model** into the existing `linkall-core` protocol. 

The desktop app will act as the Client, sending requests over the established, encrypted (ChaCha20-Poly1305) mDNS tunnel. The Android app will act as the Server, querying the local file system and returning metadata or file chunks upon request.

## 2. Rust Core Protocol Extensions

We will extend the core framing protocol (likely in `linkall-core/src/network.rs` or `frames.rs`) to include new bidirectional frame types.

### New Frames:
1. **`RemoteDirListRequest`**: Sent by desktop.
   * Payload: `path: String` (e.g., `/` for root, `/DCIM/Camera` for photos).
2. **`RemoteDirListResponse`**: Sent by Android.
   * Payload: `Vec<FileMetadata>`
   * `FileMetadata` struct: `name`, `size`, `is_dir`, `modified_at`, `mime_type` (optional).
3. **`RemoteFilePullRequest`**: Sent by desktop.
   * Payload: `path: String`
4. **`RemoteFilePullResponse`**: Sent by Android.
   * Payload: Triggers the existing `FileChunk` stream back to the requester. Can also return an error if the file is unreadable.

## 3. Android Kotlin / JNI Implementation

The Android service needs to listen for these new request frames and interface with the Android OS storage APIs.

### Key Changes:
*   **Permissions**: Add `android.permission.READ_MEDIA_IMAGES`, `READ_MEDIA_VIDEO`, and optionally `MANAGE_EXTERNAL_STORAGE` (for full file system access on modern Android versions) to the `AndroidManifest.xml`.
*   **JNI Bridge**: Expose new native functions to pass the `RemoteDirListRequest` up to Kotlin.
*   **File System Querying**:
    *   For standard directories: Use `java.io.File(path).listFiles()`.
    *   For Photos/Media: Use the `MediaStore` ContentResolver API to efficiently fetch thumbnails and media metadata without scanning raw directories (which can be slow on scoped storage).
*   **Pull Handler**: When a `RemoteFilePullRequest` is received, open an `InputStream` to the requested file URI/path and feed the bytes into the existing Rust chunked transfer pipeline.

## 4. Desktop GUI & API Integration

The desktop wrappers (macOS SwiftUI, Linux GTK, Windows WinUI) require UI updates and new FFI/IPC bindings.

### FFI/IPC Additions (Rust side):
*   `linkall_request_remote_dir(peer_id, path)`
*   Callback: `on_remote_dir_response(peer_id, path, files_json)`
*   `linkall_pull_remote_file(peer_id, path)`

### Desktop UI Additions:
*   **Device Context Menu**: Add a "Browse Device" button next to connected Android peers in the mesh network list.
*   **File Explorer View**: A new window/sheet that renders the directory structure returned by the callback.
*   **Download Flow**: When a user clicks a file or photo, it triggers `linkall_pull_remote_file`. The file downloads via the existing high-speed queue and saves silently to the OS-native `Downloads` folder, notifying the user upon completion.

## 5. Security & Edge Cases

*   **Path Traversal Prevention**: The Android Kotlin side must strictly sanitize all requested paths to ensure they do not attempt to break out of allowed directories (e.g., blocking `../../data/data/app.linkall/`).
*   **Pagination / Large Directories**: Directories like `/DCIM/Camera` can contain tens of thousands of items. The `RemoteDirListResponse` should ideally support pagination or streaming to prevent memory exhaustion and timeout drops during the handshake.
*   **Timeouts**: If the Android device is asleep and takes too long to query `MediaStore`, the desktop client must have a sensible timeout and retry mechanism so the UI doesn't freeze.
