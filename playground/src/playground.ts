// playground.ts - Aetheris Engine Playground Controller

import { shortcuts } from './shortcuts';
import { WorldRegistry } from './world-registry';

const CONNECTED_MODE = true; // Always connected for VS-01+ authoritative validation

// Declared globally via vite.config.ts
declare const __APP_VERSION__: string;

const ViewState = {
    Logo: 0,
    Roaming: 1,
    Entering: 2,
    Playing: 3,
};

class AetherisPlayground {
    private gameWorker!: Worker;
    private renderWorker!: Worker;
    private statusEl: HTMLElement;
    private memory!: WebAssembly.Memory;
    private entityCount: number = 0;
    private currentRequestId: string | null = null;
    private world!: WorldRegistry;
    private _monitorRafId: number | null = null;
    private heldKeys: Set<string> = new Set();
    private isSessionActive: boolean = false;
    private lastLoggedManifest: string = '';

    constructor() {
        this.statusEl = document.getElementById('status')!;
        this.initPlayground().catch(err => {
            console.error('[Playground] Fatal boot error:', err);
            this.statusEl.innerText = `FATAL ERROR: ${err.message || err}`;
        });
    }

    private async initPlayground() {
        console.log('[Playground] Initializing host...');
        // T340.30 — SharedArrayBuffer + WebGPU Feature Detection
        if (typeof SharedArrayBuffer === 'undefined') {
            const isMobile = /Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(navigator.userAgent);
            if (self.crossOriginIsolated === false) {
                this.statusEl.innerText = 'Security Isolation Required: Cross-Origin Isolation (COOP/COEP) is not enabled.';
            } else if (isMobile) {
                this.statusEl.innerText = 'Unsupported Platform: SharedArrayBuffer is not available on this mobile device.';
            } else {
                this.statusEl.innerText = 'Browser Requirement: This engine requires SharedArrayBuffer support.';
            }
            return;
        }

        if (!('gpu' in navigator)) {
            this.statusEl.innerText = 'WebGPU not supported in this browser. Check chrome://gpu or your browser flags.';
            return;
        }

        // T340.10 — Shared Memory Allocation
        this.memory = new WebAssembly.Memory({
            initial: 256,
            maximum: 4096,
            shared: true
        });

        // Instantiate workers with module type for Vite support
        this.gameWorker = new Worker(new URL('./game.worker.ts', import.meta.url), { type: 'module' });
        this.renderWorker = new Worker(new URL('./render.worker.ts', import.meta.url), { type: 'module' });

        // 3.1 Initialize Theme & World System
        this.world = new WorldRegistry();
        this.world.setRenderWorker(this.renderWorker);
        this.world.boot();

        // Add error listeners to workers
        this.gameWorker.onerror = (e) => this.handleFatalError('Game Worker Thread crashed', e);
        this.renderWorker.onerror = (e) => this.handleFatalError('Render Worker Thread crashed', e);

        console.log(`%cAetheris Engine — PLAYGROUND`, `color: #4ade80; font-weight: bold; font-size: 1.2em; border-bottom: 2px solid #4ade80;`);

        this.applyMode();
        this.world.syncThemeToWorker(); // Force initial color sync (THEME_WORLD_DESIGN §3.4.1)
        this.initThemeSelector();
        this.initCollapsibleSections();
        this.initVersionDisplay();
        await this.init();
    }

    private initVersionDisplay() {
        const el = document.getElementById('app-version');
        if (el) el.innerText = __APP_VERSION__;
    }

    private initThemeSelector() {
        const selector = document.getElementById('theme-selector') as HTMLSelectElement;
        if (!selector) return;

        // Populate themes
        const themes = this.world.availableThemes;
        selector.innerHTML = '';
        themes.forEach(t => {
            const option = document.createElement('option');
            option.value = t.slug;
            option.textContent = t.displayName;
            if (t.slug === this.world.activeTheme) {
                option.selected = true;
            }
            selector.appendChild(option);
        });

        // Handle manual change
        selector.addEventListener('change', () => {
            this.world.switchTheme(selector.value);
        });

        // Keep in sync with hotkeys
        window.addEventListener('aetheris:theme_changed', () => {
            selector.value = this.world.activeTheme;
        });
    }

    private handleFatalError(source: string, error: ErrorEvent | string) {
        console.error(`[Playground] ${source}:`, error);
        this.statusEl.innerText = `CRASH: ${source}`;
    }

