# aetheris-client-wasm

The WASM browser client runtime for the Aetheris multiplayer platform.

## Overview

`aetheris-client-wasm` is a WebAssembly library that runs entirely in the browser, powering the Aetheris game client. It uses **Web Workers** for multi-threaded execution, **WebGPU** for hardware-accelerated rendering, and **WebTransport** for low-latency real-time networking.

## Architecture Highlights

- **Multi-threaded Workers**: Game logic, rendering, and networking each run on dedicated Web Workers via `wasm-bindgen` thread support (SharedArrayBuffer).
- **WebGPU Renderer**: Hardware-accelerated 2D/3D rendering via `wgpu` with a custom star-field, debug overlay, and theme system.
- **WebTransport Networking**: Low-latency UDP-like datagrams over HTTP/3 (`WebTransportBridge`) connected to the Aetheris Engine.
- **gRPC Auth**: WASM-compatible authentication via `tonic-web-wasm-client` (no `tonic::transport` dependency).
- **Zero-Copy State Sync**: Shared world state between workers via `SharedArrayBuffer` and `Mutex`-guarded queues.
- **Observability**: Structured tracing via `tracing-wasm` and in-browser metrics flushed over HTTP.

## Usage

Build the WASM package:

```bash
just wasm        # Release build
just wasm-dev    # Debug build (faster iteration)
```

Run the playground:

```bash
just playground  # Isolated sandbox (no server)
just dev         # Full dev session with Vite
```

For more details, see the [Client Design Document](../../docs/CLIENT_DESIGN.md).

---

License: MIT / Apache-2.0
