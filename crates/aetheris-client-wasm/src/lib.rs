//! Aetheris WASM client logic.
//!
//! This crate implements the browser-based client for the Aetheris Engine,
//! using `WebWorkers` for multi-threaded execution and `WebGPU` for rendering.

#![warn(clippy::all, clippy::pedantic)]
// Required to declare `#[thread_local]` statics on nightly (wasm32 target).
// This feature gate is only active when compiling for WASM — see the
// `_TLS_ANCHOR` declaration below.
#![cfg_attr(
    all(target_arch = "wasm32", feature = "nightly"),
    feature(thread_local)
)]

pub mod auth;
pub mod shared_world;
pub mod world_state;

#[cfg(target_arch = "wasm32")]
pub mod metrics;

#[cfg(test)]
#[cfg(target_arch = "wasm32")]
pub mod smoke_test;

/// Protobuf message types generated from `auth.proto` (prost only — no service stubs).
/// Used by `auth.rs` so it doesn't need the `aetheris-protocol` `grpc` feature,
/// which would pull in `tonic::transport` and hence `mio` (incompatible with wasm32).
pub mod auth_proto {
    #![allow(clippy::must_use_candidate, clippy::doc_markdown)]
    tonic::include_proto!("aetheris.auth.v1");
}

#[cfg(target_arch = "wasm32")]
pub mod transport;

#[cfg(target_arch = "wasm32")]
pub mod render;

#[cfg(target_arch = "wasm32")]
pub mod render_primitives;

#[cfg(target_arch = "wasm32")]
#[cfg_attr(feature = "nightly", thread_local)]
static _TLS_ANCHOR: u8 = 0;

use std::sync::atomic::AtomicUsize;
#[cfg(target_arch = "wasm32")]
use std::sync::atomic::Ordering;

#[allow(dead_code)]
static NEXT_WORKER_ID: AtomicUsize = AtomicUsize::new(1);

#[cfg(target_arch = "wasm32")]
thread_local! {
    static WORKER_ID: usize = NEXT_WORKER_ID.fetch_add(1, Ordering::Relaxed);
}

/// Helper to get `performance.now()` in both Window and Worker contexts.
#[must_use]
pub fn performance_now() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let global = js_sys::global();

        // Try WorkerGlobalScope first
        if let Ok(worker) = global.clone().dyn_into::<web_sys::WorkerGlobalScope>() {
            return worker.performance().map(|p| p.now()).unwrap_or(0.0);
        }

        // Try Window
        if let Ok(window) = global.dyn_into::<web_sys::Window>() {
            return window.performance().map(|p| p.now()).unwrap_or(0.0);
        }

        // Fallback to Date
        js_sys::Date::now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0.0
    }
}

