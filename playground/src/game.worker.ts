// game.worker.ts - Aetheris Engine Simulation
import init, { AetherisClient, wasm_init_telemetry, wasm_flush_telemetry, wasm_record_frame_time, wasm_get_metrics } from 'aetheris-client-wasm';
// Vite resolves this URL at bundle time to the correct .wasm asset path.
import wasmUrl from 'aetheris-client-wasm/aetheris_client_wasm_bg.wasm?url';

let client: AetherisClient | null = null;
let simIntervalId: number | ReturnType<typeof setInterval> | null = null;
let flushIntervalId: number | ReturnType<typeof setInterval> | null = null;
let currentSessionToken: string | null = null;
let metricsIntervalId: number | ReturnType<typeof setInterval> | null = null;
let isPaused = false;
let isBusy = false;
let lastConnectParams: { url: string, token: string, certHash?: Uint8Array } | null = null;
let reconnectAttemptCount = 0;
let isReconnecting = false;
const heldKeys = new Set<string>();

// M10105 — FIFO Queue for UI commands to prevent async race conditions
const commandQueue: ((c: AetherisClient) => Promise<void> | void)[] = [];

/**
 * Processes the next command in the queue if the client is not busy.
 */
async function processQueue() {
    if (!client || isBusy || commandQueue.length === 0) return;

    isBusy = true;
    const op = commandQueue.shift()!;
    try {
        await op(client);
    } catch (e) {
        console.error('[GameWorker] Queued operation failed:', e);
    } finally {
        isBusy = false;
        // Check for more commands immediately
        setTimeout(processQueue, 0);
    }
}

/**
 * M10146 — Triggers the reconnection state machine with exponential backoff.
 */
async function attemptReconnection() {
    if (!client || !lastConnectParams || isReconnecting) return;

    isReconnecting = true;
    reconnectAttemptCount++;
    const baseDelay = Math.min(10000, 500 * Math.pow(1.5, reconnectAttemptCount - 1));
    const jitter = (Math.random() * 0.4) + 0.8; // ±20% jitter
    const delay = baseDelay * jitter;

    console.log(`[GameWorker] Reconnecting in ${Math.round(delay)}ms... (attempt ${reconnectAttemptCount})`);
    self.postMessage({ type: 'reconnecting', payload: { attempt: reconnectAttemptCount, delay } });

    setTimeout(async () => {
        try {
            await withClient(async (c) => {
                await c.reconnect(lastConnectParams!.url, lastConnectParams!.certHash);
            });
            console.log('[GameWorker] Reconnection successful');
            reconnectAttemptCount = 0;
            isReconnecting = false;
            self.postMessage({ type: 'connection_ready', payload: { clientId: 'P-123', tick: 0 } });
        } catch (e) {
            console.warn('[GameWorker] Reconnection attempt failed:', e);
            isReconnecting = false;
            attemptReconnection();
        }
    }, delay);
}

/** 
 * Safely executes a function with the client by queuing it and returning
 * the resulting Promise so callers can await completion.
 */
function withClient(op: (c: AetherisClient) => (Promise<void> | void)): Promise<void> {
    return new Promise<void>((resolve, reject) => {
        commandQueue.push(async (c) => {
            try {
                await op(c);
                resolve();
            } catch (e) {
                reject(e);
            }
        });
        processQueue();
    });
}

// M10105 — finalize message (sent from main thread beforeunload) flushes telemetry.
// self.onclose is not a standard WorkerGlobalScope event; use the 'finalize' message instead.

/**
 * Bootstraps common systems like telemetry and flush timers.
 * Satisfies M10105 AC-01 (Safe to call before client init).
 */
function bootstrapSystems() {
    const telemetryUrl = import.meta.env.VITE_TELEMETRY_URL || '';
    if (telemetryUrl) {
        wasm_init_telemetry(telemetryUrl);
    }

    if (flushIntervalId) clearInterval(flushIntervalId);
    flushIntervalId = setInterval(() => {
        try { wasm_flush_telemetry(); } catch { /* non-fatal */ }
    }, 5000);
}


let manifestPollCounter = 0;
let lastLoggedManifestWorker = '';

/**
 * Runs a single metrics poll cycle without scheduling the next one.
 * Use this for immediate/on-demand polls to avoid creating duplicate loops.
 */
