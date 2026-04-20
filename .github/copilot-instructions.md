<!--
  Live document — update this file whenever:
    • the three-worker protocol changes (SAB layout, postMessage shape)
    • a new module is added to aetheris-client-wasm
    • the SabSlot fields change (breaks repr(C) alignment)
    • the WebTransport connection flow changes
    • the render pipeline stages change
    • auth flow changes (OTP, Google OAuth, token format)
    • playground or build pipeline changes
    • the pinned nightly toolchain date changes
  Version history is tracked via git. Bump the Version field on every edit.
-->

---
Version: 1.0.0
Last Updated: 2026-04-19
Rust Edition: 2024
MSRV: 1.95.0 (stable) / nightly-2025-07-01 (WASM atomics build)
Workspace Version: 0.2.1
Phase: 1 (in progress)
Protocol Dependency: aetheris-protocol 0.2.4
---

# Copilot Instructions — aetheris-client

This is the **browser WASM runtime** for Aetheris. It implements a three-worker
execution model (Main Thread + Game Worker + Render Worker) connected via
`SharedArrayBuffer`, delivering 60 Hz simulation and WebGPU rendering without
ever blocking the DOM.

---

## Repository Layout

```
crates/
  aetheris-client-wasm/         # Primary browser runtime (cdylib + rlib)
    src/
      lib.rs                    # AetherisClient — lifecycle, connection state machine
      world_state.rs            # ClientWorld — implements WorldState trait
      shared_world.rs           # SabSlot (48 bytes, repr(C)), SabHeader, SharedWorld
      transport.rs              # WebTransportBridge — browser WebTransport API wrapper
      render.rs                 # RenderState — wgpu device/surface/pipeline + interpolation
      render_primitives.rs      # Vertex, MeshData, bind group layouts
      auth.rs                   # AuthServiceClient — OTP + Google OAuth via tonic-web-wasm
      metrics.rs                # MetricsCollector — ULID trace IDs, batched flush
      smoke_test.rs             # WASM unit test harness
      auth_proto/               # prost-generated auth proto messages (no tonic stubs)
      shaders/                  # WGSL shaders (basic.wgsl)
  aetheris-client-native/       # Phase 4+ stub — native desktop client (empty)
playground/
  package.json                  # v0.1.15, Vite + TypeScript
  index.html                    # Main entry point
  playground.html               # Sandbox / stress-test mode
docs/
  CLIENT_DESIGN.md              # Three-worker architecture, SAB protocol
  WORKER_COMMUNICATION_DESIGN.md # postMessage + SAB protocol specification
  INPUT_PIPELINE_DESIGN.md      # InputCommand, prediction, reconciliation
  ASSET_STREAMING_DESIGN.md     # Progressive asset loading (Phase 2+)
  PLAYGROUND_DESIGN.md          # Sandbox mode, stress test UI
```

---

## Three-Worker Architecture

```
┌─────────────────────────────────────────────────────┐
│  Main Thread (DOM owner — never touched by WASM)    │
│  • HTML/CSS HUD, React/Vue overlays                 │
│  • Input event listeners (keyboard/mouse/gamepad)   │
│  • OffscreenCanvas holder                           │
│  • postMessage router                               │
└──────────┬──────────────────────────┬───────────────┘
           │ postMessage               │ canvas.transferControlToOffscreen()
           ▼                           ▼
┌──────────────────────┐  ┌───────────────────────────┐
│   Game Worker        │  │   Render Worker            │
│   (WASM, 60 Hz)      │  │   (OffscreenCanvas + wgpu) │
│                      │  │                            │
│ • poll WebTransport  │  │ • read entities from SAB   │
│ • apply_updates()    │  │ • run interpolation engine │
│ • simulate()         │  │ • encode GPU commands      │
│ • write → SAB        │  │ • submit to WebGPU surface │
└──────────────────────┘  └───────────────────────────┘
           │                           ▲
           └──── SharedArrayBuffer ────┘
                 (zero-copy, atomic)
```

**Why three workers?**
- Main Thread isolation: DOM layout never competes with game logic.
- Game Worker: 60 Hz WASM loop has its own GC heap and event queue.
- Render Worker: GPU command encoding is fully off-main-thread.

---

## SharedArrayBuffer Layout

`SHARED_MEMORY_SIZE = 786,448 bytes` (16-byte header + 2 × 384 KiB buffers).

```
SabHeader (16 bytes, repr(C))
  state: AtomicU64  ← packed: (entity_count << 32) | flip_bit
  tick:  AtomicU64  ← latest server tick in active buffer

Buffer A: SabSlot[8192]  (48 bytes each = 384 KiB)
Buffer B: SabSlot[8192]  (48 bytes each = 384 KiB)
```

### `SabSlot` — 48-byte entity state (`repr(C)`, `Pod`, `Zeroable`)

