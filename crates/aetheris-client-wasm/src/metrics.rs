//! M10105 — WASM Client Observability Foundation
//!
//! Thread-local metric accumulators running in the Game Worker.
//! Batches `TelemetryEvent` entries and flushes them every 5 seconds
//! via a fire-and-forget `fetch()` call to the server's JSON telemetry
//! endpoint (out-of-band, independent of WebTransport).
//!
//! # ID Generation — ULID
//!
//! Both `session_id` and `trace_id` are derived from a single ULID generated
//! at Game Worker startup:
//!
//! - `session_id` — ULID Crockford base32 (26 chars). Sortable by creation
//!   time in Loki; groups all events from the same browser tab.
//! - `trace_id` — same ULID u128 encoded as 32 lowercase hex chars. This is
//!   the **W3C TraceContext-compatible** format (Jaeger / OpenTelemetry native).
//!   Because ULID MSBs are the timestamp, trace IDs are chronologically ordered
//!   in Jaeger's timeline view without any extra sorting.
//!
//! # Architecture
//!
//! The Game Worker owns all metric state because:
//! - The Render Worker must not block on network I/O.
//! - The Render Worker posts `frame_time_ms` via `postMessage` to the
//!   Game Worker, which accumulates it here.
//! - The 5-second flush fires from the Game Worker's setInterval loop.
//!
//! # Cardinality Rule (Risk M1062 / M10105 §5.2)
//!
//! Server-side Prometheus labels use only static values (`client_type =
//! "wasm_playground"`). Client `session_id` is sent as a _field_ in the
//! `TelemetryBatch.session_id` field (used for Loki log correlation only),
//! never as a Prometheus label.

#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

/// Maximum number of events held in the client-side ring buffer.
/// Matches M1019 §4 budget (64 × ~256 bytes = ≤ 16 KB).
pub const TELEMETRY_BUFFER_CAP: usize = 64;

/// How many of the most recent frame-time samples to keep for p99 calculation.
const FRAME_SAMPLE_CAP: usize = 128;

/// Number of frames between `postMessage` flushes from Render → Game worker.
/// Sending every frame at 60 FPS would be ~4 MB/s of IPC. Every 60 frames (~1s)
/// costs < 100 bytes/s.
pub const RENDER_METRIC_IPC_INTERVAL: u32 = 60;

// ---------------------------------------------------------------------------
// ULID generation — no rand feature, uses getrandom 0.2 directly
// ---------------------------------------------------------------------------

/// Generate a ULID using `js_sys::Date::now()` for the timestamp and
/// `getrandom` for the 80 random bits. Returns `(session_id, trace_id)`:
///
/// - `session_id`: Crockford base32 string (26 chars) — human-readable, sortable in Loki.
/// - `trace_id`: 32 lowercase hex chars from the same ULID u128 — W3C TraceContext /
///   Jaeger / OpenTelemetry native format. Chronologically sortable because the
///   ULID MSBs are the 48-bit millisecond timestamp.
pub fn generate_ulid_ids() -> (String, String) {
    let timestamp_ms = js_sys::Date::now() as u64;

    // 10 random bytes = 80 random bits (ULID random component)
    let mut random_bytes = [0u8; 10];
    getrandom::getrandom(&mut random_bytes)
        .expect("getrandom is always available in WASM with the 'js' feature");

    // Pack bytes into u128 (left-aligned; lower bits zeroed — matches ULID spec)
    let random: u128 = random_bytes
        .iter()
        .fold(0u128, |acc, &b| (acc << 8) | u128::from(b));

    let id = ulid::Ulid::from_parts(timestamp_ms, random);

    // session_id: Crockford base32 (e.g. "01JSZG2XKQP4V3R8N0CDWM7HFT")
    let session_id = id.to_string();

    // trace_id: same 128 bits as 32 lowercase hex chars
    // Example: "01961f8e5a80014bf7c3d6a1b2e8f902"
    // Fully W3C TraceContext-compatible (traceparent format uses this exact encoding).
    let trace_id = format!("{:032x}", id.0);

    (session_id, trace_id)
}