async function pollMetricsOnce(forceManifest: boolean = false) {
    manifestPollCounter++;
    const shouldRequestManifest = forceManifest || manifestPollCounter >= 10;
    if (shouldRequestManifest) manifestPollCounter = 0;

    try {
        const metrics = wasm_get_metrics();
        if (client) {
            await withClient(async (c) => {
                // 1. Periodic request from server (throttled to 10s)
                if (shouldRequestManifest) {
                    console.log(`[GameWorker] Outgoing System Manifest request (force=${forceManifest})...`);
                    await c.request_system_manifest();
                }

                // 2. Continuous retrieval from local WASM state (polled every 1s)
                // This ensures we catch the server response as soon as it arrives,
                // regardless of the outgoing request throttle.
                const manifest = (c as any).get_system_info();

                if (manifest && ((manifest instanceof Map && manifest.size > 0) || (Object.keys(manifest).length > 0))) {
                    const manifestObj = (manifest instanceof Map) ? Object.fromEntries(manifest) : (manifest as any);

                    // Deterministic stringify by sorting keys to avoid false-positive changes
                    const sortedKeys = Object.keys(manifestObj).sort();
                    const manifestStr = JSON.stringify(sortedKeys.map(k => [k, manifestObj[k]]));

                    if (manifestStr !== lastLoggedManifestWorker) {
                        lastLoggedManifestWorker = manifestStr;
                        // We don't log in the worker anymore to avoid redundancy with the main thread,
                        // but we still de-duplicate here to save IPC bandwidth.
                        self.postMessage({ type: 'wasm_metrics', payload: metrics, manifest });
                    } else {
                        self.postMessage({ type: 'wasm_metrics', payload: metrics });
                    }
                } else {
                    self.postMessage({ type: 'wasm_metrics', payload: metrics });
                }
            });
        } else if (metrics) {
            self.postMessage({ type: 'wasm_metrics', payload: metrics });
        }
    } catch (e) {
        /* non-fatal */
    }
}

/**
 * Consolidated metrics polling scheduler (M10105 / M10115).
 * Calls pollMetricsOnce then re-arms itself. Never call this directly for
 * immediate/one-shot polls — use pollMetricsOnce() instead to avoid creating
 * duplicate scheduler loops.
 */
async function pollMetrics(forceManifest: boolean = false) {
    if (isPaused) {
        metricsIntervalId = setTimeout(pollMetrics, 1000);
        return;
    }
    await pollMetricsOnce(forceManifest);
    metricsIntervalId = setTimeout(pollMetrics, 1000);
}

