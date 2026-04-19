/**
 * world-registry.ts — Aetheris World & Theme Management
 *
 * Handles the boot sequence, hot-swapping themes, and world state persistence.
 * Synchronizes visual changes with the Render Worker.
 */

export interface ThemeManifest {
    slug: string;
    displayName: string;
    cssPath: string;
    colorScheme: 'dark' | 'light';
    reducedEffects?: boolean;
    highContrast?: boolean;
}

export interface WorldManifest {
    slug: string;
    displayName: string;
    description: string;
    version: string;
    defaultTheme: string;
    themes: ThemeManifest[];
    worldCssPath: string;
    shortcutNamespace: string;
}

// Global registry of available worlds
export const WORLDS: Record<string, WorldManifest> = {
    'aetheris-playground': {
        slug: 'aetheris-playground',
        displayName: 'Aetheris Playground',
        description: 'Engine sandbox — developer & QA tooling',
        version: '0.1.0',
        defaultTheme: 'frost-dawn',
        themes: [
            { slug: 'frost-dawn', displayName: 'Frost-Dawn', cssPath: '/worlds/aetheris-playground/themes/frost-dawn.css', colorScheme: 'dark' },
            { slug: 'blueprint', displayName: 'Blueprint', cssPath: '/worlds/aetheris-playground/themes/blueprint.css', colorScheme: 'dark' },
            { slug: 'blueprint-lite', displayName: 'Blueprint Lite', cssPath: '/worlds/aetheris-playground/themes/blueprint-lite.css', colorScheme: 'dark', reducedEffects: true },
        ],
        worldCssPath: '/worlds/aetheris-playground/world.css',
        shortcutNamespace: 'game',
    }
};

export const STORAGE_WORLD_KEY = 'aetheris:world';
export const STORAGE_THEME_KEY = 'aetheris:theme';

export class WorldRegistry {
    private renderWorker: Worker | null = null;
    private currentWorld: WorldManifest | null = null;
    private currentTheme: ThemeManifest | null = null;
    private themeMessagePending = false;

    /**
     * Initializes the registry, applies the active world/theme CSS,
     * and handles query-param overrides.
     */
    boot(): void {
        const params = new URLSearchParams(window.location.search);
        const storedWorld = localStorage.getItem(STORAGE_WORLD_KEY);
        const storedTheme = localStorage.getItem(STORAGE_THEME_KEY);
        const queryWorld = params.get('world');
        const queryTheme = params.get('theme');

        const worldSlug = queryWorld || storedWorld || 'aetheris-playground';
        const world = WORLDS[worldSlug] || WORLDS['aetheris-playground'];

        // Determine theme and log its source
        let themeSlug = queryTheme || storedTheme;
        let source = themeSlug === queryTheme ? 'URL' : 'Storage';
        if (!themeSlug || !world.themes.find(t => t.slug === themeSlug)) {
            themeSlug = world.defaultTheme;
            source = 'Default';
        }

        const theme = world.themes.find(t => t.slug === themeSlug)!;

        this.currentWorld = world;
        this.currentTheme = theme;

        // Apply attributes to <html>
        document.documentElement.setAttribute('data-world', world.slug);
        document.documentElement.setAttribute('data-theme', theme.slug);

        // Inject CSS links into <head>
        this.applyCssLinks(world, theme);

        console.log(`[WorldRegistry] Booted: World=${world.slug}, Theme=${theme.slug} (Source: ${source})`);

        // Persist only if not overridden by query params
        if (!params.has('world')) localStorage.setItem(STORAGE_WORLD_KEY, world.slug);
        if (!params.has('theme')) localStorage.setItem(STORAGE_THEME_KEY, theme.slug);
    }

    setRenderWorker(worker: Worker): void {
        this.renderWorker = worker;
        // Immediate sync after worker is available
        this.syncThemeToWorker();
    }

    /** Hot-swap theme (no reload). */
    switchTheme(slug: string): void {
        if (!this.currentWorld) return;
        const theme = this.currentWorld.themes.find(t => t.slug === slug);
        if (!theme) {
            console.error(`[WorldRegistry] Unknown theme: ${slug}`);
            return;
        }

        this.currentTheme = theme;
        document.documentElement.setAttribute('data-theme', theme.slug);
        localStorage.setItem(STORAGE_THEME_KEY, theme.slug);

        this.applyCssLinks(this.currentWorld, theme);
        this.syncThemeToWorker();

        // Notify listeners (bi-directional sync)
        window.dispatchEvent(new CustomEvent('aetheris:theme_changed', { detail: { theme: slug } }));
    }

    /** Switch World (triggers reload). */
    switchWorld(slug: string): void {
        if (!WORLDS[slug]) {
            console.error(`[WorldRegistry] Unknown world: ${slug}`);
            return;
        }
        localStorage.setItem(STORAGE_WORLD_KEY, slug);
        localStorage.removeItem(STORAGE_THEME_KEY); // Reset theme for new world
        window.location.reload();
    }

    get activeWorld(): string { return this.currentWorld?.slug || ''; }
    get activeTheme(): string { return this.currentTheme?.slug || ''; }
    get availableThemes(): ThemeManifest[] { return this.currentWorld?.themes || []; }

    private applyCssLinks(world: WorldManifest, theme: ThemeManifest): void {
        this.ensureLinkElement('world-css', world.worldCssPath);
        this.ensureLinkElement('theme-css', theme.cssPath);
    }

    private ensureLinkElement(id: string, href: string): void {
        let link = document.getElementById(id) as HTMLLinkElement;
        if (!link) {
            link = document.createElement('link');
            link.id = id;
            link.rel = 'stylesheet';
            document.head.appendChild(link);
        }
        if (link.getAttribute('href') !== href) {
            link.setAttribute('href', href);
        }
    }

    /**
     * Reads the resolved CSS variables and notifies the Render Worker.
     * Use requestAnimationFrame to debounce high-frequency updates (THEME_WORLD_DESIGN §3.4.1).
     */
    syncThemeToWorker(): void {
        if (!this.renderWorker || this.themeMessagePending) return;

        this.themeMessagePending = true;
        requestAnimationFrame(() => {
            if (!this.renderWorker || !this.currentTheme) {
                this.themeMessagePending = false;
                return;
            }

            const style = getComputedStyle(document.documentElement);
            const bgBase = style.getPropertyValue('--bg-base').trim();
            const textPrimary = style.getPropertyValue('--text-primary').trim();

            this.renderWorker.postMessage({
                type: 'theme_changed',
                payload: {
                    bgBase: bgBase || '#070c1a',
                    textPrimary: textPrimary || '#f1f5f9',
                    reducedEffects: this.currentTheme.reducedEffects ?? false,
                }
            });

            this.themeMessagePending = false;
        });
    }
}

export const worldRegistry = new WorldRegistry();
