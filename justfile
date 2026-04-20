# Run fast quality gate checks (fmt, clippy, test, security, docs-check)
[group('check')]
check: fmt clippy test security docs-check

# Run ALL CI-equivalent checks (fast + wasm, docs-strict, udeps)
[group('check')]
check-all: check wasm docs-strict udeps

# Check formatting
[group('lint')]
fmt:
    cargo fmt --all --check

# Link to local aetheris-protocol for development
[group('dev')]
link-dev:
    python3 scripts/dev_toggle.py --enable --path Cargo.toml

# Unlink local aetheris-protocol (switch back to crates.io)
[group('dev')]
unlink-dev:
    python3 scripts/dev_toggle.py --disable --path Cargo.toml

# Run clippy lints
[group('lint')]
clippy:
    cargo clippy --workspace -- -D warnings

# Automatically apply formatting and clippy fixes
[group('lint')]
fix:
    cargo fmt --all
    cargo clippy --workspace --fix --allow-dirty --allow-staged

# Run all unit and integration tests
[group('test')]
test:
    cargo nextest run --workspace --profile ci --no-tests=pass

# Run WASM smoke tests using wasm-pack (Node.js)
[group('test')]
test-wasm:
    wasm-pack test --node crates/aetheris-client-wasm --features metrics

# Run WASM smoke tests using wasm-pack (Headless Browser)
[group('test')]
test-wasm-browser:
    wasm-pack test --chrome --headless crates/aetheris-client-wasm --features metrics

# Run security audits (licenses, advisories, vulnerabilities)
[group('security')]
security:
    cargo deny check
    cargo audit

# Build documentation
[group('doc')]
docs:
    cargo doc --workspace --no-deps

# Check documentation quality (linting, frontmatter, spelling, links, branding)
[group('doc')]
docs-check:
    python3 scripts/doc_lint.py
    python3 scripts/check_links.py
    python3 scripts/check_branding.py
    codespell

# Build documentation (mirrors the CI job — warnings are errors)
[group('doc')]
docs-strict:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# Pinned nightly for WASM multi-threading (LLVM 20 — generates __wasm_init_tls).
# LLVM 22 (nightly latest) renamed this symbol; wasm-bindgen 0.2.118 doesn't support it yet.
wasm_nightly := "nightly-2025-07-01"

# WASM multi-thread build flags (shared across wasm and wasm-dev targets)
wasm_flags := "-C target-feature=+atomics,+bulk-memory,+mutable-globals -C link-arg=--shared-memory -C link-arg=--import-memory --cfg=web_sys_unstable_apis"

# Shared post-build step: run wasm-bindgen and write the package.json shim
[private]
_wasm_post profile:
    wasm-bindgen target/wasm32-unknown-unknown/{{profile}}/aetheris_client_wasm.wasm \
        --out-dir crates/aetheris-client-wasm/pkg \
        --target web
    printf '{"name":"aetheris-client-wasm","type":"module","main":"./aetheris_client_wasm.js","types":"./aetheris_client_wasm.d.ts"}\n' \
        > crates/aetheris-client-wasm/pkg/package.json

# Build the WASM client (release) — compiles + wasm-bindgen + package.json shim
[group('build')]
wasm:
    RUSTFLAGS="{{wasm_flags}}" \
    cargo +{{wasm_nightly}} build \
        --target wasm32-unknown-unknown \
        --release \
        -Z build-std=std,panic_abort \
        -p aetheris-client-wasm
    just _wasm_post release

# Build the WASM client (debug) — faster iteration
[group('build')]
wasm-dev:
    RUSTFLAGS="{{wasm_flags}}" \
    cargo +{{wasm_nightly}} build \
        --target wasm32-unknown-unknown \
        -Z build-std=std,panic_abort \
        -p aetheris-client-wasm
    just _wasm_post debug

# Install npm dependencies for the playground
[group('build')]
client-install:
    npm install --prefix playground

# Build the playground bundle (production)
[group('build')]
client-build: client-install
    npm run build --prefix playground

# Build the playground bundle in dev/watch mode
[group('build')]
client-dev: client-install
    npm run dev --prefix playground

# Start the Vite dev server (background)
[group('run')]
vite: client-install
    @just wait-for-cert
    cd playground && VITE_SERVER_CERT_HASH=$(cat ../target/dev-certs/cert.sha256 2>/dev/null || echo "missing") npm run dev &

