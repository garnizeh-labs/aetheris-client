---Version: 0.1.0-draft
Status: Phase 1 — MVP / Phase 3 — Extensible
Phase: P1 | P3
Last Updated: 2026-04-16
Authors: Team (Antigravity)
Spec References: [ENGINE_DESIGN, CLIENT_DESIGN, API_DESIGN, VOID_RUSH_GDD, NEXUS_PLATFORM_DESIGN]
Tier: 3
---

# Aetheris Engine — Worker Communication Design Document

## Executive Summary

The Aetheris Engine's client runs on a **three-worker architecture**: Main Thread, Game Worker, and Render Worker. These workers communicate through two mechanisms:

| Mechanism | Latency | Use Case | Direction |
|---|---|---|---|
| **SharedArrayBuffer** (SAB) + `Atomics` | ~0 (shared memory) | High-frequency entity state (60 Hz+) | Game Worker → Render Worker, Game Worker → Main Thread |
| **`postMessage`** (structured clone) | ~1–5 ms | Low-frequency events, commands, initialization | Bidirectional between any pair |

This document is the **canonical source** for:

- The SAB memory layout and double-buffer flip-bit protocol.
- The `EntityDisplayState` repr(C) struct and its wire format.
- The postMessage topic/payload protocol.
- SAB extensibility rules for non-game applications (Nexus platform).
- Security requirements (cross-origin isolation for Spectre mitigation).

It consolidates fragments previously scattered across [CLIENT_DESIGN.md](CLIENT_DESIGN.md) §6, [API_DESIGN.md](API_DESIGN.md) §9, [VOID_RUSH_GDD.md](VOID_RUSH_GDD.md) §12.2, and [NEXUS_PLATFORM_DESIGN.md](NEXUS_PLATFORM_DESIGN.md) §3.2–3.4.

### Design Principle

