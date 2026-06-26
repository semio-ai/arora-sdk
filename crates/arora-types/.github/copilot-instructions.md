# Copilot Instructions for arora-types

## Project Overview

`arora-types` is a Rust library that provides **low-level types and serialization** methods for exchanging data across module boundaries and over the network. It also exposes JavaScript/TypeScript bindings via **WebAssembly** (using `wasm-bindgen`).

## Repository Structure

```
src/
  lib.rs          – Root module; exports all sub-modules
  call.rs         – Call-related types
  keyvalue.rs     – Key-value map types
  value.rs        – Generic Value enum and ConversionError
  wasm_value.rs   – WASM bindings for Value (wasm32 only)
  module/
    mod.rs        – Module-level shared types
    high.rs       – High-level ModuleDefinition (uses names)
    low.rs        – Low-level Header (uses UUIDs)
  ty/
    mod.rs        – Primitive type definitions
    high.rs       – High-level structured type descriptors
    low.rs        – Low-level structured type descriptors
tests/
  integration/    – JavaScript integration tests (run against the built WASM package)
```

## High-level vs. Low-level Types

- **High-level** types (e.g. `ModuleDefinition`) use **names** to reference entities. They are resolved via a registry and serialized as `module.yaml`.
- **Low-level** types (e.g. `Header`) use **UUIDs** directly. They are serialized as `header.yaml` and used by the engine at runtime.

## Build & Test Commands

### Rust

```bash
# Run all Rust tests
cargo test --locked

# Check the WASM build compiles
cargo build --locked --tests --target wasm32-unknown-unknown

# Release build
cargo build --locked --release

# Format code (uses 2-space indentation, see .rustfmt.toml)
cargo fmt

# Lint
cargo clippy
```

### WebAssembly / JavaScript

```bash
# Build the WASM package (outputs to pkg/)
npm run build:wasm      # wasm-pack build --target bundler --out-dir pkg

# Run JavaScript integration tests
npm test

# Build and test in one step
npm run build:wasm && npm test
```

> **Note:** `wasm-pack` must be installed (`cargo install wasm-pack`) and the `wasm32-unknown-unknown` target must be available (`rustup target add wasm32-unknown-unknown`).

## Coding Conventions

- **Rust formatting**: 2-space indentation (enforced by `.rustfmt.toml`). Run `cargo fmt` before committing.
- **Serde**: All public types that cross boundaries derive `Serialize` and `Deserialize`. Use `serde(rename)` or `serde(rename_all)` to control wire names.
- **WASM-only code**: Gate it with `#[cfg(target_arch = "wasm32")]`. The `wasm_value.rs` module is the canonical example.
- **UUIDs**: Use `uuid::Uuid` throughout. Helper functions `gen_uuid_from_str` and `gen_bb_uuid` are available in `lib.rs`.
- **Error types**: Prefer the existing `arora_types::value::ConversionError` for value-conversion failures.
- **Naming**: Follow standard Rust naming conventions (`snake_case` for functions/fields, `PascalCase` for types/enums).

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `serde` | Serialization / deserialization |
| `serde_yaml` | YAML format (primary wire format for module definitions) |
| `uuid` | UUID generation and parsing |
| `semver` | Semantic versioning |
| `derive_more` | Derive helper macros (e.g. `Display`) |
| `wasm-bindgen` | Rust ↔ JavaScript FFI (WASM target only) |
| `js-sys` | JavaScript standard library bindings (WASM target only) |

## CI

The CI workflow (`.github/workflows/ci.yml`) runs on every push/PR to `main` and:
1. Runs `cargo test --locked`
2. Checks `cargo build --locked --tests --target wasm32-unknown-unknown`
3. Builds a release binary with `cargo build --locked --release`

All three steps must pass before merging.
