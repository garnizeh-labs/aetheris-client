/**
 * shortcuts.ts — Aetheris Keyboard Shortcut Registry
 *
 * Provides a centralized registry for all keyboard shortcuts, organized by
 * namespace (engine > debug > ui > game) with conflict detection and a
 * help overlay renderer.
 *
 * Namespace precedence (highest → lowest):
 *   engine > debug > ui > game
 *
 * Suppression rules:
 *   - 'game' shortcuts are suppressed when focus is on INPUT/TEXTAREA/SELECT
 *   - 'ui' shortcuts are suppressed when focus is on the engine canvas
 *   - 'engine' and 'debug' shortcuts are never suppressed
 */

export type ShortcutNamespace = 'engine' | 'debug' | 'ui' | 'game';

export interface ShortcutDescriptor {
    /**
     * Character-level key, matched against KeyboardEvent.key.
     * Use for UI shortcuts and named keys: 'Escape', 'F3', '?', 'Enter'.
     * Required if `code` is not provided.
     */
    key?: string;
    /**
     * Physical key position, matched against KeyboardEvent.code.
     * Use for game controls: 'KeyW', 'KeyA', 'KeyS', 'KeyD', 'Space'.
     * Takes precedence over `key` when both are provided.
     */
    code?: string;
    ctrl?: boolean;
    shift?: boolean;
    alt?: boolean;
    meta?: boolean;
    namespace: ShortcutNamespace;
    /** Human-readable label shown in the help overlay */
    label: string;
    /** Category for grouping in the help overlay */
    category?: string;
    handler: () => void;
    /** If true, calls e.preventDefault() on the triggering event */
    preventDefault?: boolean;
}

type ShortcutId = string;

const NAMESPACE_PRECEDENCE: Record<ShortcutNamespace, number> = {
    engine: 4,
    debug: 3,
    ui: 2,
    game: 1,
};

function shortcutId(d: Pick<ShortcutDescriptor, 'key' | 'code' | 'ctrl' | 'shift' | 'alt' | 'meta'>): ShortcutId {
    const trigger = d.code ?? d.key;
    if (!trigger) {
        throw new Error('[ShortcutRegistry] Descriptor must have at least one of "key" or "code"');
    }
    // Omit the shift modifier when the trigger is already a single printable character
    // (e.g. '?' already encodes Shift+/). Including shift would produce 'shift+?' which
    // never matches the registered id '?' because the char itself carries the shift.
    const isPrintableChar = trigger.length === 1;
    const mods = [
        d.ctrl && 'ctrl',
        (!isPrintableChar && d.shift) && 'shift',
        d.alt && 'alt',
        d.meta && 'meta',
    ]
        .filter(Boolean)
        .join('+');
    return mods ? `${mods}+${trigger.toLowerCase()}` : trigger.toLowerCase();
}

export class ShortcutRegistry {
    private readonly shortcuts = new Map<ShortcutId, ShortcutDescriptor>();
    private overlayVisible = false;
    private overlayEl: HTMLElement | null = null;

    /**
     * Registers a keyboard shortcut.
     * Throws in development if a higher-or-equal-priority shortcut is already
     * registered for the same combination.
     */
    register(descriptor: ShortcutDescriptor): void {
        if (!descriptor.key && !descriptor.code) {
            if (import.meta.env.DEV) {
                console.error('[ShortcutRegistry] Registration failed: "key" or "code" is required.', descriptor);
            }
            return;
        }

        const id = shortcutId(descriptor);
        const existing = this.shortcuts.get(id);

        if (existing) {
            const existingPrio = NAMESPACE_PRECEDENCE[existing.namespace];
            const newPrio = NAMESPACE_PRECEDENCE[descriptor.namespace];

            if (existingPrio >= newPrio) {
                // Higher-priority shortcut already owns this combination.
                if (import.meta.env.DEV) {
                    console.warn(
                        `[ShortcutRegistry] Conflict: "${id}" is already registered by namespace ` +
                        `"${existing.namespace}" (label: "${existing.label}"). ` +
                        `New registration by "${descriptor.namespace}" (label: "${descriptor.label}") was rejected.`
                    );
                }
                return;
            }
        }

        this.shortcuts.set(id, descriptor);
    }