    /** Updates static UI elements to reflect the active mode. */
    private updateBadgeStatus(status: 'live' | 'isolated' | 'offline' | 'reconnecting') {
        const infraBadge = document.getElementById('infra-badge');
        const engineBadge = document.getElementById('engine-badge');
        if (!infraBadge || !engineBadge) return;

        switch (status) {
            case 'live':
                infraBadge.innerText = 'INFRA: LIVE';
                infraBadge.style.background = 'color-mix(in srgb, var(--accent-success) 10%, transparent)';
                infraBadge.style.color = 'var(--accent-success)';
                infraBadge.style.borderColor = 'var(--accent-success)';

                engineBadge.innerText = 'ENGINE: SERVER CTRL';
                engineBadge.style.background = 'color-mix(in srgb, var(--accent-success) 10%, transparent)';
                engineBadge.style.color = 'var(--accent-success)';
                engineBadge.style.borderColor = 'var(--accent-success)';
                break;
            case 'isolated':
                infraBadge.innerText = 'INFRA: NONE';
                infraBadge.style.background = 'color-mix(in srgb, var(--text-muted) 10%, transparent)';
                infraBadge.style.color = 'var(--text-muted)';
                infraBadge.style.borderColor = 'var(--border-subtle)';

                engineBadge.innerText = 'ENGINE: LOCAL SIM';
                engineBadge.style.background = 'color-mix(in srgb, var(--accent-primary) 10%, transparent)';
                engineBadge.style.color = 'var(--accent-primary)';
                engineBadge.style.borderColor = 'var(--accent-primary)';
                break;
            case 'offline':
                infraBadge.innerText = 'INFRA: OFFLINE';
                infraBadge.style.background = 'color-mix(in srgb, var(--accent-danger) 10%, transparent)';
                infraBadge.style.color = 'var(--accent-danger)';
                infraBadge.style.borderColor = 'var(--accent-danger)';

                engineBadge.innerText = 'ENGINE: HALTED';
                engineBadge.style.background = 'color-mix(in srgb, var(--accent-danger) 10%, transparent)';
                engineBadge.style.color = 'var(--accent-danger)';
                engineBadge.style.borderColor = 'var(--accent-danger)';
                break;
            case 'reconnecting':
                infraBadge.innerText = 'INFRA: RECONNECTING';
                infraBadge.style.background = 'color-mix(in srgb, var(--accent-warning) 10%, transparent)';
                infraBadge.style.color = 'var(--accent-warning)';
                infraBadge.style.borderColor = 'var(--accent-warning)';

                engineBadge.innerText = 'ENGINE: BLOCKED';
                engineBadge.style.background = 'color-mix(in srgb, var(--accent-warning) 10%, transparent)';
                engineBadge.style.color = 'var(--accent-warning)';
                engineBadge.style.borderColor = 'var(--accent-warning)';
                break;
        }
    }

    private setViewState(state: number) {
        console.log(`[Playground] Setting ViewState to ${state}`);
        this.renderWorker.postMessage({ type: 'set_view_state', payload: { state } });
    }

    private applyMode() {
        document.title = "Aetheris Playground — Live Mode";
        this.updateBadgeStatus('live');
        
        const sectionSystem = document.getElementById('section-system');
        if (sectionSystem) sectionSystem.style.display = 'flex';
        
        const authSection = document.getElementById('section-auth');
        if (authSection) authSection.style.display = 'flex';

        // M10105 — Performance: pause workers when tab is hidden to prevent lag on resume
        document.addEventListener('visibilitychange', () => {
            const paused = document.visibilityState === 'hidden';
            console.log(`[Playground] Tab visibility changed: ${document.visibilityState} (paused=${paused})`);

            this.gameWorker.postMessage({ type: 'pause_toggle', payload: { paused } });
            this.renderWorker.postMessage({ type: 'pause_toggle', payload: { paused } });

            if (paused) {
                this.heldKeys.clear();
                this.updateInputUI();
                this.gameWorker.postMessage({ type: 'clear_keys' });
            }
        });

        // M10105 — Reliable teardown: ensure telemetry is flushed before tab closes.
        window.addEventListener('beforeunload', () => {
            this.gameWorker.postMessage({ type: 'finalize' });
        });
    }

