# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 1.2.x   | ✅ Yes    |
| < 0.1   | ❌ No     |

---

## Reporting a Vulnerability

**Please do not file public GitHub issues for security vulnerabilities.**

Email: **security@linkall.example** (replace with real address before shipping)
PGP key: published at `https://linkall.example/security.asc`

We aim to:
- Acknowledge receipt within **48 hours**
- Confirm or deny the vulnerability within **7 days**
- Issue a patch within **30 days** for confirmed vulnerabilities
- Credit reporters in the release notes (unless anonymity is requested)

---

## Threat Model

Link All syncs clipboard content across devices on the same LAN.
Understanding what it protects against — and what it does not — is essential
for evaluating its security posture.

### In scope (Link All defends against these)

| Threat | Mitigation |
|--------|-----------|
| **Network eavesdropping** | All clipboard content is encrypted with AES-256-GCM (256-bit key). A passive observer on the LAN sees only ciphertext. |
| **Man-in-the-middle on first connection** | TOFU / PIN-based pairing. The PIN is derived from the X25519 shared secret via HKDF, so a MITM attacker produces a different PIN. A user who visually compares PINs detects the attack. |
| **Replay attacks** | Session frames use a monotonically increasing per-session nonce counter. Out-of-order or replayed frames are rejected. |
| **Rogue device on LAN** | Unknown devices are rejected until the user approves them (TOFU prompt / PIN confirmation). The device's key fingerprint is pinned after first approval; any key change causes an error. |
| **Fingerprint substitution after trust** | The trust store records the SHA-256 of the peer's long-term identity public key. A reconnecting device presenting a different key triggers a security error and the session is terminated. |
| **Oversized payload denial-of-service** | Frame size is hard-limited (40 MB). Content filter enforces `max_payload_bytes` (default 10 MB). Rate limiter caps pushes per second per peer. |
| **Extension-based malware delivery** | The extension block-list rejects `.exe`, `.bat`, `.ps1`, `.sh`, and other executable types. |

### Out of scope (Link All does NOT protect against these)

| Threat | Reasoning |
|--------|-----------|
| **Malicious device you have already trusted** | After trust is established, a compromised peer can push arbitrary clipboard content to your devices. Revoke compromised devices via `linkall-cli devices revoke`. |
| **Physical access to your device** | If an attacker can read the trust store (`trust.json`, mode 0600) or the running process memory, they can impersonate trusted devices. Full-disk encryption is assumed. |
| **Network-level MITM after trust** | The current implementation does not re-verify the peer's long-term identity on every session beyond fingerprint pinning. A sophisticated MITM who can re-use the pinned fingerprint is not defended against. Future versions will add certificate-pinned long-term identity keys. |
| **Local privilege escalation** | If an attacker gains OS-level access to your machine, they can read clipboard content directly. Link All does not add any protection beyond what the OS provides. |
| **Metadata leakage** | Device names and mDNS service records are unencrypted. Observers on the LAN can see which devices are running Link All and their names. Only clipboard *content* is encrypted. |
| **Clipboard content sensitivity analysis** | The optional `sensitive_text` filter is heuristic and will miss many patterns. Do not rely on it to prevent syncing of passwords or keys. |
| **Wide-area network exposure** | Link All is designed for LAN use only. If you expose port 47823 to the internet (e.g. via port forwarding), you are outside the threat model. Firewall rules should restrict the port to the local subnet. |

---

## Cryptographic Primitives

| Primitive | Algorithm | Key size | Notes |
|-----------|-----------|----------|-------|
| Key exchange | X25519 ECDH | 256-bit | Ephemeral per session (forward secrecy) |
| Key derivation | HKDF-SHA256 | — | IKM = ECDH shared secret; context = "linkall-v1-session" |
| Symmetric encryption | AES-256-GCM | 256-bit | nonce = 96-bit counter |
| PIN derivation | HKDF-SHA256 | — | IKM = ECDH shared secret; context = "deskdrop-pin" |
| Key fingerprint | SHA-256 | 256-bit | Of the peer's ephemeral public key bytes |
| Random number generation | OS CSPRNG | — | via `rand::rngs::OsRng` |

All cryptographic code uses audited Rust crates:
- `x25519-dalek 2.x` — X25519 key exchange
- `hkdf 0.12` / `sha2 1.x` — HKDF-SHA256
- `aes-gcm 0.10` — AES-256-GCM AEAD

---

## Known Weaknesses (planned mitigations)

1. **mDNS device name is unencrypted** — Device names in mDNS TXT records
   are visible to all LAN participants. **Planned**: use opaque UUIDs in
   mDNS and reveal the friendly name only post-handshake.

2. **IPC socket has no authentication** — The Unix socket is mode 0600 so
   only the owning user can connect, but there is no token-based auth.
   On shared machines with `sudo` access this is a risk. **Planned**: HMAC
   challenge–response using a per-run secret in the daemon's environment.

---

## Dependency Security

We run `cargo audit` on every CI push and weekly via a scheduled workflow.
Known-vulnerable dependency versions are blocked from merging via the CI
security audit job.

To audit your local build:
```bash
cargo install cargo-audit
cargo audit
```