    /** Removes a previously registered shortcut. */
    unregister(keyOrCode: string, mods?: Pick<ShortcutDescriptor, 'ctrl' | 'shift' | 'alt' | 'meta'>): void {
        // Since we don't know if it's a key or code, we try both
        const keyId = shortcutId({ key: keyOrCode, ...mods });
        const codeId = shortcutId({ code: keyOrCode, ...mods });
        this.shortcuts.delete(keyId);
        this.shortcuts.delete(codeId);
    }

    /**
     * Attaches the global keydown listener to the given target (default: window).
     * Returns an unsubscribe function.
     */
    attach(target: EventTarget = window): () => void {
        const listener = (e: Event) => this.handleEvent(e as KeyboardEvent);
        target.addEventListener('keydown', listener);
        return () => target.removeEventListener('keydown', listener);
    }

    /**
     * Processes a keyboard event, looking for a matching shortcut.
     * Returns true if a shortcut was found and executed.
     */
    public handleEvent(e: KeyboardEvent): boolean {
        const mods = {
            ctrl: e.ctrlKey || undefined,
            shift: e.shiftKey || undefined,
            alt: e.altKey || undefined,
            meta: e.metaKey || undefined,
        };

        // Try code-based lookup first (game/positional shortcuts)
        const codeId = shortcutId({ code: e.code, ...mods });
        // Fall back to key-based lookup (UI/character shortcuts)
        const keyId = shortcutId({ key: e.key, ...mods });

        const descriptor = this.shortcuts.get(codeId) ?? this.shortcuts.get(keyId);
        if (!descriptor) return false;

        const target = e.target as HTMLElement;
        const isFormElement =
            target.tagName === 'INPUT' ||
            target.tagName === 'TEXTAREA' ||
            target.tagName === 'SELECT' ||
            target.isContentEditable;

        const isCanvas = target.tagName === 'CANVAS';

        // Apply suppression rules
        if (descriptor.namespace === 'game' && isFormElement) return false;
        if (descriptor.namespace === 'ui' && isCanvas) return false;

        if (descriptor.preventDefault) {
            e.preventDefault();
        }

        descriptor.handler();
        return true;
    }

    // ─── Help Overlay ─────────────────────────────────────────────────────────

    /**
     * Registers the shortcut help overlay element.
     * The overlay is rendered lazily when first shown.
     */
    setOverlayElement(el: HTMLElement): void {
        this.overlayEl = el;
    }

    /** Shows or hides the shortcut help overlay. */
    toggleOverlay(): void {
        if (this.overlayVisible) {
            this.hideOverlay();
        } else {
            this.showOverlay();
        }
    }

    showOverlay(): void {
        if (!this.overlayEl) return;
        this.renderOverlay();
        this.overlayEl.style.display = 'flex';
        this.overlayVisible = true;
    }

    hideOverlay(): void {
        if (!this.overlayEl) return;
        this.overlayEl.style.display = 'none';
        this.overlayVisible = false;
    }

    get isOverlayVisible(): boolean {
        return this.overlayVisible;
    }

