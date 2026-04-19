// main.ts - Aetheris Engine Host
import { WorldRegistry } from './world-registry';

async function initEngine() {
    const canvas = document.getElementById('engine-canvas') as HTMLCanvasElement | null;
    const statusEl = document.getElementById('status') as HTMLDivElement | null;

    if (!canvas || !statusEl) {
        console.error('Aetheris: Missing required UI elements (#engine-canvas or #status)');
        return;
    }

    // T340.30 — Implement isSABAvailable() check + graceful UI fallback
    if (typeof SharedArrayBuffer === 'undefined') {
        const isMobile = /Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(navigator.userAgent);
        if (self.crossOriginIsolated === false) {
            statusEl.innerText = 'Security Isolation Required: Cross-Origin Isolation (COOP/COEP) is not enabled.';
        } else if (isMobile) {
            statusEl.innerText = 'Unsupported Platform: SharedArrayBuffer is not available on this mobile device.';
        } else {
            statusEl.innerText = 'Browser Requirement: This engine requires SharedArrayBuffer support.';
        }
        return;
    }

    if (!('gpu' in navigator)) {
        statusEl.innerText = 'WebGPU not supported in this browser.';
        return;
    }

    try {
        // 1. Initialize a Shared WebAssembly Memory
        const memory = new WebAssembly.Memory({
            initial: 256,
            maximum: 4096,
            shared: true
        });

        // 2. Transfer canvas control to the render thread
        const offscreen = canvas.transferControlToOffscreen();

        // 3. Spawn Workers
        const gameWorker = new Worker(new URL('./game.worker.ts', import.meta.url), { type: 'module' });
        const renderWorker = new Worker(new URL('./render.worker.ts', import.meta.url), { type: 'module' });

        // 3.1 Initialize Theme & World System
        const world = new WorldRegistry();
        world.setRenderWorker(renderWorker);
        world.boot();
        world.syncThemeToWorker();

        const authView = document.getElementById('auth-view') as HTMLDivElement | null;

        const authError = document.getElementById('auth-error') as HTMLDivElement | null;
        const authStatus = document.getElementById('auth-status') as HTMLDivElement | null;
        const emailStep = document.getElementById('email-step') as HTMLDivElement | null;
        const codeStep = document.getElementById('code-step') as HTMLDivElement | null;
        const reconnectOverlay = document.getElementById('reconnect-overlay') as HTMLDivElement | null;

        const emailInput = document.getElementById('auth-email') as HTMLInputElement | null;
        const codeInput = document.getElementById('auth-code') as HTMLInputElement | null;

        const requestOtpBtn = document.getElementById('btn-request-otp') as HTMLButtonElement | null;
        const loginOtpBtn = document.getElementById('btn-login-otp') as HTMLButtonElement | null;
        const googleLoginBtn = document.getElementById('btn-google-login') as HTMLButtonElement | null;
        const backOtpBtn = document.getElementById('btn-back-otp') as HTMLButtonElement | null;
        const logoutBtn = document.getElementById('btn-logout') as HTMLButtonElement | null;

        if (!authView || !authError || !authStatus || !emailStep || !codeStep || !reconnectOverlay ||
            !emailInput || !codeInput || !requestOtpBtn || !loginOtpBtn || !googleLoginBtn || !backOtpBtn || !logoutBtn) {
            throw new Error('Aetheris: Missing required Authentication UI elements');
        }

        // T370.10 — Pre-fill credentials for smoke-test convenience
        if (import.meta.env.DEV || import.meta.env.VITE_ENABLE_SMOKE_PREFILL === 'true') {
            emailInput.value = 'smoke-test@aetheris.dev';
            codeInput.value = '000001';
        }

        let currentRequestId: string | null = null;

        // 4. Input handling helpers
        function showError(msg: string) {
            authStatus!.innerText = '';
            authError!.innerText = msg;
            authError!.style.display = 'block';
        }

        function showStatus(msg: string) {
            authError!.style.display = 'none';
            authStatus!.innerText = msg;
        }

        function hideError() {
            authError!.style.display = 'none';
        }

        // 5. Worker Handshake & State Management
        gameWorker.postMessage({ type: 'init', memory });

        gameWorker.onmessage = async (e) => {
            const { type, payload } = e.data;

            switch (type) {
                case 'ready': {
                    const { sharedWorldPtr } = e.data;
                    console.log(`Aetheris: Game Worker Ready (SharedWorld @ 0x${sharedWorldPtr.toString(16)})`);
                    renderWorker.postMessage({
                        type: 'init',
                        memory,
                        sharedWorldPtr,
                        canvas: offscreen
                    }, [offscreen]);
                    statusEl.innerText = 'Protocol Initialized';
                    break;
                }

                case 'connection_ready':
                    authView.style.display = 'none';
                    canvas.style.display = 'block';
                    reconnectOverlay.style.display = 'none';
                    logoutBtn.style.display = 'block';
                    statusEl.innerText = `Connected: ${payload.clientId}`;
                    break;

                case 'connection_error': {
                    const errMsg = (payload.reason instanceof Error)
                        ? payload.reason.message
                        : String(payload.reason ?? 'Unknown error');
                    showError(errMsg);
                    if (!payload.retriable) {
                        authView.style.display = 'flex';
                        canvas.style.display = 'none';
                    }
                    reconnectOverlay.style.display = 'none';
                    break;
                }

                case 'reconnecting':
                    reconnectOverlay.style.display = 'flex';
                    break;

                case 'otp_requested':
                    currentRequestId = payload.requestId;
                    emailStep.style.display = 'none';
                    codeStep.style.display = 'flex';
                    hideError();
                    break;

                case 'login_success':
                    statusEl.innerText = 'Identity Verified. Connecting to Data Plane...';
                    // Trigger transport connection via Game Worker
                    gameWorker.postMessage({
                        type: 'connect',
                        payload: {
                            url: import.meta.env.VITE_SERVER_URL || 'https://127.0.0.1:4433',
                            token: payload.sessionToken,
                            certHash: import.meta.env.VITE_SERVER_CERT_HASH
                        }
                    });
                    break;
                case 'logout_success':
                    window.location.reload();
                    break;
            }
        };

        // 6. Auth UI Listeners
        requestOtpBtn.onclick = () => {
            const email = emailInput.value.trim();
            if (!email) return showError('Email is required');
            hideError();
            gameWorker.postMessage({ type: 'request_otp', payload: { email } });
        };

        loginOtpBtn.onclick = () => {
            const code = codeInput.value.trim();
            if (code.length !== 6) return showError('Enter 6-digit code');
            if (!currentRequestId) return showError('Session expired');
            hideError();
            gameWorker.postMessage({
                type: 'login_otp',
                payload: { requestId: currentRequestId, code }
            });
        };

        backOtpBtn.onclick = () => {
            emailStep.style.display = 'flex';
            codeStep.style.display = 'none';
            currentRequestId = null;
        };

        googleLoginBtn.onclick = () => {
            showStatus('Google Login: PKCE flow starting...');
            // In a real implementation, we would drive the PKCE flow here
            // For now, let's assume we capture the ID token and send to Game Worker
            // This is a placeholder for the actual OAuth2 redirect/popup logic
            console.warn('Google OIDC PKCE flow not fully implemented in this stub');
        };

        // Forward raw input to game worker
        window.onkeydown = (e) => {
            if (canvas.style.display === 'block') {
                gameWorker.postMessage({ type: 'key_down', payload: { key: e.key } });
            }
        };

        window.onmousemove = (e) => {
            if (canvas.style.display === 'block') {
                gameWorker.postMessage({ type: 'mouse_move', payload: { dx: e.movementX, dy: e.movementY } });
            }
        };


        logoutBtn.onclick = () => {
            gameWorker.postMessage({ type: 'logout', payload: {} });
        };



        console.log("%cAetheris Engine — CONNECTED APP", "color: #6366f1; font-weight: bold; font-size: 1.2em;");
        console.log("Loading Live Client (index.html). If you want the Isolated Sandbox, use /playground.html");
        console.log(`Aetheris: Host Handshake Started`);
    } catch (err) {
        statusEl.innerText = `Initialization Error: ${err}`;
        console.error(err);
    }
}

initEngine();
