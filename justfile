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

# Run clippy lints
[group('lint')]
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Automatically apply formatting and clippy fixes
[group('lint')]
fix:
    cargo fmt --all
    cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged

# Run all unit and integration tests
[group('test')]
test:
    cargo nextest run --workspace --profile ci

# Run security audits (licenses, advisories, vulnerabilities)
[group('security')]
security:
    cargo deny check
    cargo audit

# Build documentation
[group('doc')]
docs:
    cargo doc --workspace --no-deps

# Check documentation quality (linting, frontmatter, spelling, links)
[group('doc')]
docs-check:
    python3 scripts/doc_lint.py
    python3 scripts/check_links.py
    uvx codespell

# Build documentation (mirrors the CI job — warnings are errors)
[group('doc')]
docs-strict:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# Pinned nightly for udeps / wasm (matches Aetheris workspace)
wasm_nightly := "nightly-2025-07-01"

# Build WASM target with SharedArrayBuffer + WebGPU flags
[group('build')]
wasm:
    RUSTFLAGS="-C target-feature=+atomics,+bulk-memory,+mutable-globals -C link-arg=--shared-memory -C link-arg=--import-memory --cfg=web_sys_unstable_apis" \
    cargo +{{wasm_nightly}} build \
        -p aetheris-client-wasm \
        --target wasm32-unknown-unknown \
        --release \
        -Z build-std=std,panic_abort

# Start the local dev server (requires wasm-pack and a server)
[group('build')]
dev: wasm
    @echo "WASM build done. Use a local server (e.g. miniserve playground/) to test."

# Check for unused dependencies (requires nightly; runs on main in CI)
[group('lint')]
udeps:
    cargo +{{wasm_nightly}} udeps --workspace --all-targets

# Remove all build artefacts reproducible via just build
[group('maintenance')]
clean:
    cargo clean

# Check semver compatibility for library crates before a release
[group('release')]
semver:
    cargo semver-checks --workspace
