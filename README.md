# Aetheris Client

High-performance, WASM-powered browser client runtime — featuring a deterministic three-worker execution model and WebGPU rendering.

## Browser-Native Performance

**Aetheris Client** brings authoritative multiplayer to the browser without compromising on frame times. By leveraging a native three-worker architecture, the client isolates networking (IO), simulation (Logic), and rendering (Visuals) into parallel streams. This ensures that even during heavy network spikes or complex simulation updates, the rendering thread maintains a butter-smooth 60Hz experience using modern WebGPU pipelines.

> **[Read the Client Design Document](CLIENT_DESIGN.md)** — three-worker architecture and WASM integration.
>
> 🚀 **Latest Milestone:** **Renderer Hardening (M1011) Complete!** Successfully integrated the Wireframe Debug Pass into the main rendering pipeline.

[![CI](https://github.com/garnizeh-labs/aetheris-client/actions/workflows/ci.yml/badge.svg)](https://github.com/garnizeh-labs/aetheris-client/actions/workflows/ci.yml)
[![Rust Version](https://img.shields.io/badge/rust-1.94%2B-blue.svg?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

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
| `just check` | **Quality** | Complete PR validation: Linters, tests, and WASM build check. |
| `just wasm` | **Build** | Compile the Rust core to WASM (Thread-ready). |
| `just dev` | **Flow** | Start the Vite development server for the playground. |

For a full list of commands, run `just --list`.

## Documentation Entry Points

- **[CLIENT_DESIGN.md](CLIENT_DESIGN.md):** Worker architecture and state synchronization.
- **[INPUT_PIPELINE_DESIGN.md](INPUT_PIPELINE_DESIGN.md):** Client-side prediction and input gathering.
- **[WORKER_COMMUNICATION_DESIGN.md](WORKER_COMMUNICATION_DESIGN.md):** Thread safety and zero-copy message passing.

## Design Philosophy

1. **Worker Isolation:** Logic and I/O never block the rendering frame budget.
2. **Type Safety:** Shared types between Rust (WASM) and TypeScript (Glue) via `wasm-bindgen`.
3. **Hardware Forward:** Built for WebGPU and WebTransport from the ground up.

---
License: MIT / Apache-2.0
