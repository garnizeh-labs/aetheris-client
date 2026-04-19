# Aetheris Client

High-performance, WASM-powered browser client runtime — featuring a deterministic three-worker execution model and WebGPU rendering.

## Browser-Native Performance

**Aetheris Client** brings authoritative multiplayer to the browser without compromising on frame times. By leveraging a native three-worker architecture, the client isolates networking (IO), simulation (Logic), and rendering (Visuals) into parallel streams. This ensures that even during heavy network spikes or complex simulation updates, the rendering thread maintains a butter-smooth 60Hz experience using modern WebGPU pipelines.

> **[Read the Client Design Document](docs/CLIENT_DESIGN.md)** — three-worker architecture and WASM integration.
>
> 🚀 **Latest Milestone:** **Hardening & Standardization (M1011) Complete!** Atomic TOCTOU fix in SharedWorld, per-face normals in the renderer, and full CI standardization across the Aetheris platform.

[![CI](https://github.com/garnizeh-labs/aetheris-client/actions/workflows/ci.yml/badge.svg)](https://github.com/garnizeh-labs/aetheris-client/actions/workflows/ci.yml)
[![Rust Version](https://img.shields.io/badge/rust-1.95.0%2B-blue.svg?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Conventional Commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-yellow.svg)](https://conventionalcommits.org)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg?style=flat-square)](https://github.com/garnizeh-labs/aetheris-client/pulls)

## Workspace Components

The client is split into focused crates for lean WASM builds and clear native/browser isolation:

- **[`aetheris-client-wasm`](crates/aetheris-client-wasm)**: The browser runtime. Implements the simulation loop, WebGPU renderer, and WebTransport networking as a SharedArrayBuffer-backed WASM module.
- **[`aetheris-client-native`](crates/aetheris-client-native)**: The native runtime stub. Provides the same trait surface for desktop and server-side headless clients.

## Quickstart

```bash
# 1. Install dependencies
npm install --prefix playground

# 2. Build WASM core
just wasm

# 3. Start development server
just dev
```

### 🛠️ Common Tasks

| Command | Category | Description |
| :--- | :--- | :--- |
| `just check` | **Quality** | Complete PR validation: fmt, clippy, tests, security audit, and doc lint. |
| `just check-all` | **CI** | Full validation: includes `udeps` and strict rustdoc checks. |
| `just wasm` | **Build** | Compile the Rust core to WASM (thread-ready, atomics enabled). |
| `just wasm-dev` | **Build** | Debug WASM build for faster iteration. |
| `just dev` | **Flow** | Start the Vite development server (connected mode). |
| `just playground` | **Flow** | Start the Vite development server (isolated sandbox mode). |
| `just fix` | **Lint** | Automatically formats code and applies non-breaking clippy fixes. |

For a full list of commands, run `just --list`.

## The Three-Worker Architecture

Aetheris Client resolves around three isolated workers that communicate via SharedArrayBuffer:

1. **Game Worker** — Runs the WASM simulation core at 60Hz. Owns the physics tick, entity state, and all game logic.
2. **Render Worker** — Reads the lock-free double-buffer from the Game Worker via SAB and drives the WebGPU render pipeline. Never blocks on game logic.
3. **Main Thread** — Owns the DOM, input capture, and lifecycle. Routes messages between workers without touching simulation state.

## Documentation Index

- **[CLIENT_DESIGN.md](docs/CLIENT_DESIGN.md):** Worker architecture and state synchronization.
- **[INPUT_PIPELINE_DESIGN.md](docs/INPUT_PIPELINE_DESIGN.md):** Client-side prediction and input gathering.
- **[WORKER_COMMUNICATION_DESIGN.md](docs/WORKER_COMMUNICATION_DESIGN.md):** Thread safety and zero-copy message passing.
- **[ASSET_STREAMING_DESIGN.md](docs/ASSET_STREAMING_DESIGN.md):** Progressive asset loading and streaming strategy.
- **[PLAYGROUND_DESIGN.md](docs/PLAYGROUND_DESIGN.md):** Isolated sandbox architecture for local development and QA.

## Design Philosophy

1. **Worker Isolation:** Logic and I/O never block the rendering frame budget.
2. **Type Safety:** Shared types between Rust (WASM) and TypeScript (Glue) via `wasm-bindgen`.
3. **Hardware Forward:** Built for WebGPU and WebTransport from the ground up.
4. **Lock-Free Replication:** Entity state is published via a single atomic `Release` store into a SharedArrayBuffer double-buffer, eliminating all TOCTOU races between simulation and rendering.

---

License: MIT / Apache-2.0