> **SAB for continuous state. postMessage for discrete events.**
>
> If data changes every tick (positions, tickers, cursors) → SAB.
> If data arrives occasionally (trade confirmations, document edits, errors) → postMessage.

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Three-Worker Architecture Recap](#2-three-worker-architecture-recap)
3. [SharedArrayBuffer — Zero-Copy State Bridge](#3-sharedarraybuffer--zero-copy-state-bridge)
4. [Double-Buffer Flip-Bit Protocol](#4-double-buffer-flip-bit-protocol)
5. [postMessage Protocol](#5-postmessage-protocol)
6. [EntityDisplayState — Wire Layout](#6-entitydisplaystate--wire-layout)
7. [SAB Memory Layout — Engine Default](#7-sab-memory-layout--engine-default)
8. [SAB Memory Layout — Extensibility](#8-sab-memory-layout--extensibility)
9. [TypeScript Reading Patterns](#9-typescript-reading-patterns)
10. [Security & Cross-Origin Isolation](#10-security--cross-origin-isolation)
11. [Performance Contracts](#11-performance-contracts)
12. [Open Questions](#12-open-questions)
13. [Appendix A — Glossary](#appendix-a--glossary)
14. [Appendix B — Decision Log](#appendix-b--decision-log)

---

## 1. Executive Summary

The Aetheris Engine's client runs on a **three-worker architecture**: Main Thread, Game Worker, and Render Worker. These workers communicate through two mechanisms:

| Mechanism | Latency | Use Case | Direction |
|---|---|---|---|
| **SharedArrayBuffer** (SAB) + `Atomics` | ~0 (shared memory) | High-frequency entity state (60 Hz+) | Game Worker → Render Worker, Game Worker → Main Thread |
| **`postMessage`** (structured clone) | ~1–5 ms | Low-frequency events, commands, initialization | Bidirectional between any pair |

This document is the **canonical source** for:

- The SAB memory layout and double-buffer flip-bit protocol.
- The `EntityDisplayState` repr(C) struct and its wire format.
- The postMessage topic/payload protocol.
- SAB extensibility rules for non-game applications (Nexus platform).
- Security requirements (cross-origin isolation for Spectre mitigation).

It consolidates fragments previously scattered across [CLIENT_DESIGN.md](CLIENT_DESIGN.md) §6, [API_DESIGN.md](API_DESIGN.md) §9, [VOID_RUSH_GDD.md](VOID_RUSH_GDD.md) §12.2, and [NEXUS_PLATFORM_DESIGN.md](NEXUS_PLATFORM_DESIGN.md) §3.2–3.4.

### Design Principle

> **SAB for continuous state. postMessage for discrete events.**
>
> If data changes every tick (positions, tickers, cursors) → SAB.
> If data arrives occasionally (trade confirmations, document edits, errors) → postMessage.

---

## 2. Three-Worker Architecture Recap

```
┌──────────────────────────────────────┐
│  Main Thread (DOM Owner)             │
│  • HTML/CSS HUD, React/Vue overlays  │
│  • Input event listeners (keyboard,  │
│    mouse, touch, text)               │
│  • Nexus UI Bridge (reads SAB)       │
│  • OffscreenCanvas element holder    │
└────────┬──────────────────┬──────────┘
         │ postMessage       │ canvas.transferControlToOffscreen()
         │                   │
┌────────▼──────────┐   ┌───▼──────────────────────┐
│  Game Worker       │   │  Render Worker            │
│  (Worker)          │   │  (Worker + OffscreenCanvas)│
│                    │   │                            │
│  • WASM game loop  │   │  • wgpu WebGPU surface     │
│    60 Hz tick      │   │  • Interpolation engine     │
│  • WebTransport    │   │  • Scene graph              │
│    socket          │   │  • GPU command submission    │
│  • Client ECS      │   │                            │
│  • SAB producer    │   │  • SAB consumer (reads)     │
└────────────────────┘   └────────────────────────────┘
         │                           ▲
         │  SharedArrayBuffer        │
         └───────────────────────────┘
```

**Why three workers?**

1. **Main Thread isolation.** DOM layout and HUD React rerenders never compete with game logic. The browser never marks the tab as frozen.
2. **Game Worker independence.** The 60 Hz game loop runs in a clean execution context with its own GC heap, separate from the main thread.
3. **Render Worker with OffscreenCanvas.** GPU command encoding and submission happen entirely off the main thread. Frame drops caused by a slow GPU never affect input latency.

Each browser tab gets its own `Worker` instances (not `SharedWorker`) — tabs cannot observe or affect each other's game state.

---

## 3. SharedArrayBuffer — Zero-Copy State Bridge

### 3.1 Why SharedArrayBuffer?

`postMessage` uses structured cloning — it copies data. At 60 Hz with 500 entities × 48 bytes = 24 KB per tick, that's 1.44 MB/s of allocation + GC pressure per `postMessage` channel. With SAB, the copy is eliminated entirely: the Game Worker writes directly into memory that the Render Worker reads.

| Approach | Copy Cost | GC Pressure | Latency |
|---|---|---|---|
| `postMessage` (structured clone) | O(N) per tick | High (24 KB/tick → GC) | 1–5 ms per message |
| `postMessage` (transferable) | O(1) per tick | Low | 1–5 ms, but ownership transfers (cannot reuse) |
| **SharedArrayBuffer** (selected) | **Zero** | **None** | **~0 (shared memory)** |

### 3.2 Ownership Model

```text
SharedArrayBuffer lifecycle:
  1. Main Thread allocates SAB (new SharedArrayBuffer(size))
  2. Main Thread posts SAB reference to Game Worker and Render Worker
  3. Game Worker is the sole WRITER (double-buffered)
  4. Render Worker and Main Thread are READERS
  5. SAB lives for the lifetime of the page
```

```,oldString:

**Invariant:** The Game Worker is the **only writer**. Multiple readers (Render Worker, Main Thread) are safe because of the double-buffer protocol — readers always read from the stable (non-active-write) buffer.

---

## 4. Double-Buffer Flip-Bit Protocol

### 4.1 Concept

The SAB contains **two copies** of every data region (Buffer A and Buffer B). At any moment, one buffer is "active" (being read) and the other is "inactive" (being written to). After the Game Worker finishes writing all entity state into the inactive buffer, it atomically flips the active index.

```

Tick N:
  Active = Buffer A (readers read A)
  Game Worker writes into Buffer B

Tick N → N+1:
  Game Worker: Atomics.store(flip_bit, 1)  // Release fence
  Active = Buffer B (readers now read B)
  Game Worker writes into Buffer A

Tick N+1 → N+2:
  Game Worker: Atomics.store(flip_bit, 0)  // Release fence
  Active = Buffer A (readers now read A)
  Game Worker writes into Buffer B
  ...

```

### 4.2 Writer Protocol (Game Worker — Rust/WASM)

```rust
/// Game Worker — called at end of every tick after ECS simulate.
pub fn write_display_state(
    sab: &SharedArrayBuffer,
    entities: &[(NetworkId, Position, Velocity, Rotation, Health, AnimationId, Flags)],
) {
    let flip_bit = AtomicU32Ref::new(&sab[0..4]);
    let entity_count = AtomicU32Ref::new(&sab[4..8]);

    // 1. Determine which buffer is INACTIVE (the one we write to)
    let active = flip_bit.load(Ordering::Relaxed);
    let write_index = 1 - active; // 0 → 1, 1 → 0
    let write_offset = buffer_offset(write_index);

    // 2. Write all entity data into the inactive buffer
    for (i, entity) in entities.iter().enumerate() {
        let slot_offset = write_offset + i * ENTITY_SLOT_SIZE;
        write_entity_slot(sab, slot_offset, entity);
    }

    // 3. Update entity count (for the buffer we just wrote)
    entity_count.store(entities.len() as u32, Ordering::Release);

    // 4. Atomically flip the active buffer
    //    Release ordering ensures all writes above are visible
    //    before the flip bit changes.
    flip_bit.store(write_index as u32, Ordering::Release);
}
```

### 4.3 Reader Protocol (Render Worker / Main Thread — TypeScript)

```typescript
// Render Worker — called every requestAnimationFrame
function readEntityState(sab: SharedArrayBuffer): EntityDisplayState[] {
    const u32 = new Uint32Array(sab);
    const f32 = new Float32Array(sab);

    // 1. Load active buffer index (Acquire — sees all writes before the flip)
    const activeIndex = Atomics.load(u32, 0); // offset 0 = flip_bit
    const entityCount = Atomics.load(u32, 1); // offset 1 = entity_count

    // 2. Calculate read offset
    const readByteOffset = HEADER_SIZE + activeIndex * BUFFER_SIZE;

    // 3. Read all entities from the ACTIVE buffer (guaranteed stable)
    const entities: EntityDisplayState[] = [];
    for (let i = 0; i < entityCount; i++) {
        const slotByteOffset = readByteOffset + i * ENTITY_SLOT_SIZE;
        entities.push(readEntitySlot(sab, slotByteOffset));
    }

    return entities;
}
```

### 4.4 Correctness Guarantees

| Property | Guarantee | Mechanism |
|---|---|---|
| **No torn reads** | Reader sees a complete, consistent entity snapshot | Double-buffering: reader reads buffer A while writer writes buffer B |
| **Happens-before** | All entity writes are visible before flip | `Ordering::Release` on writer, `Ordering::Acquire` on reader |
| **No reader blocking** | Reader never waits for writer | Lock-free: reader always reads immediately from the active buffer |
| **No writer blocking** | Writer never waits for reader | Lock-free: writer always writes into the inactive buffer |
| **At most one tick stale** | Reader may see tick N or N−1, never N−2 | Flip replaces active buffer each tick; previous active becomes overwrite target |

---

## 5. postMessage Protocol

### 5.1 Message Envelope

All inter-worker `postMessage` calls use a standard envelope:

```typescript
interface WorkerMessage {
    /** Topic string for routing to subscribers. */
    type: string;
    /** Payload — varies by topic. Must be structured-clone-compatible. */
    payload?: any;
}
```

### 5.2 Main Thread → Game Worker Messages

| `type` | Payload | Purpose |
|---|---|---|
| `'connect'` | `{ certHash: string }` | Initiate WebTransport connection to server |
| `'disconnect'` | — | Graceful disconnect |
| `'input'` | `Uint8Array` (encoded InputSchema) | Pre-encoded input command |
| `'key_down'` | `{ key: string }` | Raw keyboard event for InputMapper |
| `'key_up'` | `{ key: string }` | Raw keyboard event for InputMapper |
| `'mouse_move'` | `{ dx: number, dy: number }` | Raw mouse delta |
| `'mouse_down'` | `{ button: number }` | Raw mouse button event |
| `'mouse_up'` | `{ button: number }` | Raw mouse button event |
| `'command'` | `NexusCommand` | Nexus UI command (text edit, trade order, quiz answer) |
| `'resize'` | `{ width: number, height: number }` | Viewport resize notification |

### 5.3 Game Worker → Main Thread Messages

| `type` | Payload | Purpose |
|---|---|---|
| `'state_update'` | `Float32Array` (transferable) | Fallback state push when SAB unavailable |
| `'connection_error'` | `{ reason: string }` | WebTransport connection failure |
| `'connection_ready'` | `{ clientId: string, tick: number }` | Connection established, handshake complete |
| `'game_event'` | `{ event: string, data: any }` | Death, level-up, achievement, score change |
| `'document_update'` | `{ docId: string, ops: any[] }` | Nexus: collaborative document changes |
| `'trade_fill'` | `{ orderId: string, price: number, qty: number }` | Nexus: trade execution confirmation |
| `'room_event'` | `{ type: string, roomId: string, ... }` | Room entry/exit/lock/unlock notifications |
| `'error'` | `{ code: string, message: string }` | Generic error from WASM |

### 5.4 Main Thread → Render Worker Messages

| `type` | Payload | Purpose |
|---|---|---|
| `'init'` | `{ canvas: OffscreenCanvas, sab: SharedArrayBuffer }` | Transfer canvas and SAB reference |
| `'resize'` | `{ width: number, height: number }` | Viewport resize for GPU surface recreation |
| `'camera'` | `{ position: Float32Array, target: Float32Array }` | Camera update from Main Thread UI (e.g., minimap click) |

### 5.5 Game Worker → Render Worker Messages

Direct Game Worker → Render Worker communication is **not used**. All render state flows through the SAB. The only Game Worker → Render Worker path is indirect:

```
Game Worker → SAB (write) → Render Worker (read)
```

If a one-time event must reach the Render Worker (e.g., asset URL for a new model), it flows through the Main Thread as a relay:

```
Game Worker → postMessage → Main Thread → postMessage → Render Worker
```

This avoids a direct `MessagePort` between workers, keeping the topology simple.

---

## 6. EntityDisplayState — Wire Layout

### 6.1 Engine-Level Struct (repr(C))

The canonical `EntityDisplayState` struct defines the per-entity data written into the SAB by the Game Worker:

```rust
/// Per-entity render state for the SAB.
/// Written by Game Worker (Rust/WASM). Read by Render Worker (TypeScript/wgpu).
///
/// CRITICAL: This struct must be `repr(C)` and `Pod` (no padding ambiguity,
/// no interior pointers). Any change to this struct is a breaking change to
/// the SAB wire protocol.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EntityDisplayState {
    /// Network-wide unique entity identifier.
    pub network_id: u64,          // offset  0, 8 bytes
    /// World-space position (x, y, z).
    pub position: [f32; 3],       // offset  8, 12 bytes
    /// Velocity vector for client-side extrapolation on packet loss.
    pub velocity: [f32; 3],       // offset 20, 12 bytes
    /// Orientation quaternion (w, x, y, z).
    pub rotation: [f32; 4],       // offset 32, 16 bytes
    /// Normalized health (0.0 = dead, 1.0 = full).
    pub health_normalized: f32,   // offset 48, 4 bytes
    /// Animation state machine ID for the Render Worker's animation system.
    pub animation_id: u32,        // offset 52, 4 bytes
    /// Bitfield flags: alive(0), visible(1), local_player(2), ...
    pub flags: u32,               // offset 56, 4 bytes
}
// Total: 60 bytes per entity
```

### 6.2 Void Rush Compact Layout (48 bytes)

For Void Rush specifically, the GDD defines a more compact 48-byte layout optimized for 2D gameplay (single rotation angle, no quaternion):

| Offset | Field | Type | Size |
|---|---|---|---|
| 0 | `network_id` | `u64` | 8 bytes |
| 8 | `x` | `f32` | 4 bytes |
| 12 | `y` | `f32` | 4 bytes |
| 16 | `z` | `f32` | 4 bytes |
| 20 | `rotation` (yaw only) | `f32` | 4 bytes |
| 24 | `dx` | `f32` | 4 bytes |
| 28 | `dy` | `f32` | 4 bytes |
| 32 | `dz` | `f32` | 4 bytes |
| 36 | `hp` | `u16` | 2 bytes |
| 38 | `shield` | `u16` | 2 bytes |
| 40 | `entity_type` | `u8` | 1 byte |
| 41 | `flags` | `u8` | 1 byte |
| 42 | `_padding` | — | 6 bytes |
| **Total** | | | **48 bytes** |

The engine provides both layouts. Applications select their entity slot size at initialization via `SabConfig::entity_slot_size`.

### 6.3 Flags Bitfield

| Bit | Name | Meaning |
|---|---|---|
| 0 | `ALIVE` | Entity is alive (0 = despawned/dead) |
| 1 | `VISIBLE` | Entity should be rendered |
| 2 | `LOCAL_PLAYER` | This entity is the local player (special camera/HUD treatment) |
| 3 | `INTERPOLATE` | Use interpolation (vs. extrapolation) for this entity |
| 4 | `HAS_ANIMATION` | `animation_id` field is meaningful |
| 5–31 | Reserved | Application-specific flags |

---

## 7. SAB Memory Layout — Engine Default

### 7.1 Layout Structure

The engine-default SAB layout serves game workloads (Void Rush):

```text
SharedArrayBuffer (Engine Default — 768 KiB):

Offset      Size     Region
──────────────────────────────────────────────
0x0000      4B       Flip Bit (Atomic<u32>): 0 or 1
0x0004      4B       Entity Count (Atomic<u32>)
0x0008      8B       Reserved (alignment)
──────────────────────────────────────────────
0x0010      ...      Buffer A: EntityDisplayState[MAX_ENTITIES]
                     (entity_slot_size × max_entities)
──────────────────────────────────────────────
...         ...      Buffer B: EntityDisplayState[MAX_ENTITIES]
                     (entity_slot_size × max_entities)
──────────────────────────────────────────────
```

```,oldString:
```,oldString:

### 7.2 Default Configuration

```rust
pub struct SabConfig {
    /// Size of each entity slot in bytes.
    /// Engine default: 60 (EntityDisplayState).
    /// Void Rush override: 48 (compact layout).
    pub entity_slot_size: usize,

    /// Maximum entities in the SAB.
    /// Engine default: 4096.
    /// Void Rush override: 8192.
    pub max_entities: usize,

    /// Additional regions beyond entity transforms (Nexus platform).
    /// Engine default: empty (game-only SAB).
    pub additional_regions: Vec<SabRegion>,
}

impl Default for SabConfig {
    fn default() -> Self {
        Self {
            entity_slot_size: 60, // EntityDisplayState
            max_entities: 4096,
            additional_regions: Vec::new(),
        }
    }
}
```

### 7.3 Size Calculations

| Configuration | Entity Slot | Max Entities | Header | Buffer A | Buffer B | Total |
|---|---|---|---|---|---|---|
| Engine Default | 60 B | 4,096 | 16 B | 240 KiB | 240 KiB | **480 KiB** |
| Void Rush | 48 B | 8,192 | 16 B | 384 KiB | 384 KiB | **768 KiB** |
| Nexus Platform | 60 B | 560 | 16 B | 33 KiB × 2 + Regions | — | **64 KiB** (see §8) |

All configurations fit comfortably in L2 cache (typically 256 KiB – 1 MiB per core).

---

## 8. SAB Memory Layout — Extensibility

### 8.1 Nexus Platform Extended Layout

Non-game applications need SAB regions beyond entity transforms. The Nexus platform defines five regions within a 64 KiB SAB:

```
SharedArrayBuffer (Nexus Configuration — 64 KiB):

Offset     Region   Purpose                         Slot Size   Max Slots   Reader
─────────────────────────────────────────────────────────────────────────────────────
0x0000     Header   Flip bit + server tick + reserved  16 B         —        All
0x0010     A        Entity Transforms                  36 B       560        Render Worker
0x5000     B        Cursor Positions                   28 B       292        Main Thread
0x7000     C        Financial Tickers                  32 B       256        Main Thread
0x9000     D        Presence / Avatar Status            13 B       315        Main Thread
0xA000     E        Application-specific (free)          —         —         Configurable
0xFFFF     End      Total: 64 KiB
```

### 8.2 Region Detail

**Region A — Entity Transforms** (Render Worker reads)

| Field | Type | Size |
|---|---|---|
| `network_id` | `u64` | 8 B |
| `position` (x,y,z) | `[f32; 3]` | 12 B |
| `rotation` (w,x,y,z) | `[f32; 4]` | 16 B |
| **Total** | | **36 B** |

Nexus drops `velocity`, `health_normalized`, `animation_id`, and `flags` — metaverse avatars use animation systems driven by postMessage events, not per-tick SAB data.

**Region B — Cursor Positions** (Main Thread reads for editor overlays)

| Field | Type | Size |
|---|---|---|
| `client_id` | `u64` | 8 B |
| `doc_network_id` | `u64` | 8 B |
| `line` | `u32` | 4 B |
| `column` | `u32` | 4 B |
| `selection_length` | `u32` | 4 B |
| **Total** | | **28 B** |

**Region C — Financial Tickers** (Main Thread reads for trading dashboards)

| Field | Type | Size |
|---|---|---|
| `ticker_id` | `u64` | 8 B |
| `price_cents` | `i64` | 8 B |
| `volume` | `u64` | 8 B |
| `timestamp_ms` | `u64` | 8 B |
| **Total** | | **32 B** |

**Region D — Presence / Avatar Status** (Main Thread reads for presence indicators)

| Field | Type | Size |
|---|---|---|
| `client_id` | `u64` | 8 B |
| `status` | `u8` | 1 B |
| `proximity` | `f32` | 4 B |
| **Total** | | **13 B** |

**Region E — Application-Specific** (0xA000 – 0xFFFF, ~24 KiB free)

Reserved for per-application data. Examples:

- Quiz/exam: current question index, timer, answer selection state.
- Whiteboard: polyline vertex buffer for real-time collaborative drawing.
- Game HUD: custom game-specific stats (shields, ammo, score).

Applications register Region E sub-regions via `SabConfig::additional_regions`.

### 8.3 Registering Custom Regions

```rust
let config = SabConfig {
    entity_slot_size: 36, // Nexus compact
    max_entities: 560,
    additional_regions: vec![
        SabRegion {
            name: "cursors",
            offset: 0x5000,
            slot_size: 28,
            max_slots: 292,
        },
        SabRegion {
            name: "tickers",
            offset: 0x7000,
            slot_size: 32,
            max_slots: 256,
        },
        SabRegion {
            name: "presence",
            offset: 0x9000,
            slot_size: 13,
            max_slots: 315,
        },
        SabRegion {
            name: "app-specific",
            offset: 0xA000,
            slot_size: 0, // Application manages internally
            max_slots: 0,
        },
    ],
};
```

### 8.4 Double-Buffering Strategy for Extended Regions

The engine header's flip bit covers **only Region A** (entity transforms). Extended regions (B–E) use a different strategy:

| Region | Update Frequency | Double-Buffered? | Rationale |
|---|---|---|---|
| A (Transforms) | Every tick (60 Hz) | Yes (flip bit) | Torn reads are visible as jitter |
| B (Cursors) | Every tick (60 Hz) | Yes (per-region flip bit at region header) | Cursor jumps are visible |
| C (Tickers) | Every tick (60 Hz) | Yes (per-region flip bit) | Price tearing is unacceptable |
| D (Presence) | Every ~10 ticks | No (atomic per-field writes) | Status changes are rare, single-field atomic suffices |
| E (App-specific) | Application-defined | Application chooses | SDK provides `DoubleBufferedRegion` helper |

Each double-buffered extended region has a 4-byte per-region flip bit at its starting offset, followed by two copies of the data.

---

## 9. TypeScript Reading Patterns

### 9.1 Entity Transform Reader (Render Worker)

```typescript
const HEADER_SIZE = 16;
const ENTITY_SLOT_SIZE = 60; // or 48 for Void Rush

export function readEntityTransforms(sab: SharedArrayBuffer, maxEntities: number): EntityDisplayState[] {
    const u32 = new Uint32Array(sab, 0, 4);
    const activeBuffer = Atomics.load(u32, 0);  // flip bit
    const entityCount = Atomics.load(u32, 1);    // entity count

    const bufferSize = ENTITY_SLOT_SIZE * maxEntities;
    const readOffset = HEADER_SIZE + activeBuffer * bufferSize;

    const view = new DataView(sab, readOffset, entityCount * ENTITY_SLOT_SIZE);
    const entities: EntityDisplayState[] = [];

    for (let i = 0; i < entityCount; i++) {
        const o = i * ENTITY_SLOT_SIZE;
        entities.push({
            networkId: view.getBigUint64(o, true),      // little-endian
            position: [
                view.getFloat32(o + 8, true),
                view.getFloat32(o + 12, true),
                view.getFloat32(o + 16, true),
            ],
            velocity: [
                view.getFloat32(o + 20, true),
                view.getFloat32(o + 24, true),
                view.getFloat32(o + 28, true),
            ],
            rotation: [
                view.getFloat32(o + 32, true),
                view.getFloat32(o + 36, true),
                view.getFloat32(o + 40, true),
                view.getFloat32(o + 44, true),
            ],
            healthNormalized: view.getFloat32(o + 48, true),
            animationId: view.getUint32(o + 52, true),
            flags: view.getUint32(o + 56, true),
        });
    }

    return entities;
}
```

### 9.2 Nexus UI Bridge — Cursor Reader (Main Thread)

```typescript
const CURSOR_REGION_OFFSET = 0x5000;
const CURSOR_SLOT_SIZE = 28;

export function readCursors(sab: SharedArrayBuffer): CursorState[] {
    // Per-region flip bit at the start of the cursor region
    const regionHeader = new Uint32Array(sab, CURSOR_REGION_OFFSET, 2);
    const activeBuf = Atomics.load(regionHeader, 0);
    const cursorCount = Atomics.load(regionHeader, 1);

    const dataOffset = CURSOR_REGION_OFFSET + 8 + activeBuf * (CURSOR_SLOT_SIZE * MAX_CURSORS);
    const view = new DataView(sab, dataOffset, cursorCount * CURSOR_SLOT_SIZE);
    const cursors: CursorState[] = [];

    for (let i = 0; i < cursorCount; i++) {
        const o = i * CURSOR_SLOT_SIZE;
        cursors.push({
            clientId: view.getBigUint64(o, true),
            docNetworkId: view.getBigUint64(o + 8, true),
            line: view.getUint32(o + 16, true),
            column: view.getUint32(o + 20, true),
            selectionLength: view.getUint32(o + 24, true),
        });
    }

    return cursors;
}
```

### 9.3 Nexus UI Bridge — Ticker Reader (Main Thread)

```typescript
const TICKER_REGION_OFFSET = 0x7000;
const TICKER_SLOT_SIZE = 32;

export function readTickers(sab: SharedArrayBuffer): TickerState[] {
    const regionHeader = new Uint32Array(sab, TICKER_REGION_OFFSET, 2);
    const activeBuf = Atomics.load(regionHeader, 0);
    const tickerCount = Atomics.load(regionHeader, 1);

    const dataOffset = TICKER_REGION_OFFSET + 8 + activeBuf * (TICKER_SLOT_SIZE * MAX_TICKERS);
    const view = new DataView(sab, dataOffset, tickerCount * TICKER_SLOT_SIZE);
    const tickers: TickerState[] = [];

    for (let i = 0; i < tickerCount; i++) {
        const o = i * TICKER_SLOT_SIZE;
        tickers.push({
            tickerId: view.getBigUint64(o, true),
            priceCents: view.getBigInt64(o + 8, true),
            volume: view.getBigUint64(o + 16, true),
            timestampMs: view.getBigUint64(o + 24, true),
        });
    }

    return tickers;
}
```

---

## 10. Security & Cross-Origin Isolation

### 10.1 SharedArrayBuffer Requires Cross-Origin Isolation

Since the Spectre vulnerability mitigations (2018+), all browsers disable `SharedArrayBuffer` unless the page is **cross-origin isolated**. The server must send these HTTP headers:

```http
Cross-Origin-Embedder-Policy: require-corp
Cross-Origin-Opener-Policy: same-origin
```

**Without these headers, `SharedArrayBuffer` construction throws a `TypeError`.** The engine's client startup code must detect and report this failure:

```typescript
function checkSabAvailability(): boolean {
    if (typeof SharedArrayBuffer === 'undefined') {
        console.error(
            'SharedArrayBuffer is not available. ' +
            'Ensure the server sends Cross-Origin-Embedder-Policy: require-corp ' +
            'and Cross-Origin-Opener-Policy: same-origin headers.'
        );
        return false;
    }
    return true;
}
```

### 10.2 Vite Development Server

The Vite dev server (`playground/vite.config.ts`) must set these headers for local development:

```typescript
// vite.config.ts
export default defineConfig({
    server: {
        headers: {
            'Cross-Origin-Embedder-Policy': 'require-corp',
            'Cross-Origin-Opener-Policy': 'same-origin',
        },
    },
});
```

### 10.3 Security Implications

| Threat | Impact | Mitigation |
|---|---|---|
| **Spectre side-channel** | Malicious code could read arbitrary process memory via high-resolution timers | Cross-origin isolation prevents the page from embedding cross-origin resources without CORS |
| **SAB data exfiltration** | Third-party scripts reading game state from SAB | Cross-origin isolation blocks cross-origin iframes and popups. Only same-origin scripts access SAB. |
| **Game Worker tampering** | Modified WASM writing false data to SAB | WASM module integrity checked via SRI (Subresource Integrity) hash. Server validates all authoritative state. |
| **SAB size limits** | Extremely large SAB allocation could DoS the browser | Engine caps SAB at `SabConfig` limits. Typical: 64 KiB – 768 KiB. |

### 10.4 Fallback When SAB Is Unavailable

If `SharedArrayBuffer` is unavailable (older browsers, misconfigured headers), the engine falls back to `postMessage` with `Transferable` objects:

```typescript
// Fallback: Game Worker → Render Worker via postMessage
if (!sabAvailable) {
    // Transfer Float32Array ownership (zero-copy, but one-shot)
    const stateBuffer = new Float32Array(entityCount * FLOATS_PER_ENTITY);
    fillStateBuffer(stateBuffer, world);
    self.postMessage(
        { type: 'state_update', entities: stateBuffer },
        [stateBuffer.buffer] // Transfer ownership
    );
}
```

This fallback has higher latency (~1–5 ms per message) and prevents the Game Worker from reusing the buffer, but it works everywhere.

---

## 11. Performance Contracts

### 11.1 Metrics

| Metric | Source | Threshold | Action |
|---|---|---|---|
| `aetheris_client_sab_write_seconds` | Game Worker span | > 0.5 ms | Too many entities; reduce max_entities or entity_slot_size |
| `aetheris_client_sab_read_seconds` | Render Worker span | > 0.3 ms | Render Worker overloaded; reduce entity count or simplify interpolation |
| `aetheris_client_postmessage_count` | Per-second counter | > 120 | Excessive postMessage traffic; move high-frequency data to SAB |
| `aetheris_client_sab_fallback_active` | Boolean gauge | true | SAB unavailable — performance degraded, investigate headers |

### 11.2 Budget Allocation

Within the Game Worker's 16.6 ms tick budget:

| Operation | Budget | Notes |
|---|---|---|
| Poll transport | 1.0 ms | Drain inbound datagrams |
| Apply server updates | 2.0 ms | Reconcile prediction |
| Apply local input | 0.5 ms | Predict own movement |
| Simulate (client) | 2.0 ms | Physics prediction |
| **Write SAB** | **0.5 ms** | Write entities to inactive buffer + flip |
| Send input | 0.5 ms | Encode + transmit |
| **Remaining margin** | **10.1 ms** | Absorbs GC pauses, transport latency spikes |

### 11.3 Memory Budget

| Component | Size | Notes |
|---|---|---|
| SAB (game default) | 480 KiB | 4,096 entities × 60 B × 2 buffers |
| SAB (Void Rush) | 768 KiB | 8,192 entities × 48 B × 2 buffers |
| SAB (Nexus) | 64 KiB | Compact transforms + specialized regions |
| WASM heap (Game Worker) | ~4–8 MiB | Client ECS, transport buffers, input history |
| GPU buffers (Render Worker) | ~16–64 MiB | Vertex/index/texture — managed by wgpu |

---

## 12. Open Questions

| # | Question | Context | Status |
|---|---|---|---|
| Q1 | **SAB versioning** | When `EntityDisplayState` struct changes (new field), how do we handle version mismatch between WASM and TypeScript readers? | Open — embed a version byte at SAB offset 0x0008 (reserved). Reader checks version before interpreting slots. |
| Q2 | **Multi-SAB for large worlds** | Can a single 768 KiB SAB handle 8,192 entities? What if we need 20K? Split into multiple SABs? | Deferred — 8K entities is the design ceiling. Interest management keeps visible entities well below this. |
| Q3 | **Render Worker → Game Worker feedback** | Can the Render Worker signal frame timing back to the Game Worker for adaptive quality? | Open — could use a small SAB in the reverse direction (4 bytes: current FPS). |
| Q4 | **Audio Worker** | Should audio processing (spatial audio, voice) be a fourth worker? Or run in the Render Worker? | Deferred to P3. AudioWorklet is a possibility. |
| Q5 | **SAB compression for mobile** | Mobile browsers have tighter memory limits. Should the engine support a 32 KiB "mobile" SAB profile? | Open — add `SabProfile::Mobile` with reduced max_entities. |

---

## Appendix A — Glossary

| Term | Definition |
|---|---|
| **SharedArrayBuffer (SAB)** | JavaScript API providing shared memory between workers. Requires cross-origin isolation. |
| **Double-Buffer** | Two copies of data; one is written while the other is read. Prevents torn reads. |
| **Flip Bit** | An `Atomic<u32>` value (0 or 1) indicating which buffer is currently "active" (readable). |
| **EntityDisplayState** | `repr(C)` struct defining per-entity render data in the SAB. |
| **postMessage** | Browser API for asynchronous message passing between workers. Uses structured cloning. |
| **Structured Clone** | Deep-copy serialization used by `postMessage`. Slower than SAB for high-frequency data. |
| **Transferable** | `postMessage` mode that transfers ownership of a buffer (zero-copy, but sender loses access). |
| **Cross-Origin Isolation** | HTTP header policy (`COEP` + `COOP`) required for `SharedArrayBuffer` access post-Spectre. |
| **OffscreenCanvas** | API allowing canvas rendering in a Worker thread, keeping the Main Thread free for DOM. |
| **repr(C)** | Rust attribute ensuring C-compatible memory layout (no padding reordering). Required for SAB structs. |
| **Pod (Plain Old Data)** | Rust bytemuck trait: type has no padding, no pointers, can be safely cast to/from bytes. |

---

## Appendix B — Decision Log

| # | Decision | Rationale | Revisit If... | Date |
|---|---|---|---|---|
| WC1 | Three-worker architecture (Main + Game + Render) | Prevents DOM layout from blocking game loop, and GPU submission from blocking input. Each worker has its own GC heap. | Browser vendors provide better main-thread scheduling (e.g., `scheduler.yield()`). Could consolidate to two workers. | 2026-04-16 |
| WC2 | SAB for high-frequency state, postMessage for events | SAB is zero-copy but only supports fixed-layout numeric data. postMessage supports arbitrary structured data but copies. The split matches the data profile perfectly. | Browsers add zero-copy structured clone (unlikely). Could consolidate on postMessage. | 2026-04-16 |
| WC3 | Double-buffer with Atomics flip bit (not triple-buffer) | Double-buffer is simpler and sufficient at 60 Hz write / 60–144 Hz read. Triple-buffer adds complexity for marginal benefit. | Reader frame rate >> writer tick rate (e.g., 240 Hz display + 20 Hz server). Triple-buffer prevents reader from re-reading the same frame. | 2026-04-16 |
| WC4 | `repr(C) + Pod` for EntityDisplayState | Must be safe to interpret as raw bytes across WASM and JavaScript boundaries. No padding ambiguity, no pointers. | Need variable-length entity data (strings, arrays). Would require a length-prefixed format instead. | 2026-04-16 |
| WC5 | No direct Game Worker → Render Worker MessagePort | Keeps topology simple (star with Main Thread as hub for events). All high-frequency data goes through SAB anyway. | Frequent one-off events from Game Worker to Render Worker (e.g., particle spawn triggers). Add a dedicated MessagePort. | 2026-04-16 |
| WC6 | SAB layout is application-configurable via SabConfig | Games need different layouts than corporate platforms. Fixed layout would force Nexus to waste space on game fields (health, animation_id) or vice versa. | Configuration complexity becomes a pain point. Provide 3 preset profiles (Game, Nexus, Minimal) and disallow custom. | 2026-04-16 |
| WC7 | Fallback to postMessage + Transferable when SAB unavailable | Some environments (non-isolated pages, older browsers) lack SAB. Graceful degradation is better than hard failure. | SAB support reaches 100% of target browsers. Remove fallback to simplify code. | 2026-04-16 |
