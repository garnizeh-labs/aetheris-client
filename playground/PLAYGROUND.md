# Aetheris Playground — Infrastructure Manual

Broadcasting from the **Authoritative Mode**. The playground has been refactored to align with **VS-01 (One Ship, One Sector)** requirements. It serves as a dedicated validation tool for the server-authoritative pipeline, ensuring that authentication, possession, and flight physics are working correctly within the distributed environment.

---

## 🚀 Quick Start

The playground now mandates a connection to an active `aetheris-server`.

| Command | Intent |
| :--- | :--- |
| `just playground` | Primary entry point. Connects to local server with real Auth + Metrics. |
| `just playground-tls` | Full TLS/gRPC validation for production-like handshake testing. |

### 1. Launching
```bash
just playground
```
👉 [http://localhost:5173/playground.html](http://localhost:5173/playground.html)

### 2. Authentication
1. Launch the server (usually in a separate terminal).
2. Enter your email in the **Access Control** panel.
3. Retrieve the 6-digit code from the server logs.
4. Verify & Connect.

---

## 🛠 Features & Controls

### 1. Session Control
Once authenticated, you can start a session:
- **Start Session**: Requests a ship from the server. Upon successful possession, your ship will be spawned in the sector.
- **Stop Session**: Clears the world and disconnects your authoritative entity.

### 2. Telemetry Panel
Provides high-fidelity diagnostics from the WASM client:
- **FPS (Host/WASM)**: Real-time rendering and simulation performance.
- **RTT**: Round-trip time to the server via WebTransport.
- **Entities**: Count of active entities synchronized from the server.

### 3. Keyboard Input
Visual feedback for active input commands being sent to the server.
- **WASD/Arrows**: Movement controls.
- **Space**: Secondary actions.

---

## 📐 Architectural Context

- **Authoritative Validation**: Network authority is always enabled. The client no longer runs a local sandbox simulation; all state transitions are derived from server-replicated snapshots.
- **VS-01 Compliance**: The scope is strictly limited to "One Ship, One Sector" validation. Extraneous sandbox features (manual spawning, local stress tests) have been removed to ensure the playground remains a focused validation tool.
- **3-Worker Topology**: The **Main, Game, Render** worker model remains active, providing a true-to-life environment for performance benchmarking.
- **Client-Side Prediction**: Simulation of local player transformations is currently **optional and disabled by default** to ensure absolute parity with server authoritative state during VS-01 validation.
    - **Status**: Pure Server-Authority (Local simulation is OFF).
    - **How to Enable**: Modify `ClientWorld::new()` in `crates/aetheris-client-wasm/src/world_state.rs` to call `with_prediction(true)` instead of `false`. This will activate local input replay and reconciliation.

---

## 🐛 Troubleshooting

| Issue | Solution |
| :--- | :--- |
| **"Authentication Required"** | You must log in via the Access Control panel before starting a session. |
| **RTT is "N/A"** | Ensure the server is running and the WebTransport connection has been established. |
| **Canvas is blank** | Ensure `SharedArrayBuffer` and **WebGPU** are supported and enabled in your browser. |

---
*Validation complete. Stay aligned, Pilot.*