```rust
// crates/aetheris-client-wasm/src/shared_world.rs
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct SabSlot {
    pub network_id: u64,    // Offset  0 — unique entity ID
    pub x: f32,             // Offset  8 — world-space X
    pub y: f32,             // Offset 12 — world-space Y
    pub z: f32,             // Offset 16 — world-space Z
    pub rotation: f32,      // Offset 20 — yaw (radians)
    pub dx: f32,            // Offset 24 — velocity X
    pub dy: f32,            // Offset 28 — velocity Y
    pub dz: f32,            // Offset 32 — velocity Z
    pub hp: u16,            // Offset 36 — health points
    pub shield: u16,        // Offset 38 — shield points
    pub entity_type: u16,   // Offset 40 — entity class discriminant
    pub flags: u8,          // Offset 42 — bitfield: Alive=0, Visible=1, LocalPlayer=2, Interpolate=3
    pub padding: [u8; 5],   // Offset 43 — alignment padding to 48 bytes
}
```

> **Never reorder or change `SabSlot` fields without updating the Render Worker's
> WGSL buffer layout and all JS-side views.** This struct crosses the WASM/JS boundary.

### Atomic Double-Buffer Protocol

```rust
// Game Worker — commit entity writes (called after simulation)
let packed: u64 = (entity_count as u64) << 32 | flip_bit as u64;
header.state.store(packed, Ordering::Release);

// Render Worker — read stable snapshot
let packed = header.state.load(Ordering::Acquire);
let entity_count = (packed >> 32) as u32;
let flip_bit     = (packed & 0xFFFF_FFFF) as usize;
// read from Buffer[flip_bit], entity_count slots are valid
```

Packing both values into a single `AtomicU64` eliminates the TOCTOU window that
would exist if `entity_count` and `flip_bit` were separate atomics.

---

## `ClientWorld` — Implementing `WorldState`

```rust
// crates/aetheris-client-wasm/src/world_state.rs
use aetheris_protocol::traits::WorldState;
use aetheris_protocol::types::{ClientId, ComponentKind, LocalId, NetworkId, Transform};
use aetheris_protocol::events::{ComponentUpdate, ReplicationEvent};

pub struct ClientWorld {
    pub entities: HashMap<NetworkId, SabSlot>,
    pub latest_tick: u64,
}

impl WorldState for ClientWorld {
    fn apply_updates(&mut self, updates: &[(ClientId, ComponentUpdate)]) {
        for (_, update) in updates {
            if update.tick > self.latest_tick {
                self.latest_tick = update.tick;
            }
            // ComponentKind(1) == Transform
            if update.component_kind == ComponentKind(1) {
                if let Ok(t) = rmp_serde::from_slice::<Transform>(&update.payload) {
                    let slot = self.entities.entry(update.network_id).or_insert(SabSlot {
                        network_id: update.network_id.0,
                        x: t.x, y: t.y, z: t.z,
                        rotation: t.rotation,
                        entity_type: t.entity_type,
                        flags: 1, // ALIVE
                        ..Zeroable::zeroed()
                    });
                    slot.x = t.x; slot.y = t.y; slot.z = t.z;
                    slot.rotation = t.rotation;
                }
            }
        }
    }

    // Client does not produce deltas — extract_deltas() returns Vec::new()
    fn extract_deltas(&mut self) -> Vec<ReplicationEvent> { Vec::new() }

    fn get_local_id(&self, nid: NetworkId) -> Option<LocalId> { Some(LocalId(nid.0)) }
    fn get_network_id(&self, lid: LocalId) -> Option<NetworkId> { Some(NetworkId(lid.0)) }
    fn spawn_networked(&mut self) -> NetworkId { NetworkId(0) } // server-assigned; unused client-side
    fn spawn_networked_for(&mut self, _: ClientId) -> NetworkId { NetworkId(0) }
    fn despawn_networked(&mut self, nid: NetworkId) -> Result<(), aetheris_protocol::error::WorldError> {
        self.entities.remove(&nid);
        Ok(())
    }
    fn spawn_kind(&mut self, _: u16, _: f32, _: f32, _: f32) -> NetworkId { NetworkId(0) }
}
```

---

## WebTransport Connection

```rust
// crates/aetheris-client-wasm/src/transport.rs
// The browser's WebTransport API is accessed via web-sys.
// Connection lifecycle:
//   1. Construct with server URL + cert hash
//   2. Await connection.ready (Promise)
//   3. Read datagrams via ReadableStream
//   4. Write datagrams via WritableStream
//   5. Handle streams for reliable messages

// Cert hash format expected by browser:
// serverCertificateHashes: [{ algorithm: "sha-256", value: <ArrayBuffer> }]
// The server logs its hash at startup — copy it into the client config.
```

