# Aetheris Client

High-performance, WASM-powered browser client runtime — featuring a deterministic three-worker execution model and WebGPU rendering.

## Browser-Native Performance

**Aetheris Client** brings authoritative multiplayer to the browser without compromising on frame times. By leveraging a native three-worker architecture, the client isolates networking (IO), simulation (Logic), and rendering (Visuals) into parallel streams. This ensures that even during heavy network spikes or complex simulation updates, the rendering thread maintains a butter-smooth 60Hz experience using modern WebGPU pipelines.

> [!IMPORTANT]
> 🚀 **Current State:** **Milestone M1020** — Ship Classes & ECS Synchronization (Implemented).
> 
> Features introduced in this phase:
> - **Performance Observability:** Integrated M10105 lifecycle events and performance polling for WASM execution metrics.
> - **State Reconciliation:** Updated `world_state.rs` to reconcile new protocol types (`ShipStats`, `Loadout`, `ShipClass`).
> - **Stability Fixes:** Improved `game.worker.ts` with async-await for WASM calls and robust reconnection logic.

[![Build Status](https://github.com/garnizeh-labs/aetheris-client/actions/workflows/ci.yml/badge.svg)](https://github.com/garnizeh-labs/aetheris-client/actions)
[![Crates.io](https://img.shields.io/crates/v/aetheris-client.svg)](https://crates.io/crates/aetheris-client)
[![Docs.rs](https://docs.rs/aetheris-client/badge.svg)](https://docs.rs/aetheris-client)
[![Rust Version](https://img.shields.io/badge/rust-1.95.0%2B-blue.svg?logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT%2FApache--2.0-green.svg)](LICENSE-MIT)
[![Convention Commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-yellow.svg)](https://conventionalcommits.org)

## Workspace Components

The client is split into focused crates for lean WASM builds and clear native/browser isolation:

- **[`aetheris-client-wasm`](crates/aetheris-client-wasm)**: The browser runtime. Implements the simulation loop, WebGPU renderer, and WebTransport networking as a SharedArrayBuffer-backed WASM module.
- **[`aetheris-client-native`](crates/aetheris-client-native)**: The native runtime stub. Provides the same trait surface for desktop and server-side headless clients.

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
