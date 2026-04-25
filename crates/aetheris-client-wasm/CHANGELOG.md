# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.21](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.20...aetheris-client-wasm-v0.3.21) - 2026-04-25

### Added

- *(client)* orchestrate cinematic entry flow with ViewState transitions and ship arrival animations (M10156)
- *(client)* implement WASM telemetry and rendering refinements

## [0.3.20](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.18...aetheris-client-wasm-v0.3.20) - 2026-04-24

### Added

- *(client)* stabilize ship movement and implement global camera shake
- implement infinite toroidal world physics and seamless background in playground

### Fixed

- stabilize toroidal transitions, reconciliation and background parallax

### Other

- release v0.3.18
- Merge pull request #41 from garnizeh-labs/vs-01-refinement
- remove client-side prediction logic from entity component updates
- vs-01 refinement - reduced log spam and documented prediction status

## [0.3.19](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.18...aetheris-client-wasm-v0.3.19) - 2026-04-24

### Added

- *(client)* stabilize ship movement and implement global camera shake
- implement infinite toroidal world physics and seamless background in playground

### Fixed

- stabilize toroidal transitions, reconciliation and background parallax

### Other

- Merge pull request #41 from garnizeh-labs/vs-01-refinement
- remove client-side prediction logic from entity component updates
- vs-01 refinement - reduced log spam and documented prediction status

## [0.3.18](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.16...aetheris-client-wasm-v0.3.18) - 2026-04-23

### Added

- *(client)* implement mandatory state_hash for WorldState compliance

### Fixed

- *(metrics)* use getrandom_v02 for ULID generation
- *(ci)* restrict getrandom_backend cfg to WASM target
- *(client)* elevate getrandom to regular dependencies to force feature unification
- *(client)* stabilize state hashing and restore WASM thread flags
- *(client)* force getrandom v0.3 unification with wasm_js feature
- update getrandom_v03 dependency to version 0.3.4 with wasm_js feature enabled
- *(client)* enable wasm_js feature for getrandom v0.3

### Other

- release v0.3.17

## [0.3.17](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.16...aetheris-client-wasm-v0.3.17) - 2026-04-23

### Added

- *(client)* implement mandatory state_hash for WorldState compliance

### Fixed

- *(metrics)* use getrandom_v02 for ULID generation
- *(ci)* restrict getrandom_backend cfg to WASM target
- *(client)* elevate getrandom to regular dependencies to force feature unification
- *(client)* stabilize state hashing and restore WASM thread flags
- *(client)* force getrandom v0.3 unification with wasm_js feature
- update getrandom_v03 dependency to version 0.3.4 with wasm_js feature enabled
- *(client)* enable wasm_js feature for getrandom v0.3

## [0.3.16](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.14...aetheris-client-wasm-v0.3.16) - 2026-04-23

### Added

- *(client)* implement ReplicationBatch handling and update protocol

### Other

- release v0.3.15

## [0.3.15](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.14...aetheris-client-wasm-v0.3.15) - 2026-04-23

### Added

- *(client)* implement ReplicationBatch handling and update protocol

## [0.3.14](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.12...aetheris-client-wasm-v0.3.14) - 2026-04-22

### Fixed

- *(client)* address clippy lints in shared_world and world_state

### Other

- release v0.3.13
- Merge pull request #30 from garnizeh-labs/fix/update-cargo-lock-vs-05-06

## [0.3.13](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.12...aetheris-client-wasm-v0.3.13) - 2026-04-22

### Fixed

- *(client)* address clippy lints in shared_world and world_state

### Other

- Merge pull request #30 from garnizeh-labs/fix/update-cargo-lock-vs-05-06

## [0.3.12](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.10...aetheris-client-wasm-v0.3.12) - 2026-04-22

### Added

- *(client)* enhance room bounds handling with seqlock for consistent reads and updates
- *(client)* update input pipeline and world state for VS-05/VS-06

### Fixed

- *(client)* M10105 — tick-gated pending_clear suppression

### Other

- release v0.3.11

## [0.3.11](https://github.com/garnizeh-labs/aetheris-client/compare/aetheris-client-wasm-v0.3.10...aetheris-client-wasm-v0.3.11) - 2026-04-22

### Added

- *(client)* enhance room bounds handling with seqlock for consistent reads and updates
- *(client)* update input pipeline and world state for VS-05/VS-06

### Fixed

- *(client)* M10105 — tick-gated pending_clear suppression

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
