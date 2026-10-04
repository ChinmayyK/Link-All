# Contributing to Link All

Thank you for your interest in improving Link All!
This document covers how to set up the development environment, project conventions, and the PR process.

---

## Development Setup

### Prerequisites

| Tool | Version | Install |
|------|---------|---------|
| Rust | stable  | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| Xcode CLI | 15+ | `xcode-select --install` (macOS only) |
| .NET SDK | 8.0+ | https://dotnet.microsoft.com/download |
| Android NDK | 26+ | via Android Studio SDK Manager |
| cargo-ndk | latest | `cargo install cargo-ndk` |
| cargo-audit | latest | `cargo install cargo-audit` |

### Clone and build

```bash
git clone https://github.com/ChinmayyK/Link-All
cd Link-All

# Core library + daemon + CLI
cargo build

# Run unit tests
cargo test

# Run benchmarks (HTML report in target/criterion/)
cargo bench
```

### Run the daemon locally

```bash
DESKDROP_LOG=debug cargo run --bin deskdrop-daemon -- --name "Dev Machine" --port 47823
```

Then in another terminal:

```bash
cargo run --bin deskdrop-cli -- status
cargo run --bin deskdrop-cli -- ping
```

---

## Project Layout

```
deskdrop/
├── deskdrop-core/        # Rust engine — all platform-independent logic
│   └── src/
│       ├── protocol.rs     # Wire types
│       ├── crypto.rs       # X25519 + HKDF + ChaCha20-Poly1305
│       ├── trust.rs        # TOFU trust store
│       ├── pairing.rs      # PIN-based pairing
│       ├── discovery.rs    # mDNS-SD
│       ├── network.rs      # TCP transport + handshake
│       ├── engine.rs       # Orchestrator
│       ├── chunked.rs      # Large payload streaming
│       ├── dedup.rs        # Echo suppression + rate limiting
│       ├── filter.rs       # Content filter chain
│       ├── throttle.rs     # Bandwidth throttle
│       ├── retry.rs        # Exponential back-off
│       ├── history.rs      # Clipboard history ring buffer
│       ├── metrics.rs      # Latency + throughput stats
│       ├── settings.rs     # User configuration
│       ├── ipc.rs          # CLI ↔ daemon Unix socket
│       ├── sim.rs          # In-process test harness
│       ├── ffi.rs          # C ABI exports (macOS / Windows)
│       └── jni_android.rs  # JNI exports (Android)
├── deskdrop-cli/         # Management CLI tool
├── platforms/
│   ├── macos/              # Swift menu-bar app
│   ├── windows/            # C# tray app
│   ├── android/            # Kotlin foreground service
│   └── linux/              # GTK4 / headless daemon
└── scripts/                # Build automation
```

---

## Conventions

### Rust

- **Format**: `cargo fmt` before committing. CI will reject unformatted code.
- **Lint**: `cargo clippy -- -D warnings`. Fix all warnings; don't add `#[allow]` without justification.
- **Tests**: every public function should have at least one test. Use `#[cfg(test)]` modules in the same file.
- **Documentation**: every public type and function must have a `///` doc comment.
- **Error handling**: use `anyhow::Result` in binaries and tests; `thiserror` for library error types.
- **Panics**: avoid `unwrap()` and `expect()` in non-test code unless the invariant is truly guaranteed. Document why.
- **Unsafe**: absolutely no `unsafe` without a `// SAFETY:` comment explaining every invariant.

### Platform code (Swift / Kotlin / C#)

- Keep platform layers thin — they should only translate between the OS clipboard API and `deskdrop_*` FFI calls. Business logic belongs in the Rust core.
- Follow the platform's naming conventions (Swift: camelCase types; Kotlin: PascalCase classes; C#: PascalCase).
- Handle all FFI call failures gracefully — null handles, error codes, etc.

### Commits and PRs

- **Commit messages**: `<scope>: <short description>` (e.g. `crypto: add HKDF test vectors`)
- **PR title**: same format as commit messages
- **PR body**: describe *what* and *why*, not just *how*. Link related issues.
- **One logical change per PR** — split unrelated changes into separate PRs.
- **Tests required**: new features must include unit tests. Bug fixes must include a regression test.

---

## Adding a New Platform

1. Create `platforms/<name>/` directory.
2. Load `libdeskdrop_core` (`.dylib`, `.dll`, `.so`) appropriate for the OS.
3. Call `deskdrop_start()` to get a handle.
4. Implement:
   - Outgoing: watch the OS clipboard → call `deskdrop_push_text/image/file()`
   - Incoming: poll `deskdrop_poll_event()` at 20–50 ms → apply to clipboard
   - TOFU: on `PB_EVENT_TOFU_PROMPT`, show a dialog with the fingerprint
5. Call `deskdrop_stop()` on clean shutdown.
6. Add the platform to `release.yml` so CI builds it.

---

## Running the Full Test Suite

```bash
# Unit + doc tests (fast, no network)
cargo test --lib --doc

# Integration tests (requires mDNS on loopback; Linux/macOS)
cargo test --tests

# Benchmarks (compile check only, no execution — safe for CI)
cargo bench --no-run

# Security audit
cargo audit

# Generate SBOM
cargo install cargo-cyclonedx
cargo cyclonedx --format json
```

---

## Release Process

1. Update `CHANGELOG.md` — move `[Unreleased]` items to a new version section.
2. Bump version in `deskdrop-core/Cargo.toml` and `deskdrop-cli/Cargo.toml`.
3. Commit: `chore: release v0.x.y`
4. Tag: `git tag v0.x.y && git push --tags`
5. The `release.yml` workflow builds all platform artifacts and creates a GitHub Release automatically.

---

## Code of Conduct

Be excellent to each other. We follow the [Contributor Covenant](https://www.contributor-covenant.org/) v2.1.