# Start the Vite dev server in connected-playground mode (background)
[group('run')]
vite-connected: client-install
    @just wait-for-cert
    cd playground && VITE_PLAYGROUND_CONNECTED=true VITE_SERVER_CERT_HASH=$(cat ../target/dev-certs/cert.sha256 2>/dev/null || echo "missing") npm run dev &

# Full playground session in connected mode
[group('run')]
playground-connected: stop wasm-dev
    @mkdir -p logs
    cd playground && VITE_PLAYGROUND_CONNECTED=true VITE_SERVER_CERT_HASH=$(cat ../../aetheris-engine/target/dev-certs/cert.sha256 2>/dev/null || echo "missing") npm run dev >> ../logs/vite.log 2>&1 &
    @echo "Playground connected session ready at http://localhost:5173/playground.html"

# Helper to wait for server certificate
wait-for-cert:
    @echo "Waiting for target/dev-certs/cert.sha256 (Timeout: 60s)..."
    @i=0; while [ $i -lt 60 ]; do \
        if [ -f target/dev-certs/cert.sha256 ]; then \
            echo "Certificate ready."; \
            exit 0; \
        fi; \
        sleep 1; \
        i=`expr $i + 1`; \
    done; \
    echo "Error: target/dev-certs/cert.sha256 missing. Run 'just server' first."; \
    exit 1

# Stop all background processes
[group('maintenance')]
stop:
    -fuser -k 5173/tcp >/dev/null 2>&1 || true

# Build the WASM client (release) then the web bundle — full production pipeline
[group('build')]
client: wasm client-build

# Build the WASM client (debug) then the web bundle — fast dev pipeline
[group('build')]
client-dev-full: wasm-dev client-build

# Start the playground in isolated sandbox mode (no server, no auth)
[group('run')]
playground: stop wasm-dev client-install
    @mkdir -p logs
    @echo "Starting Playground (Isolated)..."
    cd playground && VITE_PLAYGROUND_CONNECTED=false npm run dev >> ../logs/vite.log 2>&1 &
    @echo ""
    @echo "  \x1b[1;36m┌─────────────────────────────────────────────────────────────┐\x1b[0m"
    @echo "  \x1b[1;36m│\x1b[0m  \x1b[1;37mAetheris Client — \x1b[1;34mPLAYGROUND (MODE: SANDBOX)\x1b[0m              \x1b[1;36m│\x1b[0m"
    @echo "  \x1b[1;36m├─────────────────────────────────────────────────────────────┤\x1b[0m"
    @echo "  \x1b[1;36m│\x1b[0m  URL: \x1b[4;34mhttp://localhost:5173/playground.html\x1b[0m                   \x1b[1;36m│\x1b[0m"
    @echo "  \x1b[1;36m│\x1b[0m  Status: \x1b[1;32mIsolated Sandbox (No server, no auth)\x1b[0m              \x1b[1;36m│\x1b[0m"
    @echo "  \x1b[1;36m└─────────────────────────────────────────────────────────────┘\x1b[0m"
    @echo ""


# Remove all build artefacts
[group('maintenance')]
clean: stop
    cargo clean
    rm -rf crates/aetheris-client-wasm/pkg
    rm -rf playground/dist
    rm -rf playground/node_modules

# Check for unused dependencies (requires nightly; runs on main in CI)
[group('lint')]
udeps:
    cargo +{{wasm_nightly}} udeps --workspace --all-targets

# Check semver compatibility for library crates before a release
[group('release')]
semver:
    cargo semver-checks --workspace

# Generate the changelog (preview only)
[group('release')]
changelog:
    git cliff -o CHANGELOG.md

# Prepare a new release (updates Cargo.toml, CHANGELOG.md, commits and tags)
# Usage: just release 0.3.0
[group('release')]
release version: check-all
    sed 's/^version = ".*"/version = "{{version}}"/' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml
    git cliff --tag v{{version}} -o CHANGELOG.md
    git add Cargo.toml CHANGELOG.md
    git commit -m "chore(release): prepare for v{{version}}"
    git tag -a v{{version}} -m "Release v{{version}}"
    @echo "Release prepared. Run 'git push origin main --tags' to finalize."