    private async init() {
        console.log('[Playground] Initializing canvas and resize observers...');
        const canvas = document.getElementById('engine-canvas') as HTMLCanvasElement;
        if (!canvas) throw new Error('Missing #engine-canvas');

        // Set initial physical size BEFORE transferring control to offscreen
        const dpr = window.devicePixelRatio || 1;
        canvas.width = Math.floor(canvas.clientWidth * dpr);
        canvas.height = Math.floor(canvas.clientHeight * dpr);
        console.debug(`[Playground] Initial Canvas Size: ${canvas.width}x${canvas.height} (DPR=${dpr})`);

        // Handle resizing — recompute dpr each time to pick up display changes
        const resizeObserver = new ResizeObserver(entries => {
            for (const entry of entries) {
                const { width, height } = entry.contentRect;
                if (width > 0 && height > 0) {
                    const currentDpr = window.devicePixelRatio || 1;
                    const physicalWidth = Math.floor(width * currentDpr);
                    const physicalHeight = Math.floor(height * currentDpr);

                    console.debug(`[Playground] Resize detected: ${width}x${height} (CSS) -> ${physicalWidth}x${physicalHeight} (Physical, DPR=${currentDpr})`);

                    this.renderWorker.postMessage({
                        type: 'resize',
                        payload: { width: physicalWidth, height: physicalHeight }
                    });
                }
            }
        });
        resizeObserver.observe(canvas);

        // ── Shortcut Registry ──────────────────────────────────────────────
        // M1011 — Register all keyboard shortcuts centrally to avoid conflicts.
        // Namespace precedence: engine > debug > ui > game (see THEME_WORLD_DESIGN.md §7)

        // Help overlay
        const shortcutOverlay = document.getElementById('shortcut-overlay');
        if (shortcutOverlay) shortcuts.setOverlayElement(shortcutOverlay);

        // Shortcut help (engine namespace — cannot be overridden)
        shortcuts.register({
            key: '?',
            namespace: 'engine',
            label: 'Toggle shortcut help',
            category: 'Engine',
            handler: () => shortcuts.toggleOverlay(),
        });
        shortcuts.register({
            key: 'F1',
            namespace: 'engine',
            label: 'Toggle shortcut help',
            category: 'Engine',
            handler: () => shortcuts.toggleOverlay(),
            preventDefault: true,
        });
        shortcuts.register({
            key: 'Escape',
            namespace: 'engine',
            label: 'Close overlay / modal',
            category: 'Engine',
            handler: () => {
                if (shortcuts.isOverlayVisible) shortcuts.hideOverlay();
            },
        });

        // Sidebar shortcuts help link
        const helpBtn = document.getElementById('shortcuts-help-btn');
        if (helpBtn) {
            helpBtn.addEventListener('click', (e) => {
                e.preventDefault();
                shortcuts.toggleOverlay();
            });
        }

        // Debug shortcuts (debug namespace — overrides ui/game)
        shortcuts.register({
            key: 'F3',
            ctrl: true,
            namespace: 'debug',
            label: 'Toggle wireframe debug overlay',
            category: 'Debug',
            handler: () => {
                console.log('[Playground] Sending toggle_debug to RenderWorker');
                this.renderWorker.postMessage({ type: 'toggle_debug' });
            },
            preventDefault: true,
        });
        shortcuts.register({
            key: 'F3',
            ctrl: true,
            shift: true,
            namespace: 'debug',
            label: 'Toggle world grid overlay',
            category: 'Debug',
            handler: () => {
                console.log('[Playground] Sending toggle_grid to RenderWorker');
                this.renderWorker.postMessage({ type: 'toggle_grid' });
            },
            preventDefault: true,
        });

        // Theme management shortcuts
        shortcuts.register({
            key: 'T',
            ctrl: true,
            alt: true,
            namespace: 'ui',
            label: 'Switch to next theme',
            category: 'UI',
            handler: () => {
                const themes = ['blueprint', 'blueprint-lite', 'frost-dawn'];
                const current = this.world.activeTheme;
                const next = themes[(themes.indexOf(current) + 1) % themes.length];
                this.world.switchTheme(next);
            }
        });

        shortcuts.register({
            key: 'L',
            ctrl: true,
            namespace: 'ui',
            label: 'Toggle Blueprint Lite theme',
            category: 'UI',
            handler: () => {
                const current = this.world.activeTheme;
                this.world.switchTheme(current === 'blueprint-lite' ? 'blueprint' : 'blueprint-lite');
            }
        });

        shortcuts.register({
            key: 'G',
            namespace: 'game',
            label: 'Spawn Training Dummy',
            category: 'Game',
            handler: () => {
                if (!this.isSessionActive) {
                    console.warn('[Playground] Cannot spawn dummy: session not active');
                    return;
                }
                console.log('[Playground] Spawning training dummy...');
                this.gameWorker.postMessage({ type: 'p_spawn_dummy' });
            }
        });

        // Game-forwarding catchall: handled separately via a raw listener
        // so we can forward arbitrary keys without registering every possible key.
        window.addEventListener('keydown', (e) => {
            const target = e.target as HTMLElement;
            if (
                target.tagName === 'INPUT' ||
                target.tagName === 'TEXTAREA' ||
                target.tagName === 'SELECT' ||
                target.isContentEditable
            ) return;

            // Use the registry's own event handler. If it returns false, the shortcut
            // was not handled and we potentially forward it to the game.
            if (!shortcuts.handleEvent(e) && canvas.style.display !== 'none') {
                // Prevent scrolling for Arrow keys when focused on the engine
                if (e.key.startsWith('Arrow')) {
                    e.preventDefault();
                }

                if (!this.heldKeys.has(e.code)) {
                    console.debug(`[Playground] KeyDown event: ${e.code}`);
                    this.heldKeys.add(e.code);
                    this.updateInputUI();
                    this.gameWorker.postMessage({
                        type: 'key_down',
                        payload: { key: e.code, shift: e.shiftKey, ctrl: e.ctrlKey, alt: e.altKey }
                    });
                }
            }
        });

        window.addEventListener('keyup', (e) => {
            if (this.heldKeys.has(e.code)) {
                this.heldKeys.delete(e.code);
                this.updateInputUI();
                this.gameWorker.postMessage({
                    type: 'key_up',
                    payload: { key: e.code }
                });
            }
        });

        // Attach the ShortcutRegistry global listener (handled by our catchall now)
        // shortcuts.attach(window);

        // Feature-detect transferControlToOffscreen before calling it.
        if (!('transferControlToOffscreen' in canvas)) {
            this.statusEl.innerText = 'Unsupported browser: OffscreenCanvas / transferControlToOffscreen is not available.';
            return;
        }
        const offscreen = canvas.transferControlToOffscreen();

        // Expose to window for global helper functions in playground.html
        (window as any).playground = this;

        this.initConnected(offscreen);
    }