// ---------------------------------------------------------------------------
// Wire format — mirrors proto TelemetryEvent (JSON, not protobuf, for fetch)
// ---------------------------------------------------------------------------

/// A single telemetry event serialised to JSON for the batch `fetch()` call.
/// Using JSON here avoids a prost encode in the WASM hot path; the server
/// accepts both gRPC-web binary and (via a thin shim) plain JSON.
#[derive(serde::Serialize, Clone)]
pub struct TelemetryEventJson {
    pub timestamp_ms: f64,
    pub level: u32, // 1=INFO 2=WARN 3=ERROR (matches proto TelemetryLevel)
    pub target: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtt_ms: Option<f64>,
    pub trace_id: String,
    pub span_name: String,
}

#[derive(serde::Serialize)]
struct TelemetryBatchJson {
    events: Vec<TelemetryEventJson>,
    session_id: String,
}

// ---------------------------------------------------------------------------
// Snapshot — for real-time UI display
// ---------------------------------------------------------------------------

/// A snapshot of current client-side metrics for real-time Display.
#[derive(serde::Serialize)]
pub struct MetricsSnapshot {
    pub fps: f64,
    pub frame_time_p99: f64,
    pub sim_time_p99: f64,
    pub rtt_ms: Option<f64>,
    pub entity_count: u32,
    pub snapshot_count: u32,
    pub dropped_events: u32,
}

// ---------------------------------------------------------------------------
// Ring buffer
// ---------------------------------------------------------------------------

/// A fixed-capacity ring buffer used as the telemetry event queue.
pub struct EventRing {
    buf: Vec<TelemetryEventJson>,
    head: usize, // write position (wraps)
    len: usize,  // number of live items (≤ CAP)
    pub dropped: u32,
}

impl EventRing {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(TELEMETRY_BUFFER_CAP),
            head: 0,
            len: 0,
            dropped: 0,
        }
    }

    pub fn push(&mut self, event: TelemetryEventJson) {
        if self.buf.len() < TELEMETRY_BUFFER_CAP {
            self.buf.push(event);
            self.len = self.buf.len();
        } else {
            // Overwrite the oldest slot (ring semantics).
            self.buf[self.head] = event;
            self.head = (self.head + 1) % TELEMETRY_BUFFER_CAP;
            self.dropped += 1;
        }
    }

    /// Drain all events in insertion order.
    pub fn drain(&mut self) -> Vec<TelemetryEventJson> {
        if self.buf.is_empty() {
            return Vec::new();
        }
        let cap = self.buf.len();
        let mut out = Vec::with_capacity(cap);
        if self.len < cap {
            // Buffer not yet fully populated — linear order.
            out.extend(self.buf.drain(..));
        } else {
            // Full ring: start from `head` (oldest).
            for i in 0..cap {
                out.push(self.buf[(self.head + i) % cap].clone());
            }
            self.buf.clear();
        }
        self.head = 0;
        self.len = 0;
        out
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0 && self.buf.is_empty()
    }
}

// ---------------------------------------------------------------------------
// P99 approximator (reservoir, sorted on flush)
// ---------------------------------------------------------------------------

struct FrameSampler {
    samples: Vec<f64>,
    cursor: usize,
}

impl FrameSampler {
    fn new() -> Self {
        Self {
            samples: Vec::with_capacity(FRAME_SAMPLE_CAP),
            cursor: 0,
        }
    }

    fn push(&mut self, v: f64) {
        if self.samples.len() < FRAME_SAMPLE_CAP {
            self.samples.push(v);
        } else {
            self.samples[self.cursor] = v;
            self.cursor = (self.cursor + 1) % FRAME_SAMPLE_CAP;
        }
    }

