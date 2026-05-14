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

#[cfg(test)]
pub mod transport_mock;

#[cfg(target_arch = "wasm32")]
pub mod render;

#[cfg(any(target_arch = "wasm32", test))]
pub mod render_primitives;

#[cfg(any(target_arch = "wasm32", test))]
pub mod assets;

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
    use crate::assets;
    use crate::metrics::with_collector;
    use crate::performance_now;
    use crate::render::RenderState;
    use crate::shared_world::{MAX_ENTITIES, SabSlot, SharedWorld};
    use crate::transport::WebTransportBridge;
    use crate::world_state::ClientWorld;
    use aetheris_encoder_serde::SerdeEncoder;
    use aetheris_protocol::events::{NetworkEvent, ReplicationEvent};
    use aetheris_protocol::traits::{Encoder, PlatformTransport, WorldState};
    use aetheris_protocol::types::{
        ClientId, ComponentKind, InputCommand, NetworkId, PlayerInputKind,
    };
    use std::cell::RefCell;
    use wasm_bindgen::prelude::*;

    fn lerp(a: f32, b: f32, alpha: f32) -> f32 {
        a + (b - a) * alpha
    }

    fn lerp_wrapped(a: f32, b: f32, alpha: f32, min: f32, max: f32) -> f32 {
        let size = max - min;
        if size <= 0.0 {
            return a + (b - a) * alpha;
        }

        // Ensure inputs are within [min, max) before calculating diff
        let a_norm = (a - min).rem_euclid(size) + min;
        let b_norm = (b - min).rem_euclid(size) + min;

        let mut diff = b_norm - a_norm;
        if diff.abs() > size * 0.5 {
            if diff > 0.0 {
                diff -= size;
            } else {
                diff += size;
            }
        }
        let res = a_norm + diff * alpha;
        // Final wrap to keep result strictly in [min, max)
        (res - min).rem_euclid(size) + min
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

    #[wasm_bindgen]
    pub async fn auth_request_otp(base_url: String, email: String) -> Result<String, JsValue> {
        crate::auth::request_otp(base_url, email)
            .await
            .map_err(|e| e.into())
    }

    #[wasm_bindgen]
    pub async fn auth_login_with_otp(
        base_url: String,
        request_id: String,
        code: String,
    ) -> Result<String, JsValue> {
        crate::auth::login_with_otp(base_url, request_id, code)
            .await
            .map_err(|e| e.into())
    }

    /// Global state held by the WASM instance.
    #[wasm_bindgen]
    pub struct PlatformClient {
        worker_id: usize,
        session_token: RefCell<Option<String>>,

        last_rtt_ms: RefCell<f64>,
        ping_counter: RefCell<u64>,

        reassembler: RefCell<aetheris_protocol::Reassembler>,
        connection_state: RefCell<ConnectionState>,
        reconnect_attempts: RefCell<u32>,

        shared_world: crate::shared_world::SharedWorld,
        world_state: RefCell<crate::world_state::ClientWorld>,
        render_state: RefCell<Option<crate::render::RenderState>>,
        transport: RefCell<Option<Box<dyn aetheris_protocol::traits::PlatformTransport>>>,

        playground_rotation_enabled: RefCell<bool>,
        playground_next_network_id: RefCell<u64>,
        first_playground_tick: RefCell<bool>,

        pub(crate) render_buffer: RefCell<Vec<SabSlot>>,
        pub(crate) asset_registry: crate::assets::AssetRegistry,

        last_input_target: RefCell<Option<NetworkId>>,
        last_input_actions: RefCell<Vec<PlayerInputKind>>,

        pending_clear: RefCell<bool>,
        last_clear_tick: RefCell<u64>,

        last_process_time: RefCell<f64>,
        tick_accumulator: RefCell<f64>,

        playground_move_x: RefCell<f32>,
        playground_move_y: RefCell<f32>,
        playground_actions: RefCell<u32>,
        last_fraction: RefCell<f32>,
        last_actions_mask: RefCell<u32>,
        last_cursor_x: RefCell<f32>,
        last_cursor_y: RefCell<f32>,
        last_cursor_send_time: RefCell<f64>,

        snapshots: RefCell<std::collections::VecDeque<SimulationSnapshot>>,
    }

    #[wasm_bindgen]
    impl PlatformClient {
        /// Creates a new PlatformClient instance.
        /// If a pointer is provided, it will use it as the backing storage (shared memory).
        #[wasm_bindgen(constructor)]
        pub fn new(shared_world_ptr: Option<u32>) -> Result<PlatformClient, JsValue> {
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

                // JS-allocated SharedArrayBuffer pointers are not in the Rust registry
                // (only Rust-owned allocations are registered). The null/alignment checks
                // above are the only feasible boundary validation for externally-provided
                // pointers; trusting the caller is required by the SAB contract.
                unsafe { SharedWorld::from_ptr(ptr) }
            } else {
                SharedWorld::new()
            };

            let global = js_sys::global();
            let (ua, lang) =
                if let Ok(worker) = global.clone().dyn_into::<web_sys::WorkerGlobalScope>() {
                    let n = worker.navigator();
                    (n.user_agent().ok(), n.language())
                } else if let Ok(window) = global.dyn_into::<web_sys::Window>() {
                    let n = window.navigator();
                    (n.user_agent().ok(), n.language())
                } else {
                    (None, None)
                };

            tracing::info!(
                "Platform Client: Environment [UA: {}, Lang: {}]",
                ua.as_deref().unwrap_or("Unknown"),
                lang.as_deref().unwrap_or("Unknown")
            );

            tracing::info!(
                "PlatformClient initialized on worker {}",
                crate::get_worker_id()
            );

            // M10105 — emit wasm_init lifecycle span
            with_collector(|c| {
                c.push_event(
                    1,
                    "wasm_client",
                    "PlatformClient initialized",
                    "wasm_init",
                    None,
                );
            });

            let mut world_state = ClientWorld::new();
            world_state.shared_world_ref = Some(shared_world.as_ptr() as usize);

            Ok(Self {
                shared_world,
                world_state: RefCell::new(world_state),
                render_state: RefCell::new(None),
                transport: RefCell::new(None),
                worker_id: crate::get_worker_id(),
                session_token: RefCell::new(None),
                snapshots: RefCell::new(std::collections::VecDeque::with_capacity(8)),
                last_rtt_ms: RefCell::new(0.0),
                ping_counter: RefCell::new(0),
                reassembler: RefCell::new(aetheris_protocol::Reassembler::new()),
                connection_state: RefCell::new(ConnectionState::Disconnected),
                reconnect_attempts: RefCell::new(0),
                playground_rotation_enabled: RefCell::new(false),
                playground_next_network_id: RefCell::new(1),
                first_playground_tick: RefCell::new(true),
                render_buffer: RefCell::new(Vec::with_capacity(crate::shared_world::MAX_ENTITIES)),
                asset_registry: assets::AssetRegistry::new(),
                last_input_target: RefCell::new(None),
                last_input_actions: RefCell::new(Vec::new()),
                pending_clear: RefCell::new(false),
                last_clear_tick: RefCell::new(0),
                last_process_time: RefCell::new(crate::performance_now()),
                tick_accumulator: RefCell::new(0.0),
                playground_move_x: RefCell::new(0.0),
                playground_move_y: RefCell::new(0.0),
                playground_actions: RefCell::new(0),
                last_fraction: RefCell::new(0.0),
                last_actions_mask: RefCell::new(0),
                last_cursor_x: RefCell::new(-1.0),
                last_cursor_y: RefCell::new(-1.0),
                last_cursor_send_time: RefCell::new(0.0),
            })
        }

        fn check_worker(&self) {
            debug_assert_eq!(
                self.worker_id,
                crate::get_worker_id(),
                "PlatformClient accessed from wrong worker! It is pin-bound to its creating thread."
            );
        }

        /// Returns the raw pointer to the shared world buffer.
        pub fn shared_world_ptr(&self) -> u32 {
            self.shared_world.as_ptr() as u32
        }

        #[wasm_bindgen]
        pub fn set_view_state(&self, state: u32) {
            use crate::render::ViewState;
            let state = match state {
                0 => ViewState::Logo,
                1 => ViewState::Roaming,
                2 => ViewState::Entering,
                3 => ViewState::Playing,
                _ => return,
            };

            let mut rs = self.render_state.borrow_mut();
            if let Some(rs) = &mut *rs {
                rs.set_view_state(state);
            } else {
                tracing::warn!("set_view_state called but render_state is None");
            }
        }

        #[wasm_bindgen(getter)]
        pub fn connection_state(&self) -> ConnectionState {
            *self.connection_state.borrow()
        }

        pub async fn connect(
            &self,
            url: String,
            cert_hash: Option<Vec<u8>>,
        ) -> Result<(), JsValue> {
            self.check_worker();

            {
                let state = self.connection_state.borrow();
                if *state == ConnectionState::Connecting
                    || *state == ConnectionState::InGame
                    || *state == ConnectionState::Reconnecting
                {
                    return Ok(());
                }
            }

            // M10105 — emit reconnect_attempt if it looks like one
            {
                let state = self.connection_state.borrow();
                let attempts = self.reconnect_attempts.borrow();
                if *state == ConnectionState::Failed && *attempts > 0 {
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
            }

            *self.connection_state.borrow_mut() = ConnectionState::Connecting;
            tracing::info!(url = %url, "Connecting to server...");

            let transport_result = WebTransportBridge::connect(&url, cert_hash.as_deref()).await;

            match transport_result {
                Ok(transport) => {
                    // Security: Send Auth message immediately after connection
                    let token_opt = self.session_token.borrow().clone();
                    if let Some(token) = token_opt {
                        if let Err(e) = transport.send_raw_auth_token(&token).await {
                            *self.connection_state.borrow_mut() = ConnectionState::Failed;
                            tracing::error!(error = ?e, "Transport handshake failed");
                            return Err(JsValue::from_str(&format!(
                                "Failed to send raw auth token: {:?}",
                                e
                            )));
                        }
                        tracing::info!("Raw auth token accepted by transport");

                        // Application Auth: Send Auth event for the server tick loop
                        let encoder = SerdeEncoder::new();
                        let auth_event = NetworkEvent::Auth {
                            session_token: token.clone(),
                        };

                        if let Ok(data) = encoder.encode_event(&auth_event) {
                            if let Err(e) = transport.send_reliable(ClientId(0), &data).await {
                                tracing::error!(error = ?e, "Application auth failed");
                            } else {
                                tracing::info!("Application auth packet sent");
                            }
                        }
                    } else {
                        tracing::warn!(
                            "Connecting without session token! Server will likely discard data."
                        );
                    }

                    *self.transport.borrow_mut() = Some(Box::new(transport));
                    *self.connection_state.borrow_mut() = ConnectionState::InGame;
                    *self.reconnect_attempts.borrow_mut() = 0;
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
                    *self.connection_state.borrow_mut() = ConnectionState::Failed;
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

        #[wasm_bindgen]
        pub async fn disconnect(&self) {
            self.check_worker();

            // Take the transport to drop it and close the connection
            let transport = self.transport.borrow_mut().take();
            if let Some(transport) = transport {
                // Trigger active disconnection
                let _ = transport.disconnect(ClientId(0)).await;
            }

            *self.connection_state.borrow_mut() = ConnectionState::Disconnected;
            tracing::info!("PlatformClient disconnected explicitly");
        }

        #[wasm_bindgen]
        pub async fn reconnect(
            &self,
            url: String,
            cert_hash: Option<Vec<u8>>,
        ) -> Result<(), JsValue> {
            self.check_worker();
            *self.connection_state.borrow_mut() = ConnectionState::Reconnecting;
            *self.reconnect_attempts.borrow_mut() += 1;

            let attempts = *self.reconnect_attempts.borrow();
            tracing::info!("Attempting reconnection... (attempt {})", attempts);

            self.connect(url, cert_hash).await
        }

        #[wasm_bindgen]
        pub async fn wasm_load_asset(
            &self,
            handle: assets::AssetHandle,
            url: String,
        ) -> Result<(), JsValue> {
            self.asset_registry.load_asset(handle, &url).await
        }

        /// Sets the session token to be used for authentication upon connection.
        pub fn set_session_token(&self, token: String) {
            *self.session_token.borrow_mut() = Some(token);
        }

        /// Initializes rendering with a canvas element.
        /// Accepts either web_sys::HtmlCanvasElement or web_sys::OffscreenCanvas.
        pub async fn init_renderer(&self, canvas: JsValue) -> Result<(), JsValue> {
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

            *self.render_state.borrow_mut() = Some(render_state);

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
        pub fn resize(&self, width: u32, height: u32) {
            let mut rs = self.render_state.borrow_mut();
            if let Some(state) = &mut *rs {
                state.resize(width, height);
            }
        }

        #[cfg(debug_assertions)]
        #[wasm_bindgen]
        pub fn set_debug_mode(&self, mode: u32) {
            self.check_worker();
            let mut rs = self.render_state.borrow_mut();
            if let Some(state) = &mut *rs {
                state.set_debug_mode(match mode {
                    0 => crate::render::DebugRenderMode::Off,
                    1 => crate::render::DebugRenderMode::Wireframe,
                    2 => crate::render::DebugRenderMode::Components,
                    _ => crate::render::DebugRenderMode::Full,
                });
            }
        }

        #[wasm_bindgen]
        pub fn set_theme_colors(&self, bg_base: &str, text_primary: &str) {
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

            let mut rs = self.render_state.borrow_mut();
            if let Some(state) = &mut *rs {
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
        pub fn cycle_debug_mode(&self) {
            let mut rs = self.render_state.borrow_mut();
            if let Some(state) = &mut *rs {
                state.cycle_debug_mode();
            }
        }

        #[cfg(debug_assertions)]
        #[wasm_bindgen]
        pub fn toggle_grid(&self) {
            let mut rs = self.render_state.borrow_mut();
            if let Some(state) = &mut *rs {
                state.toggle_grid();
            }
        }

        #[wasm_bindgen]
        pub fn latest_tick(&self) -> u64 {
            self.world_state.borrow().latest_tick
        }

        #[wasm_bindgen]
        pub fn playground_apply_input(&self, move_x: f32, move_y: f32, actions_mask: u32) {
            self.check_worker();
            *self.last_cursor_x.borrow_mut() = move_x;
            *self.last_cursor_y.borrow_mut() = move_y;
            *self.last_actions_mask.borrow_mut() = actions_mask;
        }

        /// Simulation tick called by the Network Worker at a fixed rate (e.g. 20Hz).
        pub async fn tick(&self) {
            self.check_worker();
            use aetheris_protocol::traits::{Encoder, WorldState};

            let encoder = SerdeEncoder::new();

            // 0. Reconnection Logic
            // TODO: poll transport.closed() promise and trigger reconnection state machine

            // 0.1 Periodic Ping (approx. every 1 second at 60Hz)
            let ping_data = {
                let mut ping_counter = self.ping_counter.borrow_mut();
                *ping_counter = ping_counter.wrapping_add(1);
                if *ping_counter % 60 == 0 {
                    let now = performance_now();
                    let tick_u64 = now as u64;
                    encoder
                        .encode_event(&NetworkEvent::Ping {
                            client_id: ClientId(0),
                            tick: tick_u64,
                        })
                        .ok()
                } else {
                    None
                }
            };
            if let Some(data) = ping_data {
                // IMPORTANT: We must not hold `transport_guard` across an `.await` point.
                // Doing so keeps the RefCell locked and causes a `borrow_mut` panic
                // in the next transport access (line 722). Instead, we call send_unreliable
                // via a raw pointer for the duration of the await, after the guard is dropped.
                let send_fut = {
                    let mut transport_guard = self.transport.borrow_mut();
                    if let Some(transport) = &mut *transport_guard {
                        // Extend the borrow lifetime to the raw pointer (safe: we hold
                        // the single-threaded WASM invariant and drop guard before await).
                        let t: *mut dyn aetheris_protocol::traits::PlatformTransport =
                            &mut **transport;
                        // SAFETY: single-threaded WASM; no other code can modify transport
                        // between now and the await since JS is cooperative.
                        Some(unsafe { &mut *t }.send_unreliable(ClientId(0), &data))
                    } else {
                        None
                    }
                    // transport_guard is dropped here before any await
                };
                if let Some(fut) = send_fut {
                    let _ = fut.await;
                }
            }

            // 1. Poll Network
            let mut collected_game_events = Vec::new();

            let events = {
                // SAFETY: single-threaded WASM cooperative scheduler. We capture a raw
                // pointer to the transport before dropping the borrow guard, then await
                // the future with the guard already released.
                let poll_fut = {
                    let mut transport_guard = self.transport.borrow_mut();
                    transport_guard.as_mut().map(|t| {
                        let t: *mut dyn aetheris_protocol::traits::PlatformTransport = &mut **t;
                        unsafe { &mut *t }.poll_events()
                    })
                    // transport_guard is dropped here
                };
                match poll_fut {
                    Some(fut) => match fut.await {
                        Ok(e) => Some(e),
                        Err(e) => {
                            tracing::error!("Transport poll failure: {:?}", e);
                            None
                        }
                    },
                    None => None,
                }
            };

            if let Some(events) = events {
                let mut updates: Vec<(ClientId, aetheris_protocol::events::ComponentUpdate)> =
                    Vec::new();

                for event in events {
                    match event {
                        NetworkEvent::UnreliableMessage { data, client_id }
                        | NetworkEvent::ReliableMessage { data, client_id } => {
                            match encoder.decode(&data) {
                                Ok(update) => {
                                    let last_clear_tick = *self.last_clear_tick.borrow();
                                    if last_clear_tick == 0 || update.tick > last_clear_tick {
                                        updates.push((client_id, update));
                                    } else {
                                        tracing::debug!(
                                            network_id = update.network_id.0,
                                            tick = update.tick,
                                            last_clear_tick = last_clear_tick,
                                            "Discarding stale update (tick <= last_clear_tick)"
                                        );
                                    }
                                }
                                Err(_) => {
                                    if let Ok(event) = encoder.decode_event(&data) {
                                        match event {
                                            aetheris_protocol::events::NetworkEvent::PlatformEvent {
                                                event: platform_event,
                                                ..
                                            } => {
                                                collected_game_events.push(platform_event);
                                            }
                                            aetheris_protocol::events::NetworkEvent::ClearWorld {
                                                ..
                                            } => {
                                                tracing::info!(
                                                    "Server ClearWorld ack received (via ReliableMessage) — gate lowered"
                                                );
                                                *self.pending_clear.borrow_mut() = false;
                                            }
                                            aetheris_protocol::events::NetworkEvent::EntityDespawned {
                                                network_id,
                                                ..
                                            } => {
                                                let mut world = self.world_state.borrow_mut();
                                                world.entities.remove(&network_id);
                                                tracing::warn!(
                                                    ?network_id,
                                                    "Entity despawned via decoded ReliableMessage"
                                                );
                                            }
                                            aetheris_protocol::events::NetworkEvent::EntitySpawned {
                                                network_id,
                                                kind,
                                                ..
                                            } => {
                                                tracing::info!(
                                                    ?network_id,
                                                    ?kind,
                                                    "Entity spawned via decoded ReliableMessage (awaiting replication)"
                                                );
                                            }
                                            _ => {}
                                        }
                                    } else {
                                        tracing::warn!(
                                            "Failed to decode server message as update or wire event"
                                        );
                                    }
                                }
                            }
                        }
                        NetworkEvent::ClientConnected(id) => {
                            tracing::info!(?id, "Server connected");
                        }
                        NetworkEvent::ClientDisconnected(id) => {
                            tracing::warn!(?id, "Server disconnected");
                        }
                        NetworkEvent::Disconnected(_id) => {
                            tracing::info!("Transport disconnected (session closed)");
                            *self.connection_state.borrow_mut() = ConnectionState::Disconnected;
                        }
                        NetworkEvent::Ping { client_id: _, tick } => {
                            let pong = NetworkEvent::Pong { tick };
                            if let Ok(data) = encoder.encode_event(&pong) {
                                // SAFETY: same single-threaded WASM invariant; drop guard before await.
                                let pong_fut = {
                                    let transport_guard = self.transport.borrow();
                                    transport_guard.as_ref().map(|t| {
                                        let t: *const dyn aetheris_protocol::traits::PlatformTransport = &**t;
                                        unsafe { &*t }.send_reliable(ClientId(0), &data)
                                    })
                                    // transport_guard dropped here
                                };
                                if let Some(fut) = pong_fut {
                                    let _ = fut.await;
                                }
                            }
                        }
                        NetworkEvent::Pong { tick } => {
                            let now = performance_now();
                            let rtt = now - (tick as f64);
                            *self.last_rtt_ms.borrow_mut() = rtt;

                            with_collector(|c| {
                                c.update_rtt(rtt);
                            });

                            #[cfg(feature = "metrics")]
                            metrics::gauge!("aetheris_client_rtt_ms").set(rtt);

                            tracing::trace!(rtt_ms = rtt, tick, "Received Pong / RTT update");
                        }
                        NetworkEvent::Auth { .. } => {
                            tracing::debug!("Received Auth event from server (unexpected)");
                        }
                        NetworkEvent::SessionClosed(id) => {
                            tracing::warn!(?id, "WebTransport session closed");
                        }
                        NetworkEvent::StreamReset(id) => {
                            tracing::error!(?id, "WebTransport stream reset");
                        }
                        NetworkEvent::ReplicationBatch { events, client_id } => {
                            let last_clear_tick = *self.last_clear_tick.borrow();
                            for event in events {
                                if last_clear_tick == 0 || event.tick > last_clear_tick {
                                    updates.push((
                                        client_id,
                                        aetheris_protocol::events::ComponentUpdate {
                                            network_id: event.network_id,
                                            component_kind: event.component_kind,
                                            payload: event.payload,
                                            tick: event.tick,
                                        },
                                    ));
                                }
                            }
                        }
                        NetworkEvent::Fragment {
                            client_id,
                            fragment,
                        } => {
                            let mut reassembler = self.reassembler.borrow_mut();
                            if let Some(data) = reassembler.ingest(client_id, fragment) {
                                if let Ok(update) = encoder.decode(&data) {
                                    let last_clear_tick = *self.last_clear_tick.borrow();
                                    if last_clear_tick == 0 || update.tick > last_clear_tick {
                                        updates.push((client_id, update));
                                    }
                                }
                            }
                        }
                        NetworkEvent::StressTest { .. } => {}
                        NetworkEvent::Spawn { .. } => {}
                        NetworkEvent::ClearWorld { .. } => {
                            tracing::info!("Server ClearWorld ack received — gate lowered");
                            *self.pending_clear.borrow_mut() = false;
                        }
                        NetworkEvent::PlatformEvent {
                            event: platform_event,
                            ..
                        } => {
                            collected_game_events.push(platform_event);
                        }
                        NetworkEvent::EntityDespawned { network_id, .. } => {
                            let mut world = self.world_state.borrow_mut();
                            world.entities.remove(&network_id);
                            tracing::warn!(?network_id, "Entity despawned via NetworkEvent");
                        }
                        #[allow(unreachable_patterns)]
                        _ => {
                            tracing::debug!("Unhandled outer NetworkEvent variant");
                        }
                    }
                }

                // 2. Apply updates to the Simulation World
                let pending_clear = *self.pending_clear.borrow();
                if pending_clear {
                    if !updates.is_empty() {
                        tracing::debug!(
                            count = updates.len(),
                            "Discarding updates — pending_clear gate is raised"
                        );
                    }
                } else {
                    if !updates.is_empty() {
                        let max_tick = updates.iter().map(|(_, u)| u.tick).max().unwrap_or(0);

                        let mut world = self.world_state.borrow_mut();
                        if max_tick > 0 {
                            let drift = (world.latest_tick as i32 - max_tick as i32).abs();
                            let first_tick = *self.first_playground_tick.borrow();
                            if first_tick || drift > 20 {
                                tracing::info!(
                                    latest = world.latest_tick,
                                    server = max_tick,
                                    drift,
                                    first = first_tick,
                                    "Syncing client latest_tick to server authoritative tick"
                                );
                                world.latest_tick = max_tick;
                                *self.first_playground_tick.borrow_mut() = false;
                            } else {
                                tracing::trace!(
                                    latest = world.latest_tick,
                                    server = max_tick,
                                    drift,
                                    "Client tick is in sync"
                                );
                            }
                        }

                        tracing::debug!(count = updates.len(), "Applying server updates to world");
                        world.apply_updates(&updates);
                    }
                }
            }

            // 1.5 Dispatch Collected Game Events
            for platform_event in collected_game_events {
                self.dispatch_platform_event(&platform_event);
            }

            // 2.5. Fixed-Timestep Simulation Loop (M1020)
            let now = crate::performance_now();
            let delta_ms = {
                let mut last_process_time = self.last_process_time.borrow_mut();
                let delta = now - *last_process_time;
                *last_process_time = now;
                delta
            };

            // Limit delta to prevent "spiral of death" after long freezes (max 5 frames)
            let delta_ms = delta_ms.min(100.0);
            {
                let mut tick_accumulator = self.tick_accumulator.borrow_mut();
                *tick_accumulator += delta_ms;

                const DT_MS: f64 = 1000.0 / 60.0;
                while *tick_accumulator >= DT_MS {
                    let mut world = self.world_state.borrow_mut();

                    // Apply buffered playground input
                    let applied = world.playground_apply_input(
                        *self.playground_move_x.borrow(),
                        *self.playground_move_y.borrow(),
                        *self.playground_actions.borrow(),
                    );

                    if !applied && world.latest_tick % 120 == 0 {
                        tracing::warn!(
                            tick = world.latest_tick,
                            "Simulation loop running but no LocalPlayer (0x04) entity found to apply input to"
                        );
                    }

                    world.latest_tick += 1;
                    world.simulate();
                    *tick_accumulator -= DT_MS;
                }

                // 2.5.5. Publish sub-tick fraction for smooth rendering
                let fraction = (*tick_accumulator as f32 / DT_MS as f32).clamp(0.0, 1.0);
                let alpha = 0.8;
                let mut last_fraction = self.last_fraction.borrow_mut();
                *last_fraction = *last_fraction * (1.0 - alpha) + fraction * alpha;
                self.shared_world.set_sub_tick_fraction(*last_fraction);
            }

            let sim_start = crate::performance_now();

            // 3. Write Authoritative Snapshot to Shared World for the Render Worker
            let latest_tick = self.world_state.borrow().latest_tick;
            self.flush_to_shared_world(latest_tick);

            let sim_time_ms = crate::performance_now() - sim_start;

            {
                let world = self.world_state.borrow();
                let count = world.entities.len() as u32;
                let (payload_count, payload_cap) = world
                    .player_network_id
                    .and_then(|id| world.entities.get(&id))
                    .map_or((0, 0), |s| {
                        (s.payload_count as u32, s.payload_capacity as u32)
                    });

                with_collector(|c| {
                    c.record_sim(sim_time_ms);
                    c.update_entity_count(count);
                    c.update_payload(payload_count, payload_cap);
                });
            }
        }

        fn flush_to_shared_world(&self, tick: u64) {
            let world = self.world_state.borrow();
            let entities = &world.entities;
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

            tracing::debug!(entity_count = count, tick, "Flushed world to SAB");
            self.shared_world.commit_write(count as u32, tick);

            // Metrics: Update payload count for UI if player exists
            if let Some(player_id) = world.player_network_id {
                if let Some(slot) = entities.get(&player_id) {
                    with_collector(|c| {
                        c.update_payload(slot.payload_count as u32, slot.payload_capacity as u32);
                    });
                }
            }
        }

        #[wasm_bindgen]
        pub async fn request_workspace_manifest(&self) -> Result<(), JsValue> {
            self.check_worker();

            let transport_guard = self.transport.borrow();
            if let Some(transport) = &*transport_guard {
                let encoder = SerdeEncoder::new();
                let event = NetworkEvent::RequestWorkspaceManifest {
                    client_id: ClientId(0),
                };

                if let Ok(data) = encoder.encode_event(&event) {
                    transport
                        .send_reliable(ClientId(0), &data)
                        .await
                        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
                    tracing::info!("Sent RequestWorkspaceManifest command to server");
                }
            }
            Ok(())
        }

        #[wasm_bindgen]
        pub fn get_workspace_info(&self) -> Result<JsValue, JsValue> {
            let world = self.world_state.borrow();
            serde_wasm_bindgen::to_value(&world.workspace_manifest)
                .map_err(|e| JsValue::from_str(&e.to_string()))
        }

        fn dispatch_platform_event(
            &self,
            platform_event: &aetheris_protocol::events::PlatformEvent,
        ) {
            let mut world = self.world_state.borrow_mut();
            match platform_event {
                aetheris_protocol::events::PlatformEvent::ResourceExhausted { network_id } => {
                    tracing::info!(?network_id, "Resource exhausted (via PlatformEvent)");
                    world.entities.remove(network_id);

                    for slot in world.entities.values_mut() {
                        if (slot.flags & 0x04) != 0
                            && slot.extraction_target_id == (network_id.0 as u16)
                        {
                            slot.extraction_active = 0;
                            slot.extraction_target_id = 0;
                            tracing::info!("Cleared local extraction target due to exhaustion");
                        }
                    }
                }
                aetheris_protocol::events::PlatformEvent::WorkspaceManifest { manifest } => {
                    tracing::info!(
                        count = manifest.len(),
                        "Received WorkspaceManifest from server (via PlatformEvent)"
                    );
                    world.workspace_manifest = manifest.clone();
                }
                aetheris_protocol::events::PlatformEvent::Possession { .. }
                | aetheris_protocol::events::PlatformEvent::Interaction { .. }
                | aetheris_protocol::events::PlatformEvent::Termination { .. }
                | aetheris_protocol::events::PlatformEvent::Reinitialization { .. }
                | aetheris_protocol::events::PlatformEvent::PayloadCollected { .. } => {
                    world.handle_platform_event(platform_event);
                }
            }
        }

        #[wasm_bindgen]
        pub fn wasm_get_entity_statuses(&self) -> JsValue {
            #[derive(serde::Serialize)]
            struct EntityStatus {
                network_id: String,
                integrity: u16,
                max_integrity: u16,
                priority: u16,
                max_priority: u16,
                entity_type: u16,
                is_player: bool,
            }

            let world = self.world_state.borrow();
            let mut entities: Vec<&SabSlot> = world.entities.values().collect();
            entities.sort_by_key(|slot| slot.network_id);

            let statuses: Vec<EntityStatus> = entities
                .into_iter()
                .map(|slot| {
                    // M1020 §3.3: Max vitals are derived from authoritative protocol definitions.
                    let (max_integrity, max_priority) =
                        aetheris_protocol::types::get_default_properties(slot.entity_type);

                    EntityStatus {
                        network_id: slot.network_id.to_string(),
                        integrity: slot.integrity,
                        max_integrity,
                        priority: slot.priority,
                        max_priority,
                        entity_type: slot.entity_type,
                        is_player: (slot.flags & 0x04) != 0,
                    }
                })
                .collect();

            match serde_wasm_bindgen::to_value(&statuses) {
                Ok(val) => val,
                Err(e) => {
                    web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(&format!(
                        "wasm_get_entity_statuses: serde_wasm_bindgen::to_value failed: {e}"
                    )));
                    wasm_bindgen::JsValue::NULL
                }
            }
        }

        #[wasm_bindgen]
        pub fn get_presence(&self) -> JsValue {
            #[derive(serde::Serialize)]
            struct PresenceInfo {
                id: String,
                x: f32,
                y: f32,
                name: String,
            }

            let world = self.world_state.borrow();
            let presences: Vec<PresenceInfo> = world
                .entities
                .iter()
                .filter(|(_, slot)| slot.entity_type == 0x2007)
                .map(|(id, slot)| PresenceInfo {
                    id: id.0.to_string(),
                    x: slot.x,
                    y: slot.y,
                    name: format!("User {}", id.0),
                })
                .collect();

            match serde_wasm_bindgen::to_value(&presences) {
                Ok(val) => val,
                Err(e) => {
                    web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(&format!(
                        "get_presence: serde_wasm_bindgen::to_value failed: {e}"
                    )));
                    wasm_bindgen::JsValue::NULL
                }
            }
        }

        #[wasm_bindgen]
        pub fn playground_spawn(&self, entity_type: u16, x: f32, y: f32, rotation: f32) {
            let mut world = self.world_state.borrow_mut();
            if world.entities.len() >= MAX_ENTITIES {
                tracing::warn!("playground_spawn: MAX_ENTITIES reached, spawn ignored.");
                return;
            }

            let mut next_id = self.playground_next_network_id.borrow_mut();
            // Sync ID generator if it's currently at default but world is seeded
            if *next_id == 1 && !world.entities.is_empty() {
                *next_id = world.entities.keys().map(|k| k.0).max().unwrap_or(0) + 1;
            }

            let id = aetheris_protocol::types::NetworkId(*next_id);
            *next_id += 1;
            let (integrity, priority) =
                aetheris_protocol::types::get_default_properties(entity_type);
            let slot = SabSlot {
                network_id: id.0,
                x,
                y,
                z: 0.0,
                rotation,
                dx: 0.0,
                dy: 0.0,
                dz: 0.0,
                integrity,
                priority,
                entity_type,
                flags: 0x01, // ALIVE
                extraction_active: 0,
                payload_count: 0,
                payload_capacity: 0,
                extraction_target_id: 0,
                interaction_target_id: 0,
                interaction_flash_ticks: 0,
                padding: [0; 3],
            };
            world.entities.insert(id, slot);
        }

        #[wasm_bindgen]
        pub async fn playground_spawn_net(
            &self,
            entity_type: u16,
            x: f32,
            y: f32,
            rot: f32,
        ) -> Result<(), JsValue> {
            self.check_worker();

            let transport_guard = self.transport.borrow();
            if let Some(transport) = &*transport_guard {
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
        pub fn playground_clear(&self) {
            self.world_state.borrow_mut().entities.clear();
        }

        /// Sends a StartSession command to the server.
        /// The server will spawn the session Interceptor and send back a Possession event.
        /// Only valid when connected; does nothing in local sandbox mode.
        #[wasm_bindgen]
        pub async fn start_session_net(&self) -> Result<(), JsValue> {
            self.check_worker();

            let transport_guard = self.transport.borrow();
            if let Some(transport) = &*transport_guard {
                let encoder = SerdeEncoder::new();
                let event = NetworkEvent::StartSession {
                    client_id: ClientId(0),
                };
                if let Ok(data) = encoder.encode_event(&event) {
                    transport
                        .send_reliable(ClientId(0), &data)
                        .await
                        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
                    tracing::info!("Sent StartSession command to server");
                }
            }
            Ok(())
        }

        #[wasm_bindgen]
        pub fn get_networked_entities(&self) -> Vec<u32> {
            self.world_state
                .borrow()
                .entities
                .keys()
                .map(|id| id.0 as u32)
                .collect()
        }

        #[wasm_bindgen]
        pub fn get_entity_type(&self, network_id: u32) -> u32 {
            self.world_state
                .borrow()
                .entities
                .get(&aetheris_protocol::types::NetworkId(u64::from(network_id)))
                .map(|slot| u32::from(slot.entity_type))
                .unwrap_or(0)
        }

        #[wasm_bindgen]
        pub fn player_network_id(&self) -> Option<u32> {
            self.world_state
                .borrow()
                .player_network_id
                .map(|id| id.0 as u32)
        }

        /// Sends a movement/action input command to the server.
        ///
        /// The input is encoded as an unreliable component update (Kind 128)
        /// and sent to the server for processing in the next tick.
        #[wasm_bindgen]
        pub async fn send_input(
            &self,
            tick: u64,
            move_x: f32,
            move_y: f32,
            actions_mask: u32,
            target_id_arg: Option<u64>,
        ) -> Result<(), JsValue> {
            self.check_worker();

            // 1. Identify the controlled player entity to target the command correctly
            let target_id = {
                let world = self.world_state.borrow();
                if let Some(owned_id) = world.player_network_id {
                    // If we have an explicit possession ID from the server, use it.
                    // This is the most reliable method (M1038).
                    Some(owned_id)
                } else {
                    // Fallback: Identify via replication flags if possession event hasn't arrived yet
                    world
                        .entities
                        .iter()
                        .find(|(_, slot)| (slot.flags & 0x04) != 0)
                        .map(|(id, _)| *id)
                }
            };

            let Some(target_id) = target_id else {
                tracing::trace!("[send_input] Input dropped: no controlled entity found");
                return Ok(());
            };

            // 2. Prepare actions vector
            let mut actions = Vec::new();

            // Movement action
            if move_x.abs() > f32::EPSILON || move_y.abs() > f32::EPSILON {
                actions.push(PlayerInputKind::Move {
                    x: move_x,
                    y: move_y,
                });
            }

            // Bitmask actions (M1020 mapping)
            // Bit 2: FireTool (Space) - ACTION_FIRE_WEAPON
            if (actions_mask & 0x04) != 0 {
                actions.push(PlayerInputKind::FireTool);
            }
            // Bit 1: ToggleMining (Edge-triggered)
            if (actions_mask & 0x02) != 0 && (*self.last_actions_mask.borrow() & 0x02) == 0 {
                let target = if let Some(id) = target_id_arg {
                    Some(NetworkId(id))
                } else {
                    // VS-02 Auto-target: find nearest resource
                    let world = self.world_state.borrow();
                    if let Some(player_slot) = world.entities.get(&target_id) {
                        let (player_x, player_y) = (player_slot.x, player_slot.y);
                        world
                            .entities
                            .iter()
                            .filter(|(_, slot)| slot.entity_type == 5) // Resource (kind 5)
                            .filter(|(_, slot)| {
                                let dist_sq = (slot.x - player_x) * (slot.x - player_x)
                                    + (slot.y - player_y) * (slot.y - player_y);
                                dist_sq < 25.0 // 5m radius
                            })
                            .min_by(|(_, a), (_, b)| {
                                let dist_a = (a.x - player_x) * (a.x - player_x)
                                    + (a.y - player_y) * (a.y - player_y);
                                let dist_b = (b.x - player_x) * (b.x - player_x)
                                    + (b.y - player_y) * (b.y - player_y);
                                dist_a
                                    .partial_cmp(&dist_b)
                                    .unwrap_or(std::cmp::Ordering::Equal)
                            })
                            .map(|(id, _)| *id)
                    } else {
                        None
                    }
                };

                if let Some(id) = target {
                    actions.push(PlayerInputKind::ToggleExtraction { target: id });
                } else {
                    tracing::warn!(
                        "ToggleExtraction requested without target_id and no resource nearby; dropping action"
                    );
                }
            }

            *self.last_actions_mask.borrow_mut() = actions_mask;

            // 3. Noise reduction check
            let is_repeated = {
                let last_actions = self.last_input_actions.borrow();
                last_actions.len() == actions.len()
                    && last_actions.iter().zip(actions.iter()).all(|(a, b)| a == b)
                    && *self.last_input_target.borrow() == Some(target_id)
            };

            if move_x.abs() > f32::EPSILON || move_y.abs() > f32::EPSILON || actions_mask != 0 {
                if is_repeated {
                    tracing::trace!(
                        tick,
                        move_x,
                        move_y,
                        actions_mask,
                        "Client sending input (repeated)"
                    );
                } else {
                    tracing::trace!(tick, move_x, move_y, actions_mask, "Client sending input");
                }
            }

            let transport_guard = self.transport.borrow();
            let Some(transport) = &*transport_guard else {
                return Err(JsValue::from_str(
                    "Cannot send input: transport not initialized or closed",
                ));
            };

            // Update last input state
            *self.last_input_target.borrow_mut() = Some(target_id);
            *self.last_input_actions.borrow_mut() = actions.clone();

            let cmd = InputCommand {
                tick,
                actions,
                actions_mask,
                last_seen_input_tick: None,
            }
            .clamped();

            // 4. Encode as a ComponentUpdate-compatible packet
            // We use ComponentKind(128) as the convention for InputCommands.
            // The server's TickScheduler will decode this as a standard game update.
            let payload = rmp_serde::to_vec(&cmd)
                .map_err(|e| JsValue::from_str(&format!("Failed to encode InputCommand: {e:?}")))?;

            let update = ReplicationEvent {
                network_id: target_id,
                component_kind: ComponentKind(128),
                payload,
                tick,
            };

            let mut buffer = [0u8; 1024];
            let encoder = SerdeEncoder::new();
            let len = encoder.encode(&update, &mut buffer).map_err(|e| {
                JsValue::from_str(&format!("Failed to encode input replication event: {e:?}"))
            })?;

            // 3. Send via unreliable datagram
            transport
                .send_unreliable(ClientId(0), &buffer[..len])
                .await
                .map_err(|e| {
                    JsValue::from_str(&format!("Transport error during send_input: {e:?}"))
                })?;

            Ok(())
        }

        /// Sends a cursor movement command to the server with 20Hz throttling.
        #[wasm_bindgen]
        pub async fn send_cursor_move(&self, tick: u64, x: f32, y: f32) -> Result<(), JsValue> {
            self.check_worker();

            // 1. Throttling (20Hz = 50ms)
            let now = crate::performance_now();
            {
                let mut last_cursor_send_time = self.last_cursor_send_time.borrow_mut();
                let dt = now - *last_cursor_send_time;
                let dist_sq = (x - *self.last_cursor_x.borrow()).powi(2)
                    + (y - *self.last_cursor_y.borrow()).powi(2);

                // Send if 50ms passed OR if position changed significantly (> 1%)
                if dt < 50.0 && dist_sq < 0.0001 {
                    return Ok(());
                }
                *last_cursor_send_time = now;
            }

            let transport_guard = self.transport.borrow();
            let Some(transport) = &*transport_guard else {
                return Err(JsValue::from_str(
                    "Cannot send cursor move: transport not initialized or closed",
                ));
            };

            // 2. Identify the player entity (same as send_input)
            let target_id = {
                let world = self.world_state.borrow();
                world.player_network_id.or_else(|| {
                    world
                        .entities
                        .iter()
                        .find(|(_, slot)| (slot.flags & 0x04) != 0)
                        .map(|(id, _)| *id)
                })
            };

            let Some(target_id) = target_id else {
                return Ok(());
            };

            // 3. Prepare InputCommand with CursorMove
            let cmd = InputCommand {
                tick,
                actions: vec![PlayerInputKind::CursorMove { x, y }],
                actions_mask: 0,
                last_seen_input_tick: None,
            }
            .clamped();

            let payload = rmp_serde::to_vec(&cmd)
                .map_err(|e| JsValue::from_str(&format!("Failed to encode CursorMove: {e:?}")))?;

            let update = ReplicationEvent {
                network_id: target_id,
                component_kind: ComponentKind(128),
                payload,
                tick,
            };

            let mut buffer = [0u8; 512];
            let encoder = SerdeEncoder::new();
            let len = encoder.encode(&update, &mut buffer).map_err(|e| {
                JsValue::from_str(&format!("Failed to encode cursor replication event: {e:?}"))
            })?;

            transport
                .send_unreliable(ClientId(0), &buffer[..len])
                .await
                .map_err(|e| {
                    JsValue::from_str(&format!("Transport error during send_cursor_move: {e:?}"))
                })?;

            *self.last_cursor_x.borrow_mut() = x;
            *self.last_cursor_y.borrow_mut() = y;

            Ok(())
        }

        #[wasm_bindgen]
        pub async fn playground_clear_server(&self) -> Result<(), JsValue> {
            self.check_worker();

            let transport_guard = self.transport.borrow();
            if let Some(transport) = &*transport_guard {
                let encoder = SerdeEncoder::new();
                let event = NetworkEvent::ClearWorld {
                    client_id: ClientId(0),
                };
                if let Ok(data) = encoder.encode_event(&event) {
                    transport
                        .send_reliable(ClientId(0), &data)
                        .await
                        .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
                    tracing::info!(
                        "Sent ClearWorld command to server — suppressing updates until ack"
                    );
                    // Immediately clear local state and raise the gate.  All incoming
                    // entity updates are suppressed until the server's reliable ClearWorld
                    // ack arrives, preventing stale in-flight datagrams from re-adding
                    // entities that were just despawned on the server.
                    let mut world = self.world_state.borrow_mut();
                    let latest_tick = world.latest_tick;
                    world.entities.clear();
                    world.player_network_id = None;
                    *self.pending_clear.borrow_mut() = true;
                    *self.last_clear_tick.borrow_mut() = latest_tick;
                }
            } else {
                // No transport, clear immediately (no in-flight datagrams to worry about)
                let mut world = self.world_state.borrow_mut();
                world.entities.clear();
                world.player_network_id = None;
            }
            Ok(())
        }

        #[wasm_bindgen]
        pub fn playground_set_rotation_enabled(&self, enabled: bool) {
            *self.playground_rotation_enabled.borrow_mut() = enabled;
        }

        #[wasm_bindgen]
        pub async fn playground_stress_test(
            &self,
            count: u16,
            rotate: bool,
        ) -> Result<(), JsValue> {
            self.check_worker();

            let transport_guard = self.transport.borrow();
            if let Some(transport) = &*transport_guard {
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
        pub fn tick_playground(&self) {
            self.check_worker();

            // M10105 — measure simulation time
            let sim_start = crate::performance_now();

            let now = crate::performance_now();
            let delta_ms = {
                let mut last_process_time = self.last_process_time.borrow_mut();
                let delta = now - *last_process_time;
                *last_process_time = now;
                delta
            };

            // Limit delta to prevent "spiral of death" (max 5 frames)
            let delta_ms = delta_ms.min(100.0);
            let mut tick_accumulator = self.tick_accumulator.borrow_mut();
            *tick_accumulator += delta_ms;

            const DT_MS: f64 = 1000.0 / 60.0;
            let mut steps = 0;
            while *tick_accumulator >= DT_MS {
                let mut world = self.world_state.borrow_mut();
                world.latest_tick += 1;

                // 1. Apply playground input (respected by prediction_enabled flag internally)
                world.playground_apply_input(
                    *self.playground_move_x.borrow(),
                    *self.playground_move_y.borrow(),
                    *self.playground_actions.borrow(),
                );

                // 2. Local physics simulation
                world.simulate();

                *tick_accumulator -= DT_MS;
                steps += 1;
            }

            // Sync to shared world if we simulated at least one step
            if steps > 0 {
                // Publish sub-tick fraction for smooth rendering (M10105)
                let fraction = (*tick_accumulator as f32 / DT_MS as f32).clamp(0.0, 1.0);
                let alpha = 0.8;
                let mut last_fraction = self.last_fraction.borrow_mut();
                *last_fraction = *last_fraction * (1.0 - alpha) + fraction * alpha;
                self.shared_world.set_sub_tick_fraction(*last_fraction);

                let world = self.world_state.borrow();
                let count = world.entities.len() as u32;
                self.flush_to_shared_world(world.latest_tick);

                let sim_time_ms = crate::performance_now() - sim_start;
                with_collector(|c| {
                    c.record_sim(sim_time_ms);
                    c.update_entity_count(count);
                });
            }
        }

        /// Render frame called by the Render Worker.
        pub fn render(&self) -> f64 {
            self.check_worker();

            let tick = self.shared_world.tick();
            let entities = self.shared_world.get_read_buffer();
            let bounds = self.shared_world.get_workspace_bounds();
            {
                let mut render_state = self.render_state.borrow_mut();
                if let Some(state) = &mut *render_state {
                    state.set_workspace_bounds(bounds);
                }
            }

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
                        self.snapshots.borrow().len(),
                    );
                }
                count.set(current + 1);
            });

            // 1. Buffer new snapshots — only push when tick advances
            let mut snapshots = self.snapshots.borrow_mut();
            let back_tick = snapshots.back().map(|s| s.tick).unwrap_or(0);
            if tick < back_tick && tick != 0 {
                tracing::warn!(
                    tick,
                    back_tick,
                    "Simulation time went backwards! Clearing snapshot buffer."
                );
                snapshots.clear();
            }

            if snapshots.is_empty() || tick > back_tick {
                tracing::trace!(tick, "Pushing new snapshot to buffer");
                snapshots.push_back(SimulationSnapshot {
                    tick,
                    entities: entities.to_vec(),
                });
            } else if tick == back_tick && tick != 0 {
                // Diagnostic for stagnant tick
                thread_local! {
                    static STAGNANT_COUNT: core::cell::Cell<u64> = core::cell::Cell::new(0);
                }
                STAGNANT_COUNT.with(|count| {
                    let cur = count.get() + 1;
                    if cur % 300 == 0 {
                        tracing::warn!(tick, "Render loop stalled on same tick for 300 frames");
                    }
                    count.set(cur);
                });
            }

            // 2. Calculate target playback tick with high-precision sub-tick fraction.
            // Stay 2 ticks behind latest so we always have an (s1, s2) interpolation pair.
            // We use the shared_world's sub_tick_fraction to smoothly transition between
            // simulation steps at the monitor's full refresh rate (e.g. 144Hz).
            let latest_tick = snapshots.back().map(|s| s.tick as f32).unwrap_or(0.0);
            let fraction = self.shared_world.sub_tick_fraction();
            let mut target_tick = latest_tick - 1.0 + fraction;

            // Ensure target_tick is within available snapshot range if possible
            if !snapshots.is_empty() {
                let oldest_tick = snapshots[0].tick as f32;
                if target_tick < oldest_tick {
                    target_tick = oldest_tick;
                }
            }

            let ent_count = entities.len();
            let snap_count = snapshots.len() as u32;
            drop(snapshots); // Important: drop borrow before calling render_at_tick
            let frame_time_ms = self.render_at_tick(target_tick);

            // M10105 — record accurate frame time (from WGPU) + snapshot depth.
            if tick % 60 == 0 {
                tracing::trace!(
                    tick,
                    ent_count,
                    snap_count,
                    target_tick,
                    "Render Loop Active"
                );
            }
            with_collector(|c| {
                // FPS is computed in the worker; we only report duration here.
                c.record_frame(frame_time_ms, 0.0);
                c.update_snapshot_count(snap_count);
            });

            frame_time_ms
        }

        fn render_at_tick(&self, target_tick: f32) -> f64 {
            let mut snapshots = self.snapshots.borrow_mut();
            if snapshots.len() < 2 {
                // If we don't have enough snapshots for interpolation,
                // we still want to render the background or at least one frame.
                let mut render_state = self.render_state.borrow_mut();
                if let Some(state) = &mut *render_state {
                    let entities = if !snapshots.is_empty() {
                        snapshots[0].entities.clone()
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

            for i in 0..snapshots.len() - 1 {
                if (snapshots[i].tick as f32) <= target_tick
                    && (snapshots[i + 1].tick as f32) > target_tick
                {
                    s1_idx = i;
                    found = true;
                    break;
                }
            }

            if !found {
                // If we are outside the buffer range, clamp to the nearest edge
                if target_tick < snapshots[0].tick as f32 {
                    s1_idx = 0;
                } else {
                    s1_idx = snapshots.len() - 2;
                }
            }

            let s1 = &snapshots[s1_idx];
            let s2 = &snapshots[s1_idx + 1];

            let tick_range = (s2.tick - s1.tick) as f32;
            let alpha = if tick_range > 0.0 {
                (target_tick - s1.tick as f32) / tick_range
            } else {
                1.0
            }
            .clamp(0.0, 1.0);

            // Interpolate entities into a reusable buffer to avoid per-frame heap allocations
            let mut render_buffer = self.render_buffer.borrow_mut();
            render_buffer.clear();
            render_buffer.extend_from_slice(&s2.entities);

            // Build a lookup map from the previous snapshot for O(1) access per entity.
            let prev_map: std::collections::HashMap<u64, &SabSlot> =
                s1.entities.iter().map(|e| (e.network_id, e)).collect();

            let world = self.world_state.borrow();
            for ent in &mut *render_buffer {
                if let Some(prev) = prev_map.get(&ent.network_id).copied() {
                    if let Some(bounds) = &world.workspace_bounds {
                        ent.x = lerp_wrapped(prev.x, ent.x, alpha, bounds.min_x, bounds.max_x);
                        ent.y = lerp_wrapped(prev.y, ent.y, alpha, bounds.min_y, bounds.max_y);
                    } else {
                        ent.x = lerp(prev.x, ent.x, alpha);
                        ent.y = lerp(prev.y, ent.y, alpha);
                    }
                    ent.z = lerp(prev.z, ent.z, alpha);
                    ent.rotation = lerp_rotation(prev.rotation, ent.rotation, alpha);
                } else {
                    // M1013/M1020 — Extrapolate backwards for newly spawned entities.
                    // This prevents the "blink" where an entity appears to stand still for
                    // one tick before starting to move.
                    let dt = 1.0 / 60.0;
                    let remaining = 1.0 - alpha;

                    if let Some(bounds) = &world.workspace_bounds {
                        // Use wrapped logic for backward extrapolation to handle spawns near bounds
                        ent.x = lerp_wrapped(
                            ent.x,
                            ent.x - ent.dx * dt * remaining,
                            1.0,
                            bounds.min_x,
                            bounds.max_x,
                        );
                        ent.y = lerp_wrapped(
                            ent.y,
                            ent.y - ent.dy * dt * remaining,
                            1.0,
                            bounds.min_y,
                            bounds.max_y,
                        );
                    } else {
                        ent.x -= ent.dx * dt * remaining;
                        ent.y -= ent.dy * dt * remaining;
                    }
                    ent.z -= ent.dz * dt * remaining;
                }
            }

            let mut frame_time = 0.0;
            let mut render_state = self.render_state.borrow_mut();
            if let Some(state) = &mut *render_state {
                frame_time = state.render_frame_with_compact_slots(&render_buffer);
            }

            // 3. Prune old snapshots.
            // We keep the oldest one that is still relevant for interpolation (index 0)
            // and everything newer. We prune snapshots that are entirely behind our window.
            while snapshots.len() > 2 && (snapshots[0].tick as f32) < target_tick - 1.0 {
                snapshots.pop_front();
            }

            // Safety cap: prevent unbounded growth if simulation stops but render continues
            while snapshots.len() > 16 {
                snapshots.pop_front();
            }

            frame_time
        }
    }
}