    /**
     * Connected mode: authenticates automatically using the server's auth-bypass
     * credentials (AETHERIS_AUTH_BYPASS=1) and establishes a live WebTransport
     * session. The server controls world state — renders real entities and feeds
     * production metrics to Grafana/Prometheus.
     */
    private initConnected(offscreen: OffscreenCanvas) {
        this.statusEl.innerText = 'Initializing WASM...';
        this.gameWorker.postMessage({ type: 'init', memory: this.memory });

        this.gameWorker.onmessage = (e) => {
            const { type, payload } = e.data;

            switch (type) {
                case 'ready': {
                    const { sharedWorldPtr } = e.data;
                    this.renderWorker.postMessage({
                        type: 'init',
                        memory: this.memory,
                        sharedWorldPtr,
                        canvas: offscreen
                    }, [offscreen]);

                    // M10105 — route metrics_frame from render worker to game worker
                    this.renderWorker.onmessage = (re) => {
                        if (re.data?.type === 'metrics_frame') {
                            this.gameWorker.postMessage(re.data);
                        }
                    };

                    // Replay active theme: render worker was not ready during boot-time sync.
                    this.world.syncThemeToWorker();

                    const sabStat = document.getElementById('stat-sab');
                    if (sabStat) sabStat.innerText = `0x${sharedWorldPtr.toString(16).toUpperCase()}`;

                    const bufferStat = document.getElementById('stat-session-buffer');
                    if (bufferStat) {
                        bufferStat.innerText = `[${(this.memory.buffer.byteLength / 1024 / 1024).toFixed(1)}MB] `;
                        bufferStat.style.display = 'inline';
                    }

                    const authSection = document.getElementById('section-auth');
                    if (authSection) {
                        authSection.style.display = 'flex';
                        // Prefill email for convenience if configured, but don't auto-request
                        const emailInput = document.getElementById('auth-email') as HTMLInputElement;
                        if (emailInput && (import.meta.env.DEV || import.meta.env.VITE_ENABLE_SMOKE_PREFILL === 'true')) {
                            emailInput.value = 'smoke-test@aetheris.dev';
                        }
                    }

                    this.statusEl.innerText = 'WASM READY — Authentication Required';

                    const logoutBtn = document.getElementById('btn-logout');
                    if (logoutBtn) logoutBtn.style.display = 'none'; // Hide until logged in
                    break;
                }

                case 'otp_requested': {
                    this.statusEl.innerText = 'OTP ISSUED — Please enter the 6-digit code';
                    this.currentRequestId = payload.requestId;

                    const sessionStat = document.getElementById('stat-session');
                    if (sessionStat) sessionStat.innerText = 'AWAITING LOGIN';

                    const otpEntry = document.getElementById('otp-entry');
                    if (otpEntry) {
                        otpEntry.style.display = 'flex';
                        const otpInput = document.getElementById('auth-otp') as HTMLInputElement;
                        if (otpInput) {
                            otpInput.focus();
                            // Prefill for smoke test convenience if configured, but don't auto-login
                            if (import.meta.env.DEV || import.meta.env.VITE_ENABLE_SMOKE_PREFILL === 'true') {
                                otpInput.value = '000001';
                            }
                        }
                    }
                    break;
                }

                case 'login_success': {
                    this.statusEl.innerText = 'AUTHENTICATED — Connecting transport...';

                    // M10156 — Cinematic transition
                    const logoOverlay = document.getElementById('engine-logo-overlay');
                    if (logoOverlay) logoOverlay.classList.add('hidden');
                    this.setViewState(ViewState.Roaming);

                    const sessionStatLogin = document.getElementById('stat-session');
                    if (sessionStatLogin) {
                        sessionStatLogin.innerText = 'LOGGED IN';
                        sessionStatLogin.style.color = 'var(--accent-success)';
                    }

                    const authInputs = document.getElementById('auth-inputs');
                    if (authInputs) authInputs.style.display = 'none';

                    const logoutBtnLogin = document.getElementById('btn-logout');
                    if (logoutBtnLogin) logoutBtnLogin.style.display = 'block';

                    const sectionSimulationLogin = document.getElementById('section-simulation');
                    if (sectionSimulationLogin) sectionSimulationLogin.style.display = 'flex';

                    const sectionEntitiesLogin = document.getElementById('section-entities');
                    if (sectionEntitiesLogin) sectionEntitiesLogin.style.display = 'flex';

                    const sectionSystemLogin = document.getElementById('section-system');
                    if (sectionSystemLogin) sectionSystemLogin.style.display = 'flex';

                    this.gameWorker.postMessage({
                        type: 'connect',
                        payload: {
                            url: import.meta.env.VITE_SERVER_URL || 'https://127.0.0.1:4433',
                            token: payload.sessionToken,
                            certHash: import.meta.env.VITE_SERVER_CERT_HASH
                        }
                    });
                    break;
                }

                case 'connection_ready': {
                    this.statusEl.classList.remove('error');
                    this.statusEl.style.display = 'none'; // Hide the generic status line as requested
                    this.updateBadgeStatus('live');

                    const authSection = document.getElementById('section-auth');
                    if (authSection) authSection.style.display = 'flex';

                    const startBtn = document.getElementById('btn-start');
                    if (startBtn) startBtn.style.display = 'block';

                    const clientIdRow = document.getElementById('stat-client-id-row');
                    const clientIdVal = document.getElementById('stat-client-id');
                    if (clientIdRow && clientIdVal) {
                        clientIdVal.innerText = payload.clientId;
                        clientIdRow.style.display = 'flex';
                    }

                    this.startMonitoring();
                    console.log(`[Playground] Connected mode live. clientId=${payload.clientId}`);
                    break;
                }

                case 'connection_error': {
                    const rawMsg = (payload.reason instanceof Error)
                        ? payload.reason.message
                        : String(payload.reason ?? 'Unknown error');

                    let displayMsg = rawMsg;
                    if (rawMsg.includes('Invalid') || rawMsg.includes('Unauthenticated') || rawMsg.includes('Bad') || rawMsg.includes('OTP')) {
                        displayMsg = `Auth Failed: ${rawMsg} (Check AETHERIS_AUTH_BYPASS)`;
                    } else if (rawMsg.includes('Rate limit')) {
                        displayMsg = `Rate Limited: ${rawMsg}. Please wait a moment.`;
                    }

                    this.statusEl.style.display = 'block'; // Ensure errors are always visible
                    this.statusEl.classList.add('error');
                    this.statusEl.innerText = `ERROR: ${displayMsg}`;
                    this.updateBadgeStatus('offline');

                    const clientIdRowErr = document.getElementById('stat-client-id-row');
                    if (clientIdRowErr) clientIdRowErr.style.display = 'none';

                    console.error('[Playground] Connection error:', rawMsg);
                    break;
                }

                case 'reconnecting': {
                    this.statusEl.style.display = 'block'; // Make sure the status is visible
                    this.statusEl.classList.remove('error');
                    this.statusEl.innerText = 'RECONNECTING...';
                    this.updateBadgeStatus('reconnecting');

                    const clientIdRowRec = document.getElementById('stat-client-id-row');
                    if (clientIdRowRec) clientIdRowRec.style.display = 'none';
                    break;
                }

                case 'logout_success':
                    window.location.reload();
                    break;

                case 'wasm_metrics':
                    this.updateWasmMetrics(payload);
                    if (e.data.entities) {
                        this.updateEntityStatuses(e.data.entities);
                    }
                    if (e.data.manifest) {
                        const manifestObj = (e.data.manifest instanceof Map)
                            ? Object.fromEntries(e.data.manifest)
                            : e.data.manifest;

                        // Deterministic stringify by sorting keys
                        const sortedKeys = Object.keys(manifestObj).sort();
                        const manifestStr = JSON.stringify(sortedKeys.map(k => [k, manifestObj[k]]));

                        if (manifestStr !== this.lastLoggedManifest) {
                            const isFirstLog = this.lastLoggedManifest === '';
                            this.lastLoggedManifest = manifestStr;

                            if (isFirstLog) {
                                console.log('[Playground] System Manifest initial state:', e.data.manifest);
                            } else {
                                console.log('[Playground] System Manifest changed:', e.data.manifest);
                            }
                        } else {
                            console.debug('[Playground] System Manifest (idempotent)');
                        }

                        this.updateSystemManifest(e.data.manifest);
                    }
                    break;
            }
        };
    }