    fn p99(&mut self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let len = self.samples.len();
        let idx = ((len as f64 * 0.99) as usize).min(len - 1);

        // Use select_nth_unstable to get the p99 value in O(n) average time
        // instead of O(n log n) full sort.
        let (_, p99_val, _) = self.samples.select_nth_unstable_by(idx, |a, b| {
            a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
        });
        *p99_val
    }
}

// ---------------------------------------------------------------------------
// MetricsCollector — owns the Game Worker's metric state
// ---------------------------------------------------------------------------

/// Central metric accumulator for the Game Worker.
///
/// Call `push_event()` for lifecycle/error events.
/// Call `record_frame()` when a frame-time sample arrives via IPC.
/// Call `record_sim()` when a `tick_playground()` finishes.
/// Call `flush()` every 5 seconds.
pub struct MetricsCollector {
    ring: EventRing,

    // FPS / frame-time
    frame_sampler: FrameSampler,
    fps_current: f64,

    // Simulation time
    sim_sampler: FrameSampler,

    // RTT
    last_rtt_ms: Option<f64>,

    // Entity count (last known)
    entity_count: u32,

    // Snapshot buffer depth (render interpolation)
    snapshot_count: u32,

    // Flush endpoint
    telemetry_url: String,

    // Session correlation (page-load UUID → Loki grouping)
    pub session_id: String,

    // W3C trace ID propagated with every event
    pub trace_id: String,
}

impl MetricsCollector {
    /// Creates a new collector and immediately generates a ULID pair:
    /// `session_id` (Crockford base32) and `trace_id` (32-char hex).
    ///
    /// `telemetry_url` — base URL of the control-plane server
    ///   (e.g. `"http://127.0.0.1:50051"`). `/telemetry/json` is appended.
    pub fn new(telemetry_url: String) -> Self {
        let (session_id, trace_id) = generate_ulid_ids();
        Self {
            ring: EventRing::new(),
            frame_sampler: FrameSampler::new(),
            fps_current: 0.0,
            sim_sampler: FrameSampler::new(),
            last_rtt_ms: None,
            entity_count: 0,
            snapshot_count: 0,
            telemetry_url,
            session_id,
            trace_id,
        }
    }

    // ------------------------------------------------------------------
    // Data ingestion
    // ------------------------------------------------------------------

    /// Record a rendered frame. `fps` is measured by the Render Worker over the
    /// last 60 frames — it owns `requestAnimationFrame` so is the authoritative
    /// source of frame rate.
    pub fn record_frame(&mut self, frame_time_ms: f64, fps: f64) {
        self.frame_sampler.push(frame_time_ms);
        self.fps_current = fps;
    }

    /// Record a simulation tick duration.
    pub fn record_sim(&mut self, sim_time_ms: f64) {
        self.sim_sampler.push(sim_time_ms);
    }

    /// Update the last known RTT (from Pong messages).
    pub fn update_rtt(&mut self, rtt_ms: f64) {
        self.last_rtt_ms = Some(rtt_ms);
    }

    /// Update entity count (from `flush_to_shared_world`).
    pub fn update_entity_count(&mut self, count: u32) {
        self.entity_count = count;
    }

    /// Update snapshot buffer depth (render interpolation buffer).
    pub fn update_snapshot_count(&mut self, count: u32) {
        self.snapshot_count = count;
    }

    /// Push a structured lifecycle / error event.
    pub fn push_event(
        &mut self,
        level: u32,
        target: &str,
        message: &str,
        span_name: &str,
        rtt_ms: Option<f64>,
    ) {
        self.ring.push(TelemetryEventJson {
            timestamp_ms: js_sys::Date::now(),
            level,
            target: target.to_string(),
            message: message.to_string(),
            rtt_ms,
            trace_id: self.trace_id.clone(),
            span_name: span_name.to_string(),
        });
    }

    // ------------------------------------------------------------------
    // Flush
    // ------------------------------------------------------------------

