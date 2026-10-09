# Resumable File Transfers Architecture

Link All handles arbitrary file transfers alongside clipboard text using a highly robust, resumable pipeline designed to survive network partitions and flaky Wi-Fi.

## 1. Chunked Pipelining (`chunked.rs`)

Instead of attempting to stream large files as single, massive packets—which would tie up the TCP tunnel and block small clipboard text—Link All utilizes a **Chunked File Pipeline**.

- **File Disassembly**: Large files are broken into `FILE_CHUNK_SIZE` byte slices (typically 256 KB).
- **Interleaving**: These `FileChunk` packets can be safely interleaved with `ClipboardPush` or `BatteryStatus` telemetry packets, preventing Head-of-Line (HOL) blocking.
- **Strict Bounding**: To prevent resource exhaustion (RAM/Disk filling attacks), payloads are strictly capped at 512 MB per file transfer. Any peer attempting to advertise a total size `> 512MB` in the `Start` frame is immediately disconnected.

## 2. Adaptive Sizing & Link Quality (`probe.rs`)

Network bandwidth fluctuates rapidly in mobile environments (e.g., walking between access points).

- **Latency Probes**: The engine periodically fires lightweight ping/pong probes to measure Round-Trip Time (RTT).
- **Dynamic Optimization**: If link quality degrades (`degraded_from()`), Link All scales back the number of in-flight file chunks permitted in the TCP send buffer. When latency drops, the buffer is increased to maximize throughput.

## 3. Resumability (`file_transfer.rs`)

If a connection drops completely during a large transfer:

1. **State Preservation**: The receiver persists the `last_confirmed_chunk` index to disk alongside the partial temporary file.
2. **Reconnection Handshake**: When the peer auto-reconnects, it queries the receiver's state.
3. **Resumption**: The sender resumes streaming starting exactly at `last_confirmed_chunk + 1`, preventing wasted bandwidth.

## 4. Integrity and Security

Link All considers file reception to be high-risk due to potential path traversal and malware vectors.

- **Pre-Transfer Checksum**: The `Start` frame includes a full SHA-256 hash computed by the sender.
- **Path Traversal Shielding (`MED-04`)**: Senders are not allowed to specify arbitrary target paths. All incoming files are stripped of `../` characters and saved strictly into the native OS root `Downloads` folder.
- **Final Validation**: Once all chunks are reassembled, the receiver runs a SHA-256 hash on the complete temporary file. If the hash does not match the `Start` frame signature, the temporary file is immediately purged and the UI displays a corruption error.
