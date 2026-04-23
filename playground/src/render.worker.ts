// render.worker.ts - Aetheris Engine Rendering
import init, { AetherisClient } from 'aetheris-client-wasm';
// Same asset URL used by the Game Worker — both workers share the same linear memory.
import wasmUrl from 'aetheris-client-wasm/aetheris_client_wasm_bg.wasm?url';

let client: AetherisClient | null = null;
let running = false;
let frameCount = 0;
let fpsWindowStart = performance.now();

const frame = () => {
    if (!running) return;

    try {
        if (client) {
            const frameTimeMs = client.render();
            frameCount++;
            if (frameCount % 60 === 0) {
                const elapsed = performance.now() - fpsWindowStart;
                const fps = (60 * 1000) / elapsed;
                fpsWindowStart = performance.now();
                self.postMessage({ type: 'metrics_frame', payload: { frameTimeMs, fps } });
            }
        }
        requestAnimationFrame(frame);
    } catch (err) {
        console.error('[RenderWorker] Fatal error in render loop:', err);
        running = false;
    }
};

self.onmessage = async (e) => {
    const { type, canvas, memory, sharedWorldPtr, payload } = e.data;

    if (type === 'init') {
        console.debug('[RenderWorker] Received init, sharedWorldPtr:', `0x${sharedWorldPtr.toString(16)}`);
        console.debug('[RenderWorker] WASM URL:', wasmUrl);

        // 1. Initialize WASM with the shared memory and explicit module URL.
        //    Both workers must use the same WebAssembly.Memory instance so they
        //    share the same linear memory (SharedArrayBuffer).
        await init({ module_or_path: wasmUrl, memory });
        console.debug('[RenderWorker] WASM init complete');

        // 2. Create the client using the pointer to the shared world allocated by the Game Worker
        client = new AetherisClient(sharedWorldPtr);
        console.debug('[RenderWorker] AetherisClient created with shared world ptr');

        try {
            console.log('[RenderWorker] Initializing renderer surface...');
            await client.init_renderer(canvas);
            console.log('Aetheris: Render Worker Initialised');

            running = true;
            fpsWindowStart = performance.now();
            requestAnimationFrame(frame);
        } catch (err) {
            console.error('Aetheris: Render Init Failed', err);
            // Notify the main thread so it can display an error and avoid waiting
            // for a renderer that will never start.
            self.postMessage({ type: 'renderer-init-failed', error: String(err) });
            // running stays false; do NOT call requestAnimationFrame
        }
    } else if (type === 'resize') {
        const { width, height } = e.data.payload;
        if (width > 0 && height > 0) {
            console.debug(`[RenderWorker] Resizing output surface to ${width}x${height}`);
            if (client) client.resize(width, height);
        } else {
            console.warn(`[RenderWorker] Invalid resize request: ${width}x${height}`);
        }
    } else if (type === 'theme_changed') {
        const { bgBase, textPrimary } = e.data.payload;
        console.debug(`[RenderWorker] Theme changed: bg=${bgBase}, text=${textPrimary}`);
        if (client) {
            client.set_theme_colors(bgBase, textPrimary);
        }
    } else if (type === 'pause_toggle') {
        const { paused } = payload;
        // If we were stopped and are now resuming, we need to restart the loop
        const wasRunning = running;
        running = !paused;
        console.log(`[RenderWorker] Running state set to: ${running}`);
        if (!wasRunning && running) {
            requestAnimationFrame(frame);
        }
    } else if (type === 'toggle_debug') {
        console.log('[RenderWorker] Received toggle_debug');
        if (client) {
            try {
                // cycle_debug_mode is only available in debug builds
                if (typeof (client as any).cycle_debug_mode === 'function') {
                    (client as any).cycle_debug_mode();
                    console.log('[RenderWorker] cycle_debug_mode called');
                } else {
                    console.warn('[RenderWorker] cycle_debug_mode not available in this build');
                }
            } catch (e) {
                console.error('[RenderWorker] Failed to toggle debug mode:', e);
            }
        } else {
            console.error('[RenderWorker] Client not initialized');
        }
    } else if (type === 'toggle_grid') {
        console.log('[RenderWorker] Received toggle_grid');
        if (client) {
            try {
                if (typeof (client as any).toggle_grid === 'function') {
                    (client as any).toggle_grid();
                    console.log('[RenderWorker] toggle_grid called');
                } else {
                    console.warn('[RenderWorker] toggle_grid not available');
                }
            } catch (e) {
                console.error('[RenderWorker] Failed to toggle grid:', e);
            }
        } else {
            console.error('[RenderWorker] Client not initialized');
        }
    }
};
