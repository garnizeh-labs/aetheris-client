<div align="center">
  <h1>Aetheris Client</h1>
  <p>High-performance, WASM-powered browser client runtime — featuring a deterministic three-worker execution model and WebGPU rendering.</p>

  [![CI](https://img.shields.io/github/actions/workflow/status/garnizeh-labs/aetheris-client/ci.yml?branch=main&style=flat-square&logo=github&label=CI)](https://github.com/garnizeh-labs/aetheris-client/actions)
  [![Rust Version](https://img.shields.io/badge/rust-1.95.0%2B-blue?style=flat-square&logo=rust)](https://www.rust-lang.org/)
  [![Conventional Commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-yellow.svg?style=flat-square)](https://conventionalcommits.org)
  [![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg?style=flat-square)](https://github.com/garnizeh-labs/aetheris-client/pulls)
</div>

---

## Browser-Native Performance

**Aetheris Client** brings authoritative multiplayer to the browser without compromising on frame times. By leveraging a native three-worker architecture, the client isolates networking (IO), simulation (Logic), and rendering (Visuals) into parallel streams. This ensures that even during heavy network spikes or complex simulation updates, the rendering thread maintains a butter-smooth 60Hz experience using modern WebGPU pipelines.

> [!IMPORTANT]
> 🚀 **Current State:** **VS-05 (Playground Input) complete!** Protocol v0.2.11 & Authoritative Input Pipeline.
>
> Features introduced in this phase:
>
> - **Protocol v0.2.11:** System Manifest (extensible metadata) and Possession flow.
> - **Input Pipeline:** Key-state forwarding (WASD+F+Space) with noise reduction.
> - **Quality Gate Passing:** `just check` and `just wasm` green for VS-05 scope.
>
### 📦 Workspace Components

| Crate | Link | Documentation |
| :--- | :--- | :--- |
| **`aetheris-client-wasm`** | [![Crates.io](https://img.shields.io/crates/v/aetheris-client-wasm?style=flat-square)](https://crates.io/crates/aetheris-client-wasm) | [![Docs.rs](https://img.shields.io/docsrs/aetheris-client-wasm?style=flat-square&logo=docs.rs&label=docs)](https://docs.rs/aetheris-client-wasm) |
| **`aetheris-client-native`** | [![Crates.io](https://img.shields.io/crates/v/aetheris-client-native?style=flat-square)](https://crates.io/crates/aetheris-client-native) | — |

---

## Workspace Components

The client is split into focused crates for lean WASM builds and clear native/browser isolation:

- **[`aetheris-client-wasm`](crates/aetheris-client-wasm)**: The browser runtime. Implements the simulation loop, WebGPU renderer, and WebTransport networking as a SharedArrayBuffer-backed WASM module.
- **`aetheris-client-native`**: The native runtime stub. Provides the same trait surface for desktop and server-side headless clients.

## Quickstart

```bash
# 1. Build WASM core and asset bundle
just wasm

# 2. Run quality gate checks (fmt, clippy, tests, security, docs)
just check

# 3. Start development server (connected or sandbox mode)
just dev # or 'just playground'
```

### 🛠️ Common Tasks

| Command | Category | Description |
| :--- | :--- | :--- |
| `just check` | **Quality** | Complete PR validation: fmt, clippy, tests, security audit, and doc lint. |
| `just wasm` | **Build** | Compile the Rust core to WASM (thread-ready, atomics enabled). |
| `just dev` | **Flow** | Start the Vite development server (connected mode). |
| `just playground` | **Flow** | Start the Vite development server (isolated sandbox mode). |

## The Three-Worker Architecture

1. **Game Worker** — Runs the WASM simulation core at 60Hz. Owns the physics tick, entity state, and game logic.
2. **Render Worker** — Reads the lock-free double-buffer via SAB and drives the WebGPU render pipeline.
3. **Main Thread** — Owns the DOM, input capture, and lifecycle. Routes messages between workers.

## Documentation Index

- **[CLIENT_DESIGN.md](docs/CLIENT_DESIGN.md):** Worker architecture and state synchronization.
- **[INPUT_PIPELINE_DESIGN.md](docs/INPUT_PIPELINE_DESIGN.md):** Client-side prediction and input gathering.
- **[ASSET_STREAMING_DESIGN.md](docs/ASSET_STREAMING_DESIGN.md):** Progressive asset loading and streaming.
- **[PLAYGROUND_DESIGN.md](docs/PLAYGROUND_DESIGN.md):** Isolated sandbox architecture for rapid QA.

---

License: MIT / Apache-2.0
