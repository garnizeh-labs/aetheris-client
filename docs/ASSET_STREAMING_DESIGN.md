---Version: 0.1.0-draft
Status: Phase 2 — Proposed / Phase 3 — Specified
Phase: P2 | P3
Last Updated: 2026-04-15
Authors: Team (Antigravity)
Spec References: [PF-3000]
Tier: 3
---

# Aetheris Asset Streaming — Technical Design Document

## Executive Summary

Standardized protocol for on-demand asset delivery over QUIC streams.

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Problem Statement](#2-problem-statement)
3. [Current State — Why Assets Don't Exist Yet](#3-current-state--why-assets-dont-exist-yet)
4. [Asset Classification & Budget](#4-asset-classification--budget)
5. [Streaming Architecture](#5-streaming-architecture)
6. [Wire Protocol — Asset Transfer Messages](#6-wire-protocol--asset-transfer-messages)
7. [QUIC Stream Mapping](#7-quic-stream-mapping)
8. [Client-Side Asset Pipeline](#8-client-side-asset-pipeline)
9. [Caching Strategy](#9-caching-strategy)
10. [Memory Pressure & Back-Pressure](#10-memory-pressure--back-pressure)
11. [Compression & Formats](#11-compression--formats)
12. [Progressive Loading & LOD](#12-progressive-loading--lod)
13. [Performance Contracts](#13-performance-contracts)
14. [Phase Roadmap](#14-phase-roadmap)
15. [Open Questions](#15-open-questions)
16. [Appendix A — Glossary](#appendix-a--glossary)
17. [Appendix B — Decision Log](#appendix-b--decision-log)

---

## 1. Executive Summary

Aetheris currently replicates only **ECS component deltas** (positions, health, flags) over the Data Plane. As the rendering pipeline matures beyond the Phase 1 clear-screen stub, clients will need **meshes, textures, shaders, audio, and UI assets** delivered efficiently.

This document defines how large assets are **discovered, chunked, transferred, cached, and loaded** over the existing QUIC/WebTransport Data Plane — without disrupting the latency-critical 16.6 ms tick pipeline that carries game state.

### Design Principles

| Principle | Rationale |
|---|---|
| **State traffic is sacred** | Asset downloads must never contend with the tick pipeline's datagram/stream budget. |
| **Stream-per-asset** | QUIC's multiplexed streams give each asset its own flow control — no HOL blocking between assets. |
| **Content-addressed** | Every asset is identified by a hash of its contents, enabling deduplication and cache validation. |
| **Progressive** | Clients render with low-LOD placeholders while high-resolution assets stream in the background. |

---

## 2. Problem Statement

Game clients need assets beyond what the ECS tick pipeline provides. The core challenges:

| Challenge | Impact |
|---|---|
| **Bandwidth contention** | A 4 MB texture download must not starve 200-byte state datagrams. |
| **Memory pressure** | WASM clients run in a constrained heap (~256 MB typical). Unbounded buffering causes OOM. |
| **Load times** | Players expect to enter a session within seconds, not wait for all assets to pre-download. |
| **Cache invalidation** | Game updates change assets; clients must know when their cache is stale. |
| **Cross-platform** | Native clients can use the filesystem; WASM clients must use IndexedDB or Cache API. |

---

## 3. Current State — Why Assets Don't Exist Yet

The Phase 1 rendering pipeline in `aetheris-client-wasm` is intentionally minimal:

- **`render.rs`**: Creates a single `wgpu` render pipeline with a hardcoded `basic.wgsl` shader. The `render_frame_with_snapshot()` function clears the screen — no mesh buffers, no texture loading, no draw calls.
- **No asset types**: Neither `aetheris-protocol` nor any client crate defines asset-related structs, messages, or traits.
- **Transport is generic**: `send_reliable(&[u8], ClientId)` and `poll_events() -> Vec<NetworkEvent>` carry opaque bytes. There is no asset-specific channel.
- **WASM reliable streams**: Not yet implemented in the WASM bridge (`transport.rs` logic triggers `NetworkEvent::ReliableMessage` but stream management is pending).

Asset streaming is a **Phase 2+** capability that depends on:

1. Reliable WASM streams being implemented.
2. The render pipeline supporting mesh/texture loading (`wgpu` buffer creation).
3. A server-side asset registry (or CDN) being operational.

---

## 4. Asset Classification & Budget

### 4.1 Asset Types

| Type | Typical Size | Delivery | Priority |
|---|---|---|---|
| **Shaders** (WGSL) | 1–10 KB | Bundled at build time or preloaded | Critical — nothing renders without them |
| **Meshes** (vertex + index) | 10–500 KB | Streamed per-entity or per-scene | High — geometry needed for visuals |
| **Textures** (GPU-compressed) | 50 KB – 4 MB | Streamed, progressive LOD | Medium — placeholders acceptable |
| **Audio** (Opus/Vorbis) | 50–500 KB | Streamed on demand | Low — can start muted |
| **UI Atlases** | 100–500 KB | Preloaded during `Loading` state | High — needed for HUD |

### 4.2 Bandwidth Budget

The Data Plane carries two classes of traffic on the same QUIC connection:

| Class | Budget | Mechanism |
|---|---|---|
| **State replication** | ≤ 50 KB/s per client (target) | DATAGRAM frames + unidirectional streams |
| **Asset streaming** | Remaining bandwidth (capped) | Dedicated unidirectional streams, QUIC flow-controlled |

Asset streams are **lower priority** than state. The QUIC layer's flow control ensures that asset streams yield bandwidth when state traffic spikes (e.g., during a large world delta).

---

## 5. Streaming Architecture

### 5.1 Component Overview

```text
┌────────────────────────────────────────────────────────┐
│                     Server                             │
│  ┌──────────────┐     ┌──────────────────────────┐     │
│  │ Tick Pipeline │     │   Asset Registry          │    │
│  │  (state)      │     │  content_hash → blob      │    │
│  └──────┬───────┘     └──────────┬───────────────┘     │
│         │                        │                     │
│    DATAGRAM /              Uni-stream /                 │
│    Uni-stream              Bi-stream                   │
│         │                        │                     │
└─────────┼────────────────────────┼─────────────────────┘
          │       QUIC Connection  │
┌─────────┼────────────────────────┼─────────────────────┐
│         ▼                        ▼                     │
│  ┌──────────────┐     ┌──────────────────────────┐     │
│  │ State Handler │     │   Asset Loader            │    │
│  │ (game.worker) │     │  (asset pipeline)         │    │
│  └──────────────┘     └──────────┬───────────────┘     │
│                                  │                     │
│                        ┌─────────▼──────────┐          │
│                        │   Asset Cache       │          │
│                        │  (IndexedDB / FS)   │          │
│                        └─────────┬──────────┘          │
│                                  │                     │
│                        ┌─────────▼──────────┐          │
│                        │   Render Worker     │          │
│                        │  (wgpu buffers)     │          │
│                        └────────────────────┘          │
│                     Client                             │
└────────────────────────────────────────────────────────┘
```

### 5.2 Flow — Asset Discovery & Delivery

1. **Discovery**: When the server spawns an entity visible to a client, the state snapshot includes a `MeshRef(content_hash)` and `TextureRef(content_hash)` component — not the data itself.
2. **Cache check**: The client's Asset Loader checks local cache (IndexedDB for WASM, filesystem for native) for `content_hash`.
3. **Request** (cache miss): Client opens a bidirectional QUIC stream and sends an `AssetRequest { hash, offset }` message. The `offset` field supports **resumable downloads**.
4. **Transfer**: Server responds on the same stream with chunked `AssetChunk { hash, offset, data, is_last }` messages. Each chunk is ≤ 64 KB.
5. **Completion**: Client reassembles chunks, verifies the hash, persists to cache, and uploads to GPU.

### 5.3 Manifest-Based Prefetch

For scene transitions or large maps, the server can push an **Asset Manifest** — a list of `(content_hash, asset_type, byte_size)` tuples — before the client enters the scene. This allows the client to:

- Schedule downloads by priority (shaders first, then meshes, then textures).
- Display a progress bar during the `Loading` client state.
- Skip already-cached assets.

---

## 6. Wire Protocol — Asset Transfer Messages

All asset messages share a common framing on reliable QUIC streams. The serialization uses the active `Encoder` trait implementation (rmp-serde in P1, Bitpack in P3).

### 6.1 Message Types

```rust
enum AssetMessage {
    /// Client → Server: request an asset by content hash
    Request {
        content_hash: [u8; 32],   // BLAKE3 hash
        offset: u64,               // resume offset (0 for fresh)
    },

    /// Server → Client: a chunk of asset data
    Chunk {
        content_hash: [u8; 32],
        offset: u64,
        data: Vec<u8>,            // ≤ 64 KB
        is_last: bool,
    },

    /// Server → Client: asset manifest for prefetching
    Manifest {
        entries: Vec<ManifestEntry>,
    },

    /// Server → Client: asset not found or access denied
    Error {
        content_hash: [u8; 32],
        reason: AssetError,
    },
}

struct ManifestEntry {
    content_hash: [u8; 32],
    asset_type: AssetType,        // Mesh, Texture, Shader, Audio, UiAtlas
    byte_size: u64,
    compression: Compression,     // None, Zstd, Brotli
}
```

### 6.2 Content Addressing

Every asset is identified by its **BLAKE3 hash** (32 bytes). This serves triple duty:

1. **Identity** — uniquely identifies the asset regardless of filename.
2. **Integrity** — the client verifies the assembled blob matches the hash.
3. **Deduplication** — identical assets shared by multiple entities are transferred once.

---

## 7. QUIC Stream Mapping

Asset transfers leverage QUIC's multiplexing to avoid interfering with game state:

| Stream Type | Purpose | Priority |
|---|---|---|
| **DATAGRAM** | Volatile state (position, rotation) | Highest |
| **Unidirectional** (server→client) | Critical state (ComponentKind per-stream) | High |
| **Bidirectional #0** | Client input | High |
| **Bidirectional #N** (N > 0) | Asset request/response | Low — QUIC deprioritized |

### 7.1 Priority & Flow Control

QUIC allows **stream-level priority hints** (as defined in RFC 9000 and the WebTransport spec). Asset streams are opened with a lower priority than state streams, ensuring that:

- The congestion window is shared but state traffic is served first.
- Asset streams are automatically throttled when the network is constrained.
- Multiple concurrent asset downloads each get independent flow control — a stalled texture doesn't block a mesh download (no HOL blocking between assets).

### 7.2 Concurrency Limits

To prevent resource exhaustion:

| Parameter | Value | Rationale |
|---|---|---|
| Max concurrent asset streams per client | 4 | Avoids overwhelming server-side stream state |
| Max chunk size | 64 KB | Fits within typical QUIC send buffer |
| Per-client asset bandwidth cap | 2 MB/s | Leaves headroom for state traffic |
| Server-wide asset bandwidth cap | 100 MB/s | Prevents asset traffic from saturating the NIC |

---

## 8. Client-Side Asset Pipeline

### 8.1 WASM Pipeline

```text
AssetRequest → QUIC bi-stream → AssetChunk[]
    → Reassemble in game.worker
    → Verify BLAKE3
    → Store in IndexedDB (key = content_hash hex)
    → Transfer to render.worker via postMessage (Transferable)
    → wgpu: create_buffer / create_texture
```

**Constraints**:

- IndexedDB is async — reads/writes must not block the game loop.
- `postMessage` with `Transferable` avoids copying large buffers between workers.
- WASM heap is limited — assets are decoded/uploaded to GPU and then freed from the heap.

### 8.2 Native Pipeline

```text
AssetRequest → QUIC bi-stream → AssetChunk[]
    → Reassemble in async task
    → Verify BLAKE3
    → Write to $CACHE_DIR/assets/{content_hash}
    → mmap or read into wgpu buffer/texture
```

**Advantages**:

- Filesystem cache survives process restarts.
- Memory-mapped files avoid redundant copies.
- No worker boundary — asset loader runs as a Tokio task alongside the render loop.

---

## 9. Caching Strategy

### 9.1 Cache Layers

| Layer | Storage | Lifetime | Eviction |
|---|---|---|---|
| **GPU** | VRAM (wgpu buffers) | Current session | LRU when VRAM budget exceeded |
| **Memory** | RAM (decoded) | Current session | LRU, bounded to 128 MB |
| **Persistent** | IndexedDB / filesystem | Cross-session | LRU, bounded to 512 MB |
| **Network** | CDN / server registry | Permanent | Versioned by content hash |

### 9.2 Cache Hit Flow

1. Entity spawns with `MeshRef(hash)`.
2. Check GPU cache → **hit**: bind immediately.
3. Check memory cache → **hit**: upload to GPU, bind.
4. Check persistent cache → **hit**: decode, upload to GPU, bind.
5. **Cache miss**: request from server (§5.2).

Content-addressed caching means **no invalidation protocol is needed** — a changed asset has a different hash and is a different cache entry. Old entries are evicted by LRU pressure.

---

## 10. Memory Pressure & Back-Pressure

### 10.1 WASM Memory Budget

WASM clients typically operate within ~256 MB. The asset pipeline must respect this:

| Budget Slice | Allocation |
|---|---|
| Game state (ECS snapshot) | ~16 MB |
| Render state (wgpu) | ~64 MB |
| **Asset pipeline** | **~64 MB** |
| Application logic | ~32 MB |
| Headroom | ~80 MB |

### 10.2 Back-Pressure Mechanism

If the client's in-flight asset buffer exceeds the budget:

1. **Pause**: Stop opening new asset streams. Existing streams continue at QUIC flow-control pace.
2. **Evict**: Drop low-priority cached assets (audio before textures, far LOD before near LOD).
3. **Resume**: When buffer drops below 75% of budget, resume new requests.

The server cooperates by respecting QUIC's **receiver flow control** — if the client stops reading, the server's send buffer fills and it naturally stops sending.

---

## 11. Compression & Formats

### 11.1 Texture Formats

| Format | Pros | Cons | Target |
|---|---|---|---|
| **KTX2 + Basis Universal** | GPU-compressed, transcodes to BC/ASTC/ETC on client | Transcode CPU cost | WASM (GPU upload without decode) |
| **PNG/JPEG → GPU upload** | Simple | CPU decode + GPU upload | Development only |
| **AVIF** | Excellent compression | No GPU-direct path | Not recommended |

**Decision (proposed)**: Use **KTX2 with Basis Universal** supercompression. The client transcodes to the GPU's native format (BC7 on desktop, ASTC on mobile/WASM) at load time. This avoids decompression entirely — the GPU reads the data directly.

> See [CLIENT_DESIGN.md](CLIENT_DESIGN.md) Open Question: "Asset Compression: Which format (KTX2, Basis Universal) is best for WASM decoding speed?"

### 11.2 Mesh Formats

| Format | Notes |
|---|---|
| **Custom binary (Phase 3)** | Vertex + index buffers in GPU-ready layout. Zero-copy upload via `bytemuck`. |
| **glTF 2.0 binary (.glb)** | Standard, but requires parsing. Suitable for development/import pipeline. |

### 11.3 Wire Compression

Asset chunks can be optionally compressed with **Zstd** (level 3) for transit. The `ManifestEntry.compression` field signals the scheme. GPU-compressed textures (KTX2) are already compact and typically skip wire compression.

---

## 12. Progressive Loading & LOD

### 12.1 Strategy

Clients render immediately with **placeholder LODs** while full assets stream:

| Phase | Visual | Asset State |
|---|---|---|
| **T+0** | Solid-color bounding box | `MeshRef` received, no data |
| **T+200ms** | Low-poly silhouette (LOD2) | Low LOD streamed first (small, fast) |
| **T+1s** | Medium mesh (LOD1) | Medium LOD replaces low |
| **T+3s** | Full detail (LOD0) | High LOD replaces medium |

### 12.2 LOD Encoding

Each asset can have up to 3 LOD levels, each a separate content-addressed blob:

```rust
ManifestEntry {
    content_hash: <lod0_hash>,  // full detail
    asset_type: Mesh,
    byte_size: 450_000,
    ...
}
ManifestEntry {
    content_hash: <lod1_hash>,  // medium
    asset_type: Mesh,
    byte_size: 45_000,
    ...
}
ManifestEntry {
    content_hash: <lod2_hash>,  // low
    asset_type: Mesh,
    byte_size: 4_500,
    ...
}
```

The client requests LOD2 first, then LOD1, then LOD0 — displaying each as it arrives.

---

## 13. Performance Contracts

| Metric | Target | Measurement |
|---|---|---|
| Time to first rendered entity (cache miss) | ≤ 500 ms | From entity spawn to LOD2 visible |
| Full LOD0 load (1 MB mesh + 2 MB texture, 50 Mbps link) | ≤ 3 s | From entity spawn to LOD0 visible |
| Asset pipeline CPU (WASM) | ≤ 5% of frame budget | Profiled with `performance.now()` |
| Cache hit rate (returning player, same session) | ≥ 95% | Prometheus `asset_cache_hits_total / asset_requests_total` |
| QUIC state latency impact from asset traffic | ≤ 1 ms p99 added | Measured with `game_tick_duration_seconds` histogram |

---

## 14. Phase Roadmap

| Phase | Scope | Dependencies |
|---|---|---|
| **P1 (current)** | No assets — hardcoded `basic.wgsl`, clear-screen render | — |
| **P2** | Static asset loading: bundled meshes/textures at build time. No network streaming. | Render pipeline supports mesh/texture creation |
| **P2.5** | Server-side asset registry. WASM reliable stream implementation. Content-addressed manifest. | `send_reliable` in WASM bridge |
| **P3** | Full streaming: QUIC bi-stream per asset, chunking, progressive LOD, IndexedDB cache | Quinn integration, Bitpack encoder |
| **P4** | CDN offload: hot assets served from edge CDN, cold assets from server | CDN infrastructure |

---

## 15. Open Questions

| Question | Context | Impact |
|---|---|---|
| **CDN vs. Server-Direct** | Should high-traffic assets be served from a CDN edge rather than the game server's QUIC connection? | Server CPU/bandwidth, operational complexity |
| **Asset Hot-Reload** | During development, can changed assets be pushed to connected clients without reconnect? | Developer iteration speed |
| **Audio Streaming** | Should long audio (music) be streamed progressively or downloaded as blobs? | Memory pressure, playback latency |
| **Shader Compilation** | wgpu shader compilation is blocking on some backends — should shaders be precompiled? | First-frame latency, platform variance |

---

## Appendix A — Glossary

| Term | Definition |
|---|---|
| **Content-Addressed** | An asset identified by the hash of its contents. Changing the asset changes the hash, making it a new entry. |
| **CRP** | Component Replication Protocol — Aetheris's wire format for ECS delta transmission. Separate from asset streaming. |
| **KTX2** | Khronos Texture format version 2. Container for GPU-compressed texture data with Basis Universal supercompression. |
| **LOD** | Level of Detail — progressively coarser versions of an asset used for distant objects or initial load. |
| **BLAKE3** | Cryptographic hash function used for asset content addressing. Fast on modern CPUs (SIMD-accelerated). |
| **Back-Pressure** | A flow control mechanism where the receiver signals the sender to slow down, preventing buffer overflow. |

---

## Appendix B — Decision Log

| Date | Decision | Rationale | Alternatives Considered |
|---|---|---|---|
| 2026-04-15 | Content-addressed assets (BLAKE3 hash as ID) | Deduplication, no invalidation protocol needed, resumable | UUID-based, filename-based |
| 2026-04-15 | Stream-per-asset on QUIC | Independent flow control, no HOL blocking between assets | Multiplex over single stream, HTTP/3 fallback |
| 2026-04-15 | KTX2 + Basis Universal (proposed) | GPU-direct upload, cross-platform transcode | Raw PNG, AVIF, proprietary |
| 2026-04-15 | 64 KB chunk size | Fits QUIC send buffer, fine-grained progress | 16 KB (overhead), 256 KB (latency) |
| 2026-04-15 | Asset streams are low-priority QUIC streams | State traffic is latency-critical, assets are throughput-critical | Separate connection, HTTP sideband |
