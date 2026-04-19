# Aetheris Playground — User Manual

Broadcasting from the **Sandbox Mode**. This playground is a hermetically sealed sandbox for the Aetheris Engine's WASM client. It allows you to stress test the rendering pipeline, iterate on shaders, and validate procedural meshes without any server dependencies.

---

## 🚀 Quick Start

The playground can be launched in different modes. **Pay attention to the URLs** as they behave differently.

| Command | Mode | Target URL | Intent |
| :--- | :--- | :--- | :--- |
| `just playground` | **Sandbox** | `/playground.html` | pure WASM simulation. No server. |
| `just playground-connected` | **Live** | `/playground.html` | Client + Server. Real Auth + Metrics. |
| `just playground-tls` | **Secure Live** | `/playground.html` | Full TLS/gRPC validation. |
| (Any of above) | **N/A** | `/` (index.html) | **Live App**: Always expects a full server. |

### 1. Sandbox Mode (Sandbox Mode)
Fastest for local iteration. No certificate or server required. Simulation runs entirely in WASM via `tick_playground`.
```bash
just playground
```
👉 [http://localhost:5173/playground.html](http://localhost:5173/playground.html)

### 2. Live Mode (Live)
Connects to a local `aetheris-server`. Requires manual OTP entry from the server logs.
```bash
just playground-connected
```
👉 [http://localhost:5173/playground.html](http://localhost:5173/playground.html)

### 3. Secure Live Mode (TLS/HTTPS)
Used to validate the Full-TLS handshake and gRPC-Web production connectivity.
```bash
just playground-tls
```
> [!IMPORTANT]
> Since this uses self-signed certificates, you might need to navigate to `https://127.0.0.1:50051` once in your browser and "Accept the Risk" if you encounter connectivity issues.

---

## 🛠 Features & Controls

### 1. Entity Spawner
The sidebar allows you to spawn any primitive ship class or game object at the origin `(0,0)`.
- **Interceptor**: High-agility scout.
- **Dreadnought**: Heavy combatant.
- **Hauler**: Large cargo vessel.
- **Asteroid**: Procedural mineral deposit.
- **Projectile**: Fast, short-lived munition.

### 2. Stress Test (The "200 Units" Challenge)
Click the **Stress Test** button to instantly populate the viewport with 200 randomized entities. 
- **Target**: Maintain ≥ 60 FPS on modern hardware.
- **Verification**: Check the **Telemetry** panel in the sidebar for real-time FPS and entity count.

### 3. Simulation Toggle
- **Auto-Rotation**: Enabling this drives a local simulation loop in the WASM Game Worker. This is used to verify that matrix transformations and interpolation snapshots are being applied correctly every frame.

### 4. Telemetry Panel
- **FPS**: Frames per second. Stabilized over a 1-second window.
- **Entities**: Current count of active render objects.
- **SAB State**: Displays the memory address of the `SharedArrayBuffer` being used for synchronization.

---

## 📐 Architectural Context

- **Theme**: The **Blueprint** aesthetic (blue/grid) is intentional. It signifies that you are in a "Simulation Mode" where network authority is disabled.
- **Persistence**: The world resets on every reload. This is a design choice to prevent hardware/browser lockouts in case of a crash caused by extreme stress tests.
- **Workers**: Even in the playground, the **3-Worker Topology** (Main, Game, Render) is preserved to ensure performance parity with the production client.

---

## 🐛 Troubleshooting

| Issue | Solution |
| :--- | :--- |
| **Canvas is blank** | Ensure `SharedArrayBuffer` is enabled in your browser (requires COOP/COEP headers, usually handled automatically by `just playground` or `just playground-tls`, and also applies to `just dev`). |
| **FPS is low** | Check if your browser has **WebGPU** hardware acceleration enabled. Check `chrome://gpu` on Chromium-based browsers. |
| **WASM build fails** | Ensure you have the `nightly-2025-07-01` toolchain installed: `rustup toolchain install nightly-2025-07-01`. |

---
*Ready for integration. See you in the vacuum, Pilot.*