    /** Starts the session by requesting a ship from the server. */
    start(event?: MouseEvent) {
        if (event) (event.target as HTMLElement).blur();

        // 1. Clear world first
        this.clear();

        // 2. Now mark session as active and update the UI.
        this.isSessionActive = true;
        this.updateSessionUI();

        // 3. M10156 — Cinematic Entry
        // Transition to 'Entering' state (local ship arrival animation)
        this.setViewState(ViewState.Entering);
        this.statusEl.innerText = 'CINEMATIC SEQUENCE — Interceptor Arriving...';

        // Delay server-side spawning until animation is almost complete (approx 2.5s)
        setTimeout(() => {
            if (!this.isSessionActive) return; // Cancelled

            // 4. Request session ship with Possession from the server.
            this.gameWorker.postMessage({ type: 'start_session' });
            this.setViewState(ViewState.Playing);

            this.entityCount = 1;
            this.updateEntityCount();
            this.statusEl.innerText = 'SESSION STARTED — Awaiting Ship Possession';
        }, 2400);
    }

    /** Clears all entities from the playground. */
    clear(event?: MouseEvent) {
        if (event) (event.target as HTMLElement).blur();
        this.isSessionActive = false;
        this.updateSessionUI();
        
        // M10156 — Return to roaming
        this.setViewState(ViewState.Roaming);

        this.entityCount = 0;
        this.gameWorker.postMessage({ type: 'p_clear' });
        this.updateEntityCount();
    }

