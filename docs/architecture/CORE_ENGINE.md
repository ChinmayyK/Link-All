# Link All Core Engine Architecture

This document describes the primary components of `linkall-core`, the shared Rust backend responsible for networking, peer lifecycle management, deduplication, and cross-process communication.

## 1. Discovery and Networking

Link All operates strictly on local networks (LAN / VPN) using a serverless peer-to-peer mesh architecture.

### mDNS Discovery (`discovery.rs`)
Nodes locate one another using Multicast DNS (mDNS). 
- **Service Type**: `_deskdrop._tcp.local.`
- **Privacy Mode (`TRU-06`)**: Link All does not broadcast human-readable device names in plain-text TXT records. Only an opaque device UUID and a supported protocol version string are published.
- **Address Selection**: The engine prioritizes connections systematically: IPv4 > Global IPv6 > Link-Local IPv6 (with valid `scope_id`). This mitigates silent connect failures.
- **Pruning**: The engine actively parses the `v=` TXT record and ignores fundamentally incompatible protocol versions before ever attempting a TCP handshake.

### Network Management (`network.rs` & `network_manager.rs`)
Once discovered, Link All establishes a framed TCP connection.
- **Timeouts & Keepalives**: All outgoing connections utilize a strict 5-second TCP connect timeout (`tokio::time::timeout`) and enable `SO_KEEPALIVE` (idle 30s, interval 5s, 3 probes).
- **Latency Optimization**: Socket `TCP_NODELAY` is enforced on both inbound and outbound sockets to remove the ~40ms Nagle algorithm delay.

## 2. Peer Lifecycle (`peer_manager.rs`)

Link All completely abstracts the mesh logic through a five-layer lifecycle model tracking the status of each discovered or known peer. 

```rust
pub struct PeerRecord {
    pub id: Uuid,                    // Internal routing only
    pub friendly_name: String,       // Human-readable canonical name
    pub platform: Option<String>,    // OS identifier (macOS, Android, etc.)
    
    // Lifecycle Flags
    pub trusted: bool,               // Layer 1: Cryptographic approval
    pub remembered: bool,            // Layer 2: Persistent pairing
    pub status: PeerConnectionState, // Layer 3: Runtime connection
    pub sync_enabled: bool,          // Layer 4: Clipboard data flows
    pub auto_connect: bool,          // Layer 5: Reconnect on startup
}
```

### Auto-Reconnection Logic
When the core engine starts or network interfaces change, the `engine.rs` attempts to automatically reconnect to peers that meet the specific criteria: `trusted && remembered && auto_connect`.

## 3. Mesh-Aware Deduplication (`dedup.rs`)

Because Link All forms a fully connected mesh network (A connects to B and C; B connects to A and C), clipboard synchronizations can easily trigger infinite feedback loops (echoes).

To solve this, Link All implements **Mesh-Aware Deduplication**:
1. **Hash Registry**: Tracks recent clipboard `ContentHash` signatures with an active Time-To-Live (TTL) eviction policy.
2. **Peer Windows**: Maintains a `HashMap<Uuid, PeerWindow>` tracking what payload hash was sent/received from which specific peer within a 5-second sliding window.
3. **Echo Suppression**: The `should_apply(from_peer, hash)` method checks the peer windows. If peer B sends a hash that A just recently broadcast to the mesh, A silently drops it, suppressing the echo without disrupting new local copies.

## 4. IPC & CLI Interfaces (`ipc.rs`)

The background daemon exposes a local UNIX Domain Socket (macOS/Linux) or Named Pipe (Windows) for platform UI wrappers and the `linkall-cli` to interact with.

**Commands include:**
- `PauseSyncPeer { device_id }` / `ResumeSyncPeer { device_id }`
- `ForgetDevice { device_id }`
- `SetAutoConnect { device_id, enabled }`
- `GetMetrics` and `GetHistory`
