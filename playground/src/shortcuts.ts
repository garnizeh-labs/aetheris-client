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
        const title = document.createElement('span');
        title.className = 'shortcut-modal-title';
        title.textContent = 'Keyboard Shortcuts';
        const closeBtn = document.createElement('button');
        closeBtn.className = 'shortcut-close-btn';
        closeBtn.id = 'shortcut-close-btn';
        closeBtn.setAttribute('aria-label', 'Close shortcuts overlay');
        closeBtn.textContent = '\u2715';
        closeBtn.addEventListener('click', () => this.hideOverlay());
        header.appendChild(title);
        header.appendChild(closeBtn);

        // Body
        const body = document.createElement('div');
        body.className = 'shortcut-modal-body';

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

            body.appendChild(group);
        }

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