    private renderOverlay(): void {
        if (!this.overlayEl) return;

        // Group shortcuts by category
        const grouped = new Map<string, ShortcutDescriptor[]>();
        for (const descriptor of this.shortcuts.values()) {
            const cat = descriptor.category ?? 'General';
            if (!grouped.has(cat)) grouped.set(cat, []);
            grouped.get(cat)!.push(descriptor);
        }

        // Sort categories: Engine/Debug first, then alphabetical
        const priorityCategories = ['Engine', 'Debug'];
        const sortedCategories = [
            ...priorityCategories.filter(c => grouped.has(c)),
            ...[...grouped.keys()].filter(c => !priorityCategories.includes(c)).sort(),
        ];

        // Build the modal tree using DOM APIs to avoid XSS.
        const modal = document.createElement('div');
        modal.className = 'shortcut-modal glass-panel';

        // Header
        const header = document.createElement('div');
        header.className = 'shortcut-modal-header';

        const tabsContainer = document.createElement('div');
        tabsContainer.className = 'shortcut-modal-tabs';

        const tabShortcuts = document.createElement('button');
        tabShortcuts.className = 'shortcut-tab active';
        tabShortcuts.textContent = 'Keyboard Shortcuts';

        const tabHelp = document.createElement('button');
        tabHelp.className = 'shortcut-tab';
        tabHelp.textContent = 'Status Badges';

        const tabTelemetry = document.createElement('button');
        tabTelemetry.className = 'shortcut-tab';
        tabTelemetry.textContent = 'Telemetry';

        tabsContainer.appendChild(tabShortcuts);
        tabsContainer.appendChild(tabHelp);
        tabsContainer.appendChild(tabTelemetry);

        const closeBtn = document.createElement('button');
        closeBtn.className = 'shortcut-close-btn';
        closeBtn.id = 'shortcut-close-btn';
        closeBtn.setAttribute('aria-label', 'Close shortcuts overlay');
        closeBtn.textContent = '\u2715';
        closeBtn.addEventListener('click', () => this.hideOverlay());

        header.appendChild(tabsContainer);
        header.appendChild(closeBtn);

        // Body
        const body = document.createElement('div');
        body.className = 'shortcut-modal-body';

        const shortcutsContent = document.createElement('div');
        shortcutsContent.className = 'shortcut-tab-content active';

        const helpContent = document.createElement('div');
        helpContent.className = 'shortcut-tab-content';

        // Build help sections via DOM API (no innerHTML).
        type BadgeSpec = { text: string; color: string; border: string; bg: string };
        const makeSection = (badges: BadgeSpec[], bodyText: string) => {
            const section = document.createElement('div');
            section.className = 'help-doc-section';
            const title = document.createElement('div');
            title.className = 'help-doc-title';
            for (const b of badges) {
                const badge = document.createElement('span');
                badge.className = 'mode-badge';
                badge.style.color = b.color;
                badge.style.borderColor = b.border;
                badge.style.background = b.bg;
                badge.textContent = b.text;
                title.appendChild(badge);
            }
            const body = document.createElement('div');
            body.textContent = bodyText;
            section.appendChild(title);
            section.appendChild(body);
            return section;
        };
        const mix = (v: string) => `color-mix(in srgb, ${v} 10%, transparent)`;
        helpContent.appendChild(makeSection(
            [
                { text: 'INFRA: LIVE', color: 'var(--accent-success)', border: 'var(--accent-success)', bg: mix('var(--accent-success)') },
                { text: 'ENGINE: SERVER CTRL', color: 'var(--accent-success)', border: 'var(--accent-success)', bg: mix('var(--accent-success)') },
            ],
            'Playground connected to Game Server. State is authoritative.'
        ));

        helpContent.appendChild(makeSection(
            [
                { text: 'INFRA: OFFLINE', color: 'var(--accent-danger)', border: 'var(--accent-danger)', bg: mix('var(--accent-danger)') },
                { text: 'ENGINE: HALTED', color: 'var(--accent-danger)', border: 'var(--accent-danger)', bg: mix('var(--accent-danger)') },
            ],
            'Connection lost or rate limited. Engine stops accepting input until restored.'
        ));
        helpContent.appendChild(makeSection(
            [
                { text: 'INFRA: RECONNECTING', color: 'var(--accent-warning)', border: 'var(--accent-warning)', bg: mix('var(--accent-warning)') },
                { text: 'ENGINE: BLOCKED', color: 'var(--accent-warning)', border: 'var(--accent-warning)', bg: mix('var(--accent-warning)') },
            ],
            'Attempting to restore session. Simulation is paused to prevent state desync.'
        ));

        const telemetryContent = document.createElement('div');
        telemetryContent.className = 'shortcut-tab-content';

        // Build telemetry sections via DOM API (no innerHTML).
        const makeTelSection = (titleText: string, bodyText: string) => {
            const section = document.createElement('div');
            section.className = 'help-doc-section';
            const title = document.createElement('div');
            title.className = 'help-doc-title';
            title.textContent = titleText;
            const body = document.createElement('div');
            body.textContent = bodyText;
            section.appendChild(title);
            section.appendChild(body);
            return section;
        };
        telemetryContent.appendChild(makeTelSection('FPS (Host)',
            "Frames Per Second of the browser's Main Thread (UI and DOM rendering). High values mean a smooth interface."));
        telemetryContent.appendChild(makeTelSection('FPS (WASM)',
            'Frames Per Second of the distinct Render Worker running the WebAssembly engine. Represents the true engine tick rate.'));
        telemetryContent.appendChild(makeTelSection('Frame Time (p99)',
            'The 99th percentile time taken by the WASM render loop to draw a single frame to the WebGL canvas.'));
        telemetryContent.appendChild(makeTelSection('Sim Time (p99)',
            'The 99th percentile time taken by the game simulation (physics, collision, and authority logic) per tick.'));
        telemetryContent.appendChild(makeTelSection('RTT',
            'Round Trip Time. High-precision network latency measured over the active WebTransport connection. Represented in milliseconds.'));
        telemetryContent.appendChild(makeTelSection('Entities',
            'The total number of actor entities currently spawned and being simulated within the spatial grid.'));
        telemetryContent.appendChild(makeTelSection('Dropped',
            'The amount of predicted frames that were dropped or reverted because they failed server reconciliation due to desync.'));
        telemetryContent.appendChild(makeTelSection('SAB',
            'SharedArrayBuffer memory lock status. Indicates when WebAssembly workers are blocked waiting for atomic memory handoffs.'));

        for (const cat of sortedCategories) {
            const items = grouped.get(cat)!;

            const group = document.createElement('div');
            group.className = 'shortcut-group';

            const groupTitle = document.createElement('div');
            groupTitle.className = 'shortcut-group-title';
            groupTitle.textContent = cat;
            group.appendChild(groupTitle);

            for (const d of items) {
                const row = document.createElement('div');
                row.className = 'shortcut-row';

                const label = document.createElement('span');
                label.className = 'shortcut-label';
                label.textContent = d.label;

                const combo = document.createElement('span');
                combo.className = 'shortcut-combo';
                const keys = shortcutId(d).split('+');
                keys.forEach((k, i) => {
                    if (i > 0) combo.appendChild(document.createTextNode(' + '));
                    const kbd = document.createElement('kbd');
                    kbd.textContent = k === 'escape' ? 'Esc' : k.toUpperCase();
                    combo.appendChild(kbd);
                });

                row.appendChild(label);
                row.appendChild(combo);
                group.appendChild(row);
            }

            shortcutsContent.appendChild(group);
        }

        body.appendChild(shortcutsContent);
        body.appendChild(helpContent);
        body.appendChild(telemetryContent);

        // Tab Event Listeners
        const updateTabs = (activeTab: HTMLButtonElement, activeContent: HTMLElement) => {
            [tabShortcuts, tabHelp, tabTelemetry].forEach(btn => btn.classList.remove('active'));
            [shortcutsContent, helpContent, telemetryContent].forEach(content => content.classList.remove('active'));

            activeTab.classList.add('active');
            activeContent.classList.add('active');
        };

        tabShortcuts.addEventListener('click', () => updateTabs(tabShortcuts, shortcutsContent));
        tabHelp.addEventListener('click', () => updateTabs(tabHelp, helpContent));
        tabTelemetry.addEventListener('click', () => updateTabs(tabTelemetry, telemetryContent));

        // Footer
        const footer = document.createElement('div');
        footer.className = 'shortcut-modal-footer';
        footer.append('Press ');
        const kbd1 = document.createElement('kbd'); kbd1.textContent = '?'; footer.appendChild(kbd1);
        footer.append(' or ');
        const kbd2 = document.createElement('kbd'); kbd2.textContent = 'F1'; footer.appendChild(kbd2);
        footer.append(' to toggle this panel');

        modal.appendChild(header);
        modal.appendChild(body);
        modal.appendChild(footer);

        // Replace overlay content
        this.overlayEl.innerHTML = '';
        this.overlayEl.appendChild(modal);
    }
}

/** Global singleton registry — import and use throughout the app. */
export const shortcuts = new ShortcutRegistry();