    /** Spawns a training dummy in front of the player. */
    spawnDummy() {
        if (!this.isSessionActive) {
            console.warn('[Playground] Cannot spawn dummy: session not active');
            return;
        }
        console.log('[Playground] Spawning training dummy via button...');
        this.gameWorker.postMessage({ type: 'p_spawn_dummy' });
    }

    private updateSessionUI() {
        const btn = document.getElementById('btn-start');
        const spawnBtn = document.getElementById('btn-spawn-dummy');
        if (!btn) return;

        if (this.isSessionActive) {
            btn.innerText = 'Stop Session (Clear World)';
            btn.classList.add('danger');
            btn.classList.remove('primary');
            btn.style.display = 'block';
            if (spawnBtn) spawnBtn.style.display = 'block';
        } else {
            btn.innerText = 'Start Session (Spawn Ship)';
            btn.classList.add('primary');
            btn.classList.remove('danger');
            btn.style.display = 'block';
            if (spawnBtn) spawnBtn.style.display = 'none';
        }
    }


    private initCollapsibleSections() {
        const sections = document.querySelectorAll('.section');
        sections.forEach(section => {
            const title = section.querySelector('.section-title');
            if (title) {
                const toggle = () => {
                    section.classList.toggle('collapsed');
                    title.setAttribute('aria-expanded',
                        section.classList.contains('collapsed') ? 'false' : 'true');
                };
                // Mouse
                title.addEventListener('click', toggle);
                // Keyboard
                title.setAttribute('tabindex', '0');
                title.setAttribute('role', 'button');
                title.setAttribute('aria-expanded', 'true');
                title.addEventListener('keydown', (e: Event) => {
                    const ke = e as KeyboardEvent;
                    if (ke.key === 'Enter' || ke.key === ' ') {
                        ke.preventDefault();
                        toggle();
                    }
                });
            }
        });
    }

    private updateSystemManifest(manifest: any) {
        const container = document.getElementById('system-info');
        if (!container) return;

        // WASM BTreeMap may arrive as a JS Map or a plain Record
        let data = (manifest instanceof Map) ? Object.fromEntries(manifest) : { ...manifest };

        // Only re-render if data actually changed to avoid DOM thrashing
        const currentHash = JSON.stringify(data);
        if ((container as any)._lastHash === currentHash) return;
        (container as any)._lastHash = currentHash;

        const entries = Object.entries(data);
        if (entries.length === 0) {
            container.style.display = 'none';
            return;
        }

        container.style.display = 'flex';

        // Build rows via DOM API so server-supplied keys/values are never parsed as HTML.
        container.innerHTML = '';
        for (const [key, val] of entries) {
            const row = document.createElement('div');
            row.className = 'stat';

            const labelEl = document.createElement('span');
            labelEl.className = 'label';
            labelEl.style.textTransform = 'capitalize';
            labelEl.textContent = key.replace(/_/g, ' ');

            const valueEl = document.createElement('span');
            valueEl.className = 'value';
            valueEl.style.fontSize = '0.65rem';
            valueEl.style.fontFamily = 'var(--world-font-mono)';
            valueEl.textContent = String(val);

            row.appendChild(labelEl);
            row.appendChild(valueEl);
            container.appendChild(row);
        }
    }

    private updateEntityCount() {
        const el = document.getElementById('stat-entities');
        if (el) el.innerText = this.entityCount.toString();
    }

