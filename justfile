# Run fast quality gate checks (fmt, clippy, test, security, docs-check)
[group('check')]
check: fmt clippy security docs-check wasm

# Check formatting
[group('lint')]
fmt:
    cargo fmt --all --check

# Run clippy lints
[group('lint')]
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

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

wasm_nightly := "nightly-2025-07-01"

# Build the WASM client using direct Cargo + wasm-bindgen
[group('build')]
wasm:
    RUSTFLAGS="-C target-feature=+atomics,+bulk-memory,+mutable-globals -C link-arg=--shared-memory -C link-arg=--import-memory --cfg=web_sys_unstable_apis" \
    cargo +{{wasm_nightly}} build \
        --target wasm32-unknown-unknown \
        --release \
        -Z build-std=std,panic_abort