self.onmessage = async (e) => {
    const { type, payload, memory } = e.data;

    if (type === 'init') {
        console.debug('[GameWorker] Received init, memory:', memory);
        console.debug('[GameWorker] WASM URL:', wasmUrl);

        await init({ module_or_path: wasmUrl, memory });
        console.debug('[GameWorker] WASM init complete');

        client = new AetherisClient();
        bootstrapSystems();
        const sharedWorldPtr = client.shared_world_ptr();
        self.postMessage({ type: 'ready', sharedWorldPtr });
    } else if (type === 'finalize') {
        // M10105 — Reliable final flush triggered by main thread beforeunload
        console.debug('[GameWorker] Finalizing telemetry before shutdown...');
        try { wasm_flush_telemetry(); } catch { /* ignore */ }

    } else if (type === 'key_down') {
        heldKeys.add(payload.key);
    } else if (type === 'key_up') {
        heldKeys.delete(payload.key);
    } else if (type === 'clear_keys') {
        heldKeys.clear();
    } else if (type === 'pause_toggle') {
        isPaused = typeof payload?.paused === 'boolean' ? payload.paused : !isPaused;
        self.postMessage({ type: 'pause_state', payload: { paused: isPaused } });
    } else if (type === 'init_playground') {
        console.debug('[GameWorker] Received init_playground');
        await init({ module_or_path: wasmUrl, memory });
        client = new AetherisClient();
        bootstrapSystems();

        const sharedWorldPtr = client.shared_world_ptr();
        self.postMessage({ type: 'ready', sharedWorldPtr });

        // M10105 — Simulation loop (high precision)
        if (simIntervalId) clearTimeout(simIntervalId as number);
        const tickLoop = async () => {
            if (isPaused) {
                simIntervalId = setTimeout(tickLoop, 500); // Throttled check
                return;
            }
            withClient((c) => {
                const input = computeInput(0); // Tick doesn't matter much for local apply
                c.playground_apply_input(input.move_x, input.move_y, input.actions_mask);
                c.tick_playground();
            });
            simIntervalId = setTimeout(tickLoop, 1000 / 60);
        };
        simIntervalId = setTimeout(tickLoop, 0);

        if (metricsIntervalId) clearTimeout(metricsIntervalId as number);
        metricsIntervalId = setTimeout(pollMetrics, 1000);

    } else if (type === 'metrics_frame') {
        // M10105 — frame time + FPS from Render Worker (posted every 60 frames)
        try { wasm_record_frame_time(payload.frameTimeMs, payload.fps); } catch { /* non-fatal */ }
    } else if (type === 'p_spawn') {
        console.log(`[GameWorker] Spawn requested: ${payload.type} at (${payload.x}, ${payload.y})`);
        if (client) {
            if (currentSessionToken) {
                // Network spawn
                withClient(async (c) => await c.playground_spawn_net(payload.type, payload.x, payload.y, payload.rot));
            } else {
                // Local spawn (Sandbox)
                withClient(c => c.playground_spawn(payload.type, payload.x, payload.y, payload.rot));
            }
        }
    } else if (type === 'p_clear') {
        console.log('[GameWorker] Clear world requested');
        withClient(async (c) => await c.playground_clear_server());
    } else if (type === 'p_toggle_rotation') {
        console.log(`[GameWorker] Toggle rotation: ${payload.enabled}`);
        withClient(c => c.playground_set_rotation_enabled(payload.enabled));
    } else if (type === 'p_stress_test') {
        console.log(`[GameWorker] Stress test requested: ${payload.count} units (rotate: ${payload.rotate})`);
        withClient(c => {
            c.playground_set_rotation_enabled(payload.rotate);
            c.playground_clear();
            for (let i = 0; i < payload.count; i++) {
                const x = (Math.random() - 0.5) * 40;
                const y = (Math.random() - 0.5) * 40;
                const etype = [1, 3, 4, 5, 6][Math.floor(Math.random() * 5)];
                c.playground_spawn(etype, x, y, Math.random() * Math.PI * 2);
            }
            console.log(`[GameWorker] Stress test spawned ${payload.count} units`);
        });
    } else if (type === 'p_stress_test_net') {
        console.log(`[GameWorker] Network Stress test requested: ${payload.count} units (rotate: ${payload.rotate})`);
        withClient(async (c) => {
            await c.playground_stress_test(payload.count, payload.rotate);
            console.log(`[GameWorker] playground_stress_test() WASM call dispatched — waiting for server response`);
        });
    } else if (type === 'start_session') {
        console.log('[GameWorker] StartSession requested — asking server for session ship + Possession');
        withClient(async (c) => {
            await c.start_session_net();
        });
    } else if (type === 'request_otp') {
        try {
            if (!client) throw new Error('Client not initialized');
            const { email } = payload;
            const baseUrl = import.meta.env.VITE_AUTH_URL || 'http://127.0.0.1:50051';
            console.log(`[Worker] Requesting OTP for ${email} at ${baseUrl}`);
            const requestId = await AetherisClient.request_otp(baseUrl, email);
            self.postMessage({ type: 'otp_requested', payload: { requestId } });
        } catch (err) {
            self.postMessage({ type: 'connection_error', payload: { reason: (err as any), retriable: true } });
        }

    } else if (type === 'login_otp') {
        try {
            if (!client) throw new Error('Client not initialized');
            const { requestId, code } = payload;
            const baseUrl = import.meta.env.VITE_AUTH_URL || 'http://127.0.0.1:50051';
            console.log(`[Worker] Logging in with OTP: ${requestId} code: ${code} at ${baseUrl}`);
            const sessionToken = await AetherisClient.login_with_otp(baseUrl, requestId, code);
            currentSessionToken = sessionToken;
            self.postMessage({ type: 'login_success', payload: { sessionToken } });
        } catch (err) {
            self.postMessage({ type: 'connection_error', payload: { reason: (err as any), retriable: true } });
        }

    } else if (type === 'p_request_metrics') {
        // Manually trigger a one-shot metrics poll (M10105 / M10115).
        // Use pollMetricsOnce to avoid spawning a duplicate scheduler loop.
        pollMetricsOnce(true).catch(e => console.error('[GameWorker] Manual metrics poll failed:', e));
    } else if (type === 'connect') {
        const { url, token, certHash: certHashStr } = payload;
        console.debug(`[GameWorker] Connect requested: url=${url}, token=${token ? '***' : 'MISSING'}, certHash=${certHashStr || 'NONE'}`);

        const certHash = certHashStr ? Uint8Array.from(atob(certHashStr.trim()), c => c.charCodeAt(0)) : undefined;
        if (certHash) {
            console.debug(`[GameWorker] Decoded certHash: ${certHash.length} bytes`);
        }

        try {
            if (!client) {
                throw new Error('client not initialized');
            }
            lastConnectParams = { url, token, certHash };
            reconnectAttemptCount = 0;

            await withClient(async (c) => {
                c.set_session_token(token);
                await c.connect(url, certHash);
            });

            self.postMessage({
                type: 'connection_ready',
                payload: { clientId: 'P-123', tick: 0 } // Real ID from server later
            });

            if (simIntervalId) clearTimeout(simIntervalId);
            let localInputTick = 0n;
            const tickLoop = async () => {
                if (isPaused) {
                    simIntervalId = setTimeout(tickLoop, 500);
                    return;
                }

                await withClient(async (c) => {
                    // M10146 — Send input to server before ticking
                    // We must use a monotonically increasing tick for inputs, 
                    // otherwise the server's anti-replay measure (Kind=128) drops inputs 
                    // generated within the same server tick window.
                    if (localInputTick === 0n) {
                        // Initialize sequence to a reasonable tick context
                        localInputTick = BigInt(c.latest_tick()) || 1n;
                    }

                    const input = computeInput(Number(localInputTick));
                    await c.send_input(localInputTick, input.move_x, input.move_y, input.actions_mask, null);
                    localInputTick++;

                    await c.tick();

                    // M10146 — Check for disconnected state and trigger reconnection
                    if (c.connection_state === 0) { // 0 is Disconnected
                        attemptReconnection();
                    }
                });

                simIntervalId = setTimeout(tickLoop, 1000 / 60);
            };
            simIntervalId = setTimeout(tickLoop, 0);

            // Reset manifest cache to ensure the new UI instance receives it
            lastLoggedManifestWorker = '';
            pollMetricsOnce(true).catch(e => console.error('[GameWorker] Initial manifest poll failed:', e));
        } catch (err) {
            console.error('[GameWorker] Connection failed:', err);
            self.postMessage({ type: 'connection_error', payload: { reason: (err as any).toString(), retriable: true } });
        }

    } else if (type === 'logout') {
        const baseUrl = import.meta.env.VITE_AUTH_URL || 'http://127.0.0.1:50051';
        try {
            if (currentSessionToken) {
                await AetherisClient.logout(baseUrl, currentSessionToken);
            }
            if (simIntervalId) clearInterval(simIntervalId);
            self.postMessage({ type: 'logout_success' });
        } catch (err: any) {
            console.error('[GameWorker] Logout failed:', err);
            if (simIntervalId) clearInterval(simIntervalId);
            self.postMessage({ type: 'logout_error', error: err?.message || String(err) });
        }
    }
};

/**
 * Computes an InputFrame from the currently held keys.
 * Maps WASD/Arrows to move_x/move_y and Space/F to action bits.
 */
function computeInput(tick: number) {
    let move_x = 0;
    let move_y = 0;
    let actions_mask = 0;

    // Movement (WASD + Arrows)
    if (heldKeys.has('KeyW') || heldKeys.has('ArrowUp')) move_y = 1.0;
    if (heldKeys.has('KeyS') || heldKeys.has('ArrowDown')) move_y = -1.0;
    if (heldKeys.has('KeyA') || heldKeys.has('ArrowLeft')) move_x = -1.0;
    if (heldKeys.has('KeyD') || heldKeys.has('ArrowRight')) move_x = 1.0;

    // Specific Actions
    if (heldKeys.has('Space')) actions_mask |= 0x01; // FirePrimary
    if (heldKeys.has('KeyF')) actions_mask |= 0x02; // ToggleMining

    return { tick, move_x, move_y, actions_mask };
}


