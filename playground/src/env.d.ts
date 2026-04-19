interface ImportMetaEnv {
  readonly VITE_SERVER_CERT_HASH: string
  readonly VITE_SERVER_URL?: string
  readonly VITE_AUTH_URL?: string
  readonly DEV: boolean
  readonly VITE_ENABLE_SMOKE_PREFILL?: string
  readonly VITE_PLAYGROUND_CONNECTED?: string
  readonly VITE_TELEMETRY_URL?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}

// Allow importing the compiled WASM binary as a URL via Vite's ?url query.
declare module 'aetheris-client-wasm/aetheris_client_wasm_bg.wasm?url' {
  const url: string;
  export default url;
}

