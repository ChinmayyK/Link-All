# Security & Protocol Architecture

This document covers the end-to-end encryption pipeline, protocol framing, and zero-trust device pairing logic used by Link All.

## 1. Protocol Framing (`protocol.rs`)

Communication between peers occurs over a persistent TCP tunnel using a strict, length-prefixed binary framing protocol. 

### The Handshake (`HelloFrame` & `HelloAckFrame`)
1. Upon connecting, the initiator sends a `HelloFrame`.
2. This frame contains the initiator's public ephemeral X25519 ECDH key, a protocol version, and a serialized `DeviceMetadata` JSON string (containing the human-readable `friendly_name` and OS platform).
3. The receiver responds with a `HelloAckFrame` containing its own ephemeral X25519 key and metadata.
4. If this is a new device, a Trust/Pairing prompt is queued to the UI.

### Encrypted Frames
Once the handshake is completed, all subsequent frames (e.g., `ClipboardPush`, `FileChunk`, `CallStateUpdate`) are wrapped in an `EncryptedFrame` structure.

## 2. Cryptographic Pipeline (`crypto.rs`)

Link All uses state-of-the-art cryptographic primitives to ensure data in transit is immune to eavesdropping.

- **Key Exchange**: Standard Curve25519 Elliptic Curve Diffie-Hellman (ECDH) is used to establish a shared secret during the handshake.
- **Key Derivation**: The raw Diffie-Hellman secret is passed through HKDF-SHA256 alongside specific salt contexts to derive **two directional** symmetric session keys — one for initiator→responder traffic, one for responder→initiator traffic (domain-separated by distinct HKDF info strings). Each side encrypts with one key and decrypts with the other. A single shared key for both directions would let each side's independent, counter-based nonce collide with the other's at the same counter value, which breaks AES-GCM catastrophically (the "forbidden attack" nonce-reuse case) — this is why the two directions never share a key.
- **Zeroization (`CRIT-02`)**: The memory containing the raw Diffie-Hellman shared secret is explicitly zeroized (zero-filled) in RAM immediately after HKDF expansion, preventing cold-boot and memory dump extraction.
- **Symmetric Encryption**: AES-256-GCM Authenticated Encryption with Associated Data (AEAD) is used to encrypt and authenticate all payloads.

### Replay Protection
Link All prevents captured packets from being replayed back to a client by using a **strictly monotonic, 64-bit big-endian counter**. This counter acts as the ChaCha20 nonce. If a packet arrives with a counter less than or equal to the highest counter seen, the packet is instantly dropped, terminating the connection.

## 3. Trust Models & Pairing (`pairing.rs` & `trust.rs`)

Link All supports two methods for authenticating devices.

### Trust On First Use (TOFU)
For low-friction environments, a user can simply click "Accept" when a new device initiates a connection. The permanent 32-byte X25519 identity key (`identity.json`) of the peer is permanently saved in the local `trust.json` registry. The UUID is only ever shown in a fingerprint confirmation dialog to prevent UI clutter.

### PIN-Based Pairing
To mitigate active Man-in-the-Middle (MITM) attacks where an attacker intercepts the `HelloFrame`, Link All implements a commutative numeric PIN system.
1. Both devices independently compute: `PIN = HKDF-SHA256(shared_secret, "deskdrop-pin") mod 10^6`
2. The PIN is displayed as a 6-digit number, split for readability (e.g., `048 291`).
3. Since both sides compute the exact same mathematical PIN derived from the shared secret, the user simply verifies the numbers match on both screens. If an attacker intercepts the handshake, their independent ECDH keys will result in drastically different PINs, exposing the MITM attack.