**Cross-Origin Isolation requirement**: `SharedArrayBuffer` requires:
```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```
Both headers must be set on the page serving the WASM module.

---

## WASM Build Pipeline

```bash
# Requires pinned nightly for WASM atomics (thread::spawn in WASM workers)
cargo +nightly-2025-07-01 build \
  --target wasm32-unknown-unknown \
  --release \
  -Z build-std=std,panic_abort \
  -p aetheris-client-wasm

# Post-build: generate JS bindings
wasm-bindgen \
  target/wasm32-unknown-unknown/release/aetheris_client_wasm.wasm \
  --out-dir crates/aetheris-client-wasm/pkg \
  --target web
```

Or via justfile:
```bash
just build-wasm   # wraps the above
just dev          # Vite dev server for playground (localhost:5173)
```

**Compilation flags required for threading**:
```
-C target-feature=+atomics,+bulk-memory,+mutable-globals
-C link-arg=--shared-memory
-C link-arg=--import-memory
--cfg=web_sys_unstable_apis
```

---

## Observability — `MetricsCollector`

```rust
// crates/aetheris-client-wasm/src/metrics.rs
use aetheris_client_wasm::metrics::MetricsCollector;

// Created once per WASM session
let mut collector = MetricsCollector::new();

// Record events during the game loop
collector.record_tick(tick_duration_ms);
collector.record_rtt(rtt_ms);
collector.record_entity_count(entity_count);

// Flush batched metrics to the server (every 5 seconds)
// POST /telemetry/json with ULID session + trace IDs
collector.flush_if_due().await;
```

ULID identifiers are generated once per session (`session_id`) and per span
(`trace_id`) to enable distributed trace correlation with the server.

---

## Auth Flow — OTP + Google OAuth

```rust
// crates/aetheris-client-wasm/src/auth.rs
// Hand-written gRPC-web client (no tonic service stubs in WASM)
use aetheris_client_wasm::auth::AuthServiceClient;

let client = AuthServiceClient::new("https://api.example.com");

// OTP flow:
client.request_otp(email).await?;           // sends OTP to email
let token = client.verify_otp(email, code).await?;  // returns session_token

// Google OAuth flow:
let token = client.google_login(id_token).await?;

// Use token on WebTransport connect:
// NetworkEvent::Auth { session_token: token }
```

---

## Render Pipeline (wgpu / WebGPU)

```rust
// crates/aetheris-client-wasm/src/render.rs
// Phase 1 status: basic screen clear + wireframe debug (M1011 complete)
// M1010 (full wgpu pipeline) is the active milestone.

// Standard wgpu initialization for WebGPU target:
let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
    backends: wgpu::Backends::BROWSER_WEBGPU,
    ..Default::default()
});
let surface = instance.create_surface(canvas)?; // OffscreenCanvas
let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
    compatible_surface: Some(&surface),
    ..Default::default()
}).await?;
let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor::default(), None).await?;
```

Shaders are WGSL (`crates/aetheris-client-wasm/src/shaders/basic.wgsl`).

### Interpolation Buffer

Render Worker reads two consecutive server ticks from the SAB and blends:
- Buffer holds **100 ms of server ticks** (6 ticks at 60 Hz).
- `alpha = elapsed_since_tick / tick_duration` (0.0 → 1.0).
- Divergence < 0.5 m → smooth lerp. Divergence > 2 m → instant snap.

---

## `InputCommand` — Client Input Schema (Phase 1)

```rust
// Sent as unreliable datagram every tick from Game Worker to server
#[derive(Serialize, Deserialize)]
pub struct InputCommand {
    pub client_tick: u64,   // for lag compensation
    pub move_dir: [f32; 2], // unit vector (client frame)
    pub jump: bool,
    pub action: bool,
    pub look_dir: [f32; 2], // yaw/pitch in radians
}
```

Phase 3 replaces this with an `InputSchema` trait to allow application-defined
input types without touching the core client loop.

---

## Key Conventions

- **Never access DOM APIs from the Game Worker or Render Worker.** All DOM interaction
  goes through `postMessage` to/from the Main Thread.
- **`SabSlot` is `repr(C)` and `Pod`** — field order, sizes, and alignment are ABI.
  Adding a field requires bumping the protocol version and updating the Render Worker.
- **`extract_deltas()` always returns `Vec::new()` on the client.** The client only
  consumes deltas; it never produces them.
- **`shared_world_size()` is exposed as a WASM export** so JavaScript can allocate
  the `SharedArrayBuffer` with the exact correct size.
- All async WASM code uses `wasm-bindgen-futures::spawn_local` — never `tokio::spawn`.
- Use `console_error_panic_hook` in debug builds for readable WASM panics.
- The `web_sys_unstable_apis` cfg flag is required for `OffscreenCanvas` and `WebTransport`.