    /// Snapshot current metrics and push them as a single INFO event,
    /// then drain the ring buffer and fire-and-forget POST to /telemetry/json.
    ///
    /// Safe to call before `AetherisClient::new()` (satisfies M10105 AC-01).
    /// Uses `spawn_local` so it never blocks the caller.
    pub fn flush(&mut self) {
        self.flush_internal(false);
    }

    /// Internal flush implementation with keepalive support.
    fn flush_internal(&mut self, keepalive: bool) {
        // Build summary event from accumulated metrics
        let frame_p99 = self.frame_sampler.p99();
        let sim_p99 = self.sim_sampler.p99();
        let summary = format!(
            "fps={:.1} frame_p99={:.2}ms sim_p99={:.2}ms rtt={} entities={} snapshots={} dropped={}",
            self.fps_current,
            frame_p99,
            sim_p99,
            self.last_rtt_ms
                .map_or_else(|| "N/A".to_string(), |r| format!("{r:.1}ms")),
            self.entity_count,
            self.snapshot_count,
            self.ring.dropped,
        );

        self.push_event(1, "metrics", &summary, "metrics_snapshot", self.last_rtt_ms);

        // Drain ring and reset dropped counter so each flush sends the delta
        let events = self.ring.drain();
        let _ = std::mem::replace(&mut self.ring.dropped, 0);

        if events.is_empty() {
            return;
        }

        let batch = TelemetryBatchJson {
            events,
            session_id: self.session_id.clone(),
        };

        let url = format!("{}/telemetry/json", self.telemetry_url);

        spawn_local(async move {
            if let Err(e) = post_telemetry(&url, &batch, keepalive).await {
                // Non-fatal: telemetry must not affect gameplay
                web_sys::console::warn_1(&format!("[Metrics] flush failed: {e}").into());
            }
        });
    }

    /// Force-flush remaining events (called on `beforeunload`).
    /// Uses keepalive: true to ensure the request survives worker termination.
    pub fn flush_sync_fire_and_forget(&mut self) {
        self.flush_internal(true);
    }

