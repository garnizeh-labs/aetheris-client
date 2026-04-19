# aetheris-client-native

Native desktop client placeholder for the Aetheris multiplayer platform.

## Overview

`aetheris-client-native` is a placeholder crate for the future native desktop client. It will share core logic with `aetheris-client-wasm` while using native APIs for rendering, networking, and input.

> **Status**: Phase 4 — not yet implemented. The primary client target is `aetheris-client-wasm` (browser/WASM).

## Planned Features

- **Native Rendering**: Hardware-accelerated rendering via `wgpu` with native window management.
- **Native Transport**: Direct UDP/QUIC transport via `aetheris-transport-quinn`.
- **Shared World State**: Shared simulation logic with the WASM client via `aetheris-protocol` traits.

For more details, see the [Client Design Document](../../docs/CLIENT_DESIGN.md).

---

License: MIT / Apache-2.0