#[allow(dead_code)]
pub(crate) fn get_worker_id() -> usize {
    #[cfg(target_arch = "wasm32")]
    {
        WORKER_ID.with(|&id| id)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use crate::metrics::with_collector;
    use crate::performance_now;
    use crate::render::RenderState;
    use crate::shared_world::{MAX_ENTITIES, SabSlot, SharedWorld};
    use crate::transport::WebTransportBridge;
    use crate::world_state::ClientWorld;
    use aetheris_encoder_serde::SerdeEncoder;
    use aetheris_protocol::events::NetworkEvent;
    use aetheris_protocol::traits::GameTransport;
    use aetheris_protocol::types::ClientId;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ConnectionState {
        Disconnected,
        Connecting,
        InGame,
        Reconnecting,
        Failed,
    }

    /// A snapshot of the world for interpolation.
    #[derive(Clone)]
    pub struct SimulationSnapshot {
        pub tick: u64,
        pub entities: Vec<SabSlot>,
    }

    /// Global state held by the WASM instance.
    #[wasm_bindgen]
    pub struct AetherisClient {
        shared_world: SharedWorld,
        world_state: ClientWorld,
        render_state: Option<RenderState>,
        transport: Option<WebTransportBridge>,
        worker_id: usize,
        session_token: Option<String>,

        // Interpolation state (Render Worker only)
        snapshots: std::collections::VecDeque<SimulationSnapshot>,

        // Network metrics
        last_rtt_ms: f64,
        ping_counter: u64,

        reassembler: aetheris_protocol::Reassembler,
        connection_state: ConnectionState,
        reconnect_attempts: u32,
        playground_rotation_enabled: bool,
        playground_next_network_id: u64,
        first_playground_tick: bool,

        // Reusable buffer for zero-allocation rendering (Render Worker only)
        render_buffer: Vec<SabSlot>,
    }

    #[wasm_bindgen]
    impl AetherisClient {
        /// Creates a new AetherisClient instance.
        /// If a pointer is provided, it will use it as the backing storage (shared memory).
        #[wasm_bindgen(constructor)]
        pub fn new(shared_world_ptr: Option<u32>) -> Result<AetherisClient, JsValue> {
            console_error_panic_hook::set_once();

            // tracing_wasm doesn't have a clean try_init for global default.
            // We use a static atomic to ensure only the first worker sets the global default.
            use std::sync::atomic::{AtomicBool, Ordering};
            static LOGGER_INITIALIZED: AtomicBool = AtomicBool::new(false);
            if !LOGGER_INITIALIZED.swap(true, Ordering::SeqCst) {
                let config = tracing_wasm::WASMLayerConfigBuilder::new()
                    .set_max_level(tracing::Level::INFO)
                    .build();
                tracing_wasm::set_as_global_default_with_config(config);
            }

            let shared_world = if let Some(ptr_val) = shared_world_ptr {
                let ptr = ptr_val as *mut u8;

                // Security: Validate the incoming pointer before use
                if ptr_val == 0 || !ptr_val.is_multiple_of(8) {
                    return Err(JsValue::from_str(
                        "Invalid shared_world_ptr: null or unaligned",
                    ));
                }

                if !SharedWorld::is_valid(ptr) {
                    return Err(JsValue::from_str("Invalid or stale shared_world_ptr"));
                }

                unsafe { SharedWorld::from_ptr(ptr) }
            } else {
                SharedWorld::new()
            };

            tracing::info!(
                "AetherisClient initialized on worker {}",
                crate::get_worker_id()
            );

            // M10105 — emit wasm_init lifecycle span
            with_collector(|c| {
                c.push_event(
                    1,
                    "wasm_client",
                    "AetherisClient initialized",
                    "wasm_init",
                    None,
                );
            });

            Ok(Self {
                shared_world,
                world_state: ClientWorld::new(),
                render_state: None,
                transport: None,
                worker_id: crate::get_worker_id(),
                session_token: None,
                snapshots: std::collections::VecDeque::with_capacity(8),
                last_rtt_ms: 0.0,
                ping_counter: 0,
                reassembler: aetheris_protocol::Reassembler::new(),
                connection_state: ConnectionState::Disconnected,
                reconnect_attempts: 0,
                playground_rotation_enabled: false,
                playground_next_network_id: 1,
                first_playground_tick: true,
                render_buffer: Vec::with_capacity(crate::shared_world::MAX_ENTITIES),
            })
        }

        #[wasm_bindgen]
        pub async fn request_otp(base_url: String, email: String) -> Result<String, String> {
            crate::auth::request_otp(base_url, email).await
        }

        #[wasm_bindgen]
        pub async fn login_with_otp(
            base_url: String,
            request_id: String,
            code: String,
        ) -> Result<String, String> {
            crate::auth::login_with_otp(base_url, request_id, code).await
        }

        #[wasm_bindgen]
        pub async fn logout(base_url: String, session_token: String) -> Result<(), String> {
            crate::auth::logout(base_url, session_token).await
        }

        #[wasm_bindgen(getter)]
        pub fn connection_state(&self) -> ConnectionState {
            self.connection_state
        }

        fn check_worker(&self) {
            debug_assert_eq!(
                self.worker_id,
                crate::get_worker_id(),
                "AetherisClient accessed from wrong worker! It is pin-bound to its creating thread."
            );
        }

        /// Returns the raw pointer to the shared world buffer.
        pub fn shared_world_ptr(&self) -> u32 {
            self.shared_world.as_ptr() as u32
        }

        pub async fn connect(
            &mut self,
            url: String,
            cert_hash: Option<Vec<u8>>,
        ) -> Result<(), JsValue> {
            self.check_worker();

            if self.connection_state == ConnectionState::Connecting
                || self.connection_state == ConnectionState::InGame
                || self.connection_state == ConnectionState::Reconnecting
            {
                return Ok(());
            }

            // M10105 — emit reconnect_attempt if it looks like one
            if self.connection_state == ConnectionState::Failed && self.reconnect_attempts > 0 {
                with_collector(|c| {
                    c.push_event(
                        2,
                        "transport",
                        "Triggering reconnection",
                        "reconnect_attempt",
                        None,
                    );
                });
            }

            self.connection_state = ConnectionState::Connecting;
            tracing::info!(url = %url, "Connecting to server...");

            let transport_result = WebTransportBridge::connect(&url, cert_hash.as_deref()).await;

            match transport_result {
                Ok(transport) => {
                    // Security: Send Auth message immediately after connection
                    if let Some(token) = &self.session_token {
                        use aetheris_encoder_serde::SerdeEncoder;
                        use aetheris_protocol::traits::GameTransport;
                        use aetheris_protocol::types::ClientId;

                        let encoder = SerdeEncoder::new();
                        let auth_event = NetworkEvent::Auth {
                            session_token: token.clone(),
                        };

                        match encoder.encode_event(&auth_event) {
                            Ok(data) => {
                                if let Err(e) = transport.send_reliable(ClientId(0), &data).await {
                                    self.connection_state = ConnectionState::Failed;
                                    tracing::error!(error = ?e, "Handshake failed: could not send auth packet");
                                    return Err(JsValue::from_str(&format!(
                                        "Failed to send auth packet: {:?}",
                                        e
                                    )));
                                }
                                tracing::info!("Auth packet sent to server");
                            }
                            Err(e) => {
                                self.connection_state = ConnectionState::Failed;
                                tracing::error!(error = ?e, "Handshake failed: could not encode auth packet");
                                return Err(JsValue::from_str("Failed to encode auth packet"));
                            }
                        }
                    } else {
                        tracing::warn!(
                            "Connecting without session token! Server will likely discard data."
                        );
                    }

                    self.transport = Some(transport);
                    self.connection_state = ConnectionState::InGame;
                    self.reconnect_attempts = 0;
                    tracing::info!("WebTransport connection established");
                    // M10105 — connect_handshake lifecycle span
                    with_collector(|c| {
                        c.push_event(
                            1,
                            "transport",
                            &format!("WebTransport connected: {url}"),
                            "connect_handshake",
                            None,
                        );
                    });
                    Ok(())
                }
                Err(e) => {
                    self.connection_state = ConnectionState::Failed;
                    tracing::error!(error = ?e, "Failed to establish WebTransport connection");
                    // M10105 — connect_handshake_failed lifecycle span (ERROR level)
                    with_collector(|c| {
                        c.push_event(
                            3,
                            "transport",
                            &format!("WebTransport failed: {url} — {e:?}"),
                            "connect_handshake_failed",
                            None,
                        );
                    });
                    Err(JsValue::from_str(&format!("failed to connect: {e:?}")))
                }
            }
        }

        /// Sets the session token to be used for authentication upon connection.
        pub fn set_session_token(&mut self, token: String) {
            self.session_token = Some(token);
        }

        /// Initializes rendering with a canvas element.
        /// Accepts either web_sys::HtmlCanvasElement or web_sys::OffscreenCanvas.
        pub async fn init_renderer(&mut self, canvas: JsValue) -> Result<(), JsValue> {
            self.check_worker();
            use wasm_bindgen::JsCast;

            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
                flags: wgpu::InstanceFlags::default(),
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });

            // Handle both HtmlCanvasElement and OffscreenCanvas for Worker support
            let (surface_target, width, height) =
                if let Ok(html_canvas) = canvas.clone().dyn_into::<web_sys::HtmlCanvasElement>() {
                    let width = html_canvas.width();
                    let height = html_canvas.height();
                    tracing::info!(
                        "Initializing renderer on HTMLCanvasElement ({}x{})",
                        width,
                        height
                    );
                    (wgpu::SurfaceTarget::Canvas(html_canvas), width, height)
                } else if let Ok(offscreen_canvas) =
                    canvas.clone().dyn_into::<web_sys::OffscreenCanvas>()
                {
                    let width = offscreen_canvas.width();
                    let height = offscreen_canvas.height();
                    tracing::info!(
                        "Initializing renderer on OffscreenCanvas ({}x{})",
                        width,
                        height
                    );

                    // Critical fix for wgpu 0.20+ on WASM workers:
                    // Ensure the context is initialized with the 'webgpu' id before creating the surface.
                    let _ = offscreen_canvas.get_context("webgpu").map_err(|e| {
                        JsValue::from_str(&format!("Failed to get webgpu context: {:?}", e))
                    })?;

                    (
                        wgpu::SurfaceTarget::OffscreenCanvas(offscreen_canvas),
                        width,
                        height,
                    )
                } else {
                    return Err(JsValue::from_str(
                        "Aetheris: Provided object is not a valid Canvas or OffscreenCanvas",
                    ));
                };

            let surface = instance
                .create_surface(surface_target)
                .map_err(|e| JsValue::from_str(&format!("Failed to create surface: {:?}", e)))?;

            let render_state = RenderState::new(&instance, surface, width, height)
                .await
                .map_err(|e| JsValue::from_str(&format!("Failed to init renderer: {:?}", e)))?;

            self.render_state = Some(render_state);

            // M10105 — emit render_pipeline_setup lifecycle span
            with_collector(|c| {
                c.push_event(
                    1,
                    "render_worker",
                    &format!("Renderer initialized ({}x{})", width, height),
                    "render_pipeline_setup",
                    None,
                );
            });

            Ok(())
        }

        #[wasm_bindgen]
        pub fn resize(&mut self, width: u32, height: u32) {
            if let Some(state) = &mut self.render_state {
                state.resize(width, height);
            }
        }

        #[wasm_bindgen]
        pub fn set_debug_mode(&mut self, mode: u32) {
            self.check_worker();
            if let Some(state) = &mut self.render_state {
                state.set_debug_mode(match mode {
                    0 => crate::render::DebugRenderMode::Off,
                    1 => crate::render::DebugRenderMode::Wireframe,
                    2 => crate::render::DebugRenderMode::Components,
                    _ => crate::render::DebugRenderMode::Full,
                });
            }
        }

        #[wasm_bindgen]
        pub fn set_theme_colors(&mut self, bg_base: &str, text_primary: &str) {
            self.check_worker();
            let clear = crate::render::parse_css_color(bg_base);
            let label = crate::render::parse_css_color(text_primary);

            tracing::info!(
                "Aetheris Client: Applying theme colors [bg: {} -> {:?}, text: {} -> {:?}]",
                bg_base,
                clear,
                text_primary,
                label
            );

            if let Some(state) = &mut self.render_state {
                state.set_clear_color(clear);
                #[cfg(debug_assertions)]
                state.set_label_color([
                    label.r as f32,
                    label.g as f32,
                    label.b as f32,
                    label.a as f32,
                ]);
            }
        }

        #[cfg(debug_assertions)]
        #[wasm_bindgen]
        pub fn cycle_debug_mode(&mut self) {
            if let Some(state) = &mut self.render_state {
                state.cycle_debug_mode();
            }
        }

        #[cfg(debug_assertions)]
        #[wasm_bindgen]
        pub fn toggle_grid(&mut self) {
            if let Some(state) = &mut self.render_state {
                state.toggle_grid();
            }
        }

        /// Simulation tick called by the Network Worker at a fixed rate (e.g. 20Hz).
        pub async fn tick(&mut self) {
            self.check_worker();
            use aetheris_protocol::traits::{Encoder, GameTransport, WorldState};

            let encoder = SerdeEncoder::new();

            // 0. Reconnection Logic
            // TODO: poll transport.closed() promise and trigger reconnection state machine
            if let Some(_transport) = &self.transport {}

            // 0.1 Periodic Ping (approx. every 1 second at 60Hz)
            if let Some(transport) = &mut self.transport {
                self.ping_counter = self.ping_counter.wrapping_add(1);
                if self.ping_counter % 60 == 0 {
                    // Use current time as timestamp (ms)
                    let now = performance_now();
                    let tick_u64 = now as u64;

                    if let Ok(data) = encoder.encode_event(&NetworkEvent::Ping {
                        client_id: ClientId(0), // Client doesn't know its ID yet usually
                        tick: tick_u64,
                    }) {
                        web_sys::console::debug_1(
                            &format!("[Aetheris] Sending Ping: tick={tick_u64}").into(),
                        );
                        let _ = transport.send_unreliable(ClientId(0), &data).await;
                    }
                }
            }

            // 1. Poll Network
            if let Some(transport) = &mut self.transport {
                let events = match transport.poll_events().await {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::error!("Transport poll failure: {:?}", e);
                        return;
                    }
                };
                let mut updates: Vec<(ClientId, aetheris_protocol::events::ComponentUpdate)> =
                    Vec::new();

                for event in events {
                    match event {
                        NetworkEvent::UnreliableMessage { data, client_id }
                        | NetworkEvent::ReliableMessage { data, client_id } => {
                            if let Ok(update) = encoder.decode(&data) {
                                updates.push((client_id, update));
                            }
                        }
                        NetworkEvent::ClientConnected(id) => {
                            tracing::info!(?id, "Server connected");
                        }
                        NetworkEvent::ClientDisconnected(id) => {
                            tracing::warn!(?id, "Server disconnected");
                        }
                        NetworkEvent::Ping { client_id: _, tick } => {
                            // Immediately reflect the ping as a pong with same tick
                            let pong = NetworkEvent::Pong { tick };
                            if let Ok(data) = encoder.encode_event(&pong) {
                                let _ = transport.send_reliable(ClientId(0), &data).await;
                            }
                        }
                        NetworkEvent::Pong { tick } => {
                            // Calculate RTT from our own outgoing pings
                            let now = performance_now();
                            let rtt = now - (tick as f64);
                            self.last_rtt_ms = rtt;

                            with_collector(|c| {
                                c.update_rtt(rtt);
                                // M10105 — also emit span if it looks like a reconnect
                                if self.connection_state == ConnectionState::Reconnecting {
                                    c.push_event(
                                        2,
                                        "transport",
                                        "RTT recovered",
                                        "reconnect_success",
                                        Some(rtt),
                                    );
                                }
                            });

                            #[cfg(feature = "metrics")]
                            metrics::gauge!("aetheris_client_rtt_ms").set(rtt);

                            web_sys::console::debug_1(
                                &format!("[Aetheris] Received Pong: tick={tick} RTT={rtt:.1}ms")
                                    .into(),
                            );
                            tracing::debug!(rtt_ms = rtt, "RTT Update");
                        }
                        NetworkEvent::Auth { .. } => {
                            // Client initiated event usually, ignore if received from server
                            tracing::debug!("Received Auth event from server (unexpected)");
                        }
                        NetworkEvent::SessionClosed(id) => {
                            tracing::warn!(?id, "WebTransport session closed");
                        }
                        NetworkEvent::StreamReset(id) => {
                            tracing::error!(?id, "WebTransport stream reset");
                        }
                        NetworkEvent::Fragment {
                            client_id,
                            fragment,
                        } => {
                            if let Some(data) = self.reassembler.add(client_id, fragment) {
                                if let Ok(update) = encoder.decode(&data) {
                                    updates.push((client_id, update));
                                }
                            }
                        }
                        NetworkEvent::StressTest { .. } => {
                            // Client-side, we don't handle incoming stress test events usually,
                            // they are processed by the server.
                        }
                        NetworkEvent::Spawn { .. } => {
                            // Handled by GameWorker via p_spawn
                        }
                        NetworkEvent::ClearWorld { .. } => {
                            tracing::info!("Server initiated world clear");
                            self.world_state.entities.clear();
                        }
                    }
                }

                // 2. Apply updates to the Simulation World
                self.world_state.apply_updates(&updates);
            }

            // 2.5. Client-side rotation animation (local, not replicated by server)
            if self.playground_rotation_enabled {
                for slot in self.world_state.entities.values_mut() {
                    slot.rotation = (slot.rotation + 0.05) % std::f32::consts::TAU;
                }
            }

            // Advance local tick every frame so the render worker always sees a new snapshot,
            // even when the server sends no updates (static entities, client-side animation).
            self.world_state.latest_tick += 1;

            // 3. Write Authoritative Snapshot to Shared World for the Render Worker
            self.flush_to_shared_world(self.world_state.latest_tick);
        }

        fn flush_to_shared_world(&mut self, tick: u64) {
            let entities = &self.world_state.entities;
            let write_buffer = self.shared_world.get_write_buffer();

            let mut count = 0;
            for (i, slot) in entities.values().enumerate() {
                if i >= MAX_ENTITIES {
                    tracing::warn!("Max entities reached in shared world! Overflow suppressed.");
                    break;
                }
                write_buffer[i] = *slot;
                count += 1;
            }

            self.shared_world.commit_write(count as u32, tick);
        }

        #[wasm_bindgen]
        pub fn playground_spawn(&mut self, entity_type: u16, x: f32, y: f32, rotation: f32) {
            // Security: Prevent overflow in playground mode
            if self.world_state.entities.len() >= MAX_ENTITIES {
                tracing::warn!("playground_spawn: MAX_ENTITIES reached, spawn ignored.");
                return;
            }

            // Sync ID generator if it's currently at default but world is seeded
            if self.playground_next_network_id == 1 && !self.world_state.entities.is_empty() {
                self.playground_next_network_id = self
                    .world_state
                    .entities
                    .keys()
                    .map(|k| k.0)
                    .max()
                    .unwrap_or(0)
                    + 1;
            }

            let id = aetheris_protocol::types::NetworkId(self.playground_next_network_id);
            self.playground_next_network_id += 1;
            let slot = SabSlot {
                network_id: id.0,
                x,
                y,
                z: 0.0,
                rotation,
                dx: 0.0,
                dy: 0.0,
                dz: 0.0,
                hp: 100,
                shield: 100,
                entity_type,
                flags: 0x01, // ALIVE
                padding: [0; 5],
            };
            self.world_state.entities.insert(id, slot);
        }

        #[wasm_bindgen]
        pub async fn playground_spawn_net(
            &mut self,
            entity_type: u16,
            x: f32,
            y: f32,
            rot: f32,
        ) -> Result<(), JsValue> {
            self.check_worker();

            if let Some(transport) = &self.transport {
                let encoder = SerdeEncoder::new();
                let event = NetworkEvent::Spawn {
                    client_id: ClientId(0),
                    entity_type,
                    x,
                    y,
                    rot,
                };

                if let Ok(data) = encoder.encode_event(&event) {
                    transport
                        .send_reliable(ClientId(0), &data)
                        .await
                        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
                    tracing::info!(entity_type, x, y, "Sent Spawn command to server");
                }
            } else {
                // Local fallback
                self.playground_spawn(entity_type, x, y, rot);
            }
            Ok(())
        }

        #[wasm_bindgen]
        pub fn playground_clear(&mut self) {
            self.world_state.entities.clear();
        }

        #[wasm_bindgen]
        pub async fn playground_clear_server(&mut self) -> Result<(), JsValue> {
            self.check_worker();

            if let Some(transport) = &self.transport {
                let encoder = SerdeEncoder::new();
                let event = NetworkEvent::ClearWorld {
                    client_id: ClientId(0),
                };
                if let Ok(data) = encoder.encode_event(&event) {
                    transport
                        .send_reliable(ClientId(0), &data)
                        .await
                        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
                    tracing::info!("Sent ClearWorld command to server");
                    self.world_state.entities.clear();
                }
            } else {
                // No transport, clear immediately
                self.world_state.entities.clear();
            }
            Ok(())
        }

        #[wasm_bindgen]
        pub fn playground_set_rotation_enabled(&mut self, enabled: bool) {
            self.playground_rotation_enabled = enabled;
            // Note: Rotation toggle is currently local-authoritative in playground_tick,
            // but for server-side replication it would need a network event.
        }

        #[wasm_bindgen]
        pub async fn playground_stress_test(
            &mut self,
            count: u16,
            rotate: bool,
        ) -> Result<(), JsValue> {
            self.check_worker();

            if let Some(transport) = &self.transport {
                let encoder = SerdeEncoder::new();
                let event = NetworkEvent::StressTest {
                    client_id: ClientId(0),
                    count,
                    rotate,
                };

                if let Ok(data) = encoder.encode_event(&event) {
                    transport
                        .send_reliable(ClientId(0), &data)
                        .await
                        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
                    tracing::info!(count, rotate, "Sent StressTest command to server");
                }
                self.playground_set_rotation_enabled(rotate);
            } else {
                // Fallback to local behavior if not connected
                self.playground_set_rotation_enabled(rotate);
                self.playground_clear();
                for _ in 0..count {
                    self.playground_spawn(1, 0.0, 0.0, 0.0); // Simple spawn
                }
            }

            Ok(())
        }

        #[wasm_bindgen]
        pub async fn tick_playground(&mut self) {
            self.check_worker();

            // M10105 — emit tick_playground_loop_start lifecycle span
            if self.first_playground_tick {
                self.first_playground_tick = false;
                with_collector(|c| {
                    c.push_event(
                        1,
                        "wasm_client",
                        "Playground simulation loop started",
                        "tick_playground_loop_start",
                        None,
                    );
                });
            }

            // M10105 — measure simulation time
            let sim_start = crate::performance_now();
            self.world_state.latest_tick += 1;

            if self.playground_rotation_enabled {
                for slot in self.world_state.entities.values_mut() {
                    slot.rotation = (slot.rotation + 0.05) % std::f32::consts::TAU;
                }
            }

            // Sync to shared world
            let count = self.world_state.entities.len() as u32;
            self.flush_to_shared_world(self.world_state.latest_tick);

            let sim_time_ms = crate::performance_now() - sim_start;
            with_collector(|c| {
                c.record_sim(sim_time_ms);
                c.update_entity_count(count);
            });
        }

        /// Render frame called by the Render Worker.
        pub fn render(&mut self) -> f64 {
            self.check_worker();

            let tick = self.shared_world.tick();
            let entities = self.shared_world.get_read_buffer();

            // Periodic diagnostic log for render worker
            thread_local! {
                static FRAME_COUNT: core::cell::Cell<u64> = core::cell::Cell::new(0);
            }
            FRAME_COUNT.with(|count| {
                let current = count.get();
                if current % 300 == 0 {
                    tracing::debug!(
                        "Aetheris Render Stats: Tick={}, Entities={}, Snapshots={}",
                        tick,
                        entities.len(),
                        self.snapshots.len(),
                    );
                }
                count.set(current + 1);
            });

            if tick == 0 {
                // Background only or placeholder if no simulation is running yet
                let mut frame_time_ms = 0.0;
                if let Some(state) = &mut self.render_state {
                    frame_time_ms = state.render_frame_with_compact_slots(&[]);
                    with_collector(|c| {
                        // FPS is computed in the worker; we only report duration here.
                        // FPS=0 here as it is not authoritative.
                        c.record_frame(frame_time_ms, 0.0);
                    });
                }
                return frame_time_ms;
            }

            // 1. Buffer new snapshots — only push when tick advances
            if self.snapshots.is_empty()
                || tick > self.snapshots.back().map(|s| s.tick).unwrap_or(0)
            {
                self.snapshots.push_back(SimulationSnapshot {
                    tick,
                    entities: entities.to_vec(),
                });
            }

            // 2. Calculate target playback tick.
            // Stay 2 ticks behind latest so we always have an (s1, s2) interpolation pair.
            // This is rate-independent: works for both the 20 Hz server and the 60 Hz playground.
            let mut frame_time_ms = 0.0;
            if !self.snapshots.is_empty() {
                let latest_tick = self.snapshots.back().unwrap().tick as f32;
                let target_tick = latest_tick - 2.0;
                frame_time_ms = self.render_at_tick(target_tick);
            }

            // M10105 — record accurate frame time (from WGPU) + snapshot depth.
            let snap_count = self.snapshots.len() as u32;
            with_collector(|c| {
                // FPS is computed in the worker; we only report duration here.
                c.record_frame(frame_time_ms, 0.0);
                c.update_snapshot_count(snap_count);
            });

            frame_time_ms
        }

        fn render_at_tick(&mut self, target_tick: f32) -> f64 {
            if self.snapshots.len() < 2 {
                // If we don't have enough snapshots for interpolation,
                // we still want to render the background or at least one frame.
                if let Some(state) = &mut self.render_state {
                    let entities = if !self.snapshots.is_empty() {
                        self.snapshots[0].entities.clone()
                    } else {
                        Vec::new()
                    };
                    return state.render_frame_with_compact_slots(&entities);
                }
                return 0.0;
            }

            // Find snapshots S1, S2 such that S1.tick <= target_tick < S2.tick
            let mut s1_idx = 0;
            let mut found = false;

            for i in 0..self.snapshots.len() - 1 {
                if (self.snapshots[i].tick as f32) <= target_tick
                    && (self.snapshots[i + 1].tick as f32) > target_tick
                {
                    s1_idx = i;
                    found = true;
                    break;
                }
            }

            if !found {
                // If we are outside the buffer range, clamp to the nearest edge
                if target_tick < self.snapshots[0].tick as f32 {
                    s1_idx = 0;
                } else {
                    s1_idx = self.snapshots.len() - 2;
                }
            }

            let s1 = self.snapshots.get(s1_idx).unwrap();
            let s2 = self.snapshots.get(s1_idx + 1).unwrap();

            let tick_range = (s2.tick - s1.tick) as f32;
            let alpha = if tick_range > 0.0 {
                (target_tick - s1.tick as f32) / tick_range
            } else {
                1.0
            }
            .clamp(0.0, 1.0);

            // Interpolate entities into a reusable buffer to avoid per-frame heap allocations
            self.render_buffer.clear();
            self.render_buffer.extend_from_slice(&s2.entities);

            for ent in &mut self.render_buffer {
                let prev = s1.entities.iter().find(|e| e.network_id == ent.network_id);

                if let Some(prev) = prev {
                    ent.x = lerp(prev.x, ent.x, alpha);
                    ent.y = lerp(prev.y, ent.y, alpha);
                    ent.z = lerp(prev.z, ent.z, alpha);
                    ent.rotation = lerp_rotation(prev.rotation, ent.rotation, alpha);
                }
            }

            let mut frame_time = 0.0;
            if let Some(state) = &mut self.render_state {
                frame_time = state.render_frame_with_compact_slots(&self.render_buffer);
            }

            // 3. Prune old snapshots.
            // We keep the oldest one that is still relevant for interpolation (index 0)
            // and everything newer. We prune snapshots that are entirely behind our window.
            while self.snapshots.len() > 2 && (self.snapshots[0].tick as f32) < target_tick - 1.0 {
                self.snapshots.pop_front();
            }

            // Safety cap: prevent unbounded growth if simulation stops but render continues
            while self.snapshots.len() > 16 {
                self.snapshots.pop_front();
            }

            frame_time
        }
    }

    fn lerp(a: f32, b: f32, alpha: f32) -> f32 {
        a + (b - a) * alpha
    }

    fn lerp_rotation(a: f32, b: f32, alpha: f32) -> f32 {
        // Simple rotation lerp for Phase 1.
        // Handles 2pi wraparound for smooth visuals.
        let mut diff = b - a;
        while diff < -std::f32::consts::PI {
            diff += std::f32::consts::TAU;
        }
        while diff > std::f32::consts::PI {
            diff -= std::f32::consts::TAU;
        }
        a + diff * alpha
    }

    /// Fallback entry point for non-worker environments.
    #[cfg(not(test))]
    #[wasm_bindgen(start)]
    pub fn main() {
        console_error_panic_hook::set_once();
    }
}
