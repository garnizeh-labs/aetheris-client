//! The Aetheris native client application for desktop platforms.
//!
//! **Phase:** P4 - Platform Expansion
//! **Constraint:** Target native QUIC (Quinn) and Vulkan/Metal/DX12 via wgpu.
//! **Purpose:** High-performance desktop alternative to the WASM client, reusing the
//! same Trait Facade logic while leveraging native thread scheduling and I/O.

#![warn(clippy::all, clippy::pedantic)]

fn main() {
    println!("Aetheris Native Client initialized.");
}
