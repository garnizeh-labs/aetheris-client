# Aetheris Playground Guide

The Aetheris Playground is a developer tool for testing the Aetheris Engine's simulation and rendering pipelines in both isolated (Sandbox) and server-authoritative (Connected) modes.

## Keyboard Controls

| Key | Action | Mapping |
|---|---|---|
| **W** / **↑** | Thrust Forward | `move_y = 1.0` |
| **S** / **↓** | Thrust Backward | `move_y = -1.0` |
| **A** / **←** | Strafe Left | `move_x = -1.0` |
| **D** / **→** | Strafe Right | `move_x = 1.0` |
| **Q** | Visual: Roll Left | (Indicator only) |
| **E** | Visual: Roll Right | (Indicator only) |
| **Space** | Fire Primary | `actions_mask |= 0x01` |
| **F** | Mining Action | `actions_mask |= 0x02` |

## System Shortcuts

| Shortcut | Action |
|---|---|
| **F1** / **?** | Show Keyboard Shortcuts |
| **F3** | Cycle Debug Rendering (Wireframe, Components) |
| **F4** | Toggle Global Grid |
| **Alt+T** | Cycle Theme (Blueprint, Frost Dawn, etc.) |

## Simulation Modes

### Sandbox Mode (Local)
Input is processed locally in the Game Worker. `playground_apply_input` uses a local Newtonian model to update the player ship's position. Useful for UI/UX testing and isolated physics validation.

### Connected Mode (Server)
Input is dispatched to the server via `send_input`. The server computes the authoritative state and sends back updates. The client performs interpolation/prediction.
