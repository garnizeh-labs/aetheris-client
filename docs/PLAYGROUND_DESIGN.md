---
Version: 0.2.0
Status: Phase 1 — Live
Phase: P1
Last Updated: 2026-04-18
Authors: Team (Antigravity)
Tier: 3
---

# Aetheris Playground Design

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Objective](#1-objective)
3. [Architecture](#2-architecture)
4. [Communication Protocol](#3-communication-protocol-main---game-worker)
5. [UI Aesthetics](#4-ui-aesthetics-blueprint-theme)
6. [Simulation & Physics](#5-simulation--physics)
7. [Feature Inventory](#6-feature-inventory)
8. [Entry Points & Commands](#7-entry-points--commands)
9. [Roadmap](#8-roadmap)
10. [Appendix A — Glossary](#appendix-a--glossary)
11. [Appendix B — Decision Log](#appendix-b--decision-log)

---

## Executive Summary

The Aetheris Playground is the official interactive sandbox for the engine. It runs the full 3-Worker Topology (Main, Game, Render) with the production WASM binary, providing performance parity with real game sessions while eliminating all server and auth dependencies.

It operates in two modes: **Sandbox** (client-authoritative, no server required) for rapid iteration on rendering and client logic, and **Live** (live WebTransport session via auth-bypass) for validating behaviour with real server state and feeding production metrics into Grafana/Prometheus.

The playground is the primary tool for validating every client-side milestone before integration.

---

## 1. Objective

The Aetheris Playground is the **official interactive sandbox** for the engine. It is a dedicated, sandbox environment for rapid iteration and validation of client-side systems (rendering, physics interpolation, and UI). It bypasses the production authentication and networking layers to provide a zero-latency, deterministic sandbox.

It serves two audiences:

- **Engine developers** — validate render pipeline changes, shader iterations, and WASM builds without standing up a full server session.
- **Feature reviewers** — quickly assess the visual and performance state of the engine on any branch.

The playground supports two operating modes controlled by the `VITE_PLAYGROUND_CONNECTED` environment variable:

| Mode | Env Var | Description |
|---|---|---|
| **Sandbox** (default) | unset | Client-authoritative sandbox. No server required. |
| **Live** | `VITE_PLAYGROUND_CONNECTED=true` | Live WebTransport session. Server-authoritative. Metrics flow into Grafana. |

## 2. Architecture

The Playground follows the same 3-worker topology as the production client:

- **Main Thread**: Drives the Blueprint UI and handles user inputs.
- **Game Worker**: Runs a standalone WASM `AetherisClient` in "Playground Mode".
- **Render Worker**: Renders the `SharedWorld` (SAB) controlled by the Game Worker.

### 2.1 Playground Mode (Sandbox)

In this mode, the `AetherisClient` skips:

- Authentication (OTP/Google).
- WebTransport connection.
- Reassembly of network fragments.

Instead, it reacts to manual IPC commands via the Game Worker.

### 2.2 Playground Mode (Live)

In this mode, the playground performs a full auth + transport handshake automatically using the server's auth-bypass credentials (`smoke-test@aetheris.dev` / `000001`, requires `AETHERIS_AUTH_BYPASS=1` on the server). The world is server-authoritative. The Entity Spawner, Stress Test, and Element Rotation controls are hidden until login completes; once authenticated and connected they are revealed and route commands through the network rather than operating locally. The Telemetry panel is always active and Grafana/Prometheus receive real session metrics.

## 3. Communication Protocol (Main <-> Game Worker)

The Playground introduces a subset of `postMessage` events:

| Event Type | Source | Payload | Description |
|---|---|---|---|
| `init_playground` | Main → Game | `{ memory }` | Bootstraps the Game Worker in playground mode. Triggers telemetry init. |
| `p_spawn` | Main → Game | `{ type, x, y, rot }` | Spawns a new entity at the given coordinates. In Live mode routes to `playground_spawn_net()`; in Sandbox to `playground_spawn()`. |
| `p_clear` | Main → Game | `{}` | Wipes all entities. In Live mode sends a `ClearWorld` protocol event to the server; in Sandbox clears local state only. |
| `p_toggle_rotation` | Main → Game | `{ enabled }` | Sets the global `simulate_rotation` flag on the WASM client. Available in both modes. |
| `p_stress_test` | Main → Game | `{ count, rotate }` | Spawns $N$ randomized entities locally (Sandbox only). Also sets rotation state from `rotate`. |
| `p_stress_test_net` | Main → Game | `{ count, rotate }` | Invokes `playground_stress_test(count, rotate)` via the server (Live mode only). |
| `pause_toggle` | Main → Game/Render | `{ paused }` | Suspends the tick loop and render loop when the browser tab is hidden; resumes on visibility restore. |
| `metrics_frame` | Render → Game | `{ frameTimeMs, fps }` | Posted every 60 rendered frames (~1 s). Game Worker feeds values into the WASM `MetricsCollector`. |
| `wasm_metrics` | Game → Main | `MetricsSnapshot` | Posted every second by the Game Worker. Main thread updates the Telemetry panel UI. |
| `resize` | Main → Render | `{ width, height }` | Notifies the Render Worker to resize the wgpu surface. Triggered by `ResizeObserver` on the canvas element. |

## 4. UI Aesthetics (Blueprint Theme)

To prevent confusion with the production client, the Playground uses a **Blueprint Theme**:

- **Background**: Dark slate blue (`#1e293b`) with a white `#ffffff11` grid pattern. Canvas background is `#000` (opaque black) to prevent transparency artifacts on the GPU surface.
- **Entities**: Rendered with standard materials, but contrasting against the workspace.
- **Sidebar**: Neutral white/light-gray overlays with blueprint-style thin lines.
- **Element Rotation control**: Segmented radio button (ON / OFF) replacing the former toggle switch.

## 5. Simulation & Physics

The Playground simulation is **Client-Authoritative** in Sandbox mode and **Server-Authoritative** in Live mode.

Every `tick_playground()` call (Sandbox):

1. Increments rotation of every entity if enabled.
2. Advances the local tick counter so the Render Worker receives fresh snapshots.
3. Commits the current world state to the `SharedWorld` pointer.
4. The Render Worker picks up the change as usual.

In Live mode, `tick()` processes incoming server replication events. The `ClearWorld` network event causes the server to despawn all entities and the client to clear its local entity map.

The tick loop uses `setTimeout` (not `setInterval`) for precise 60 Hz scheduling. A `pause_toggle` IPC message throttles both workers to a 500 ms sleep when the browser tab is hidden, preventing accumulated lag on resume.

---

## 6. Feature Inventory

This section is the living record of all implemented playground features. Update it alongside any code change that adds, removes, or modifies a capability.

### 6.1 Entity Spawner

**Status:** Implemented (P1)

Allows manual spawning of any supported entity type at a **uniformly random position** within ±20 units via the Blueprint sidebar. Each click picks a random `(x, y)` and rotation so entities do not pile up at the origin.

| Entity Type | Internal ID | Description |
|---|---|---|
| Interceptor | `1` | High-agility scout ship |
| Dreadnought | `3` | Heavy combatant |
| Hauler | `4` | Large cargo vessel |
| Asteroid | `5` | Procedural mineral deposit |
| Projectile | `6` | Fast, short-lived munition. |

**WASM surface (Sandbox):** `playground_spawn(type, x, y, rot)`
**WASM surface (Live):** `playground_spawn_net(type, x, y, rot)` — forwards the spawn request to the server over WebTransport.

### 6.2 Stress Test

**Status:** Implemented (P1)

Populates the viewport with 200 randomized entities in a single click. Entity types are drawn from the supported set; positions and rotations are uniformly randomized within a ±20 unit bounding square. The rotation state of the **Element Rotation** segmented control is read at dispatch time and forwarded as the `rotate` parameter.

- **Target:** ≥ 60 FPS on modern hardware under full load.
- **WASM surface (Sandbox):** `playground_clear()` + `playground_spawn()` × N (client-side loop).
- **WASM surface (Live):** `playground_stress_test(count, rotate)` — single call; the server generates positions using a deterministic LCG seeded with `0xDEAD_BEEF`.

### 6.3 Element Rotation Simulation

**Status:** Implemented (P1)

A **segmented radio button** (ON / OFF, default OFF) that drives a local rotation loop inside the WASM Game Worker. When enabled, every entity's rotation is incremented each tick at 60 Hz. Works in both Sandbox and Live modes.

In Live mode the local tick loop applies the rotation client-side after receiving replication updates from the server, so the visual result is consistent regardless of mode. Used to verify that matrix transformations and interpolation snapshots apply correctly across the SAB boundary.

- **WASM surface:** `playground_set_rotation_enabled(enabled)`
- **Tick rate:** 60 Hz (recursive `setTimeout` loop in `playground/src/game.worker.ts`)

### 6.4 Telemetry Panel

**Status:** Implemented (P1, extended M10105)

Real-time sidebar panel exposing two tiers of metrics:

**Host metrics** (JavaScript, updated by `startMonitoring()` loop):

| Metric | Element ID | Description |
|---|---|---|
| **FPS (Host)** | `stat-fps` | Frames per second measured by the JS `requestAnimationFrame` loop, stabilized over a 1-second window |
| **Session** | `stat-session` | Auth state (`LOGGED OUT` / `AWAITING LOGIN` / `LOGGED IN`) |
| **SAB** | `stat-sab` | `SharedArrayBuffer` status and latest snapshot count |

**WASM metrics** (sourced from `MetricsCollector`, posted as `wasm_metrics` every second):

| Metric | Element ID | Description |
|---|---|---|
| **FPS (WASM)** | `stat-wasm-fps` | FPS accumulated inside the WASM render loop; color-coded (blue ≥55, amber ≥30, red <30) |
| **Frame Time** | `stat-frame-time` | p99 render frame time in ms |
| **Sim Time** | `stat-sim-time` | p99 WASM tick/simulation time in ms |
| **RTT** | `stat-rtt` | Round-trip time to server in ms (Live mode only; shows `N/A` in Sandbox); color-coded (green <100 ms, amber <250 ms, red ≥250 ms) |
| **Entities** | `stat-entities` | Live entity count from the WASM world state |
| **Dropped** | `stat-dropped` | Cumulative count of telemetry events dropped due to ring-buffer overflow; shown in red when > 0 |

**Metrics pipeline:**

```
Render Worker: measures frame time per requestAnimationFrame
    → postMessage({ type: 'metrics_frame', ... }) every 60 frames
    → Main Thread routes to Game Worker
    → Game Worker: wasm_record_frame_time() → MetricsCollector
    → setTimeout(pollMetrics, 1000): wasm_get_metrics() → postMessage({ type: 'wasm_metrics' })
    → Main Thread: updateWasmMetrics() → DOM
```

WASM telemetry events are flushed every 5 seconds via a fire-and-forget `fetch()` to `POST /telemetry/json` (see [6.6 WASM Observability](#66-wasm-observability-m10105)). An additional flush fires on tab close via the `beforeunload` event and on worker resume after a `pause_toggle`.

### 6.5 Blueprint Theme

**Status:** Implemented (P1)

Visual identity that distinguishes the Playground from the production client. Prevents accidental confusion between simulation mode and live sessions. See [Section 4](#4-ui-aesthetics-blueprint-theme) for the full spec.

---

### 6.6 WASM Observability (M10105)

**Status:** Implemented (P1)

A thread-local `MetricsCollector` running inside the Game Worker accumulates telemetry events and performance samples. It is initialized at `init_playground` time via `wasm_init_telemetry(url)` using the `VITE_TELEMETRY_URL` environment variable (default `http://127.0.0.1:50055`).

**Session identity:**

Each Game Worker startup generates a single [ULID](https://github.com/ulid/spec) that produces two IDs:

- `session_id` — Crockford base32 (26 chars). Groups all events from the same browser tab in Loki.
- `trace_id` — ULID u128 as 32 lowercase hex chars. W3C TraceContext-compatible; chronologically ordered in Jaeger.

**Lifecycle span events emitted automatically:**

| Span Name | Level | Trigger |
|---|---|---|
| `wasm_init` | INFO | `AetherisClient::new()` |
| `connect_handshake` | INFO | WebTransport connected |
| `connect_handshake_failed` | ERROR | WebTransport connection error |
| `render_pipeline_setup` | INFO | `init_renderer()` completed |

**Flush schedule:**

| Trigger | Action |
|---|---|
| Every 5 s | `wasm_flush_telemetry()` via `setInterval` |
| Tab close | `beforeunload` event |
| Worker resume | After `pause_toggle { paused: false }` |

**Cardinality rule (M10105 §5.2):** Server-side Prometheus labels use only static values (`client_type = "wasm_playground"`). `session_id` is sent as a field in the batch payload for Loki correlation only — never as a Prometheus label.

**Source:** `crates/aetheris-client-wasm/src/metrics.rs`

---

### 6.7 Canvas Resize Support

**Status:** Implemented (P1)

A `ResizeObserver` attached to `#engine-canvas` in the Main Thread detects CSS layout changes and posts a `resize` message to the Render Worker. The Render Worker calls `AetherisClient::resize(width, height)` to resize the wgpu surface in-place without reinitializing the renderer. Physical pixel dimensions are computed using `window.devicePixelRatio`.

---

### 6.8 Worker Pause / Resume

**Status:** Implemented (P1)

The `document.visibilitychange` event is monitored in the Main Thread. When the browser tab is hidden, a `pause_toggle { paused: true }` message is sent to both workers:

- **Game Worker**: tick loop throttles to a 500 ms `setTimeout` check interval.
- **Render Worker**: `requestAnimationFrame` loop stops until `running` is restored; resumes automatically when `pause_toggle { paused: false }` is received.

This prevents accumulated lag and frame bursts when the user returns to the tab.

---

## 7. Entry Points & Commands

The playground lives in `playground/` and is served by the same Vite dev server as the production client entry point.

| URL | Mode | Command |
|---|---|---|
| `http://localhost:5173/playground.html` | Sandbox — HTTP | `just playground` |
| `http://localhost:5173/playground.html` | Sandbox — HTTPS/TLS | `just playground-tls` |
| `http://localhost:5173/playground.html` | **Live** — live session | `just playground-connected` |

The playground HTML entry point is `playground/playground.html`. The production client entry point remains `playground/index.html`.

> **Note:** Even in playground mode, the full 3-Worker Topology (Main, Game, Render) is preserved to ensure performance parity with production.

---

## 8. Roadmap

Features planned for future playground iterations. Move items to [Section 6](#6-feature-inventory) once implemented.

| Feature | Priority | Notes |
|---|---|---|
| Entity inspector panel | P2 | Click-to-select entity; show position, rotation, type in sidebar |
| Free-camera controls | P2 | Pan/zoom the viewport without needing a connected server |
| Custom spawn coordinates | P2 | Input fields for X/Y instead of random positions |
| Replay recording | P3 | Record a session and replay it frame-by-frame for debugging |
| Network simulation mode | P3 | Inject artificial latency/packet-loss to test interpolation under stress |
| Shader hot-reload | P3 | Live-patch WGSL shaders without a full WASM rebuild |

---

## Appendix A — Glossary

| Term | Definition |
|---|---|
| **Playground Mode** | Operating mode of `AetherisClient` that skips auth and WebTransport, reacting only to IPC commands. |
| **Sandbox Mode** | Playground variant where no server is involved; world state is fully client-authoritative. |
| **Live Mode** | Playground variant that establishes a real WebTransport session using auth-bypass credentials. |
| **Blueprint Theme** | Visual identity (blue/grid) applied to the playground UI to distinguish it from the production client. |
| **SAB** | `SharedArrayBuffer` — the zero-copy bridge between the Game Worker and Render Worker. |
| **3-Worker Topology** | The Main Thread + Game Worker + Render Worker architecture shared by the playground and the production client. |
| **Auth Bypass** | Server-side flag (`AETHERIS_AUTH_BYPASS=1`) that accepts the smoke-test credentials without a real OTP flow. |
| **MetricsCollector** | Thread-local WASM struct that accumulates telemetry events and performance samples inside the Game Worker (`crates/aetheris-client-wasm/src/metrics.rs`). |
| **ULID** | Universally Unique Lexicographically Sortable Identifier — used to generate the `session_id` and `trace_id` for each Game Worker session. |
| **ClearWorld** | Protocol event (`NetworkEvent::ClearWorld`) that instructs the server to despawn all entities and the client to clear its local entity map. |

---

## Appendix B — Decision Log

| Date | Decision | Rationale |
|---|---|---|
| 2026-04-17 | Promoted `client/` to `playground/` | The directory had become the official engine sandbox; renaming aligns naming with purpose. |
| 2026-04-17 | Added Live Mode via `VITE_PLAYGROUND_CONNECTED` | Enables Grafana/Prometheus validation and catches server-side bugs without opening the full production client. |
| 2026-04-17 | Auth bypass uses `smoke-test@aetheris.dev` / `000001` | Consistent with existing smoke-test conventions; credentials are meaningless outside `AETHERIS_AUTH_BYPASS=1`. |
| 2026-04-17 | Prometheus exporter started unconditionally in `main.rs` | The exporter was defined but never installed; `:9000/metrics` was silently a 404 until this fix. |
| 2026-04-18 | Entity spawner uses random positions instead of origin | Entities piling at `(0, 0)` made visual validation impossible; ±20 unit random spread matches stress-test behaviour. |
| 2026-04-18 | Spawner/rotation/clear enabled in both modes | Guards that blocked connected-mode actions prevented feature parity testing; in Live mode commands route through the network. |
| 2026-04-18 | Rotation control changed from toggle to segmented radio | Toggle state was ambiguous after a stress test; radio buttons make ON/OFF state visually explicit and are read by `stressTest()` at dispatch time. |
| 2026-04-18 | Tick loop changed from `setInterval` to recursive `setTimeout` | `setInterval` can queue multiple callbacks if a tick takes longer than the interval, causing burst CPU spikes; `setTimeout` self-schedules only after the previous tick completes. |
| 2026-04-18 | `pause_toggle` IPC on `visibilitychange` | Prevents accumulated tick/render lag after the user switches tabs. On resume, a telemetry flush is triggered to avoid data loss. |
| 2026-04-18 | WASM observability (M10105) via `MetricsCollector` | Separate out-of-band JSON flush to `/telemetry/json` keeps metrics independent of the WebTransport data path; avoids head-of-line blocking. ULID-based IDs give Jaeger trace correlation without any server-side changes. |
| 2026-04-18 | Canvas background changed to `#000` | Transparent canvas caused see-through artifacts on some GPU drivers when the wgpu surface was not fully initialized; opaque black is safe on all platforms. |
| 2026-04-18 | `ResizeObserver` forwards physical pixels to Render Worker | CSS `clientWidth/Height` in the Main Thread is the only reliable source of canvas dimensions; the Render Worker has no access to the DOM after `transferControlToOffscreen`. |
| 2026-04-21 | System Manifest (on-demand pull) | Replaces rigid version fields with extensible `BTreeMap`. Permissions (JTI) allow exposing debug metrics only to admins. |
| 2026-04-21 | Possession flag (0x04) late-synchronization | Fixes race where `Possession` event arrives before entity replication. Client re-checks ID against every update. |
| 2026-04-21 | Input log suppression (repeated commands) | Downgrades repeated inputs to `TRACE` to avoid flooding the console while holding movement keys. |