    private startMonitoring() {
        // Cancel any existing monitor loop to prevent duplicates on reconnect.
        if (this._monitorRafId !== null) {
            cancelAnimationFrame(this._monitorRafId);
            this._monitorRafId = null;
        }

        let lastTime = performance.now();
        let frames = 0;

        const loop = () => {
            frames++;
            const now = performance.now();
            if (now - lastTime >= 1000) {
                const fps = (frames * 1000) / (now - lastTime);
                const fpsEl = document.getElementById('stat-fps');
                if (fpsEl) {
                    fpsEl.innerText = fps.toFixed(1);
                    // Color code based on stability
                    fpsEl.style.color = fps >= 55 ? 'var(--accent-primary)' : (fps >= 30 ? 'var(--accent-warning)' : 'var(--accent-danger)');
                }
                frames = 0;
                lastTime = now;
            }
            this._monitorRafId = requestAnimationFrame(loop);
        };
        this._monitorRafId = requestAnimationFrame(loop);
    }

    private updateWasmMetrics(metrics: any) {
        const fpsEl = document.getElementById('stat-wasm-fps');
        if (fpsEl) {
            fpsEl.innerText = metrics.fps.toFixed(1);
            fpsEl.style.color = metrics.fps >= 55 ? 'var(--accent-primary)' : (metrics.fps >= 30 ? 'var(--accent-warning)' : 'var(--accent-danger)');
        }

        const frameTimeEl = document.getElementById('stat-frame-time');
        if (frameTimeEl) frameTimeEl.innerText = `${metrics.frame_time_p99.toFixed(2)}ms`;

        const simTimeEl = document.getElementById('stat-sim-time');
        if (simTimeEl) simTimeEl.innerText = `${metrics.sim_time_p99.toFixed(2)}ms`;

        const rttEl = document.getElementById('stat-rtt');
        if (rttEl) {
            if (metrics.rtt_ms !== null && metrics.rtt_ms !== undefined) {
                rttEl.innerText = `${metrics.rtt_ms.toFixed(1)}ms`;
                rttEl.style.color = metrics.rtt_ms < 100 ? 'var(--accent-success)' : (metrics.rtt_ms < 250 ? 'var(--accent-warning)' : 'var(--accent-danger)');
            } else {
                rttEl.innerText = 'N/A';
                rttEl.style.color = 'var(--text-muted)'; // Muted color for zero/no-data
            }
        }

        const entitiesEl = document.getElementById('stat-entities');
        if (entitiesEl) {
            entitiesEl.innerText = metrics.entity_count.toString();
            this.entityCount = metrics.entity_count; // Keep local count in sync
        }

        const droppedEl = document.getElementById('stat-dropped');
        if (droppedEl) {
            droppedEl.innerText = metrics.dropped_events.toString();
            droppedEl.style.color = metrics.dropped_events > 0 ? 'var(--accent-danger)' : '';
        }

        const cargoEl = document.getElementById('stat-cargo');
        if (cargoEl) {
            cargoEl.innerText = `${metrics.cargo_ore} / ${metrics.cargo_capacity}`;
            if (metrics.cargo_capacity > 0 && metrics.cargo_ore >= metrics.cargo_capacity) {
                cargoEl.style.color = 'var(--accent-danger)';
            } else {
                cargoEl.style.color = '';
            }
        }

        const sabStat = document.getElementById('stat-sab');
        if (sabStat) {
            sabStat.innerText = `ACTIVE (${metrics.snapshot_count} Snaps)`;
        }
    }

