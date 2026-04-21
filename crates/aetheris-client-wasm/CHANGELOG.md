# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.10](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.8...aetheris-client-wasm-v0.3.10) - 2026-04-21

### Added

- *(client)* harden mining render loop and fix protocol v0.2.11 CI compilation
- *(mining)* update client input to targeted model and fix protocol v0.2.11 errors
- *(client)* implement GameEvent handling and mining target cleanup

### Fixed

- *(client)* allow unreachable wildcards for future-proof protocol matching

### Other

- release v0.3.9
- *(render)* optimize mining target lookup and harden laser buffer

## [0.3.9](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.8...aetheris-client-wasm-v0.3.9) - 2026-04-21

### Added

- *(client)* harden mining render loop and fix protocol v0.2.11 CI compilation
- *(mining)* update client input to targeted model and fix protocol v0.2.11 errors
- *(client)* implement GameEvent handling and mining target cleanup

### Fixed

- *(client)* allow unreachable wildcards for future-proof protocol matching

### Other

- *(render)* optimize mining target lookup and harden laser buffer

## [0.3.8](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.6...aetheris-client-wasm-v0.3.8) - 2026-04-20

### Added

- complete VS-01 client logic and documentation

### Other

- release v0.3.7

## [0.3.7](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.6...aetheris-client-wasm-v0.3.7) - 2026-04-20

### Added

- complete VS-01 client logic and documentation

## [0.3.6](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.4...aetheris-client-wasm-v0.3.6) - 2026-04-20

### Added

- *(client)* synchronize ECS state handling and ship stats for M1020

### Fixed

- *(client)* update world state for Transform.entity_type compatibility

### Other

- release v0.3.5

## [0.3.5](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.4...aetheris-client-wasm-v0.3.5) - 2026-04-20

### Added

- *(client)* synchronize ECS state handling and ship stats for M1020

### Fixed

- *(client)* update world state for Transform.entity_type compatibility

## [0.3.4](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.2.1...aetheris-client-wasm-v0.3.4) - 2026-04-20

### Added

- *(protocol)* add NetworkEvent::Disconnected and enforcement of branding rules (M10146)

### Fixed

- *(client-wasm)* fix Disconnected variant in transport_mock
- *(client-wasm)* align with protocol v0.2.5 changes and clean unused imports

### Other

- *(client-wasm)* relocate wasm-only dependencies to target-specific section
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
