# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
## [0.2.1] - 2026-04-19

### 🚀 Features

- Migrate to full Rust workspace with WASM client and CI standards

### 🐛 Bug Fixes

- Gate set_debug_mode under cfg(debug_assertions)
- Move wasm-only deps to cfg(target_arch = wasm32) target section
- Apply code review — correctness, safety, and polish
- *(playground)* Code review batch 2 — playground TS + lib.rs
- *(shared-world)* Use u64::from() instead of as cast (clippy::cast_lossless)

### 📚 Documentation

- Add crate-level READMEs for wasm and native crates

### ⚙️ Miscellaneous Tasks

- Release v0.2.0
## [0.2.0] - 2026-04-19

### 🚀 Features

- Migrate to full Rust workspace with WASM client and CI standards

### 🐛 Bug Fixes

- Gate set_debug_mode under cfg(debug_assertions)
- Move wasm-only deps to cfg(target_arch = wasm32) target section
- Apply code review — correctness, safety, and polish
- *(playground)* Code review batch 2 — playground TS + lib.rs
- *(shared-world)* Use u64::from() instead of as cast (clippy::cast_lossless)

### 📚 Documentation

- Add crate-level READMEs for wasm and native crates