    /// Snapshot current metrics for UI display.
    pub fn snapshot(&mut self) -> MetricsSnapshot {
        MetricsSnapshot {
            fps: self.fps_current,
            frame_time_p99: self.frame_sampler.p99(),
            sim_time_p99: self.sim_sampler.p99(),
            rtt_ms: self.last_rtt_ms,
            entity_count: self.entity_count,
            snapshot_count: self.snapshot_count,
            dropped_events: self.ring.dropped,
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP helper — JSON body, no gRPC framing needed for the thin JSON shim
// ---------------------------------------------------------------------------

async fn post_telemetry(
    url: &str,
    batch: &TelemetryBatchJson,
    keepalive: bool,
) -> Result<(), String> {
    use wasm_bindgen::JsCast;

    let body = serde_json::to_string(batch).map_err(|e| format!("serialize: {e}"))?;

    let headers = web_sys::Headers::new().map_err(|e| format!("headers: {e:?}"))?;
    headers
        .set("Content-Type", "application/json")
        .map_err(|e| format!("header set: {e:?}"))?;

    // M10105 — Use AbortController to prevent leaked promise handles on failure
    let controller =
        web_sys::AbortController::new().map_err(|e| format!("abort_controller: {e:?}"))?;
    let signal = controller.signal();

    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_body(&JsValue::from_str(&body));
    init.set_headers(&headers);
    init.set_signal(Some(&signal));

    // Modern fetch keepalive for terminal flushes
    if keepalive {
        js_sys::Reflect::set(
            init.as_ref(),
            &JsValue::from_str("keepalive"),
            &JsValue::TRUE,
        )
        .map_err(|e| format!("keepalive: {e:?}"))?;
    }

    let request = web_sys::Request::new_with_str_and_init(url, &init).map_err(|e| {
        controller.abort();
        format!("request: {e:?}")
    })?;

    let global = js_sys::global();
    let promise = if let Ok(scope) = global.clone().dyn_into::<web_sys::WorkerGlobalScope>() {
        scope.fetch_with_request(&request)
    } else {
        global
            .dyn_into::<web_sys::Window>()
            .map_err(|_| {
                controller.abort();
                "no fetch context".to_string()
            })?
            .fetch_with_request(&request)
    };

    let resp_val = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(|e| {
            controller.abort();
            format!("fetch: {e:?}")
        })?;

    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| {
        controller.abort();
        "response cast failed".to_string()
    })?;

    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// WASM-bindgen entry point — called from game.worker.ts every 5 seconds
// ---------------------------------------------------------------------------

/// Globally accessible flush function exposed to JS.
/// Called by the Game Worker's setInterval (every 5 000 ms).
///
/// # Safety
/// Only ever called from the single Game Worker thread that owns the collector.
#[wasm_bindgen]
pub fn wasm_flush_telemetry() {
    COLLECTOR.with(|c| {
        if let Ok(mut col) = c.try_borrow_mut() {
            if let Some(collector) = col.as_mut() {
                collector.flush();
            }
        }
    });
}

/// Initialize the global collector. Called once from `AetherisClient::new()`
/// or directly from `game.worker.ts` before any `AetherisClient` is created
/// (satisfies M10105 AC-01: flush available before `new()`).
///
/// ULID pair (`session_id` + `trace_id`) is generated internally — no JS-side
/// UUID generation needed.
#[wasm_bindgen]
pub fn wasm_init_telemetry(telemetry_url: String) {
    COLLECTOR.with(|c| {
        let mut col = c.borrow_mut();
        if col.is_none() {
            *col = Some(MetricsCollector::new(telemetry_url));
        } else {
            web_sys::console::debug_1(
                &"wasm_init_telemetry: collector already initialized, ignoring".into(),
            );
        }
    });
}

/// Push a lifecycle span event from JS (e.g. render worker timing reports).
#[wasm_bindgen]
pub fn wasm_push_telemetry_event(level: u32, target: String, message: String, span_name: String) {
    COLLECTOR.with(|c| {
        if let Ok(mut col) = c.try_borrow_mut() {
            if let Some(collector) = col.as_mut() {
                collector.push_event(level, &target, &message, &span_name, None);
            }
        }
    });
}

/// Record a frame-time sample and FPS (called from Render Worker via postMessage → Game Worker).
/// `fps` is computed in the Render Worker over the last 60 frames for accuracy.
#[wasm_bindgen]
pub fn wasm_record_frame_time(frame_time_ms: f64, fps: f64) {
    COLLECTOR.with(|c| {
        if let Ok(mut col) = c.try_borrow_mut() {
            if let Some(collector) = col.as_mut() {
                collector.record_frame(frame_time_ms, fps);
            }
        }
    });
}

/// Retrieve a real-time snapshot of the metrics for UI display.
#[wasm_bindgen]
pub fn wasm_get_metrics() -> JsValue {
    COLLECTOR.with(|c| {
        if let Ok(mut col) = c.try_borrow_mut() {
            if let Some(collector) = col.as_mut() {
                let snap = collector.snapshot();
                return serde_wasm_bindgen::to_value(&snap).unwrap_or(JsValue::NULL);
            }
        }
        JsValue::NULL
    })
}

thread_local! {
    /// The singleton collector for this Game Worker thread.
    static COLLECTOR: std::cell::RefCell<Option<MetricsCollector>> =
        std::cell::RefCell::new(None);
}

/// Internal helper used by `lib.rs` without going through the JS boundary.
pub fn with_collector<F: FnOnce(&mut MetricsCollector)>(f: F) {
    COLLECTOR.with(|c| {
        if let Ok(mut col) = c.try_borrow_mut() {
            if let Some(collector) = col.as_mut() {
                f(collector);
            }
        }
    });
}