    private updateEntityStatuses(entities: any[]) {
        const container = document.getElementById('entity-list');
        if (!container) return;

        if (entities.length === 0) {
            container.innerHTML = '';
            const empty = document.createElement('div');
            empty.style.fontSize = '0.625rem';
            empty.style.color = 'var(--text-muted)';
            empty.style.textAlign = 'center';
            empty.style.padding = '12px';
            empty.textContent = 'No active entities detected';
            container.appendChild(empty);
            return;
        }

        // Sort: Player first, then by Network ID
        entities.sort((a, b) => {
            if (a.is_player && !b.is_player) return -1;
            if (!a.is_player && b.is_player) return 1;
            return parseInt(a.network_id) - parseInt(b.network_id);
        });

        // Limit to top 20 entities to avoid UI lag
        const displayEntities = entities.slice(0, 20);

        // Map entity types to labels
        const getEntityLabel = (type: number) => {
            switch (type) {
                case 1:
                case 2: return 'Interceptor';
                case 3: return 'Dreadnought';
                case 4: return 'Hauler';
                case 5: return 'Asteroid';
                case 6: return 'Cargo Drop';
                case 10: return 'Training Dummy';
                case 20: return 'Projectile';
                default: return `Type ${type}`;
            }
        };

        // Build list via DOM API to avoid XSS vulnerabilities (mirroring updateSystemManifest)
        container.innerHTML = '';
        for (const entity of displayEntities) {
            const label = getEntityLabel(entity.entity_type);
            const hpPercent = Math.min(100, Math.max(0, (entity.hp / 100) * 100));
            const shieldPercent = Math.min(100, Math.max(0, (entity.shield / 100) * 100));

            const item = document.createElement('div');
            item.className = `entity-item ${entity.is_player ? 'is-player' : ''}`;

            const header = document.createElement('div');
            header.className = 'entity-item-header';

            const typeEl = document.createElement('span');
            typeEl.className = 'entity-type';
            typeEl.textContent = `${label} ${entity.is_player ? '(YOU)' : ''}`;

            const idElement = document.createElement('span');
            idElement.className = 'entity-id';
            idElement.textContent = `#${entity.network_id}`;

            header.appendChild(typeEl);
            header.appendChild(idElement);
            item.appendChild(header);

            const vitals = document.createElement('div');
            vitals.className = 'vitals-container';

            // HP Row
            const hpRow = document.createElement('div');
            hpRow.className = 'vital-row';
            const hpLabel = document.createElement('span');
            hpLabel.className = 'vital-label';
            hpLabel.textContent = 'Hull';
            const hpProgressBg = document.createElement('div');
            hpProgressBg.className = 'progress-bg';
            const hpProgressFill = document.createElement('div');
            hpProgressFill.className = 'progress-fill hp-fill';
            hpProgressFill.style.width = `${hpPercent}%`;
            const hpVal = document.createElement('span');
            hpVal.className = 'vital-value';
            hpVal.textContent = String(entity.hp);

            hpProgressBg.appendChild(hpProgressFill);
            hpRow.appendChild(hpLabel);
            hpRow.appendChild(hpProgressBg);
            hpRow.appendChild(hpVal);
            vitals.appendChild(hpRow);

            // Shield Row
            const shieldRow = document.createElement('div');
            shieldRow.className = 'vital-row';
            const shieldLabel = document.createElement('span');
            shieldLabel.className = 'vital-label';
            shieldLabel.textContent = 'Shield';
            const shieldProgressBg = document.createElement('div');
            shieldProgressBg.className = 'progress-bg';
            const shieldProgressFill = document.createElement('div');
            shieldProgressFill.className = 'progress-fill shield-fill';
            shieldProgressFill.style.width = `${shieldPercent}%`;
            const shieldVal = document.createElement('span');
            shieldVal.className = 'vital-value';
            shieldVal.textContent = String(entity.shield);

            shieldProgressBg.appendChild(shieldProgressFill);
            shieldRow.appendChild(shieldLabel);
            shieldRow.appendChild(shieldProgressBg);
            shieldRow.appendChild(shieldVal);
            vitals.appendChild(shieldRow);

            item.appendChild(vitals);
            container.appendChild(item);
        }

        if (entities.length > 20) {
            const more = document.createElement('div');
            more.style.fontSize = '0.5rem';
            more.style.color = 'var(--text-muted)';
            more.style.textAlign = 'center';
            more.style.padding = '4px';
            more.textContent = `+ ${entities.length - 20} more entities hidden`;
            container.appendChild(more);
        }
    }

    requestOtp(email: string) {
        if (!CONNECTED_MODE) return;
        this.gameWorker.postMessage({
            type: 'request_otp',
            payload: { email }
        });
    }

    login(code: string) {
        if (!CONNECTED_MODE || !this.currentRequestId) return;
        this.gameWorker.postMessage({
            type: 'login_otp',
            payload: { requestId: this.currentRequestId, code }
        });
    }

    logout() {
        if (!CONNECTED_MODE) return;
        this.gameWorker.postMessage({ type: 'logout', payload: {} });
    }

    refreshManifest() {
        if (!CONNECTED_MODE) return;
        console.log('[Playground] Manual manifest refresh requested');
        this.gameWorker.postMessage({ type: 'p_request_metrics' });
    }

    private updateInputUI() {
        const badges = document.querySelectorAll('.key-badge');
        badges.forEach(badge => {
            const key = badge.getAttribute('data-key');
            if (key) {
                // Special mapping: 'Space' data-key needs to match KeyboardEvent.code 'Space'
                // WASD mapping: 'KeyW', 'KeyA', etc. match e.code.
                // Arrow mapping: we mirror them to WASD indicators if that's the intent, 
                // OR we just track exactly what the data-key says.
                // The badges in HTML are: KeyW, KeyA, KeyS, KeyD, KeyQ, KeyE, Space, KeyF.

                let active = this.heldKeys.has(key);

                // Mirror Arrows to WASD badges for visual feedback
                if (key === 'KeyW' && this.heldKeys.has('ArrowUp')) active = true;
                if (key === 'KeyS' && this.heldKeys.has('ArrowDown')) active = true;
                if (key === 'KeyA' && this.heldKeys.has('ArrowLeft')) active = true;
                if (key === 'KeyD' && this.heldKeys.has('ArrowRight')) active = true;

                if (active) {
                    badge.classList.add('active');
                } else {
                    badge.classList.remove('active');
                }
            }
        });
    }
}

// Bootstrap the playground controller
new AetherisPlayground();
